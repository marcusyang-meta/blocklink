//! Authenticated application streams over Iroh. Only game and published Mods are exposed.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use iroh::{
    endpoint::{presets, Connection, RecvStream, SendStream},
    Endpoint, EndpointAddr, RelayMode, SecretKey,
};
use std::sync::{OnceLock, Weak};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    runtime::Runtime,
    sync::Semaphore,
    task::JoinHandle,
};

const ALPN: &[u8] = b"blocklink/peer/1";
const PREFIX: &str = "blocklink-peer:";
const HEADER_LIMIT: usize = 8192;
fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("network runtime")
    })
}
fn err(e: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}
pub(super) struct Peer {
    endpoint: Endpoint,
    bridges: Mutex<HashMap<String, (u16, JoinHandle<()>)>>,
    connections: Mutex<HashMap<String, (String, Connection)>>,
}
impl Peer {
    pub fn start(engine: &Arc<Engine>, relay: Option<&str>) -> Result<Arc<Self>> {
        let key_path = engine.ws.root().join("peer-identity.json");
        let key: SecretKey = if key_path.exists() {
            serde_json::from_value(read_json(&key_path)?)?
        } else {
            let mut bytes = [0u8; 32];
            bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
            bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
            let key = SecretKey::from_bytes(&bytes);
            write_json(&key_path, &serde_json::to_value(&key)?)?;
            key
        };
        let mode = match relay {
            Some("disabled") => RelayMode::Disabled,
            Some("test-relay-only") if cfg!(test) => RelayMode::Default,
            Some(url) if !url.is_empty() => {
                let parsed = reqwest::Url::parse(url)?;
                if parsed.scheme() != "https"
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                {
                    bail!("中继需要 HTTPS 地址")
                }
                RelayMode::Custom(
                    [url.parse::<iroh::RelayUrl>().map_err(err)?]
                        .into_iter()
                        .collect(),
                )
            }
            _ => RelayMode::Default,
        };
        let builder = Endpoint::builder(presets::Minimal)
            .relay_mode(mode)
            .secret_key(key)
            .alpns(vec![ALPN.to_vec()]);
        let builder = if cfg!(test) && relay == Some("test-relay-only") {
            builder.clear_ip_transports()
        } else {
            builder
        };
        let endpoint = runtime().block_on(builder.bind()).map_err(err)?;
        let peer = Arc::new(Self {
            endpoint: endpoint.clone(),
            bridges: Mutex::new(HashMap::new()),
            connections: Mutex::new(HashMap::new()),
        });
        let weak = Arc::downgrade(engine);
        runtime().spawn(async move {
            let limit = Arc::new(Semaphore::new(64));
            while let Some(incoming) = endpoint.accept().await {
                let Ok(permit) = limit.clone().try_acquire_owned() else {
                    incoming.refuse();
                    continue;
                };
                let weak = weak.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let Ok(Ok(conn)) =
                        tokio::time::timeout(Duration::from_secs(15), incoming).await
                    else {
                        return;
                    };
                    let streams = Arc::new(Semaphore::new(16));
                    while let Ok((send, recv)) = conn.accept_bi().await {
                        let Ok(permit) = streams.clone().try_acquire_owned() else {
                            conn.close(1u8.into(), b"too many streams");
                            break;
                        };
                        let weak = weak.clone();
                        let conn = conn.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            let _ = serve_stream(weak, conn, send, recv).await;
                        });
                    }
                });
            }
        });
        Ok(peer)
    }
    pub fn status(&self) -> Value {
        let connections=self.connections.lock().unwrap().values().map(|(id,c)| {
            let paths=c.paths();let selected=paths.iter().find(|p|p.is_selected());
            json!({"id":id,"route":selected.as_ref().map(|p|if p.is_ip(){"direct"}else{"relay"}).unwrap_or("connecting"),"latencyMs":selected.map(|p|p.rtt().as_millis())})
        }).collect::<Vec<_>>();
        json!({"enabled":true,"endpointId":self.endpoint.id().to_string(),"connections":connections,"bridges":self.bridges.lock().unwrap().iter().map(|(id,(port,_))| json!({"id":id,"address":format!("127.0.0.1:{port}")})).collect::<Vec<_>>()})
    }
    pub fn close(&self) {
        for (_, (_, task)) in self.bridges.lock().unwrap().drain() {
            task.abort();
        }
        runtime().block_on(self.endpoint.close());
        self.connections.lock().unwrap().clear();
    }
    pub fn close_bridge(&self, id: &str) {
        if let Some((_, task)) = self.bridges.lock().unwrap().remove(id) {
            task.abort();
        }
        self.connections
            .lock()
            .unwrap()
            .retain(|_, (instance, conn)| {
                if instance == id {
                    conn.close(0u8.into(), b"game stopped");
                    false
                } else {
                    true
                }
            });
    }
    fn invite(&self, engine: &Engine, id: &str) -> Result<Value> {
        let i = engine.ws.instance(id)?;
        if engine.config(id)["server"] != true {
            bail!("请选择托管服务器")
        }
        engine.publish_lock(id)?;
        runtime()
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(20), self.endpoint.online()).await
            })
            .context("无法连接中继，请检查网络或设置自建中继")?;
        let mut cfg = engine.config(id);
        if cfg["peerToken"].as_str().is_none() {
            cfg["peerToken"] = json!(format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()));
        }
        cfg["peerEnabled"] = json!(true);
        engine.settings.lock().unwrap()["instances"][id] = cfg.clone();
        engine.save_settings()?;
        let d = json!({"version":1,"name":i.name,"serverId":id,"address":self.endpoint.addr(),"token":cfg["peerToken"]});
        Ok(
            json!({"invitation":format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(&d)?))}),
        )
    }
    async fn request(
        &self,
        d: &Value,
        kind: &str,
        hash: Option<&str>,
    ) -> Result<(Connection, SendStream, RecvStream)> {
        let addr: EndpointAddr = serde_json::from_value(d["address"].clone())?;
        let conn = tokio::time::timeout(Duration::from_secs(20), self.endpoint.connect(addr, ALPN))
            .await
            .context("联机连接超时，服主可能离线")?
            .map_err(err)?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(err)?;
        write_frame(
            &mut send,
            &json!({"serverId":d["serverId"],"token":d["token"],"kind":kind,"hash":hash}),
        )
        .await?;
        let response =
            tokio::time::timeout(Duration::from_secs(20), read_frame(&mut recv)).await??;
        if response["ok"] != true {
            bail!("{}", response["error"].as_str().unwrap_or("联机请求被拒绝"))
        }
        Ok((conn, send, recv))
    }
    fn manifest(&self, d: &Value) -> Result<Lockfile> {
        runtime().block_on(async {
            let (_conn, mut send, mut recv) = self.request(d, "manifest", None).await?;
            send.finish().map_err(err)?;
            let bytes =
                tokio::time::timeout(Duration::from_secs(30), recv.read_to_end(16 * 1024 * 1024))
                    .await?
                    .map_err(err)?;
            let lock: Lockfile = serde_json::from_slice(&bytes)?;
            lock.validate()?;
            Ok(lock)
        })
    }
    fn download(&self, engine: &Engine, d: &Value, m: &blocklink_model::Artifact) -> Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(engine.ws.root().join("downloads"))?;
        runtime().block_on(async {
            let (_conn, mut send, recv) = self.request(d, "blob", Some(&m.sha512)).await?;
            send.finish().map_err(err)?;
            let mut input = AsyncReadExt::take(recv, m.bytes + 1);
            let mut file = tokio::fs::File::from_std(temp.reopen()?);
            let count = tokio::time::timeout(
                Duration::from_secs(300),
                tokio::io::copy(&mut input, &mut file),
            )
            .await??;
            file.flush().await?;
            if count != m.bytes {
                bail!("Mod 文件长度不匹配")
            }
            Ok::<_, anyhow::Error>(())
        })?;
        temp.flush()?;
        if hash_file(temp.path(), "sha512")? != m.sha512 {
            bail!("Mod 文件校验失败")
        }
        engine.ws.import_jar(temp.path())?;
        Ok(())
    }
    pub fn bridge(self: &Arc<Self>, id: &str, invitation: &str) -> Result<u16> {
        let d = parse(invitation)?;
        let mut bridges = self.bridges.lock().unwrap();
        if let Some((port, task)) = bridges.get(id) {
            if !task.is_finished() {
                return Ok(*port);
            }
        }
        // Validate host availability before launching Minecraft.
        runtime().block_on(async {
            let (conn, mut send, _) = self.request(&d, "probe", None).await?;
            send.finish().map_err(err)?;
            conn.close(0u8.into(), b"checked");
            Ok::<_, anyhow::Error>(())
        })?;
        let listener = runtime().block_on(TcpListener::bind("127.0.0.1:0"))?;
        let port = listener.local_addr()?.port();
        let weak = Arc::downgrade(self);
        let instance_id = id.to_owned();
        let task = runtime().spawn(async move {
            let limit = Arc::new(Semaphore::new(16));
            while let Ok((socket, _)) = listener.accept().await {
                let Some(peer) = weak.upgrade() else { break };
                let d = d.clone();
                let instance_id = instance_id.clone();
                let Ok(permit) = limit.clone().try_acquire_owned() else {
                    continue;
                };
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Ok((conn, send, recv)) = peer.request(&d, "game", None).await {
                        let key = uuid::Uuid::new_v4().to_string();
                        peer.connections
                            .lock()
                            .unwrap()
                            .insert(key.clone(), (instance_id, conn));
                        let _ = pipe(socket, send, recv).await;
                        peer.connections.lock().unwrap().remove(&key);
                    }
                });
            }
        });
        bridges.insert(id.into(), (port, task));
        Ok(port)
    }
}
async fn write_frame(send: &mut SendStream, v: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(v)?;
    if bytes.len() > HEADER_LIMIT {
        bail!("联机消息过大")
    }
    send.write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(err)?;
    send.write_all(&bytes).await.map_err(err)?;
    Ok(())
}
async fn read_frame(recv: &mut RecvStream) -> Result<Value> {
    let mut size = [0; 4];
    recv.read_exact(&mut size).await.map_err(err)?;
    let n = u32::from_be_bytes(size) as usize;
    if n > HEADER_LIMIT {
        bail!("联机消息过大")
    };
    let mut bytes = vec![0; n];
    recv.read_exact(&mut bytes).await.map_err(err)?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn authorized(engine: &Engine, p: &Value) -> bool {
    let Some(id) = p["serverId"].as_str() else {
        return false;
    };
    let cfg = engine.config(id);
    cfg["server"] == true
        && cfg["peerEnabled"] == true
        && p["token"]
            .as_str()
            .is_some_and(|t| t.len() >= 64 && cfg["peerToken"].as_str() == Some(t))
}
async fn serve_stream(
    weak: Weak<Engine>,
    conn: Connection,
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<()> {
    let p = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut recv)).await??;
    let engine = weak.upgrade().context("后台已关闭")?;
    if !authorized(&engine, &p) {
        write_frame(
            &mut send,
            &json!({"ok":false,"error":"邀请已撤销或无权访问"}),
        )
        .await?;
        send.finish().map_err(err)?;
        return Ok(());
    }
    let id = field(&p, "serverId")?;
    if matches!(p["kind"].as_str(), Some("game" | "probe")) && !engine.is_running(id) {
        write_frame(
            &mut send,
            &json!({"ok":false,"error":"服主尚未启动服务器，请先开服"}),
        )
        .await?;
        send.finish().map_err(err)?;
        return Ok(());
    }
    let operation = async {
        match field(&p, "kind")? {
            "manifest" => {
                let lock = engine.publish_lock(id)?;
                write_frame(&mut send, &json!({"ok":true})).await?;
                send.write_all(&serde_json::to_vec(&lock)?)
                    .await
                    .map_err(err)?;
            }
            "blob" => {
                let lock = engine.publish_lock(id)?;
                let hash = field(&p, "hash")?;
                let m = lock
                    .mods
                    .iter()
                    .find(|m| m.sha512 == hash && m.side != Side::Server)
                    .context("文件不在已发布环境中")?;
                let owned = engine.clone();
                let hash = m.sha512.clone();
                let size = m.bytes;
                let path = tokio::task::spawn_blocking(move || {
                    owned.ws.verify_blob(&hash, size)?;
                    Ok::<_, anyhow::Error>(owned.ws.blob_path(&hash)?)
                })
                .await??;
                let mut file = tokio::fs::File::open(path).await?;
                write_frame(&mut send, &json!({"ok":true})).await?;
                tokio::io::copy(&mut file, &mut send).await?;
            }
            "game" | "probe" => {
                if !engine.is_running(id) {
                    bail!("服务器尚未启动")
                }
                let port = engine.config(id)["port"].as_u64().context("游戏端口无效")?;
                let socket = tokio::time::timeout(
                    Duration::from_secs(5),
                    TcpStream::connect(("127.0.0.1", port as u16)),
                )
                .await??;
                write_frame(&mut send, &json!({"ok":true})).await?;
                if p["kind"] == "game" {
                    return pipe(socket, send, recv).await;
                }
            }
            _ => bail!("不支持的联机请求"),
        }
        send.finish().map_err(err)?;
        let _ = send.stopped().await;
        Ok(())
    };
    // Revoke also disconnects an already open game/file stream.
    let revoked = async {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if !authorized(&engine, &p) {
                conn.close(1u8.into(), b"revoked");
                break;
            }
        }
    };
    tokio::select! { result=operation=>{if result.is_err(){conn.close(1u8.into(),b"request failed")};result}, _=revoked=>Ok(()) }
}
async fn pipe(socket: TcpStream, mut send: SendStream, mut recv: RecvStream) -> Result<()> {
    socket.set_nodelay(true)?;
    let (mut input, mut output) = socket.into_split();
    let up = async {
        tokio::io::copy(&mut input, &mut send).await?;
        send.finish().map_err(err)?;
        Ok::<_, anyhow::Error>(())
    };
    let down = async {
        tokio::io::copy(&mut recv, &mut output).await?;
        output.shutdown().await?;
        Ok::<_, anyhow::Error>(())
    };
    tokio::try_join!(up, down)?;
    Ok(())
}
pub(super) fn parse(invitation: &str) -> Result<Value> {
    if invitation.len() > 32768 {
        bail!("邀请过长")
    }
    let d: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD.decode(
            invitation
                .trim()
                .strip_prefix(PREFIX)
                .context("不是 Blocklink 联机邀请")?,
        )?,
    )?;
    if d["version"] != 1 {
        bail!("不支持的联机邀请版本")
    }
    blocklink_model::validate_uuid(field(&d, "serverId")?)?;
    let addr: EndpointAddr = serde_json::from_value(d["address"].clone())?;
    if addr.addrs.is_empty() || addr.addrs.len() > 16 || field(&d, "token")?.len() != 72 {
        bail!("联机邀请无效")
    }
    for address in &addr.addrs {
        match address {
            iroh::TransportAddr::Ip(_) => {}
            iroh::TransportAddr::Relay(url) if url.as_str().starts_with("https://") => {}
            _ => bail!("不支持的联机地址"),
        }
    }
    Ok(d)
}
pub(super) fn ensure(engine: &Arc<Engine>) -> Result<Arc<Peer>> {
    let mut slot = engine.peer.lock().unwrap();
    if let Some(peer) = slot.as_ref() {
        return Ok(peer.clone());
    }
    let relay = engine.settings.lock().unwrap()["peerRelay"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let peer = Peer::start(engine, Some(&relay))?;
    *slot = Some(peer.clone());
    Ok(peer)
}
fn existing(engine: &Engine) -> Result<Arc<Peer>> {
    engine
        .peer
        .lock()
        .unwrap()
        .clone()
        .context("联机网络尚未启用")
}
pub(super) fn invitation(engine: &Arc<Engine>, p: &Value) -> Result<Value> {
    ensure(engine)?.invite(engine, field(p, "id")?)
}
pub(super) fn join(engine: &Engine, p: &Value, report: &game::Reporter) -> Result<Value> {
    let invitation = field(p, "invitation")?;
    let d = parse(invitation)?;
    report("连接服主并验证已发布环境".into());
    let lock = existing(engine)?.manifest(&d)?;
    let id = uuid::Uuid::new_v4().to_string();
    let i: Instance = serde_json::from_value(
        json!({"schemaVersion":1,"instanceId":id,"name":p["name"].as_str().filter(|s|!s.trim().is_empty()).unwrap_or(d["name"].as_str().unwrap_or("联机实例")),"minecraft":lock.environment.minecraft,"loader":lock.environment.loader,"runtime":{"java":"auto","memoryMiB":4096},"storage":{"linkMode":"auto"},"mods":[]}),
    )?;
    engine.ws.create_instance(&i)?;
    engine.settings.lock().unwrap()["instances"][&id] =
        json!({"server":false,"peerInvitation":invitation,"remoteName":d["name"]});
    engine.save_settings()?;
    sync(engine, &i, invitation, report)?;
    Ok(json!({"id":id}))
}
pub(super) fn sync(
    engine: &Engine,
    i: &Instance,
    invitation: &str,
    report: &game::Reporter,
) -> Result<()> {
    let d = parse(invitation)?;
    let peer = existing(engine)?;
    report("通过加密联机连接检查服务器 Mods".into());
    let mut target = peer.manifest(&d)?;
    if target.content.is_some(){bail!("This room shares configuration. Use a Blocklink lobby invitation to synchronize it.")}
    i.accepts(&target)?;
    target.mods.retain(|m| m.side != Side::Server);
    for m in &target.mods {
        if engine.ws.verify_blob(&m.sha512, m.bytes).is_err() {
            report(format!("同步 Mod · {}", m.mod_id));
            peer.download(engine, &d, m)?
        }
    }
    for m in mods::lock(&engine.ws, i)?
        .mods
        .into_iter()
        .filter(|m| m.side == Side::Client)
    {
        if let Some(other) = target.mods.iter().find(|x| x.mod_id == m.mod_id) {
            if other.sha512 != m.sha512 {
                bail!("客户端 Mod {} 与服务器冲突", m.mod_id)
            }
        } else {
            target.mods.push(m)
        }
    }
    target.validate()?;
    mods::apply(&engine.ws, i, &target)?;
    Ok(())
}
pub(super) fn game_port(engine: &Engine, id: &str, invitation: &str) -> Result<u16> {
    existing(engine)?.bridge(id, invitation)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "child process fixture, called by peer integration test"]
    fn fixture_process() {
        if std::env::var_os("BLOCKLINK_PEER_FIXTURE").is_some() {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    #[test]
    fn encrypted_sync_game_forwarding_and_live_revocation() -> Result<()> {
        exercise_network("disabled")
    }
    #[test]
    #[ignore = "uses public relay; run explicitly for network acceptance"]
    fn public_relay_sync_and_game_without_direct_transport() -> Result<()> {
        exercise_network("test-relay-only")
    }
    fn exercise_network(mode: &str) -> Result<()> {
        let host_root = tempfile::tempdir()?;
        let client_root = tempfile::tempdir()?;
        let host = Arc::new(Engine::new(host_root.path())?);
        let client = Arc::new(Engine::new(client_root.path())?);
        let hp = Peer::start(&host, Some(mode))?;
        let cp = Peer::start(&client, Some(mode))?;
        if mode == "test-relay-only" {
            runtime().block_on(async {
                tokio::time::timeout(Duration::from_secs(30), async {
                    hp.endpoint.online().await;
                    cp.endpoint.online().await
                })
                .await
            })?;
        }
        *host.peer.lock().unwrap() = Some(hp.clone());
        *client.peer.lock().unwrap() = Some(cp.clone());
        let report: game::Reporter = Arc::new(|_| {});
        let created=host.execute("create",&json!({"name":"P2P test","minecraft":"1.21.1","loader":"fabric","loaderVersion":"0.19.5","server":true,"install":false}),report.clone())?;
        let id = field(&created, "id")?;
        let i = host.ws.instance(id)?;
        let jar = host_root.path().join("mod.jar");
        let mut zip = zip::ZipWriter::new(fs::File::create(&jar)?);
        zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())?;
        zip.write_all(
            br#"{"schemaVersion":1,"id":"peer_test","version":"1.0.0","environment":"*"}"#,
        )?;
        zip.finish()?;
        mods::local(&host.ws, &i, &jar, true)?;
        host.execute("publish", &json!({"id":id}), report.clone())?;
        let token = format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        {
            let mut s = host.settings.lock().unwrap();
            s["instances"][id]["peerToken"] = json!(token);
            s["instances"][id]["peerEnabled"] = json!(true);
        }
        let d = json!({"version":1,"name":"P2P test","serverId":id,"address":hp.endpoint.addr(),"token":token});
        let code = format!(
            "{PREFIX}{}",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&d)?)
        );
        let result = join(&client, &json!({"invitation":code}), &report)?;
        let ci = client.ws.instance(field(&result, "id")?)?;
        client.ws.verify_instance(&ci.instance_id)?;
        assert_eq!(mods::lock(&client.ws, &ci)?.mods.len(), 1);
        let mut wrong = d.clone();
        wrong["token"] = json!("x".repeat(72));
        assert!(cp.manifest(&wrong).is_err());
        assert!(runtime()
            .block_on(cp.request(&d, "blob", Some(&"0".repeat(128))))
            .is_err());
        let echo = std::net::TcpListener::bind("127.0.0.1:0")?;
        host.settings.lock().unwrap()["instances"][id]["port"] = json!(echo.local_addr()?.port());
        let echo_task = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut socket, _) = echo.accept().unwrap();
                let mut buf = [0; 64];
                while let Ok(n) = socket.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    if socket.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            }
        });
        let child = hidden(
            Command::new(std::env::current_exe()?)
                .args(["--exact", "peer::tests::fixture_process", "--ignored"])
                .env("BLOCKLINK_PEER_FIXTURE", "1")
                .stdout(Stdio::null())
                .stderr(Stdio::null()),
        )
        .spawn()?;
        host.children.lock().unwrap().insert(id.into(), child);
        let test_result = (|| -> Result<()> {
            let port = cp.bridge(&ci.instance_id, &code)?;
            let mut socket = std::net::TcpStream::connect(("127.0.0.1", port))?;
            socket.set_read_timeout(Some(Duration::from_secs(10)))?;
            socket.write_all(b"minecraft stream")?;
            let mut out = [0; 16];
            socket.read_exact(&mut out)?;
            assert_eq!(&out, b"minecraft stream");
            if mode == "test-relay-only" {
                assert_eq!(cp.status()["connections"][0]["route"], "relay");
            }
            host.dispatch("peer-revoke", json!({"id":id}))?;
            let mut b = [0; 1];
            assert!(matches!(socket.read(&mut b), Ok(0) | Err(_)));
            assert!(cp.manifest(&d).is_err());
            assert_eq!(mods::lock(&client.ws, &ci)?.mods.len(), 1);
            Ok(())
        })();
        if let Some(mut child) = host.children.lock().unwrap().remove(id) {
            let _ = child.kill();
            let _ = child.wait();
        }
        cp.close();
        hp.close();
        test_result?;
        echo_task.join().map_err(|_| err("echo thread"))?;
        Ok(())
    }
}
