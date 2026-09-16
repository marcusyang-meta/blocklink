//! Outbound HTTPS administration. The local RPC listener remains private.
use super::*;

fn base(value:&str)->Result<String>{
    let url=reqwest::Url::parse(value)?;
    anyhow::ensure!(url.scheme()=="https"&&url.host_str().is_some()&&url.username().is_empty()&&url.password().is_none()&&url.path()=="/"&&url.query().is_none()&&url.fragment().is_none(),"Expected an HTTPS lobby origin");
    Ok(url.to_string())
}
fn host_id(id:&str)->Result<&str>{
    anyhow::ensure!(id.len()==32&&id.bytes().all(|b|b.is_ascii_hexdigit()&&!b.is_ascii_uppercase()),"Invalid host ID");Ok(id)
}
fn http(origin:&str,path:&str,token:&str,payload:Option<&Value>)->Result<Value>{
    let client=reqwest::blocking::Client::builder().https_only(true).redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(20)).build()?;
    let url=format!("{}{path}",base(origin)?);
    let request=if let Some(p)=payload{client.post(url).json(p)}else{client.get(url)};
    let response=request.bearer_auth(token).send()?.error_for_status()?;
    let mut bytes=vec![];response.take(2_097_153).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len()<=2_097_152,"Management response too large");Ok(serde_json::from_slice(&bytes)?)
}
fn entry(engine:&Engine,id:&str)->Result<keyring::Entry>{
    use sha2::Digest;
    let scope=format!("{:x}",sha2::Sha256::digest(engine.ws.root().to_string_lossy().as_bytes()));
    Ok(keyring::Entry::new("Blocklink Managed Hosts",&format!("{scope}:{}",host_id(id)?))?)
}
pub(super) fn credentials(engine:&Engine,id:&str)->Result<Value>{
    Ok(serde_json::from_str(&entry(engine,id)?.get_password().context("Host credentials unavailable in the OS credential store")?)?)
}
pub(super) fn list(engine:&Engine)->Result<Value>{
    let path=engine.ws.root().join("managed-hosts.json");
    let mut hosts=if path.exists(){read_json(&path)?}else{json!([])};
    for host in hosts.as_array_mut().context("Invalid host registry")?{
        if let Some(id)=host["id"].as_str(){host["deployment"]=read_json(&engine.ws.root().join(format!("deployment-{id}.json"))).unwrap_or(Value::Null);}
    }
    Ok(hosts)
}
pub(super) fn register(engine:&Engine,p:&Value)->Result<Value>{
    let _gate=engine.gate.lock().unwrap();
    let url=base(engine.settings.lock().unwrap()["lobbyUrl"].as_str().context("Configure the lobby first")?)?;
    let id=uuid::Uuid::new_v4().simple().to_string();
    let secret=||format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple());
    let c=json!({"url":url,"ownerToken":secret(),"agentToken":secret()});
    // Save before enrollment, so retry uses the same remote identity.
    entry(engine,&id)?.set_password(&c.to_string())?;
    let record=json!({"id":id,"name":p["name"].as_str().unwrap_or("Linux server"),"url":url});
    let mut hosts=list(engine)?;hosts.as_array_mut().context("Invalid hosts")?.push(record.clone());
    write_json(&engine.ws.root().join("managed-hosts.json"),&hosts)?;
    enroll(engine,&id)?;Ok(record)
}
pub(super) fn enroll(engine:&Engine,id:&str)->Result<()>{
    let c=credentials(engine,id)?;let hosts=list(engine)?;
    let host=hosts.as_array().context("Invalid hosts")?.iter().find(|h|h["id"]==id).context("Host not registered")?;
    let key=keyring::Entry::new("Blocklink Lobby",field(&c,"url")?)?.get_password().context("Save the lobby registration credential first")?;
    http(field(&c,"url")?,"api/hosts",&key,Some(&json!({"id":id,"name":host["name"],"ownerToken":c["ownerToken"],"agentToken":c["agentToken"]})))?;Ok(())
}
pub(super) fn owner_request(engine:&Engine,action:&str,p:&Value)->Result<Value>{
    let id=host_id(field(p,"hostId")?)?;let c=credentials(engine,id)?;let path=format!("api/hosts/{id}");
    match action{
        "managed-status"=>http(field(&c,"url")?,&path,field(&c,"ownerToken")?,None),
        "managed-command"=>{uuid::Uuid::parse_str(field(p,"requestId")?)?;http(field(&c,"url")?,&format!("{path}/commands"),field(&c,"ownerToken")?,Some(&json!({"id":p["requestId"],"action":p["action"],"payload":p["payload"]})))},
        "managed-revoke"=>http(field(&c,"url")?,&format!("{path}/revoke"),field(&c,"ownerToken")?,Some(&json!({}))),
        _=>bail!("Unknown host operation"),
    }
}
pub(super) fn recover_jobs(root:&Path)->Result<Vec<Value>>{
    let path=root.join("jobs.json");
    let mut jobs:Vec<Value>=if path.exists(){serde_json::from_value(read_json(&path)?)?}else{vec![]};
    for j in &mut jobs{if ["queued","running"].contains(&j["status"].as_str().unwrap_or("")){
        j["status"]=json!("error");j["message"]=json!("Service restarted during this task. Check server state before retrying.");j["retryable"]=json!(false);j["cancelable"]=json!(false);
    }}
    write_json(&path,&jobs)?;
    for item in fs::read_dir(root)?{
        let path=item?.path();
        if path.file_name().is_some_and(|n|n.to_string_lossy().starts_with("deployment-")&&n.to_string_lossy().ends_with(".json")){
            let mut r=read_json(&path)?;if r["state"]=="running"{r["state"]=json!("error");r["stage"]=json!("Launcher service restarted during deployment; inspect host status before retrying");write_json(&path,&r)?;}
        }
    }
    Ok(jobs)
}
fn snapshot(engine:&Engine)->Result<Value>{
    let status=engine.status()?;
    let instances=status["instances"].as_array().context("Invalid status")?.iter().filter(|i|i["config"]["server"]==true).take(30).map(|i|json!({"instance":i["instance"],"running":i["running"],"installed":i["installed"],"port":i["config"]["port"],"published":i["published"]})).collect::<Vec<_>>();
    let jobs=status["jobs"].as_array().context("Invalid jobs")?.iter().rev().take(30).map(|j|json!({"id":j["id"],"instanceId":j["instanceId"],"action":j["action"],"status":j["status"],"message":j["message"].as_str().unwrap_or("").chars().take(512).collect::<String>(),"result":if ["create","world-backup","world-backup-restore"].contains(&j["action"].as_str().unwrap_or("")){j["result"].clone()}else{Value::Null}})).collect::<Vec<_>>();
    Ok(json!({"version":env!("CARGO_PKG_VERSION"),"instances":instances,"jobs":jobs}))
}
fn validate(engine:&Engine,action:&str,p:&mut Value)->Result<()>{
    let allowed=["create","install","launch","stop","console","logs","configure","publish","mod-add","mod-remove","mod-toggle","world-list","world-backups","world-backup","world-backup-restore","world-activate","verify","server-files","server-file-read","server-file-write","server-mods"];
    anyhow::ensure!(allowed.contains(&action)&&p.is_object(),"Action not permitted for a managed server");
    if action=="create"{anyhow::ensure!(engine.ws.instances()?.len()<30,"Managed host instance limit reached");p["server"]=json!(true);}
    else{let id=field(p,"id")?;engine.ws.instance(id)?;anyhow::ensure!(engine.config(id)["server"]==true,"Only server instances can be remotely administered");}
    for key in ["javaPath","serverId","peerInvitation","remoteInvitation","lobbyInvitation"]{anyhow::ensure!(p.get(key).is_none(),"Local executables and invitations are not remote command parameters");}
    Ok(())
}
fn config_path(root:&Path,relative:&str)->Result<PathBuf>{
    let path=safe_join(root,relative)?;
    anyhow::ensure!(["server.properties","whitelist.json","ops.json","banned-players.json","banned-ips.json","config"].contains(&relative.split('/').next().unwrap_or("")),"Only server settings and the config directory are editable");
    let mut current=root.to_path_buf();
    anyhow::ensure!(!fs::symlink_metadata(&current)?.file_type().is_symlink(),"Symlinked game directory is not allowed");
    for component in Path::new(relative).components(){current.push(component);if current.exists()||current.is_symlink(){anyhow::ensure!(!fs::symlink_metadata(&current)?.file_type().is_symlink(),"Symlinks are not allowed");}}
    Ok(path)
}
fn files(engine:&Engine,action:&str,p:&Value)->Result<Value>{
    let _gate=engine.gate.try_lock().map_err(|_|anyhow::anyhow!("Wait for the current host task before accessing files"))?;
    let id=field(p,"id")?;let instance=engine.ws.instance(id)?;
    if action=="server-mods"{return Ok(serde_json::to_value(mods::lock(&engine.ws,&instance)?.mods)?)}
    let root=engine.ws.instance_dir(id)?.join("game");let relative=p["path"].as_str().unwrap_or("");
    if action=="server-files"{
        let dir=if relative.is_empty(){root.clone()}else{config_path(&root,relative)?};let mut entries=vec![];
        if !dir.exists(){return Ok(json!({"path":relative,"entries":entries}))}
        for item in fs::read_dir(dir)?{let item=item?;let name=item.file_name().to_string_lossy().to_string();let child=if relative.is_empty(){name.clone()}else{format!("{relative}/{name}")};
            if config_path(&root,&child).is_err(){continue}let kind=item.file_type()?;
            if kind.is_file()||kind.is_dir(){entries.push(json!({"name":name,"path":child,"directory":kind.is_dir()}));}if entries.len()>=100{break}
        }return Ok(json!({"path":relative,"entries":entries}));
    }
    let path=config_path(&root,relative)?;
    let old=if path.exists(){anyhow::ensure!(path.is_file()&&fs::metadata(&path)?.len()<=16000,"Text settings editor is limited to 16 KB per file");fs::read(&path)?}else{vec![]};
    use sha2::Digest;let hash=format!("{:x}",sha2::Sha256::digest(&old));
    if action=="server-file-read"{anyhow::ensure!(path.exists(),"File not found");return Ok(json!({"path":relative,"text":String::from_utf8(old)?,"sha256":hash}));}
    engine.idle(id)?;anyhow::ensure!(field(p,"expectedSha256")?==hash,"File changed since it was opened; reload before saving");
    let text=field(p,"text")?;anyhow::ensure!(text.len()<=16000&&!text.contains('\0'),"Text settings editor is limited to 16 KB");
    let backup=engine.ws.instance_dir(id)?.join("managed-file-backups").join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&backup)?;fs::write(backup.join("original"),old)?;write_json(&backup.join("metadata.json"),&json!({"path":relative,"sha256":hash}))?;
    let mut temp=tempfile::NamedTempFile::new_in(path.parent().context("Invalid settings path")?)?;temp.write_all(text.as_bytes())?;temp.as_file().sync_all()?;temp.persist(&path).map_err(|e|e.error)?;
    Ok(json!({"path":relative,"sha256":format!("{:x}",sha2::Sha256::digest(text.as_bytes()))}))
}
fn execute_once(engine:&Arc<Engine>,command:&Value)->Result<Value>{
    let id=uuid::Uuid::parse_str(field(command,"id")?)?.to_string();let dir=engine.ws.root().join("managed-ledger");private_dir(&dir)?;let path=dir.join(format!("{id}.json"));
    if path.exists(){let previous=read_json(&path)?;return Ok(if previous["completion"].is_object(){previous["completion"].clone()}else{json!({"id":id,"ok":false,"error":"Interrupted during dispatch; inspect server state before creating a new request."})});}
    let action=field(command,"action")?;let mut p=command["payload"].clone();validate(engine,action,&mut p)?;
    // Record before dispatch. Never repeat an uncertain mutation after a crash.
    write_json(&path,&json!({"dispatching":true}))?;
    let result=if action.starts_with("server-file")||action=="server-mods"{files(engine,action,&p)}else{engine.dispatch(action,p)};
    let completion=match result{
        Ok(mut value)=>{if action=="logs"{value["text"]=json!(value["text"].as_str().unwrap_or("").chars().rev().take(8000).collect::<String>().chars().rev().collect::<String>());}
            if serde_json::to_vec(&value)?.len()>24000{json!({"id":id,"ok":false,"error":"Result too large for the management channel"})}else{json!({"id":id,"ok":true,"value":value})}},
        Err(error)=>json!({"id":id,"ok":false,"error":format!("{error:#}").chars().take(4096).collect::<String>()}),
    };
    write_json(&path,&json!({"completion":completion}))?;Ok(completion)
}
pub(super) fn start(engine:&Arc<Engine>)->Result<()>{
    let path=engine.ws.root().join("agent.json");if !path.exists(){return Ok(())}
    let config=read_json(&path)?;let url=base(field(&config,"url")?)?;let id=host_id(field(&config,"hostId")?)?.to_owned();let token=field(&config,"agentToken")?.to_owned();
    anyhow::ensure!(token.len()==64&&token.bytes().all(|b|b.is_ascii_hexdigit()),"Invalid agent credential");
    let weak=Arc::downgrade(engine);
    std::thread::spawn(move||{
        let mut completion=Value::Null;let mut delay=2;
        loop{let Some(engine)=weak.upgrade()else{break};if engine.shutting_down.load(std::sync::atomic::Ordering::SeqCst){break}
            let result=(||->Result<Value>{let mut snap=snapshot(&engine)?;
                while serde_json::to_vec(&snap)?.len()>28000{
                    if snap["jobs"].as_array().is_some_and(|a|!a.is_empty()){snap["jobs"].as_array_mut().unwrap().pop();}
                    else if snap["instances"].as_array().is_some_and(|a|!a.is_empty()){snap["instances"].as_array_mut().unwrap().pop();}else{break}
                    snap["truncated"]=json!(true);
                }
                http(&url,&format!("api/hosts/{id}/poll"),&token,Some(&json!({"snapshot":snap,"completion":completion})))
            })();
            match result{Ok(response)=>{completion=Value::Null;delay=2;if response["command"].is_object(){completion=execute_once(&engine,&response["command"]).unwrap_or_else(|e|json!({"id":response["command"]["id"],"ok":false,"error":format!("{e:#}").chars().take(4096).collect::<String>()}));}},Err(e)=>{eprintln!("Management channel unavailable: {e}");delay=(delay*2).min(30);}}
            drop(engine);std::thread::sleep(Duration::from_secs(delay));
        }
    });Ok(())
}
pub(super) fn shutdown_signal(root:&Path)->Result<Arc<std::sync::atomic::AtomicBool>>{
    let requested=Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(unix)] if root.join("agent.json").exists(){let rt=tokio::runtime::Builder::new_current_thread().enable_all().build()?;let flag=requested.clone();std::thread::spawn(move||rt.block_on(async move{if let Ok(mut signals)=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()){signals.recv().await;flag.store(true,std::sync::atomic::Ordering::SeqCst);}}));}
    #[cfg(not(unix))] let _=root;
    Ok(requested)
}
pub(super) fn shutdown(engine:&Engine){
    engine.shutting_down.store(true,std::sync::atomic::Ordering::SeqCst);let _gate=engine.gate.lock().unwrap();
    {let mut children=engine.children.lock().unwrap();for child in children.values_mut(){if let Some(stdin)=child.stdin.as_mut(){let _=stdin.write_all(b"stop\n");let _=stdin.flush();}}}
    let deadline=std::time::Instant::now()+Duration::from_secs(75);
    loop{let ids=engine.children.lock().unwrap().keys().cloned().collect::<Vec<_>>();if ids.iter().all(|id|!engine.is_running(id))||std::time::Instant::now()>deadline{break}std::thread::sleep(Duration::from_millis(200));}
}

