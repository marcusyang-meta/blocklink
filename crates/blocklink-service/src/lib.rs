pub mod auth;
mod forge;
pub mod game;
mod installer;
#[cfg(test)]
mod loader_acceptance;
mod lobby;
pub mod mods;
mod neoforge;
pub mod net;
mod peer;
mod quilt;
mod remote;
mod rtc;
mod trash;
mod worlds;
mod preflight;
mod shaders;
mod auto_launch;
mod home;
mod transfers;
mod packs;
mod content;
mod mod_catalog;
use anyhow::{bail, Context, Result};
use blocklink_core::Workspace;
use blocklink_model::{Instance, Loader, Lockfile, Side};
use net::*;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

pub fn default_root() -> PathBuf {
    directories::ProjectDirs::from("app", "Blocklink", "Blocklink")
        .expect("用户数据目录不可用")
        .data_local_dir()
        .to_path_buf()
}
fn private_dir(root: &Path) -> Result<()> {
    fs::create_dir_all(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
struct Engine {
    shutting_down: std::sync::atomic::AtomicBool,
    ws: Workspace,
    settings: Mutex<Value>,
    jobs: Mutex<Vec<Value>>,
    children: Mutex<HashMap<String, Child>>,
    gate: Mutex<()>,
    remote_port: Mutex<Option<u16>>,
    peer: Mutex<Option<Arc<peer::Peer>>>,
    lobby: Mutex<HashMap<String, Arc<lobby::Session>>>,
}
impl Engine {
    fn new(root: &Path) -> Result<Self> {
        private_dir(root)?;
        let ws = Workspace::open(root)?;
        ws.recover_all()?;
        mods::recover_environments(&ws)?;
        for instance in ws.instances()? {content::recover(&ws.instance_dir(&instance.instance_id)?)?;}
        let mut settings = if root.join("settings.json").exists() {
            read_json(&root.join("settings.json"))?
        } else {
            json!({"clientId":auth::product_client_id(),"instances":{}})
        };
        // Existing installations also adopt the product ID when upgrading.
        if settings["clientId"].as_str().unwrap_or("").is_empty() {
            let bundled=auth::product_client_id();
            if uuid::Uuid::parse_str(bundled).is_ok(){settings["clientId"]=json!(bundled);}
        }
        if settings["lobbyUrl"].as_str().is_none() {
            settings["lobbyUrl"] = json!("https://blocklink-lobby.junjie-f33.workers.dev/");
        }
        trash::reconcile(&ws, &mut settings)?;
        write_json(&root.join("settings.json"), &settings)?;
        Ok(Self {
            shutting_down: std::sync::atomic::AtomicBool::new(false),
            ws,
            settings: Mutex::new(settings),
            jobs: Mutex::new(vec![]),
            children: Mutex::new(HashMap::new()),
            gate: Mutex::new(()),
            remote_port: Mutex::new(None),
            peer: Mutex::new(None),
            lobby: Mutex::new(HashMap::new()),
        })
    }
    fn config(&self, id: &str) -> Value {
        self.settings.lock().unwrap()["instances"][id].clone()
    }
    fn active_account(&self, refresh: bool) -> Result<Value> {
        let settings = self.settings.lock().unwrap().clone();
        if settings["authMode"] == "offline" {
            return auth::offline(field(&settings, "offlineName")?);
        }
        if refresh {
            auth::account(self.ws.root())
        } else {
            Ok(auth::status(self.ws.root()))
        }
    }
    fn save_settings(&self) -> Result<()> {
        write_json(
            &self.ws.root().join("settings.json"),
            &*self.settings.lock().unwrap(),
        )
    }
    fn is_running(&self, id: &str) -> bool {
        let mut c = self.children.lock().unwrap();
        if let Some(p) = c.get_mut(id) {
            let Some(status) = p.try_wait().ok().flatten() else {
                return true;
            };
            if !status.success() {
                self.jobs.lock().unwrap().push(json!({"id":uuid::Uuid::new_v4().to_string(),"instanceId":id,"action":"运行进程","status":"error","message":format!("进程已退出（{status}），请打开实例的运行日志查看原因。")}));
            }
            c.remove(id);
            drop(c);
            if let Some(peer) = self.peer.lock().unwrap().as_ref() {
                peer.close_bridge(id);
            }
        }
        false
    }
    fn idle(&self, id: &str) -> Result<()> {
        if self.is_running(id) {
            bail!("请先停止游戏或服务器，再修改安装内容")
        }
        Ok(())
    }
    fn status(&self) -> Result<Value> {
        let instances=self.ws.instances()?.into_iter().map(|i|{let id=&i.instance_id;let dir=self.ws.instance_dir(id)?;let lock=mods::lock(&self.ws,&i)?;let receipt=read_json(&dir.join("receipt.json")).unwrap_or(Value::Null);let running=self.is_running(id);Ok(json!({"instance":i,"config":self.config(id),"installed":read_json(&dir.join("installed.json")).is_ok_and(|v|v["minecraft"]==i.minecraft && v["loader"]==json!(i.loader)),"running":running,"mods":lock.mods,"pack":read_json(&dir.join("pack.json")).unwrap_or(Value::Null),"receipt":receipt,"published":dir.join("published.json").exists()}))}).collect::<Result<Vec<_>>>()?;
        let mut bytes = 0u64;
        let mut count = 0usize;
        for e in fs::read_dir(self.ws.root().join("store/sha512"))? {
            let e = e?;
            if e.file_type()?.is_dir() {
                for b in fs::read_dir(e.path())? {
                    let b = b?;
                    if b.file_type()?.is_file() {
                        bytes += b.metadata()?.len();
                        count += 1;
                    }
                }
            } else if e.file_type()?.is_file() {
                bytes += e.metadata()?.len();
                count += 1;
            }
        }
        Ok(
            json!({"instances":instances,"jobs":*self.jobs.lock().unwrap(),"account":self.active_account(false).map(|a| if a.is_null() { a } else { auth::public(&a) }).unwrap_or(Value::Null),"settings":*self.settings.lock().unwrap(),"root":self.ws.root(),"platform":format!("{} / {}",game::os(),game::arch()),"store":{"bytes":bytes,"count":count}}),
        )
    }
    fn job(self: &Arc<Self>, action: String, mut payload: Value) -> Value {
        if ["pack-install","pack-import"].contains(&action.as_str()) && payload["newId"].is_null(){payload["newId"]=json!(uuid::Uuid::new_v4().to_string());}
        let controllable=["install","launch","pack-install","pack-import"].contains(&action.as_str());
        let id = uuid::Uuid::new_v4().to_string();
        {
            let mut jobs = self.jobs.lock().unwrap();
            if self.shutting_down.load(std::sync::atomic::Ordering::SeqCst){jobs.push(json!({"id":id,"action":action,"status":"error","message":"The launcher is restarting for an update"}));return json!({"jobId":id});}
            if jobs.len() > 80 {
                jobs.retain(|v| v["status"] == "running" || v["status"] == "queued");
            }
            jobs.push(json!({"id":id,"action":action,"instanceId":payload.get("id").or_else(||payload.get("newId")),"payload":if controllable {payload.clone()}else{Value::Null},"retryable":controllable,"cancelable":controllable,"status":"queued","message":"等待执行","started":auth::now()}));
        }
        let e = self.clone();
        let jid = id.clone();
        std::thread::spawn(move || {
            let _gate = e.gate.lock().unwrap();
            let weak_cancel=Arc::downgrade(&e);let cancel_id=jid.clone();
            let weak_event=Arc::downgrade(&e);let event_id=jid.clone();
            transfers::attach(transfers::Context{cancelled:Arc::new(move||weak_cancel.upgrade().is_some_and(|e|e.jobs.lock().unwrap().iter().any(|j|j["id"]==cancel_id&&j["cancelRequested"]==true))),event:Arc::new(move|value|{if let Some(e)=weak_event.upgrade(){if let Some(j)=e.jobs.lock().unwrap().iter_mut().find(|j|j["id"]==event_id){for (key,value) in value.as_object().unwrap(){j[key]=value.clone();}}}})});
            if transfers::check().is_err(){e.update_job(&jid,"cancelled","已取消",Value::Null);return}
            e.update_job(&jid, "running", "开始处理", Value::Null);
            let weak = Arc::downgrade(&e);
            let progress_id = jid.clone();
            let report: game::Reporter = Arc::new(move |message| {
                if let Some(e) = weak.upgrade() {
                    transfers::event(json!({"download":null})); e.update_job(&progress_id, "running", &message, Value::Null)
                }
            });
            match e.execute(&action, &payload, report) {
                Ok(v) => e.update_job(&jid, "done", "已完成", v),
                Err(error) => {
                    let recovery=if action=="launch" {payload["id"].as_str().map(|id|auto_launch::finish(&e,id,true)).transpose()}else{Ok(None)};
                    let message=match recovery {Ok(_)=>format!("{error:#}"),Err(r)=>format!("{error:#}；恢复先前环境时出错：{r:#}")};
                    e.update_job(&jid,if error.is::<transfers::Cancelled>() {"cancelled"} else {"error"},&message,Value::Null)
                },
            }
        });
        json!({"jobId":id})
    }
    fn update_job(&self, id: &str, status: &str, message: &str, result: Value) {
        if let Some(j) = self.jobs.lock().unwrap().iter_mut().find(|j| j["id"] == id) {
            j["status"] = json!(status);
            j["message"] = json!(message);
            j["result"] = result;
        }
    }
    fn publish_lock(&self, id: &str) -> Result<Lockfile> {
        self.ws.instance(id)?;
        let dir = self.ws.instance_dir(id)?;
        let file = dir.join(if self.is_running(id) {
            "running-lock.json"
        } else {
            "published.json"
        });
        let l: Lockfile = serde_json::from_value(
            read_json(&file).context("服务器尚未发布 Mod 环境，请在服务器页面发布")?,
        )?;
        l.validate()?;
        Ok(l)
    }
    fn sync_binding(self: &Arc<Self>, i: &Instance, report: &game::Reporter) -> Result<()> {
        let cfg = self.config(&i.instance_id);
        if let Some(invite) = cfg["lobbyInvitation"].as_str() {
            return lobby::sync(self, i, invite, report);
        }
        if let Some(invite) = cfg["peerInvitation"].as_str() {
            return peer::sync(self, i, invite, report);
        }
        if let Some(invite) = cfg["remoteInvitation"].as_str() {
            return remote::sync(self, i, invite, report);
        }
        let Some(server) = cfg["serverId"].as_str().filter(|s| !s.is_empty()) else {
            return Ok(());
        };
        report("检查服务器已发布的 Mod 更新".into());
        let mut target = self.publish_lock(server)?;
        i.accepts(&target)?;
        let existing = mods::lock(&self.ws, i)?;
        target.mods.retain(|m| m.side != Side::Server);
        for m in existing.mods.into_iter().filter(|m| m.side == Side::Client) {
            if let Some(other) = target.mods.iter().find(|x| x.mod_id == m.mod_id) {
                if other.sha512 != m.sha512 {
                    bail!("客户端 Mod {} 与服务器发布版本冲突", m.mod_id)
                }
            } else {
                target.mods.push(m)
            }
        }
        target.validate()?;
        content::apply(self,&i.instance_id,target.content.as_ref())?;
        mods::apply(&self.ws, i, &target)?;
        report("服务器 Mod 已同步".into());
        Ok(())
    }
    fn execute(self: &Arc<Self>, action: &str, p: &Value, report: game::Reporter) -> Result<Value> {
        if ["instance-delete", "instance-restore"].contains(&action) {
            return trash::execute(self, action, p);
        }
        if ["world-import", "world-deploy", "world-activate"].contains(&action) {
            return worlds::execute(self, action, p, report);
        }
        if action == "lobby-create" {
            return lobby::create(self, field(p, "id")?);
        }
        if action == "remote-join" {
            if p["invitation"].as_str().is_some_and(|s| {
                s.trim().starts_with("https://") || s.trim().starts_with("http://")
            }) {
                return lobby::join(self, p, &report);
            }
            if p["invitation"]
                .as_str()
                .unwrap_or("")
                .trim()
                .starts_with("blocklink-peer:")
            {
                return peer::join(self, p, &report);
            }
            return remote::join(self, p, &report);
        }
        if action=="pack-export" {return packs::export(self,p,report);}
        if ["pack-install","pack-import"].contains(&action){return packs::install(self,p,report);}
        if action == "create" {
            if p["server"] == true && !(1024..=65535).contains(&p["port"].as_u64().unwrap_or(25565))
            {
                bail!("端口范围为 1024–65535")
            }
            let version = p["minecraft"].as_str().unwrap_or("1.21.1");
            blocklink_model::validate_version(version, true)?;
            let kind = p["loader"].as_str().unwrap_or("vanilla");
            let loader = if ["fabric", "neoforge", "forge", "quilt"].contains(&kind) {
                let selected = p["loaderVersion"].as_str().filter(|s| !s.is_empty());
                let version = if let Some(v) = selected {
                    v.to_owned()
                } else {
                    loader_versions(kind, version)?
                        .as_array()
                        .and_then(|a| a.iter().find(|v| v["loader"]["stable"] == true))
                        .and_then(|v| v["loader"]["version"].as_str())
                        .context("此游戏版本没有稳定的所选 Loader，请换版本或明确选择测试版")?
                        .to_owned()
                };
                match kind {
                    "neoforge" => Loader::NeoForge { version },
                    "forge" => Loader::Forge { version },
                    "quilt" => Loader::Quilt { version },
                    _ => Loader::Fabric { version },
                }
            } else {
                if kind != "vanilla" {
                    bail!("不支持此 Loader")
                }
                Loader::Vanilla
            };
            let id = uuid::Uuid::new_v4().to_string();
            let i: Instance = serde_json::from_value(
                json!({"schemaVersion":1,"instanceId":id,"name":field(p,"name")?,"minecraft":version,"loader":loader,"runtime":{"java":"auto","memoryMiB":p["memory"].as_u64().unwrap_or(4096)},"storage":{"linkMode":"auto"},"mods":[]}),
            )?;
            self.ws.create_instance(&i)?;
            let server = p["server"] == true;
            self.settings.lock().unwrap()["instances"][&id] = json!({"server":server,"port":p["port"].as_u64().unwrap_or(25565),"javaPath":"","serverId":""});
            self.save_settings()?;
            if p["install"] != false {
                game::install(self.ws.root(), &i, server, None, report)?;
            }
            return Ok(json!({"id":id}));
        }
        if action == "settings" {
            let id = p["clientId"].as_str().unwrap_or("");
            if !id.is_empty() {
                uuid::Uuid::parse_str(id)?;
            }
            self.settings.lock().unwrap()["clientId"] = json!(id);
            self.save_settings()?;
            return Ok(json!({"saved":true}));
        }
        if action == "offline-profile" {
            let account = auth::offline(field(p, "name")?)?;
            {
                let mut settings = self.settings.lock().unwrap();
                settings["offlineName"] = account["name"].clone();
                settings["authMode"] = json!("offline");
            }
            self.save_settings()?;
            return Ok(auth::public(&account));
        }
        if action == "logout" {
            if self.settings.lock().unwrap()["authMode"] == "offline" {
                self.settings.lock().unwrap()["authMode"] = json!("microsoft");
                self.save_settings()?;
                return Ok(json!({}));
            }
            auth::logout(self.ws.root())?;
            return Ok(json!({}));
        }
        let id = field(p, "id")?;
        let mut i = self.ws.instance(id)?;
        let dir = self.ws.instance_dir(id)?;
        let config = self.config(id);
        let server = config["server"] == true;
        match action {
            "shader-check" => {self.idle(id)?;anyhow::ensure!(!server,"服务器不加载光影");shaders::prepare(&self.ws,&i,field(p,"file")?,report)},
            "shader-apply" => {self.idle(id)?;shaders::apply(&self.ws,&i,field(p,"planId")?)},
            "shader-select" => {self.idle(id)?;shaders::select(&self.ws,&i,field(p,"file")?,p["enabled"]==true)},
            "shader-restore" => {self.idle(id)?;shaders::restore(&self.ws,&i,field(p,"snapshotId")?)},
            "install" => {
                self.idle(id)?;
                game::install(
                    self.ws.root(),
                    &i,
                    server,
                    config["javaPath"].as_str(),
                    report,
                )
            }
            "configure" => {
                self.idle(id)?;
                if let Some(n) = p["name"].as_str() {
                    i.name = n.into()
                }
                if let Some(m) = p["memory"].as_u64() {
                    i.runtime.memory_mi_b = m.try_into()?
                }
                i.validate()?;
                let port = p["port"].as_u64().unwrap_or(25565);
                if !(1024..=65535).contains(&port) {
                    bail!("端口范围为 1024–65535")
                }
                let binding = p["serverId"].as_str().unwrap_or("");
                if !binding.is_empty() {
                    let si = self.ws.instance(binding)?;
                    if self.config(binding)["server"] != true
                        || i.minecraft != si.minecraft
                        || i.loader != si.loader
                    {
                        bail!("请选择游戏版本与 Loader 完全一致的服务器")
                    }
                }
                {
                    let mut s = self.settings.lock().unwrap();
                    let c = &mut s["instances"][id];
                    c["port"] = json!(port);
                    c["javaPath"] = json!(p["javaPath"].as_str().unwrap_or(""));
                    c["serverId"] = json!(binding);
                    if server && p["shareContent"].is_boolean(){c["shareContent"]=p["shareContent"].clone();}
                }
                write_json(&dir.join("instance.json"), &i)?;
                self.save_settings()?;
                Ok(json!({}))
            }
            "mod-add" => {
                self.idle(id)?;
                mods::add(
                    &self.ws,
                    &i,
                    field(p, "project")?,
                    p["version"].as_str(),
                    server,
                    report,
                )
            }
            "mod-local" => {
                self.idle(id)?;
                mods::local(&self.ws, &i, Path::new(field(p, "path")?), server)
            }
            "mod-remove" => {
                self.idle(id)?;
                mods::remove(&self.ws, &i, field(p, "modId")?)
            }
            "mod-compatibility-check" => {
                self.idle(id)?;
                if ["serverId","lobbyInvitation","peerInvitation","remoteInvitation"].iter().any(|k|config[*k].as_str().is_some_and(|s|!s.is_empty())) {bail!("绑定服务器的实例请先由服务器适配，再同步环境");}
                mods::prepare_compatibility(&self.ws,&i,server,p["allowDisable"]==true,&p["decisions"],report)
            }
            "mod-update-check" => {
                self.idle(id)?;
                mods::prepare_updates(&self.ws,&i,server,report)
            }
            "mod-update-apply" => {
                self.idle(id)?;
                worlds::backup_all(self,id,"Mod 更新前",&report)?;
                mods::apply_updates(&self.ws,&i,field(p,"planId")?)
            }
            "mod-update-restore" => {
                self.idle(id)?;
                worlds::backup_all(self,id,"Mod 回滚前",&report)?;
                mods::restore_snapshot(&self.ws,&i,field(p,"snapshotId")?)
            }
            "mod-toggle" => {
                self.idle(id)?;
                mods::toggle(&self.ws,&i,field(p,"modId")?,p["enabled"].as_bool().context("缺少启用状态")?)
            }
            "verify" => {
                if self.ws.read_lock(id)?.is_none() {
                    Ok(json!({"mods":0,"message":"此实例没有已管理的 Mod"}))
                } else {
                    Ok(serde_json::to_value(self.ws.verify_instance(id)?)?)
                }
            }
            "sync" => {
                self.idle(id)?;
                self.sync_binding(&i, &report)?;
                Ok(json!({}))
            }
            "publish" => {
                if !server {
                    bail!("只有托管服务器可以发布环境")
                };
                self.idle(id)?;
                let mut lock = mods::lock(&self.ws, &i)?;
                lock.content=content::publish(self,id)?;
                if !lock.mods.is_empty() {
                    self.ws.verify_instance(id)?;
                }
                write_json(&dir.join("published.json"), &lock)?;
                Ok(json!({"mods":lock.mods.len()}))
            }
            "launch" => {
                self.idle(id)?;
                if config["deploymentStatus"]
                    .as_str()
                    .is_some_and(|s| s != "ready")
                {
                    bail!("世界部署尚未完成，请回到源世界重新部署；此实例禁止启动以免生成空世界")
                }
                if server
                    && p["eula"] != true
                    && !fs::read_to_string(dir.join("game/eula.txt"))
                        .unwrap_or_default()
                        .lines()
                        .any(|l| l.trim() == "eula=true")
                {
                    bail!("请先阅读并同意 Minecraft EULA")
                }
                let wait_for_sync=!server && ["serverId","lobbyInvitation","peerInvitation","remoteInvitation"].iter().any(|k|config[*k].as_str().is_some_and(|s|!s.is_empty()));
                if !server && !wait_for_sync {if let Some(prompt)=auto_launch::prepare(self,&mut i,p,report.clone(),false)? {return Ok(prompt);}}
                report("启动前检查 Mods 与依赖".into());
                if !wait_for_sync {preflight::require_ready(&self.ws,&i,None)?;}
                let account = if server {
                    Value::Null
                } else {
                    report("检查玩家档案".into());
                    self.active_account(true)?
                };
                let installed = if dir.join("installed.json").exists() && read_json(&dir.join("installed.json")).is_ok_and(|v|v["minecraft"]==i.minecraft && v["loader"]==json!(i.loader)) {
                    read_json(&dir.join("installed.json"))?
                } else {
                    game::install(
                        self.ws.root(),
                        &i,
                        server,
                        config["javaPath"].as_str(),
                        report.clone(),
                    )?
                };
                let java = game::java(
                    self.ws.root(),
                    installed["major"].as_u64().unwrap_or(21),
                    config["javaPath"].as_str(),
                    &report,
                    game::runtime_arch(if server { &installed } else { &installed["meta"] }, game::os(), game::arch(), server),
                )?;
                if !wait_for_sync {preflight::require_ready(&self.ws,&i,installed["major"].as_u64())?;}
                fs::create_dir_all(dir.join("game"))?;
                let args = if server {
                    let port = config["port"].as_u64().unwrap_or(25565);
                    if self.children.lock().unwrap().keys().any(|other| {
                        other != id
                            && self.config(other)["server"] == true
                            && self.config(other)["port"].as_u64().unwrap_or(25565) == port
                    }) {
                        bail!("此端口已由另一个服务器使用")
                    }
                    if p["eula"] == true {
                        fs::write(dir.join("game/eula.txt"), "eula=true\n")?;
                    }
                    let props = dir.join("game/server.properties");
                    let old = fs::read_to_string(&props).unwrap_or_default();
                    let mut lines: Vec<_> = old
                        .lines()
                        .filter(|l| {
                            !l.starts_with("server-port=")
                                && !l.starts_with("online-mode=")
                                && !(config["worldFolder"].is_string()
                                    && l.starts_with("level-name="))
                                && !(config["onlineMode"] == false && l.starts_with("server-ip="))
                        })
                        .map(String::from)
                        .collect();
                    lines.push(format!("server-port={port}"));
                    lines.push(format!("online-mode={}", config["onlineMode"] != false));
                    if config["onlineMode"] == false {
                        lines.push("server-ip=127.0.0.1".into());
                    }
                    if let Some(folder) = config["worldFolder"].as_str() {
                        lines.push(format!("level-name={folder}"));
                    }
                    fs::write(props, lines.join("\n") + "\n")?;
                    let mut lock = mods::lock(&self.ws, &i)?;
                    lock.content=content::publish(self,id)?;
                    write_json(&dir.join("running-lock.json"), &lock)?;
                    write_json(&dir.join("published.json"), &lock)?;
                    if let Some(file) = installed["serverArgsFile"].as_str() {
                        vec![
                            format!("-Xmx{}M", i.runtime.memory_mi_b),
                            format!("@{}", game::process_path(Path::new(file)).to_string_lossy()),
                            "nogui".into(),
                        ]
                    } else {
                        vec![
                            format!("-Xmx{}M", i.runtime.memory_mi_b),
                            "-jar".into(),
                            game::process_path(Path::new(field(&installed, "serverJar")?))
                                .to_string_lossy()
                                .into_owned(),
                            "nogui".into(),
                        ]
                    }
                } else {
                    self.sync_binding(&i, &report)?;
                    if wait_for_sync {if let Some(prompt)=auto_launch::prepare(self,&mut i,p,report.clone(),true)? {return Ok(prompt);}}
                    let lock = mods::lock(&self.ws, &i)?;
                    if !lock.mods.is_empty() {
                        self.ws.verify_instance(id)?;
                    }
                    preflight::require_ready(&self.ws,&i,installed["major"].as_u64())?;
                    let mut args = game::launch_args(self.ws.root(), &i, &installed, &account)?;
                    if let Some(invite) = config["lobbyInvitation"].as_str() {
                        let port = lobby::game_port(self, id, invite)?;
                        args.extend(game::multiplayer_args(&i.minecraft, port));
                    } else if let Some(invite) = config["peerInvitation"].as_str() {
                        report("建立游戏联机通道".into());
                        let port = peer::game_port(self, id, invite)?;
                        args.extend(game::multiplayer_args(&i.minecraft, port));
                    } else if let Some(server_id) =
                        config["serverId"].as_str().filter(|s| !s.is_empty())
                    {
                        let port = self.config(server_id)["port"].as_u64().unwrap_or(25565);
                        args.extend(game::multiplayer_args(&i.minecraft, port.try_into()?));
                    }
                    args
                };
                let log = fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(dir.join("latest.log"))?;
                transfers::check()?;
                transfers::event(json!({"cancelable":false}));
                let child = hidden(
                    Command::new(java)
                        .args(args)
                        .current_dir(game::process_path(&dir.join("game")))
                        .stdin(Stdio::piped())
                        .stdout(Stdio::from(log.try_clone()?))
                        .stderr(Stdio::from(log)),
                )
                .spawn()?;
                let pid = child.id();
                self.children.lock().unwrap().insert(id.into(), child);
                if !server {let _=auto_launch::finish(self,id,false);let _=home::record(self,id);}
                report(
                    if server {
                        "服务器已启动，正在加载世界"
                    } else {
                        "游戏已启动"
                    }
                    .into(),
                );
                Ok(json!({"pid":pid}))
            }
            "stop" => {
                let mut children = self.children.lock().unwrap();
                let child = children.get_mut(id).context("进程未运行")?;
                if server && p["force"] != true {
                    child
                        .stdin
                        .as_mut()
                        .context("服务器控制台不可用")?
                        .write_all(b"stop\n")?;
                    Ok(json!({"stopping":true}))
                } else {
                    child.kill()?;
                    child.wait()?;
                    children.remove(id);
                    Ok(json!({"stopped":true}))
                }
            }
            "console" => {
                if !server {
                    bail!("仅服务器支持控制台命令")
                };
                let command = field(p, "command")?;
                if command.len() > 2048 || command.contains(['\n', '\r']) {
                    bail!("每次只允许一条命令")
                };
                self.children
                    .lock()
                    .unwrap()
                    .get_mut(id)
                    .context("服务器未运行")?
                    .stdin
                    .as_mut()
                    .context("控制台不可用")?
                    .write_all(format!("{command}\n").as_bytes())?;
                Ok(json!({}))
            }
            "world-backup" => worlds::backup_all(self,id,"手动备份",&report),
            "world-backup-restore" => worlds::restore_backup(self,id,field(p,"backupId")?,&report),
            "duplicate" => {
                self.idle(id)?;
                let mut new = i.clone();
                new.instance_id = uuid::Uuid::new_v4().to_string();
                new.name = format!("{} 副本", i.name);
                new.server = None;
                self.ws.create_instance(&new)?;
                let lock = mods::lock(&self.ws, &i)?;
                mods::apply(&self.ws, &new, &lock)?;
                self.settings.lock().unwrap()["instances"][&new.instance_id] =
                    json!({"server":server,"port":25565,"javaPath":"","serverId":"","onlineMode":config["onlineMode"],"worldFolder":config["worldFolder"],"deploymentStatus":"preparing"});
                self.save_settings()?;
                if p["includeWorlds"] == true {worlds::copy_content(self,id,&new.instance_id,&report)?;}
                self.settings.lock().unwrap()["instances"][&new.instance_id]["deploymentStatus"]=json!("ready");
                self.save_settings()?;
                Ok(json!({"id":new.instance_id}))
            }
            _ => bail!("未知操作"),
        }
    }
    fn dispatch(self: &Arc<Self>, action: &str, p: Value) -> Result<Value> {
        if self.shutting_down.load(std::sync::atomic::Ordering::SeqCst){bail!("The launcher is restarting for an update")}
        if action=="prepare-app-update" {
            let _gate=self.gate.try_lock().map_err(|_|anyhow::anyhow!("Wait for current tasks to finish before updating"))?;
            let jobs=self.jobs.lock().unwrap();
            if jobs.iter().any(|j|j["status"]=="queued"||j["status"]=="running")||self.ws.instances()?.iter().any(|i|self.is_running(&i.instance_id)){bail!("Close running games and servers and finish downloads before updating")}
            self.shutting_down.store(true,std::sync::atomic::Ordering::SeqCst);
            return Ok(json!({"ready":true}));
        }
        if (action == "remote-join"
            && p["invitation"]
                .as_str()
                .unwrap_or("")
                .trim()
                .starts_with("blocklink-peer:"))
            || (["launch", "sync"].contains(&action)
                && p["id"]
                    .as_str()
                    .is_some_and(|id| self.config(id)["peerInvitation"].is_string()))
        {
            peer::ensure(self)?;
        }
        match action {
            "shader-state" => {let i=self.ws.instance(field(&p,"id")?)?;shaders::state(&self.ws,&i)},
            "preflight" => {let i=self.ws.instance(field(&p,"id")?)?;preflight::inspect(&self.ws,&i,None)},
            "world-backups" => worlds::backups(self,field(&p,"id")?),
            "pack-search" => packs::search(&p),
            "pack-categories" => packs::categories(),
            "installed-mod-categories" => mod_catalog::list(self,&p),
            "pack-versions" => packs::versions(field(&p,"project")?),
            "pack-preview" => packs::preview(Path::new(field(&p,"path")?)),
            "job-cancel" => {let id=field(&p,"jobId")?;let mut jobs=self.jobs.lock().unwrap();let j=jobs.iter_mut().find(|j|j["id"]==id).context("任务不存在")?;if j["cancelable"]!=true || !["queued","running"].contains(&j["status"].as_str().unwrap_or("")){bail!("此步骤已经完成或暂不能取消")};j["cancelRequested"]=json!(true);Ok(json!({"requested":true}))},
            "job-retry" => {let j={let mut jobs=self.jobs.lock().unwrap();let j=jobs.iter_mut().find(|j|j["id"]==p["jobId"]).context("任务不存在")?;if j["retryable"]!=true || !["error","cancelled"].contains(&j["status"].as_str().unwrap_or("")){bail!("此任务已经重试或不能重试")};let copy=j.clone();j["retryable"]=json!(false);copy};let mut payload=j["payload"].clone();if j["action"]=="launch"{if let Some(o)=payload.as_object_mut(){o.remove("acceptPlan");o.remove("continueWithoutShader");}}Ok(self.job(field(&j,"action")?.into(),payload))},
            "home-summary" => home::summary(self),
            "home-art" => home::artwork(self,field(&p,"id")?),
            "world-list" => worlds::list(self, field(&p, "id")?),
            "world-scan" => worlds::scan(Path::new(field(&p, "path")?)),
            "lobby-status" => Ok(lobby::status(self)),
            "lobby-configure" => lobby::configure(self, &p),
            "lobby-close" => lobby::close(self, field(&p, "id")?),
            "peer-invite" => {
                let _gate = self.gate.lock().unwrap();
                peer::invitation(self, &p)
            }
            "peer-status" => Ok(self
                .peer
                .lock()
                .unwrap()
                .as_ref()
                .map(|p| p.status())
                .unwrap_or(json!({"enabled":false}))),
            "peer-revoke" => {
                let id = field(&p, "id")?;
                self.ws.instance(id)?;
                {
                    let mut s = self.settings.lock().unwrap();
                    s["instances"][id]["peerToken"] = Value::Null;
                    s["instances"][id]["peerEnabled"] = json!(false);
                }
                self.save_settings()?;
                Ok(json!({"revoked":true}))
            }
            "peer-stop" => {
                {
                    let mut s = self.settings.lock().unwrap();
                    for (_, c) in s["instances"].as_object_mut().context("配置无效")? {
                        c["peerEnabled"] = json!(false);
                        c["peerToken"] = Value::Null;
                    }
                }
                self.save_settings()?;
                if let Some(peer) = self.peer.lock().unwrap().take() {
                    peer.close()
                }
                Ok(json!({"stopped":true}))
            }
            "peer-relay" => {
                if self.peer.lock().unwrap().is_some() {
                    bail!("请先关闭联机网络，再修改中继")
                }
                let relay = p["url"].as_str().unwrap_or("");
                if !relay.is_empty() {
                    let u = reqwest::Url::parse(relay)?;
                    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() {
                        bail!("中继需要 HTTPS 地址")
                    }
                }
                self.settings.lock().unwrap()["peerRelay"] = json!(relay);
                self.save_settings()?;
                Ok(json!({"saved":true}))
            }
            "stop" | "console" => self.execute(action, &p, Arc::new(|_| {})),
            "remote-invite" => {
                let _gate = self.gate.lock().unwrap();
                remote::invitation(self, &p)
            }
            "remote-revoke" => {
                let id = field(&p, "id")?;
                self.ws.instance(id)?;
                self.settings.lock().unwrap()["instances"][id]["shareToken"] = Value::Null;
                self.save_settings()?;
                Ok(json!({"revoked":true}))
            }
            "sharing-stop" => {
                let _gate = self.gate.lock().unwrap();
                remote::disable(self)?;
                Ok(json!({"stopped":true}))
            }
            "status" => self.status(),
            "trash-list" => trash::list(self),
            "versions" => game::versions(),
            "loaders" => loader_versions(
                p["loader"].as_str().unwrap_or("fabric"),
                field(&p, "minecraft")?,
            ),
            "search" => mods::search(
                p["query"].as_str().unwrap_or(""),
                field(&p, "minecraft")?,
                p["loader"].as_str().unwrap_or("fabric"),
                &p,
            ),
            "mod-update-state" => mods::update_state(&self.ws,&self.ws.instance(field(&p,"id")?)?),
            "mod-versions" => mods::versions(
                field(&p, "project")?,
                field(&p, "minecraft")?,
                p["loader"].as_str().unwrap_or("fabric"),
            ),
            "auth-start" => {
                let client_id = self.settings.lock().unwrap()["clientId"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                auth::start(&client_id)
            }
            "auth-poll" => {
                let client_id = self.settings.lock().unwrap()["clientId"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                let result = auth::poll(self.ws.root(), &client_id, field(&p, "code")?)?;
                if result["pending"] != true {
                    self.settings.lock().unwrap()["authMode"] = json!("microsoft");
                    self.save_settings()?;
                }
                Ok(result)
            }
            "logs" => {
                let dir = self.ws.instance_dir(field(&p, "id")?)?;
                let mut f = match fs::File::open(dir.join("latest.log")) {
                    Ok(f) => f,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(json!({"text":"尚无运行日志"}))
                    }
                    Err(e) => return Err(e.into()),
                };
                let n = f.metadata()?.len();
                f.seek(SeekFrom::Start(n.saturating_sub(128 * 1024)))?;
                let mut b = vec![];
                f.read_to_end(&mut b)?;
                Ok(json!({"text":String::from_utf8_lossy(&b)}))
            }
            "lobby-create" | "offline-profile" | "remote-join" | "create" | "settings"
            | "shader-check" | "shader-apply" | "shader-select" | "shader-restore"
            | "logout" | "install" | "configure" | "mod-add" | "mod-local" | "mod-remove" | "mod-compatibility-check" | "mod-update-check" | "mod-update-apply" | "mod-update-restore" | "mod-toggle"
            | "verify" | "sync" | "publish" | "launch" | "duplicate" | "world-import" | "pack-install" | "pack-import" | "pack-export"
            | "world-backup" | "world-backup-restore" | "world-deploy" | "world-activate" | "instance-delete" | "instance-restore" => {
                Ok(self.job(action.into(), p))
            }
            _ => bail!("未知操作"),
        }
    }
}
pub fn serve(root: PathBuf) -> Result<()> {
    let engine = Arc::new(Engine::new(&root)?);
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| anyhow::anyhow!("{e}"))?;
    let port = server.server_addr().to_ip().context("服务地址错误")?.port();
    let token = format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    let peer_enabled = engine.settings.lock().unwrap()["instances"]
        .as_object()
        .is_some_and(|items| items.values().any(|c| c["peerEnabled"] == true));
    if peer_enabled {
        if let Err(e) = peer::ensure(&engine) {
            engine.jobs.lock().unwrap().push(json!({"id":"peer-start","action":"联机网络","status":"error","message":format!("{e:#}")}));
        }
    }
    let sharing = engine.settings.lock().unwrap()["sharing"].clone();
    if sharing["enabled"] == true {
        if let Err(e) = remote::enable(
            &engine,
            sharing["host"].as_str().unwrap_or("localhost"),
            sharing["port"].as_u64().unwrap_or(25566) as u16,
        ) {
            engine.jobs.lock().unwrap().push(json!({"id":"sharing-start","action":"共享服务","status":"error","message":format!("{e:#}")}));
        }
    }
    write_json(
        &root.join("service.json"),
        &json!({"port":port,"token":token,"pid":std::process::id()}),
    )?;
    while !engine.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
        let Some(mut req)=server.recv_timeout(Duration::from_millis(500))? else{continue};
        let auth = req
            .headers()
            .iter()
            .find(|h| h.field.equiv("Authorization"))
            .map(|h| h.value.as_str())
            .unwrap_or("");
        if req.method() != &tiny_http::Method::Post
            || req.url() != "/rpc"
            || auth != format!("Bearer {token}")
            || req.headers().iter().any(|h| h.field.equiv("Origin"))
        {
            let _ =
                req.respond(tiny_http::Response::from_string("Forbidden").with_status_code(403));
            continue;
        }
        let engine = engine.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<Value> {
                let mut body = String::new();
                req.as_reader().take(1_048_577).read_to_string(&mut body)?;
                if body.len() > 1_048_576 {
                    bail!("请求过大")
                };
                let v: Value = serde_json::from_str(&body)?;
                engine.dispatch(field(&v, "action")?, v["payload"].clone())
            })();
            let value = match result {
                Ok(v) => json!({"ok":true,"value":v}),
                Err(e) => json!({"ok":false,"error":format!("{e:#}")}),
            };
            let _ = req.respond(
                tiny_http::Response::from_string(value.to_string()).with_header(
                    tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap(),
                ),
            );
        });
    }
    Ok(())
}
pub fn rpc(root: &Path, action: &str, payload: Value) -> Result<Value> {
    let endpoint = read_json(&root.join("service.json"))?;
    let port = endpoint["port"]
        .as_u64()
        .filter(|p| *p > 0 && *p <= 65535)
        .context("无效服务端口")?;
    let response: Value = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(260))
        .build()?
        .post(format!("http://127.0.0.1:{port}/rpc"))
        .bearer_auth(field(&endpoint, "token")?)
        .json(&json!({"action":action,"payload":payload}))
        .send()?
        .error_for_status()?
        .json()?;
    if response["ok"] != true {
        bail!("{}", response["error"].as_str().unwrap_or("后台操作失败"))
    }
    Ok(response["value"].clone())
}
pub fn ensure_service(root: &Path) -> Result<()> {
    if rpc(root, "status", json!({})).is_ok() {
        return Ok(());
    }
    private_dir(root)?;
    hidden(
        // A separate AppImage mount must outlive the UI's mount while hosting.
        Command::new(service_executable()?)
            .arg("--service")
            .arg(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    )
    .spawn()?;
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(100));
        if rpc(root, "status", json!({})).is_ok() {
            return Ok(());
        }
    }
    bail!("后台启动失败，请检查数据目录权限或后台日志")
}
fn service_executable() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(path) = std::env::var_os("APPIMAGE") {
        let path = PathBuf::from(path);
        if path.is_absolute() && path.is_file() { return Ok(path); }
    }
    Ok(std::env::current_exe()?)
}

