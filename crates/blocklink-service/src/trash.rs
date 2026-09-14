use super::*;
use anyhow::ensure;

fn detached(mut c: Value) -> Value {
    if let Some(o) = c.as_object_mut() {
        for key in [
            "shareToken",
            "peerToken",
            "peerEnabled",
            "lobbyInvitation",
            "peerInvitation",
            "remoteInvitation",
            "remoteName",
        ] {
            o.remove(key);
        }
        o.insert("serverId".into(), json!(""));
    }
    c
}
fn remove_config(s: &mut Value, id: &str) {
    if let Some(instances) = s["instances"].as_object_mut() {
        instances.remove(id);
        for c in instances.values_mut() {
            if c["serverId"] == id {
                c["serverId"] = json!("");
            }
        }
    }
}
pub(super) fn reconcile(ws: &Workspace, s: &mut Value) -> Result<()> {
    for id in ws.pending_deletion_ids()? {
        remove_config(s,&id);
        // A locked leftover must not prevent unrelated instances from opening.
        // Keep its directory for another cleanup attempt on the next startup.
        let _=ws.finish_permanent_delete(&id);
    }
    for e in fs::read_dir(ws.trash_dir()?)? {
        let id = e?.file_name().to_string_lossy().into_owned();
        ws.trash_instance_dir(&id)?;
        remove_config(s, &id);
    }
    Ok(())
}
pub(super) fn list(e: &Engine) -> Result<Value> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(e.ws.trash_dir()?)? {
        let id = entry?.file_name().to_string_lossy().into_owned();
        let dir = e.ws.trash_instance_dir(&id)?;
        let i: Instance = serde_json::from_value(read_json(&dir.join("instance.json"))?)?;
        i.validate()?;
        ensure!(i.instance_id == id, "回收站实例标识不匹配");
        let meta = read_json(&dir.join("trash.json"))?;
        entries.push(json!({"id":id,"name":i.name,"minecraft":i.minecraft,"server":meta["config"]["server"],"deletedAt":meta["deletedAt"]}));
    }
    Ok(json!(entries))
}
// Check world session locks without following links. The engine's mutation gate
// prevents managed games from launching between this check and the move.
fn world_locks(dir: &Path, depth: usize, locks: &mut Vec<fs::File>) -> Result<()> {
    ensure!(depth < 64, "实例目录层级过深");
    let m = fs::symlink_metadata(dir)?;
    ensure!(
        !m.file_type().is_symlink(),
        "实例包含符号链接，请先检查数据目录"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            m.file_attributes() & 0x400 == 0,
            "实例包含目录联接，请先检查数据目录"
        );
    }
    if m.is_dir() {
        for child in fs::read_dir(dir)? {
            world_locks(&child?.path(), depth + 1, locks)?;
        }
    } else if dir.file_name().is_some_and(|n| n == "session.lock") {
        let f = fs::OpenOptions::new().read(true).write(true).open(dir)?;
        fs2::FileExt::try_lock_exclusive(&f).context("存档正在使用，请先关闭游戏或服务器")?;
        locks.push(f);
    }
    Ok(())
}
pub(super) fn execute(e: &Arc<Engine>, action: &str, p: &Value) -> Result<Value> {
    let id = field(p, "id")?;
    if action == "instance-delete" {
        let i = e.ws.instance(id)?;
        ensure!(p["name"] == i.name, "请确认要删除的实例名称");
        e.idle(id)?;
        for other in e.ws.instances()? {
            if e.config(&other.instance_id)["serverId"] == id {
                e.idle(&other.instance_id)?;
            }
        }
        let dir = e.ws.instance_dir(id)?;
        let mut locks = Vec::new();
        world_locks(&dir, 0, &mut locks)?;
        lobby::close(e, id)?;
        if let Some(peer) = e.peer.lock().unwrap().as_ref() {
            peer.close_bridge(id);
        }
        if p["permanent"]==true {
            drop(locks);
            e.ws.begin_permanent_delete(id).context("无法删除，请关闭占用该实例文件的程序")?;
            remove_config(&mut e.settings.lock().unwrap(),id);
            let saved=e.save_settings();
            e.ws.finish_permanent_delete(id).context("部分文件仍被占用。请关闭占用程序后重启启动器，将继续完成删除")?;
            saved?;
            return Ok(json!({"deleted":true,"permanent":true,"id":id}));
        }
        let old = e.settings.lock().unwrap().clone();
        let config = detached(e.config(id));
        write_json(
            &dir.join("trash.json"),
            &json!({"config":config,"deletedAt":auth::now()}),
        )?;
        // Windows can refuse directory renames while a descendant handle is open.
        drop(locks);
        e.ws.archive_instance(id)
            .context("无法移动实例目录，请关闭占用该目录的程序")?;
        remove_config(&mut e.settings.lock().unwrap(), id);
        if let Err(error) = e.save_settings() {
            *e.settings.lock().unwrap() = old;
            e.ws.restore_instance(id)
                .context("设置保存失败，数据仍保留在回收站")?;
            return Err(error);
        }
        Ok(json!({"deleted":true,"id":id}))
    } else {
        let dir = e.ws.trash_instance_dir(id)?;
        let meta = read_json(&dir.join("trash.json"))?;
        let old = e.settings.lock().unwrap().clone();
        e.settings.lock().unwrap()["instances"][id] = detached(meta["config"].clone());
        if let Err(error) = e.save_settings() {
            *e.settings.lock().unwrap() = old;
            return Err(error);
        }
        if let Err(error) = e.ws.restore_instance(id) {
            *e.settings.lock().unwrap() = old;
            e.save_settings()?;
            return Err(error.into());
        }
        Ok(json!({"restored":true,"id":id}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permanent_delete_removes_worlds_and_backups_but_preserves_shared_files()->Result<()> {
        let tmp=tempfile::tempdir()?;let e=Arc::new(Engine::new(tmp.path())?);
        let a=e.execute("create",&json!({"name":"disposable","minecraft":"26.2","loader":"vanilla","install":false}),Arc::new(|_|{}))?;
        let id=field(&a,"id")?;let dir=e.ws.instance_dir(id)?;
        fs::create_dir_all(dir.join("game/saves/world"))?;fs::write(dir.join("game/saves/world/level.dat"),b"world")?;
        fs::create_dir_all(dir.join("world-backups/one"))?;fs::write(dir.join("world-backups/one/level.dat"),b"backup")?;
        let shared=tmp.path().join("shared.jar");fs::write(&shared,b"keep")?;fs::hard_link(&shared,dir.join("game/shared.jar"))?;
        assert!(execute(&e,"instance-delete",&json!({"id":id,"name":"wrong","permanent":true})).is_err());
        let lock=fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(dir.join("game/saves/world/session.lock"))?;fs2::FileExt::try_lock_exclusive(&lock)?;
        assert!(execute(&e,"instance-delete",&json!({"id":id,"name":"disposable","permanent":true})).is_err());drop(lock);
        let result=execute(&e,"instance-delete",&json!({"id":id,"name":"disposable","permanent":true}))?;
        assert_eq!(result["permanent"],true);assert!(!dir.exists());assert!(list(&e)?.as_array().unwrap().is_empty());assert!(e.ws.pending_deletion_ids()?.is_empty());assert_eq!(fs::read(shared)?,b"keep");assert!(e.config(id).is_null());
        assert!(execute(&e,"instance-restore",&json!({"id":id})).is_err());Ok(())
    }
    #[test]
    fn interrupted_permanent_delete_finishes_on_restart()->Result<()> {
        let tmp=tempfile::tempdir()?;let e=Arc::new(Engine::new(tmp.path())?);
        let a=e.execute("create",&json!({"name":"disposable","minecraft":"26.2","loader":"vanilla","install":false}),Arc::new(|_|{}))?;let id=field(&a,"id")?;
        e.ws.begin_permanent_delete(id)?;drop(e);let e=Engine::new(tmp.path())?;
        assert!(e.ws.instances()?.is_empty());assert!(e.ws.pending_deletion_ids()?.is_empty());assert!(e.config(id).is_null());Ok(())
    }
    #[test]
    fn delete_restore_preserves_world_and_shared_files_and_detaches_bindings() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let e = Arc::new(Engine::new(tmp.path())?);
        let report: game::Reporter = Arc::new(|_| {});
        let create = |name: &str| {
            e.execute(
                "create",
                &json!({"name":name,"minecraft":"26.2","loader":"vanilla","install":false}),
                report.clone(),
            )
        };
        let a = create("server")?;
        let b = create("client")?;
        let id = field(&a, "id")?;
        let bid = field(&b, "id")?;
        e.settings.lock().unwrap()["instances"][id] =
            json!({"server":true,"port":25565,"shareToken":"revoked","peerToken":"revoked"});
        e.settings.lock().unwrap()["instances"][bid]["serverId"] = json!(id);
        let game = e.ws.instance_dir(id)?.join("game");
        fs::create_dir(game.join("world"))?;
        fs::write(game.join("world/level.dat"), b"saved world")?;
        let session = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(game.join("world/session.lock"))?;
        fs2::FileExt::try_lock_exclusive(&session)?;
        assert!(execute(&e, "instance-delete", &json!({"id":id,"name":"server"})).is_err());
        assert!(e.ws.instance(id).is_ok());
        drop(session);
        let shared = tmp.path().join("shared.jar");
        fs::write(&shared, b"shared bytes")?;
        fs::hard_link(&shared, game.join("example.jar"))?;
        assert!(execute(&e, "instance-delete", &json!({"id":id,"name":"wrong"})).is_err());
        execute(&e, "instance-delete", &json!({"id":id,"name":"server"}))?;
        assert!(e.ws.instance(id).is_err());
        assert_eq!(e.config(bid)["serverId"], "");
        assert_eq!(fs::read(&shared)?, b"shared bytes");
        assert_eq!(list(&e)?.as_array().unwrap().len(), 1);
        drop(e);
        let e = Arc::new(Engine::new(tmp.path())?);
        execute(&e, "instance-restore", &json!({"id":id}))?;
        assert_eq!(
            fs::read(e.ws.instance_dir(id)?.join("game/world/level.dat"))?,
            b"saved world"
        );
        assert!(e.config(id)["shareToken"].is_null());
        assert_eq!(e.config(id)["server"], true);
        assert_eq!(e.config(bid)["serverId"], "");
        assert!(execute(&e, "instance-restore", &json!({"id":id})).is_err());
        assert!(execute(
            &e,
            "instance-delete",
            &json!({"id":"../outside","name":"x"})
        )
        .is_err());
        Ok(())
    }
}
