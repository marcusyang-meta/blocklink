//! Read-only HTTPS Mod distribution. Pairing trusts one host certificate; never disables TLS validation.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::{
    net::{TcpListener, TcpStream},
    sync::atomic::{AtomicUsize, Ordering},
};
fn tls_config(root: &Path) -> Result<Arc<ServerConfig>> {
    let identity = read_json(&root.join("host-identity.json"))?;
    let certs = rustls_pemfile::certs(&mut field(&identity, "certificate")?.as_bytes())
        .collect::<std::io::Result<Vec<_>>>()?;
    let key = rustls_pemfile::private_key(&mut field(&identity, "key")?.as_bytes())?
        .context("缺少服务端私钥")?;
    Ok(Arc::new(
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(certs, key)?,
    ))
}
pub(super) fn enable(engine: &Arc<Engine>, host: &str, port: u16) -> Result<()> {
    if port < 1024 {
        bail!("同步端口需要在 1024–65535 之间")
    }
    let url = reqwest::Url::parse(&format!("https://{host}:{port}"))?;
    if url.host_str().is_none()
        || url.path() != "/"
        || !url.username().is_empty()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("请输入主机 IP 或域名")
    }
    let root = engine.ws.root();
    let identity_path = root.join("host-identity.json");
    if identity_path.exists() {
        let identity = read_json(&identity_path)?;
        if identity["host"] != host {
            bail!("此后台已配置其他主机地址；请先关闭共享，再使用新地址生成邀请")
        }
    } else {
        let cert = rcgen::generate_simple_self_signed(vec![host.into()])?;
        write_json(
            &identity_path,
            &json!({"host":host,"certificate":cert.cert.pem(),"key":cert.key_pair.serialize_pem()}),
        )?;
    }
    if let Some(old) = engine.remote_port.lock().unwrap().as_ref() {
        if *old == port {
            return Ok(());
        }
        bail!("后台共享已使用其他端口")
    }
    let listener = TcpListener::bind(("0.0.0.0", port)).context("同步端口不可用")?;
    listener.set_nonblocking(true)?;
    let config = tls_config(root)?;
    *engine.remote_port.lock().unwrap() = Some(port);
    {
        engine.settings.lock().unwrap()["sharing"] =
            json!({"host":host,"port":port,"enabled":true});
    }
    engine.save_settings()?;
    let weak = Arc::downgrade(engine);
    std::thread::spawn(move || {
        let active = Arc::new(AtomicUsize::new(0));
        loop {
            let Some(engine) = weak.upgrade() else { break };
            if *engine.remote_port.lock().unwrap() != Some(port) {
                break;
            }
            let socket = match listener.accept() {
                Ok((socket, _)) => socket,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    drop(engine);
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(_) => break,
            };
            if active.fetch_add(1, Ordering::SeqCst) >= 24 {
                active.fetch_sub(1, Ordering::SeqCst);
                continue;
            }
            let config = config.clone();
            let active = active.clone();
            std::thread::spawn(move || {
                let _ = connection(&engine, socket, config);
                active.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(())
}
pub(super) fn disable(engine: &Engine) -> Result<()> {
    *engine.remote_port.lock().unwrap() = None;
    {
        let mut s = engine.settings.lock().unwrap();
        s["sharing"] = json!({"enabled":false});
        for (_, c) in s["instances"].as_object_mut().context("配置无效")? {
            c["shareToken"] = Value::Null;
        }
    }
    engine.save_settings()?;
    let identity = engine.ws.root().join("host-identity.json");
    if identity.exists() {
        fs::rename(
            identity,
            engine
                .ws
                .root()
                .join("downloads")
                .join(format!("retired-identity-{}.json", uuid::Uuid::new_v4())),
        )?;
    }
    std::thread::sleep(Duration::from_millis(150));
    Ok(())
}
fn reply(stream: &mut impl Write, status: u16, body: &[u8], kind: &str) -> Result<()> {
    write!(stream,"HTTP/1.1 {status} {}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",if status==200{"OK"}else{"Error"},body.len())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}
fn connection(engine: &Engine, socket: TcpStream, config: Arc<ServerConfig>) -> Result<()> {
    // Windows accepts inherit the listener's nonblocking mode.
    socket.set_nonblocking(false)?;
    socket.set_read_timeout(Some(Duration::from_secs(15)))?;
    socket.set_write_timeout(Some(Duration::from_secs(120)))?;
    let mut stream = StreamOwned::new(ServerConnection::new(config)?, socket);
    let mut buf = vec![];
    let mut byte = [0u8; 1];
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while buf.len() < 16_384 {
        if std::time::Instant::now() > deadline {
            bail!("请求头超时")
        }
        if stream.read(&mut byte)? == 0 {
            return Ok(());
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; 48];
    let mut req = httparse::Request::new(&mut headers);
    if !req.parse(&buf)?.is_complete() {
        return reply(&mut stream, 400, b"Invalid request", "text/plain");
    }
    let path = req.path.unwrap_or("");
    let parts: Vec<_> = path.split('/').collect();
    let auth = req
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("authorization"))
        .collect::<Vec<_>>();
    if req.method != Some("GET")
        || parts.len() < 4
        || parts[1] != "sync"
        || auth.len() != 1
        || req
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("origin"))
    {
        return reply(&mut stream, 403, b"Forbidden", "text/plain");
    }
    let id = parts[2];
    if blocklink_model::validate_uuid(id).is_err() {
        return reply(&mut stream, 404, b"Not found", "text/plain");
    }
    let cfg = engine.config(id);
    let token = cfg["shareToken"].as_str().unwrap_or("");
    if token.is_empty() || auth[0].value != format!("Bearer {token}").as_bytes() {
        return reply(&mut stream, 403, b"Forbidden", "text/plain");
    }
    let lock = match engine.publish_lock(id) {
        Ok(l) => l,
        Err(_) => return reply(&mut stream, 409, b"No published environment", "text/plain"),
    };
    if parts.len() == 4 && parts[3] == "manifest" {
        return reply(
            &mut stream,
            200,
            &serde_json::to_vec(&lock)?,
            "application/json",
        );
    }
    if parts.len() == 5 && parts[3] == "objects" {
        let hash = parts[4];
        if let Some(m) = lock
            .mods
            .iter()
            .find(|m| m.sha512 == hash && m.side != Side::Server)
        {
            let path = engine.ws.blob_path(hash)?;
            engine.ws.verify_blob(hash, m.bytes)?;
            let mut file = fs::File::open(path)?;
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/java-archive\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",m.bytes)?;
            std::io::copy(&mut file, &mut stream)?;
            stream.flush()?;
            return Ok(());
        }
    }
    reply(&mut stream, 404, b"Not found", "text/plain")
}
pub(super) fn invitation(engine: &Arc<Engine>, p: &Value) -> Result<Value> {
    let id = field(p, "id")?;
    let i = engine.ws.instance(id)?;
    if engine.config(id)["server"] != true {
        bail!("此实例不是托管服务器")
    };
    engine.publish_lock(id)?;
    let host = field(p, "host")?;
    let port: u16 = p["port"].as_u64().unwrap_or(25566).try_into()?;
    enable(engine, host, port)?;
    let mut cfg = engine.config(id);
    if cfg["shareToken"].as_str().unwrap_or("").is_empty() || p["rotate"] == true {
        cfg["shareToken"] = json!(format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4()));
        engine.settings.lock().unwrap()["instances"][id] = cfg.clone();
        engine.save_settings()?;
    }
    let identity = read_json(&engine.ws.root().join("host-identity.json"))?;
    let data = json!({"schemaVersion":1,"name":i.name,"endpoint":format!("https://{host}:{port}/sync/{id}"),"certificate":identity["certificate"],"token":cfg["shareToken"],"serverId":id});
    Ok(
        json!({"invitation":format!("blocklink:{}",URL_SAFE_NO_PAD.encode(serde_json::to_vec(&data)?)),"endpoint":data["endpoint"]}),
    )
}
fn parse(code: &str) -> Result<Value> {
    if code.len() > 32_768 {
        bail!("邀请过长")
    };
    let d: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD.decode(
            code.trim()
                .strip_prefix("blocklink:")
                .context("不是 Blocklink 邀请")?,
        )?,
    )?;
    if d["schemaVersion"] != 1 {
        bail!("不支持此邀请版本")
    };
    let u = reqwest::Url::parse(field(&d, "endpoint")?)?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        bail!("邀请必须使用 HTTPS")
    };
    blocklink_model::validate_uuid(field(&d, "serverId")?)?;
    if u.path() != format!("/sync/{}", field(&d, "serverId")?) {
        bail!("邀请路径无效")
    };
    if field(&d, "token")?.len() < 32 {
        bail!("无效配对凭据")
    };
    Ok(d)
}
fn client(d: &Value) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .use_rustls_tls()
        .tls_built_in_root_certs(false)
        .add_root_certificate(reqwest::Certificate::from_pem(
            field(d, "certificate")?.as_bytes(),
        )?)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .build()?)
}
fn fetch_lock(d: &Value) -> Result<Lockfile> {
    let response = client(d)?
        .get(format!("{}/manifest", field(d, "endpoint")?))
        .bearer_auth(field(d, "token")?)
        .send()?
        .error_for_status()?;
    let mut b = vec![];
    response.take(16_777_217).read_to_end(&mut b)?;
    if b.len() > 16_777_216 {
        bail!("服务器环境过大")
    };
    let l: Lockfile = serde_json::from_slice(&b)?;
    l.validate()?;
    Ok(l)
}
pub(super) fn join(engine: &Engine, p: &Value, report: &game::Reporter) -> Result<Value> {
    let invitation = field(p, "invitation")?;
    let d = parse(invitation)?;
    report("验证服务器身份与已发布环境".into());
    let lock = fetch_lock(&d)?;
    let id = uuid::Uuid::new_v4().to_string();
    let i: Instance = serde_json::from_value(
        json!({"schemaVersion":1,"instanceId":id,"name":p["name"].as_str().filter(|s|!s.trim().is_empty()).unwrap_or(d["name"].as_str().unwrap_or("服务器实例")),"minecraft":lock.environment.minecraft,"loader":lock.environment.loader,"runtime":{"java":"auto","memoryMiB":4096},"storage":{"linkMode":"auto"},"mods":[]}),
    )?;
    engine.ws.create_instance(&i)?;
    engine.settings.lock().unwrap()["instances"][&id] =
        json!({"server":false,"port":25565,"remoteInvitation":invitation,"remoteName":d["name"]});
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
    let mut lock = fetch_lock(&d)?;
    i.accepts(&lock)?;
    lock.mods.retain(|m| m.side != Side::Server);
    let c = client(&d)?;
    for m in &lock.mods {
        if engine.ws.verify_blob(&m.sha512, m.bytes).is_ok() {
            continue;
        }
        report(format!("同步服务器 Mod · {}", m.mod_id));
        let mut response = c
            .get(format!("{}/objects/{}", field(&d, "endpoint")?, m.sha512))
            .bearer_auth(field(&d, "token")?)
            .send()?
            .error_for_status()?;
        let mut f = tempfile::NamedTempFile::new_in(engine.ws.root().join("downloads"))?;
        let n = std::io::copy(&mut response.by_ref().take(m.bytes + 1), &mut f)?;
        f.flush()?;
        if n != m.bytes || hash_file(f.path(), "sha512")? != m.sha512 {
            bail!("服务器 Mod 校验失败")
        };
        engine.ws.import_jar(f.path())?;
    }
    for m in mods::lock(&engine.ws, i)?
        .mods
        .into_iter()
        .filter(|m| m.side == Side::Client)
    {
        if let Some(other) = lock.mods.iter().find(|x| x.mod_id == m.mod_id) {
            if other.sha512 != m.sha512 {
                bail!("客户端 Mod {} 与服务器冲突", m.mod_id)
            }
        } else {
            lock.mods.push(m)
        }
    }
    lock.validate()?;
    mods::apply(&engine.ws, i, &lock)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paired_https_transfers_only_published_mods_and_revokes_access() -> Result<()> {
        let host_dir = tempfile::tempdir()?;
        let client_dir = tempfile::tempdir()?;
        let host = Arc::new(Engine::new(host_dir.path())?);
        let client = Engine::new(client_dir.path())?;
        let report: game::Reporter = Arc::new(|_| {});
        let created=host.execute("create",&json!({"name":"测试服务器","minecraft":"1.21.1","loader":"fabric","loaderVersion":"0.19.5","server":true,"install":false}),report.clone())?;
        let id = field(&created, "id")?;
        let i = host.ws.instance(id)?;
        let jar = host_dir.path().join("testmod.jar");
        let mut ar = zip::ZipWriter::new(fs::File::create(&jar)?);
        ar.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())?;
        ar.write_all(br#"{"schemaVersion":1,"id":"testmod","version":"1.0.0","environment":"*"}"#)?;
        ar.finish()?;
        mods::local(&host.ws, &i, &jar, true)?;
        host.execute("publish", &json!({"id":id}), report.clone())?;
        let reservation = TcpListener::bind("127.0.0.1:0")?;
        let port = reservation.local_addr()?.port();
        drop(reservation);
        let invite = invitation(&host, &json!({"id":id,"host":"127.0.0.1","port":port}))?;
        let code = field(&invite, "invitation")?;
        let joined = join(&client, &json!({"invitation":code}), &report)?;
        let ci = client.ws.instance(field(&joined, "id")?)?;
        let l = mods::lock(&client.ws, &ci)?;
        assert_eq!(l.mods.len(), 1);
        client.ws.verify_instance(&ci.instance_id)?;
        let parsed = parse(code)?;
        assert_eq!(
            self::client(&parsed)?
                .get(format!("{}/manifest", field(&parsed, "endpoint")?))
                .send()?
                .status(),
            403
        );
        // A different certificate is not trusted even when the hostname is unchanged.
        let wrong = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()])?;
        let mut bad = parsed.clone();
        bad["certificate"] = json!(wrong.cert.pem());
        assert!(fetch_lock(&bad).is_err());
        host.settings.lock().unwrap()["instances"][id]["shareToken"] = Value::Null;
        assert!(sync(&client, &ci, code, &report).is_err());
        assert_eq!(
            mods::lock(&client.ws, &ci)?.mods[0].sha512,
            l.mods[0].sha512
        );
        Ok(())
    }
}