fn loader_versions(kind: &str, mc: &str) -> Result<Value> {
    match kind {
        "fabric" => game::loaders(mc),
        "neoforge" => neoforge::versions(mc),
        "forge" => forge::versions(mc),
        "quilt" => quilt::versions(mc),
        _ => bail!("不支持此 Loader"),
    }
}
#[cfg(test)]mod job_control_tests {
 use super::*;
 #[test]fn updater_refuses_busy_service_and_rejects_late_jobs()->Result<()>{
  let temp=tempfile::tempdir()?;let e=Arc::new(Engine::new(temp.path())?);
  e.jobs.lock().unwrap().push(json!({"id":"busy","status":"running"}));assert!(e.dispatch("prepare-app-update",json!({})).is_err());assert!(!e.shutting_down.load(std::sync::atomic::Ordering::SeqCst));
  e.jobs.lock().unwrap().clear();assert_eq!(e.dispatch("prepare-app-update",json!({}))?["ready"],true);assert!(e.dispatch("status",json!({})).is_err());let job=e.job("create".into(),json!({}));assert!(e.jobs.lock().unwrap().iter().any(|j|j["id"]==job["jobId"]&&j["status"]=="error"));Ok(())
 }
 #[test]fn cancelled_queued_job_can_retry_once()->Result<()> {
  let temp=tempfile::tempdir()?;let e=Arc::new(Engine::new(temp.path())?);let gate=e.gate.lock().unwrap();
  let first=e.job("install".into(),json!({"id":uuid::Uuid::new_v4().to_string()}));let id=field(&first,"jobId")?;
  e.dispatch("job-cancel",json!({"jobId":id}))?;drop(gate);
  for _ in 0..100{if e.jobs.lock().unwrap().iter().any(|j|j["id"]==id&&j["status"]=="cancelled"){break}std::thread::sleep(Duration::from_millis(10));}
  assert!(e.jobs.lock().unwrap().iter().any(|j|j["id"]==id&&j["status"]=="cancelled"));
  let second=e.dispatch("job-retry",json!({"jobId":id}))?;assert_ne!(first["jobId"],second["jobId"]);assert!(e.dispatch("job-retry",json!({"jobId":id})).is_err());
  for _ in 0..100{if e.jobs.lock().unwrap().iter().any(|j|j["id"]==second["jobId"]&&j["status"]=="error"){break}std::thread::sleep(Duration::from_millis(10));}
  assert!(e.jobs.lock().unwrap().iter().any(|j|j["id"]==second["jobId"]&&j["status"]=="error"));Ok(())
 }
}
