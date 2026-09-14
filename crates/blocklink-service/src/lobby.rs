//! Cloudflare room discovery/signaling; WebRTC carries game and published Mod streams.
use super::*;
use futures_util::{SinkExt, StreamExt};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock, Weak,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
    task::JoinHandle,
};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use webrtc::peer_connection::{PeerConnection, RTCIceServer};

#[cfg(test)]
fn test_relay_only() -> bool {
    std::env::var("BLOCKLINK_TEST_RELAY_ONLY").as_deref() == Ok("1")
}
#[cfg(not(test))]
fn test_relay_only() -> bool {
    false
}
fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("lobby runtime")
    })
}
fn base_url(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value)?;
    let local = url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if (url.scheme() != "https" && !local)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        bail!("大厅地址必须是 HTTPS 根地址，本机测试可使用 localhost")
    }
    Ok(url)
}
fn parse(invitation: &str) -> Result<(String, String, String)> {
    if invitation.len() > 4096 {
        bail!("邀请过长")
    }
    let url = reqwest::Url::parse(invitation.trim())?;
    let id = url
        .path()
        .strip_prefix("/invite/")
        .context("不是 Blocklink 房间邀请")?;
    let token = url.fragment().context("邀请缺少访问凭据")?;
    if id.len() != 32
        || token.len() != 64
        || !id
            .bytes()
            .chain(token.bytes())
            .all(|c| c.is_ascii_hexdigit())
    {
        bail!("房间邀请无效")
    }
    let origin = url.origin().ascii_serialization() + "/";
    base_url(&origin)?;
    Ok((origin, id.into(), token.into()))
}
async fn http(base: &str, path: &str, token: &str, payload: Option<Value>) -> Result<Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let url = base_url(base)?.join(path)?;
    let request = if let Some(payload) = payload {
        client.post(url).json(&payload)
    } else {
        client.get(url)
    };
    let mut response = request.bearer_auth(token).send().await?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 262144 {
            bail!("大厅响应过大")
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    if !status.is_success() {
        bail!("{}", value["error"].as_str().unwrap_or("大厅请求失败"))
    }
    Ok(value)
}
pub(super) struct Session {
    base: String,
    room_id: String,
    token: String,
    host: bool,
    instance_id: String,
    tx: mpsc::Sender<Value>,
    alive: AtomicBool,
    view: Mutex<Value>,
    pcs: tokio::sync::Mutex<HashMap<String, Arc<dyn PeerConnection>>>,
    bridge: Mutex<Option<(u16, JoinHandle<()>)>>,
    ice: Vec<RTCIceServer>,
    mux: tokio::sync::Mutex<Option<rtc::Mux>>,
    cancelled: tokio::sync::watch::Sender<bool>,
    invitation: Mutex<Option<String>>,
    serving: Mutex<HashMap<String, JoinHandle<()>>>,
}
impl Session {
    fn status(&self) -> Value {
        json!({"id":self.instance_id,"roomId":self.room_id,"host":self.host,"connected":self.alive.load(Ordering::Relaxed),"room":self.view.lock().unwrap().clone(),"invitation":self.invitation.lock().unwrap().clone(),"address":self.bridge.lock().unwrap().as_ref().map(|(port,_)|format!("127.0.0.1:{port}"))})
    }
    async fn shutdown(&self) {
        self.alive.store(false, Ordering::Relaxed);
        self.cancelled.send_replace(true);
        self.mux.lock().await.take();
        for (_, task) in self.serving.lock().unwrap().drain() {
            task.abort();
        }
        if let Some((_, task)) = self.bridge.lock().unwrap().take() {
            task.abort();
        }
        let peers = std::mem::take(&mut *self.pcs.lock().await);
        for (_, pc) in peers {
            let _ = pc.close().await;
        }
    }
    async fn connect(
        engine: Weak<Engine>,
        base: String,
        room_id: String,
        token: String,
        host: bool,
        instance_id: String,
    ) -> Result<Arc<Self>> {
        let mut url = base_url(&base)?.join(&format!("api/rooms/{room_id}/socket"))?;
        url.set_scheme(if url.scheme() == "https" { "wss" } else { "ws" })
            .map_err(|_| anyhow::anyhow!("大厅地址无效"))?;
        let mut request = url.as_str().into_client_request()?;
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {token}").parse()?);
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(20),
            tokio_tungstenite::connect_async(request),
        )
        .await??;
        let turn = http(
            &base,
            &format!("api/rooms/{room_id}/turn"),
            &token,
            Some(json!({})),
        )
        .await?;
        let ice: Vec<RTCIceServer> = turn["iceServers"]
            .as_array()
            .context("缺少 ICE 配置")?
            .iter()
            .map(|server| {
                Ok(RTCIceServer {
                    urls: serde_json::from_value(server["urls"].clone())?,
                    username: server["username"].as_str().unwrap_or_default().into(),
                    credential: server["credential"].as_str().unwrap_or_default().into(),
                })
            })
            .collect::<Result<_>>()?;
        let (tx, mut rx) = mpsc::channel::<Value>(128);
        let session = Arc::new(Self {
            base,
            room_id,
            token,
            host,
            instance_id,
            tx,
            alive: AtomicBool::new(true),
            view: Mutex::new(Value::Null),
            pcs: tokio::sync::Mutex::new(HashMap::new()),
            bridge: Mutex::new(None),
            ice,
            mux: tokio::sync::Mutex::new(None),
            cancelled: tokio::sync::watch::channel(false).0,
            invitation: Mutex::new(None),
            serving: Mutex::new(HashMap::new()),
        });
        let weak = Arc::downgrade(&session);
        tokio::spawn(async move {
            let (mut output, mut input) = socket.split();
            let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
            let mut last_message = std::time::Instant::now();
            loop {
                let Some(session) = weak.upgrade() else { break };
                if !session.alive.load(Ordering::Relaxed) {
                    break;
                }
                tokio::select! {
                                    _ = heartbeat.tick() => {
                                        if last_message.elapsed() > Duration::from_secs(75) {break;}
                                        if output.send(Message::Text(json!({"type":"ping"}).to_string().into())).await.is_err() {break;}
                                        if host {if let Some(engine) = engine.upgrade() {if let Ok(mut data) = metadata(&engine, &session.instance_id) {data["type"] = json!("update"); if output.send(Message::Text(data.to_string().into())).await.is_err() {break;}}}}
                                    }
                                    value = rx.recv() => {let Some(value) = value else {break}; if output.send(Message::Text(value.to_string().into())).await.is_err() {break;}}
                                    message = input.next() => {
                                        let Some(Ok(message)) = message else {break}; last_message = std::time::Instant::now();
                                        if let Message::Text(text) = message {
                                            if text.len() > 65536 {break;}
                                            let Ok(value) = serde_json::from_str::<Value>(&text) else {break};
                                            match value["type"].as_str() {
                                                Some("welcome" | "room") => {
                                                    *session.view.lock().unwrap() = value["room"].clone();
                                                    if host {
                                                        let ids: Vec<String> = value["room"]["members"].as_array().into_iter().flatten().filter_map(|m|m["id"].as_str().map(str::to_owned)).collect();
                                                        let mut pcs = session.pcs.lock().await;
                                                        let removed: Vec<_> = pcs.keys().filter(|id|!ids.contains(id)).cloned().collect();
                                                        for id in removed {if let Some(task)=session.serving.lock().unwrap().remove(&id){task.abort();}
                if let Some(pc) = pcs.remove(&id) {let _ = pc.close().await;}}
                                                    }
                                                }
                                                Some("closed" | "host-offline") => break,
                                                Some("signal") => {
                                                    let session = session.clone(); let engine = engine.clone();
                                                    tokio::spawn(async move {if let Err(error) = session.signal(engine, value).await {#[cfg(test)] eprintln!("Lobby signal error: {error:#}"); *session.view.lock().unwrap() = json!({"error":error.to_string()});}});
                                                }
                                                _ => {}
                                            }
                                        } else if message.is_close() {break;}
                                    }
                                }
            }
            let _ = output.close().await;
            if let Some(session) = weak.upgrade() {
                session.shutdown().await;
            }
        });
        Ok(session)
    }
    async fn signal(&self, engine: Weak<Engine>, value: Value) -> Result<()> {
        let from = field(&value, "from")?;
        #[cfg(test)]
        eprintln!("signal from {from}: {}", value["kind"]);
        if self.host && value["kind"] == "offer" {
            let mut peers = self.pcs.lock().await;
            if peers.len() >= 32 && !peers.contains_key(from) {
                bail!("连接数已达上限")
            }
            if let Some(old) = peers.remove(from) {
                if let Some(task) = self.serving.lock().unwrap().remove(from) {
                    task.abort();
                }
                old.close().await?;
            }
            let (pc, gathered, channels) =
                rtc::connection(self.ice.clone(), test_relay_only()).await?;
            let serving = rtc::serve(
                engine,
                self.instance_id.clone(),
                channels,
                self.cancelled.subscribe(),
            );
            self.serving.lock().unwrap().insert(from.into(), serving);
            peers.insert(from.to_owned(), pc.clone());
            drop(peers);
            pc.set_remote_description(serde_json::from_value(value["payload"].clone())?)
                .await?;
            let answer = rtc::description(&pc, gathered, false).await?;
            self.tx
                .send(json!({"type":"signal","to":from,"kind":"answer","payload":answer}))
                .await?;
        } else if !self.host && from == "host" && value["kind"] == "answer" {
            if let Some(pc) = self.pcs.lock().await.get("host").cloned() {
                pc.set_remote_description(serde_json::from_value(value["payload"].clone())?)
                    .await?;
            }
        }
        Ok(())
    }
    async fn initiate(&self) -> Result<()> {
        let (pc, gathered, _channels) =
            rtc::connection(self.ice.clone(), test_relay_only()).await?;
        // Establish SCTP before later opening one channel for each request.
        let control = pc.create_data_channel("blocklink/ready", None).await?;
        self.pcs.lock().await.insert("host".into(), pc.clone());
        let offer = rtc::description(&pc, gathered, true).await?;
        #[cfg(test)]
        eprintln!("offer gathered");
        self.tx
            .send(json!({"type":"signal","kind":"offer","payload":offer}))
            .await?;
        let mux = rtc::client(control);
        let mut io = mux.open().await?;
        *self.mux.lock().await = Some(mux);
        rtc::write_frame(&mut io, &json!({"kind":"manifest"})).await?;
        let response =
            tokio::time::timeout(Duration::from_secs(45), rtc::read_frame(&mut io)).await??;
        if response["ok"] != true {
            bail!("房主尚未准备好环境")
        }
        let mut discard = Vec::new();
        io.take(16 * 1024 * 1024).read_to_end(&mut discard).await?;
        Ok(())
    }
    async fn request(&self, kind: &str, hash: Option<&str>) -> Result<rtc::Io> {
        if !self.alive.load(Ordering::Relaxed) {
            bail!("房间连接已断开，请重新加入")
        }
        let mut io = self
            .mux
            .lock()
            .await
            .as_ref()
            .context("尚未连接房主")?
            .open()
            .await?;
        rtc::write_frame(&mut io, &json!({"kind":kind,"hash":hash})).await?;
        let response =
            tokio::time::timeout(Duration::from_secs(45), rtc::read_frame(&mut io)).await??;
        if response["ok"] != true {
            bail!("{}", response["error"].as_str().unwrap_or("房主拒绝请求"))
        }
        Ok(io)
    }
    async fn manifest(&self) -> Result<Lockfile> {
        let mut io = self.request("manifest", None).await?;
        io.shutdown().await?;
        let mut bytes = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(30),
            io.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes),
        )
        .await??;
        if bytes.len() > 16 * 1024 * 1024 {
            bail!("环境清单过大")
        }
        let lock: Lockfile = serde_json::from_slice(&bytes)?;
        lock.validate()?;
        Ok(lock)
    }
}
fn metadata(engine: &Engine, id: &str) -> Result<Value> {
    let i = engine.ws.instance(id)?;
    let lock = engine.publish_lock(id)?;
    Ok(
        json!({"name":i.name,"minecraft":i.minecraft,"loader":serde_json::to_value(&i.loader)?["kind"],"modCount":lock.mods.len(),"running":engine.is_running(id)}),
    )
}
pub(super) fn status(engine: &Engine) -> Value {
    json!({"sessions":engine.lobby.lock().unwrap().values().map(|s|s.status()).collect::<Vec<_>>()})
}
pub(super) fn configure(engine: &Engine, p: &Value) -> Result<Value> {
    let url = base_url(field(p, "url")?)?.to_string();
    let key = p["hostKey"].as_str().unwrap_or("");
    if !key.is_empty() {
        keyring::Entry::new("Blocklink Lobby", &url)?.set_password(key)?;
    }
    engine.settings.lock().unwrap()["lobbyUrl"] = json!(url);
    engine.save_settings()?;
    Ok(json!({"ok":true}))
}
pub(super) fn create(engine: &Arc<Engine>, id: &str) -> Result<Value> {
    if engine.config(id)["server"] != true {
        bail!("请选择托管服务器")
    }
    let base = engine.settings.lock().unwrap()["lobbyUrl"]
        .as_str()
        .context("请先在设置中配置 Cloudflare 大厅地址")?
        .to_owned();
    close(engine, id)?;
    let key = keyring::Entry::new("Blocklink Lobby", &base)?
        .get_password()
        .context("请先保存大厅开房凭据")?;
    let created =
        runtime().block_on(http(&base, "api/rooms", &key, Some(metadata(engine, id)?)))?;
    let room_id = field(&created, "id")?.to_owned();
    let invitation = format!(
        "{}invite/{}#{}",
        base,
        room_id,
        field(&created, "invitation")?
    );
    let session = runtime().block_on(Session::connect(
        Arc::downgrade(engine),
        base,
        room_id,
        field(&created, "owner")?.into(),
        true,
        id.into(),
    ))?;
    *session.invitation.lock().unwrap() = Some(invitation.clone());
    engine.lobby.lock().unwrap().insert(id.into(), session);
    Ok(json!({"invitation":invitation,"expiresAt":created["expiresAt"]}))
}
pub(super) fn close(engine: &Engine, id: &str) -> Result<Value> {
    let session = engine.lobby.lock().unwrap().remove(id);
    if let Some(session) = session {
        runtime().block_on(async {
            let result = if session.host {
                http(
                    &session.base,
                    &format!("api/rooms/{}/close", session.room_id),
                    &session.token,
                    Some(json!({})),
                )
                .await
                .map(|_| ())
            } else {
                Ok(())
            };
            session.shutdown().await;
            result
        })?;
    }
    Ok(json!({"ok":true}))
}
fn guest(engine: &Arc<Engine>, id: &str, invitation: &str) -> Result<Arc<Session>> {
    if let Some(session) = engine
        .lobby
        .lock()
        .unwrap()
        .get(id)
        .filter(|s| s.alive.load(Ordering::Relaxed))
        .cloned()
    {
        return Ok(session);
    }
    let (base, room_id, token) = parse(invitation)?;
    runtime().block_on(async {
        let joined = http(&base,&format!("api/rooms/{room_id}/join"),"",Some(json!({"invitation":token,"name":engine.active_account(false).ok().and_then(|a|a["name"].as_str().map(str::to_owned)).unwrap_or("朋友".into())}))).await?;
        let session = Session::connect(Arc::downgrade(engine),base,room_id,field(&joined,"token")?.into(),false,id.into()).await?;
        if let Err(error) = session.initiate().await {session.shutdown().await; return Err(error);}
        engine.lobby.lock().unwrap().insert(id.into(),session.clone()); Ok(session)
    })
}
pub(super) fn join(engine: &Arc<Engine>, p: &Value, report: &game::Reporter) -> Result<Value> {
    let invitation = field(p, "invitation")?;
    let id = uuid::Uuid::new_v4().to_string();
    report("正在连接 Cloudflare 房间与房主".into());
    let session = guest(engine, &id, invitation)?;
    let lock = runtime().block_on(session.manifest())?;
    let name = p["name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            session.view.lock().unwrap()["name"]
                .as_str()
                .unwrap_or("联机房间")
                .to_owned()
        });
    let instance: Instance = serde_json::from_value(
        json!({"schemaVersion":1,"instanceId":id,"name":name,"minecraft":lock.environment.minecraft,"loader":lock.environment.loader,"runtime":{"java":"auto","memoryMiB":4096},"storage":{"linkMode":"auto"},"mods":[]}),
    )?;
    engine.ws.create_instance(&instance)?;
    engine.settings.lock().unwrap()["instances"][&id] =
        json!({"server":false,"lobbyInvitation":invitation,"remoteName":name});
    engine.save_settings()?;
    sync(engine, &instance, invitation, report)?;
    Ok(json!({"id":id}))
}
pub(super) fn sync(
    engine: &Arc<Engine>,
    i: &Instance,
    invitation: &str,
    report: &game::Reporter,
) -> Result<()> {
    let session = guest(engine, &i.instance_id, invitation)?;
    report("检查房主发布的环境".into());
    let mut target = runtime().block_on(session.manifest())?;
    i.accepts(&target)?;
    target.mods.retain(|m| m.side != Side::Server);
    if let Some(bundle)=&target.content {
        if engine.ws.verify_blob(&bundle.sha512,bundle.bytes).is_err(){
            report("同步服务器配置与脚本".into());let temp=tempfile::NamedTempFile::new_in(engine.ws.root().join("downloads"))?;
            runtime().block_on(async{let mut io=session.request("blob",Some(&bundle.sha512)).await?;io.shutdown().await?;let mut out=tokio::fs::File::from_std(temp.reopen()?);let count=tokio::time::timeout(Duration::from_secs(300),tokio::io::copy(&mut io.take(bundle.bytes+1),&mut out)).await??;out.flush().await?;if count!=bundle.bytes{bail!("Shared content length mismatch")};Ok::<_,anyhow::Error>(())})?;
            if hash_file(temp.path(),"sha512")?!=bundle.sha512{bail!("Shared content checksum mismatch")};engine.ws.import_jar(temp.path())?;
        }
    }
    for artifact in &target.mods {
        if engine
            .ws
            .verify_blob(&artifact.sha512, artifact.bytes)
            .is_err()
        {
            report(format!("同步 Mod · {}", artifact.mod_id));
            let temp = tempfile::NamedTempFile::new_in(engine.ws.root().join("downloads"))?;
            runtime().block_on(async {
                let mut io = session.request("blob", Some(&artifact.sha512)).await?;
                io.shutdown().await?;
                let mut file = tokio::fs::File::from_std(temp.reopen()?);
                let count = tokio::time::timeout(
                    Duration::from_secs(300),
                    tokio::io::copy(&mut io.take(artifact.bytes + 1), &mut file),
                )
                .await??;
                file.flush().await?;
                if count != artifact.bytes {
                    bail!("Mod 文件长度不匹配")
                }
                Ok::<_, anyhow::Error>(())
            })?;
            if hash_file(temp.path(), "sha512")? != artifact.sha512 {
                bail!("Mod 校验失败")
            }
            engine.ws.import_jar(temp.path())?;
        }
    }
    for artifact in mods::lock(&engine.ws, i)?
        .mods
        .into_iter()
        .filter(|m| m.side == Side::Client)
    {
        if let Some(other) = target.mods.iter().find(|m| m.mod_id == artifact.mod_id) {
            if other.sha512 != artifact.sha512 {
                bail!("客户端 Mod {} 与服务器冲突", artifact.mod_id)
            }
        } else {
            target.mods.push(artifact)
        }
    }
    target.validate()?;
    content::apply(engine,&i.instance_id,target.content.as_ref())?;
    mods::apply(&engine.ws, i, &target)?;
    Ok(())
}
pub(super) fn game_port(engine: &Arc<Engine>, id: &str, invitation: &str) -> Result<u16> {
    let session = guest(engine, id, invitation)?;
    if let Some((port, task)) = session.bridge.lock().unwrap().as_ref() {
        if !task.is_finished() {
            return Ok(*port);
        }
    }
    runtime().block_on(async {
        let mut probe=session.request("probe",None).await?;probe.shutdown().await?;
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await?;let port=listener.local_addr()?.port();let weak=Arc::downgrade(&session);
        let task=tokio::spawn(async move {let limit=Arc::new(tokio::sync::Semaphore::new(16));while let Ok((mut socket,_))=listener.accept().await {let Some(session)=weak.upgrade() else {break};let Ok(permit)=limit.clone().try_acquire_owned() else {continue};tokio::spawn(async move {let _permit=permit;let _=socket.set_nodelay(true);let mut cancelled=session.cancelled.subscribe();if *cancelled.borrow(){return}
if let Ok(mut io)=session.request("game",None).await {tokio::select!{_=cancelled.changed()=>{},_=tokio::io::copy_bidirectional(&mut socket,&mut io)=>{}}}});}});
        *session.bridge.lock().unwrap()=Some((port,task));Ok(port)
    })
}