#[cfg(test)] mod tests{
    use super::*;
    #[test] fn rejects_arbitrary_actions_and_client_instances()->Result<()>{
        let root=tempfile::tempdir()?;let e=Arc::new(Engine::new(root.path())?);
        assert!(base("http://example.com").is_err());assert!(base("https://user@example.com").is_err());assert!(validate(&e,"settings",&mut json!({})).is_err());assert!(validate(&e,"create",&mut json!({"javaPath":"/bin/sh"})).is_err());
        let mut p=json!({"name":"server","server":false});validate(&e,"create",&mut p)?;assert_eq!(p["server"],true);Ok(())
    }
    #[test] fn replay_does_not_create_duplicate_server_and_crash_is_uncertain()->Result<()>{
        let root=tempfile::tempdir()?;let e=Arc::new(Engine::new(root.path())?);let cmd=json!({"id":uuid::Uuid::new_v4().to_string(),"action":"create","payload":{"name":"Managed","minecraft":"1.21.1","loader":"vanilla","install":false}});
        let first=execute_once(&e,&cmd)?;assert_eq!(first["ok"],true);assert_eq!(execute_once(&e,&cmd)?,first);
        for _ in 0..100{if e.jobs.lock().unwrap().iter().all(|j|j["status"]=="done"){break}std::thread::sleep(Duration::from_millis(10));}assert_eq!(e.ws.instances()?.len(),1);
        let snap=snapshot(&e)?;assert!(snap.get("settings").is_none());assert!(snap.get("account").is_none());
        let id=uuid::Uuid::new_v4().to_string();write_json(&root.path().join("managed-ledger").join(format!("{id}.json")),&json!({"dispatching":true}))?;let mut retry=cmd;retry["id"]=json!(id);assert_eq!(execute_once(&e,&retry)?["ok"],false);Ok(())
    }
    #[test] fn settings_edits_are_confined_and_detect_stale_writes()->Result<()>{
        let root=tempfile::tempdir()?;let e=Arc::new(Engine::new(root.path())?);let created=e.execute("create",&json!({"name":"settings","minecraft":"1.21.1","loader":"vanilla","server":true,"install":false}),Arc::new(|_|{}))?;
        let id=field(&created,"id")?;let game=e.ws.instance_dir(id)?.join("game");fs::create_dir_all(&game)?;fs::write(game.join("server.properties"),"motd=Before\n")?;
        let read=files(&e,"server-file-read",&json!({"id":id,"path":"server.properties"}))?;
        assert!(files(&e,"server-file-read",&json!({"id":id,"path":"../settings.json"})).is_err());assert!(files(&e,"server-file-read",&json!({"id":id,"path":"/etc/passwd"})).is_err());
        assert!(files(&e,"server-file-write",&json!({"id":id,"path":"server.properties","expectedSha256":"stale","text":"bad"})).is_err());
        files(&e,"server-file-write",&json!({"id":id,"path":"server.properties","expectedSha256":read["sha256"],"text":"motd=After\n"}))?;
        assert_eq!(fs::read_to_string(game.join("server.properties"))?,"motd=After\n");assert_eq!(fs::read_dir(e.ws.instance_dir(id)?.join("managed-file-backups"))?.count(),1);
        #[cfg(unix)] {std::os::unix::fs::symlink(root.path(),game.join("config"))?;assert!(files(&e,"server-file-read",&json!({"id":id,"path":"config/settings.json"})).is_err());}Ok(())
    }
    #[test] fn restart_marks_unfinished_jobs_without_replaying()->Result<()>{let root=tempfile::tempdir()?;write_json(&root.path().join("jobs.json"),&json!([{"id":"x","status":"running","retryable":true}]))?;let jobs=recover_jobs(root.path())?;assert_eq!(jobs[0]["status"],"error");assert_eq!(jobs[0]["retryable"],false);Ok(())}
}