#[cfg(test)]
#[path = "lobby_world_test.rs"]
mod world_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires lobby/test/local-server.mjs on localhost:8787"]
    fn cloudflare_room_mod_sync_tcp_and_revocation() -> Result<()> {
        let host_root = tempfile::tempdir()?;
        let client_root = tempfile::tempdir()?;
        let host = Arc::new(Engine::new(host_root.path())?);
        let client = Arc::new(Engine::new(client_root.path())?);
        let report: game::Reporter = Arc::new(|m| eprintln!("{m}"));
        let created=host.execute("create",&json!({"name":"Cloudflare test","minecraft":"1.21.1","loader":"fabric","loaderVersion":"0.19.5","server":true,"install":false}),report.clone())?;
        let id = field(&created, "id")?;
        let instance = host.ws.instance(id)?;
        let jar = host_root.path().join("mod.jar");
        let mut zip = zip::ZipWriter::new(fs::File::create(&jar)?);
        zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())?;
        zip.write_all(
            br#"{"schemaVersion":1,"id":"lobby_test","version":"1.0.0","environment":"*"}"#,
        )?;
        zip.start_file(
            "payload.bin",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )?;
        zip.write_all(&vec![42; 2 * 1024 * 1024])?;
        zip.finish()?;
        mods::local(&host.ws, &instance, &jar, true)?;
        host.settings.lock().unwrap()["instances"][id]["shareContent"]=json!(true);
        let shared=host.ws.instance_dir(id)?.join("game/config");fs::create_dir_all(&shared)?;fs::write(shared.join("lobby-test.toml"),b"version = 1")?;
        host.execute("publish", &json!({"id":id}), report.clone())?;
        let test_base =
            std::env::var("BLOCKLINK_TEST_LOBBY_URL").unwrap_or("http://127.0.0.1:8787/".into());
        let base = test_base.as_str();
        let test_key = if base.starts_with("https://") {
            keyring::Entry::new("Blocklink Lobby", base)?.get_password()?
        } else {
            "isolated-test-host-key".into()
        };
        let room = runtime().block_on(http(
            base,
            "api/rooms",
            &test_key,
            Some(metadata(&host, id)?),
        ))?;
        let session = runtime().block_on(Session::connect(
            Arc::downgrade(&host),
            base.into(),
            field(&room, "id")?.into(),
            field(&room, "owner")?.into(),
            true,
            id.into(),
        ))?;
        host.lobby.lock().unwrap().insert(id.into(), session);
        let invitation = format!(
            "{base}invite/{}#{}",
            field(&room, "id")?,
            field(&room, "invitation")?
        );
        let result = join(&client, &json!({"invitation":invitation}), &report)?;
        let ci = client.ws.instance(field(&result, "id")?)?;
        client.ws.verify_instance(&ci.instance_id)?;
        assert_eq!(mods::lock(&client.ws, &ci)?.mods.len(), 1);
        let received=client.ws.instance_dir(&ci.instance_id)?.join("game/config/lobby-test.toml");assert_eq!(fs::read(&received)?,b"version = 1");
        fs::write(shared.join("lobby-test.toml"),b"version = 2")?;host.execute("publish",&json!({"id":id}),report.clone())?;sync(&client,&ci,&invitation,&report)?;assert_eq!(fs::read(&received)?,b"version = 2");
        let cp = client.lobby.lock().unwrap()[&ci.instance_id].clone();
        assert!(runtime()
            .block_on(cp.request("blob", Some(&"0".repeat(128))))
            .is_err());
        let echo = std::net::TcpListener::bind("127.0.0.1:0")?;
        host.settings.lock().unwrap()["instances"][id]["port"] = json!(echo.local_addr()?.port());
        let echo_task = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut socket, _) = echo.accept().unwrap();
                let mut buffer = [0; 4096];
                while let Ok(n) = socket.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    if socket.write_all(&buffer[..n]).is_err() {
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
        let result = (|| -> Result<()> {
            let port = game_port(&client, &ci.instance_id, &invitation)?;
            let mut socket = std::net::TcpStream::connect(("127.0.0.1", port))?;
            socket.set_read_timeout(Some(Duration::from_secs(10)))?;
            socket.write_all(b"minecraft stream")?;
            let mut bytes = [0; 16];
            socket.read_exact(&mut bytes)?;
            assert_eq!(&bytes, b"minecraft stream");
            close(&host, id)?;
            let mut byte = [0];
            match socket.read(&mut byte) {
                Ok(0) => {}
                Err(e)
                    if !matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                other => panic!("revocation did not disconnect: {other:?}"),
            }
            assert!(runtime().block_on(cp.manifest()).is_err());
            Ok(())
        })();
        if let Some(mut child) = host.children.lock().unwrap().remove(id) {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = close(&client, &ci.instance_id);
        let _ = close(&host, id);
        result?;
        echo_task.join().unwrap();
        Ok(())
    }
}
