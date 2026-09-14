//! World copies are independent, immutable until selected by a game process.
//! Replacing a server world switches a setting only after a complete staged copy.
use super::*;
use anyhow::ensure;
use fastnbt::Value as Nbt;
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn compound(v: &Nbt) -> Result<&HashMap<String, Nbt>> {
    match v {
        Nbt::Compound(c) => Ok(c),
        _ => bail!("存档 NBT 结构无效"),
    }
}
fn string(v: Option<&Nbt>) -> Option<&str> {
    match v {
        Some(Nbt::String(s)) => Some(s),
        _ => None,
    }
}
fn level(path: &Path) -> Result<Nbt> {
    ensure!(
        !path.join("db").is_dir(),
        "这是基岩版存档，需要先转换成 Java 版"
    );
    let mut bytes = Vec::new();
    GzDecoder::new(fs::File::open(path.join("level.dat"))?)
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 16 * 1024 * 1024, "level.dat 过大");
    fastnbt::from_bytes(&bytes).context("无法读取 Java 版 level.dat")
}
fn metadata(path: &Path) -> Result<Value> {
    let nbt = level(path)?;
    let data = compound(compound(&nbt)?.get("Data").context("缺少 Data")?)?;
    let version = data.get("Version").and_then(|v| compound(v).ok());
    Ok(
        json!({"path":path,"folder":path.file_name().unwrap_or_default().to_string_lossy(),
        "name":string(data.get("LevelName")).unwrap_or("未命名世界"),
        "minecraft":version.and_then(|v|string(v.get("Name"))),
        "lastPlayed":match data.get("LastPlayed"){Some(Nbt::Long(n)) if *n>0=>Some(*n as u64),_=>None},
        "hasPlayer":data.contains_key("Player")}),
    )
}
fn safe_tree(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        !meta.file_type().is_symlink(),
        "不支持符号链接存档：{}",
        path.display()
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            meta.file_attributes() & 0x400 == 0,
            "不支持目录联接或重解析点"
        );
    }
    ensure!(meta.is_dir() || meta.is_file(), "不支持特殊文件");
    Ok(())
}
fn child_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && !name.contains(['/', '\\', ':', '\n', '\r']),
        "无效世界目录"
    );
    Ok(())
}
// Hash both passes: changes during copying abort before any destination is activated.
fn snapshot(
    src: &Path,
    dest: Option<&Path>,
    depth: usize,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    base: &Path,
) -> Result<()> {
    ensure!(
        depth < 64 && files.len() < 1_000_000,
        "存档目录过深或文件过多"
    );
    safe_tree(src)?;
    if src.is_dir() {
        if let Some(dest) = dest {
            fs::create_dir_all(dest)?;
        }
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            if entry.file_name() == "session.lock" {
                continue;
            }
            snapshot(
                &entry.path(),
                dest.map(|d| d.join(entry.file_name())).as_deref(),
                depth + 1,
                files,
                base,
            )?;
        }
    } else {
        let mut input = fs::File::open(src)?;
        let mut output = dest.map(fs::File::create).transpose()?;
        let mut hash = Sha256::new();
        let mut buf = [0u8; 128 * 1024];
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            if let Some(out) = &mut output {
                out.write_all(&buf[..n])?;
            }
        }
        if let Some(out) = output {
            out.sync_all()?;
        }
        files.insert(
            src.strip_prefix(base)?.to_path_buf(),
            hash.finalize().to_vec(),
        );
    }
    Ok(())
}
fn copy_stable(src: &Path, dest: &Path) -> Result<()> {
    let mut first = BTreeMap::new();
    snapshot(src, Some(dest), 0, &mut first, src)?;
    let mut second = BTreeMap::new();
    snapshot(src, None, 0, &mut second, src)?;
    ensure!(first == second, "复制期间存档发生变化，请关闭源游戏后重试");
    Ok(())
}
fn source_lock(src: &Path) -> Result<Option<fs::File>> {
    let p = src.join("session.lock");
    if !p.exists() {
        return Ok(None);
    }
    safe_tree(&p)?;
    let f = fs::OpenOptions::new().read(true).write(true).open(p)?;
    fs2::FileExt::try_lock_exclusive(&f).context("存档被占用，请关闭源游戏或服务器")?;
    Ok(Some(f))
}
fn check_version(path: &Path, i: &Instance) -> Result<()> {
    let meta = metadata(path)?;
    ensure!(meta["minecraft"].as_str() == Some(i.minecraft.as_str()),
        "存档版本为 {}，实例为 {}。请选择完全相同的版本；无法识别的旧存档请先用原启动器打开并保存。", meta["minecraft"], i.minecraft);
    Ok(())
}
fn active_folder(engine: &Engine, id: &str) -> Result<String> {
    if let Some(name) = engine.config(id)["worldFolder"].as_str() {
        child_name(name)?;
        return Ok(name.into());
    }
    let props = engine.ws.instance_dir(id)?.join("game/server.properties");
    let old = fs::read_to_string(props).unwrap_or_default();
    let name = old
        .lines()
        .find_map(|l| l.trim().strip_prefix("level-name="))
        .unwrap_or("world");
    child_name(name)?;
    Ok(name.into())
}
pub(super) fn list(engine: &Engine, id: &str) -> Result<Value> {
    engine.ws.instance(id)?;
    let dir = engine.ws.instance_dir(id)?.join("game");
    let server = engine.config(id)["server"] == true;
    let mut worlds = Vec::new();
    let mut warnings = Vec::new();
    let active = if server {
        active_folder(engine, id)?
    } else {
        String::new()
    };
    let base = if server { dir } else { dir.join("saves") };
    if base.is_dir() {
        for e in fs::read_dir(&base)? {
            let p = e?.path();
            if p.join("level.dat").is_file() {
                match safe_tree(&p).and_then(|_| metadata(&p)) {
                    Ok(mut m) => {
                        m["active"] = json!(m["folder"] == active);
                        worlds.push(m);
                    }
                    Err(e) => warnings.push(format!("{}: {e:#}", p.display())),
                }
            }
        }
    }
    Ok(json!({"worlds":worlds,"warnings":warnings}))
}
pub(super) fn backups(engine: &Engine, id: &str) -> Result<Value> {
    let base = engine.ws.instance_dir(id)?.join("world-backups");
    let mut items = Vec::<Value>::new();
    if base.exists() {
        safe_tree(&base)?;
        for e in fs::read_dir(base)? {
            let p = e?.path();
            if uuid::Uuid::parse_str(&p.file_name().unwrap().to_string_lossy()).is_ok() {
                safe_tree(&p)?;
                items.push(read_json(&p.join("backup.json"))?);
            }
        }
    }
    items.sort_by_key(|v| std::cmp::Reverse(v["createdAt"].as_u64().unwrap_or(0)));
    Ok(json!({"items":items}))
}
pub(super) fn backup_all(engine: &Engine, id: &str, reason: &str, report: &game::Reporter) -> Result<Value> {
    engine.idle(id)?;
    let worlds = list(engine,id)?;
    ensure!(worlds["warnings"].as_array().unwrap().is_empty(), "存在无法读取的世界，备份未完成；请先修复存档");
    let dir = engine.ws.instance_dir(id)?;
    let base = dir.join("world-backups");
    fs::create_dir_all(&base)?;
    safe_tree(&base)?;
    let mut saved = Vec::new();
    for w in worlds["worlds"].as_array().unwrap() {
        let src = PathBuf::from(field(w,"path")?);
        let _lock = source_lock(&src)?;
        let bid = uuid::Uuid::new_v4().to_string();
        let stage = tempfile::Builder::new().prefix(".backup-").tempdir_in(&base)?;
        report(format!("备份世界：{}",w["name"].as_str().unwrap_or("世界")));
        copy_stable(&src,&stage.path().join("world"))?;
        let meta = json!({"id":bid,"name":w["name"],"folder":w["folder"],"minecraft":w["minecraft"],"createdAt":auth::now(),"reason":reason});
        write_json(&stage.path().join("backup.json"),&meta)?;
        fs::rename(stage.path(),base.join(&bid))?;
        saved.push(meta);
    }
    Ok(json!({"items":saved}))
}
pub(super) fn restore_backup(engine: &Engine,id: &str,bid: &str,report: &game::Reporter) -> Result<Value> {
    engine.idle(id)?;
    uuid::Uuid::parse_str(bid)?;
    let dir = engine.ws.instance_dir(id)?;
    let source = dir.join("world-backups").join(bid).join("world");
    safe_tree(&dir.join("world-backups"))?;
    safe_tree(source.parent().unwrap())?;
    check_version(&source,&engine.ws.instance(id)?)?;
    let folder = format!("restored-{}",uuid::Uuid::new_v4());
    let base = if engine.config(id)["server"]==true {dir.join("game")}else{dir.join("game/saves")};
    fs::create_dir_all(&base)?;
    safe_tree(&base)?;
    let stage=tempfile::Builder::new().prefix(".restore-").tempdir_in(&dir)?;
    report("校验并恢复为独立世界；当前世界保留".into());
    copy_stable(&source,&stage.path().join("world"))?;
    fs::rename(stage.path().join("world"),base.join(&folder))?;
    Ok(json!({"folder":folder}))
}
pub(super) fn copy_content(engine: &Engine,id: &str,new_id: &str,report: &game::Reporter) -> Result<()> {
    let src=engine.ws.instance_dir(id)?.join("game");
    let dest=engine.ws.instance_dir(new_id)?.join("game");
    for name in ["config","defaultconfigs","kubejs","scripts","options.txt","resourcepacks","shaderpacks"] {
        if src.join(name).exists() {copy_stable(&src.join(name),&dest.join(name))?;}
    }
    let worlds=list(engine,id)?;
    ensure!(worlds["warnings"].as_array().unwrap().is_empty(),"无法完整读取源世界");
    for w in worlds["worlds"].as_array().unwrap() {
        let source=PathBuf::from(field(w,"path")?);
        let _lock=source_lock(&source)?;
        let base=if engine.config(id)["server"]==true {dest.clone()}else{dest.join("saves")};
        fs::create_dir_all(&base)?;
        report(format!("复制世界：{}",w["name"]));
        copy_stable(&source,&base.join(field(w,"folder")?))?;
    }
    Ok(())
}
pub(super) fn scan(path: &Path) -> Result<Value> {
    let mut worlds = Vec::new();
    let mut warnings = Vec::new();
    fn visit(
        p: &Path,
        depth: usize,
        worlds: &mut Vec<Value>,
        warnings: &mut Vec<String>,
        visited: &mut usize,
    ) -> Result<()> {
        *visited += 1;
        ensure!(*visited <= 3000, "目录过大，请直接选择游戏的 saves 文件夹");
        safe_tree(p)?;
        if p.join("level.dat").is_file() {
            match metadata(p) {
                Ok(m) => worlds.push(m),
                Err(e) => warnings.push(format!("{}: {e:#}", p.display())),
            }
            return Ok(());
        }
        if depth == 0 {
            return Ok(());
        }
        for entry in fs::read_dir(p)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name();
                if ["assets", "libraries", "runtime", "mods", "cache", ".git"]
                    .iter()
                    .any(|s| name == *s)
                {
                    continue;
                }
                if let Err(e) = visit(&entry.path(), depth - 1, worlds, warnings, visited) {
                    warnings.push(format!("{}: {e:#}", entry.path().display()));
                }
            }
        }
        Ok(())
    }
    visit(path, 4, &mut worlds, &mut warnings, &mut 0)?;
    Ok(json!({"worlds":worlds,"warnings":warnings}))
}
fn player_transfer(world: &Path, uuid: &str) -> Result<()> {
    let uuid = uuid::Uuid::parse_str(uuid)?;
    let nbt = level(world)?;
    let data = compound(compound(&nbt)?.get("Data").context("缺少 Data")?)?;
    let mut player = compound(
        data.get("Player")
            .context("此存档没有内嵌单人玩家数据；请保留原玩家文件")?,
    )?
    .clone();
    let old_uuid = match player.get("UUID") {
        Some(Nbt::IntArray(a)) if a.len() == 4 => {
            let mut bytes = Vec::new();
            for n in a.iter() {
                bytes.extend(n.to_be_bytes());
            }
            Some(uuid::Uuid::from_slice(&bytes)?)
        }
        _ => None,
    };
    let ints = uuid
        .as_bytes()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_be_bytes(*b))
        .collect();
    player.insert("UUID".into(), Nbt::IntArray(fastnbt::IntArray::new(ints)));
    player.remove("UUIDMost");
    player.remove("UUIDLeast");
    if let Some(v) = data.get("DataVersion") {
        player.insert("DataVersion".into(), v.clone());
    }
    fs::create_dir_all(world.join("playerdata"))?;
    let mut gzip = GzEncoder::new(
        fs::File::create(world.join("playerdata").join(format!("{uuid}.dat")))?,
        Compression::default(),
    );
    gzip.write_all(&fastnbt::to_bytes(&Nbt::Compound(player))?)?;
    gzip.finish()?.sync_all()?;
    if let Some(old) = old_uuid.filter(|old| *old != uuid) {
        for dir in ["advancements", "stats"] {
            let src = world.join(dir).join(format!("{old}.json"));
            if src.is_file() {
                fs::copy(src, world.join(dir).join(format!("{uuid}.json")))?;
            }
        }
    }
    Ok(())
}
pub(super) fn execute(
    engine: &Arc<Engine>,
    action: &str,
    p: &Value,
    report: game::Reporter,
) -> Result<Value> {
    let id = field(p, "id")?;
    engine.idle(id)?;
    let i = engine.ws.instance(id)?;
    let dir = engine.ws.instance_dir(id)?;
    let server = engine.config(id)["server"] == true;
    if action == "world-activate" {
        ensure!(server, "只有服务器可以切换世界");
        let folder = field(p, "folder")?;
        child_name(folder)?;
        let path = dir.join("game").join(folder);
        safe_tree(&path)?;
        check_version(&path, &i)?;
        let old = active_folder(engine, id)?;
        engine.settings.lock().unwrap()["instances"][id]["worldFolder"] = json!(folder);
        if let Err(e) = engine.save_settings() {
            engine.settings.lock().unwrap()["instances"][id]["worldFolder"] = json!(old);
            return Err(e);
        }
        return Ok(json!({"folder":folder}));
    }
    ensure!(p["sourceStopped"] == true, "请确认源游戏已关闭，存档已保存");
    if action == "world-import" {
        ensure!(
            p["environmentConfirmed"] == true,
            "请先确认目标实例已安装源存档需要的 Loader 和 Mods"
        );
    }
    let source = if action == "world-deploy" {
        ensure!(!server, "请从单人实例部署世界");
        let folder = field(p, "folder")?;
        child_name(folder)?;
        dir.join("game/saves").join(folder)
    } else {
        PathBuf::from(field(p, "path")?)
    };
    safe_tree(&source)?;
    let source = source.canonicalize()?;
    // Reject any known running instance, including worlds selected through the file picker.
    for other in engine.ws.instances()? {
        let game = engine.ws.instance_dir(&other.instance_id)?.join("game");
        if game.exists() && source.starts_with(game.canonicalize()?) {
            engine.idle(&other.instance_id)?;
        }
    }
    let _lock = source_lock(&source)?;
    check_version(&source, &i)?;
    if action == "world-deploy" {
        ensure!(
            p["start"] != true || p["eula"] == true,
            "启动前需要同意 Minecraft EULA"
        );
        let mut new = i.clone();
        new.instance_id = uuid::Uuid::new_v4().to_string();
        new.name = field(p, "name")?.into();
        new.server = None;
        new.validate()?;
        let port = p["port"].as_u64().unwrap_or(25565);
        ensure!((1024..=65535).contains(&port), "端口范围为 1024–65535");
        let offline = p["offline"] == true;
        let player = if p["transferPlayer"] == true {
            let a = engine.active_account(false)?;
            ensure!(
                a["offline"] == offline,
                "背包迁移要求玩家档案与服务器验证模式一致"
            );
            Some(field(&a, "id")?.to_owned())
        } else {
            None
        };
        engine.ws.create_instance(&new)?;
        engine.settings.lock().unwrap()["instances"][&new.instance_id] = json!({"server":true,"port":port,"onlineMode":!offline,"worldFolder":"world","javaPath":"","deploymentStatus":"preparing"});
        engine.save_settings()?;
        let result = (|| -> Result<Value> {
            report("安装同版本服务器，并准备世界副本".into());
            game::install(engine.ws.root(), &new, true, None, report.clone())?;
            let nd = engine.ws.instance_dir(&new.instance_id)?;
            let mut lock = mods::lock(&engine.ws, &i)?;
            lock.mods.retain(|m| m.side != Side::Client);
            mods::apply(&engine.ws, &new, &lock)?;
            for name in ["config", "defaultconfigs", "kubejs", "scripts"] {
                let src = dir.join("game").join(name);
                if src.exists() {
                    copy_stable(&src, &nd.join("game").join(name))?;
                }
            }
            let stage = tempfile::Builder::new()
                .prefix(".world-stage-")
                .tempdir_in(&nd)?;
            copy_stable(&source, &stage.path().join("world"))?;
            if let Some(uuid) = &player {
                player_transfer(&stage.path().join("world"), uuid)?;
            }
            fs::rename(stage.path().join("world"), nd.join("game/world"))?;
            write_json(&nd.join("published.json"), &lock)?;
            engine.settings.lock().unwrap()["instances"][&new.instance_id]["deploymentStatus"] =
                json!("ready");
            engine.save_settings()?;
            report("世界已部署；可在托管服务器页面启动并邀请朋友".into());
            Ok(json!({"id":new.instance_id,"deployed":true}))
        })();
        if result.is_err() {
            engine.settings.lock().unwrap()["instances"][&new.instance_id]["deploymentStatus"] =
                json!("failed");
            engine.save_settings()?;
        }
        if result.is_ok() && p["start"] == true {
            engine.execute("launch", &json!({"id":new.instance_id,"eula":true}), report)?;
        }
        return result;
    }
    let base = if server {
        dir.join("game")
    } else {
        dir.join("game/saves")
    };
    fs::create_dir_all(&base)?;
    ensure!(
        !base.canonicalize()?.starts_with(&source),
        "不能将存档复制到其自身目录中"
    );
    let folder = format!("import-{}", uuid::Uuid::new_v4());
    let stage = tempfile::Builder::new()
        .prefix(".world-stage-")
        .tempdir_in(&dir)?;
    report("复制并校验存档；原存档会保留".into());
    copy_stable(&source, &stage.path().join("world"))?;
    fs::rename(stage.path().join("world"), base.join(&folder))?;
    if server {
        let old = active_folder(engine, id)?;
        engine.settings.lock().unwrap()["instances"][id]["worldFolder"] = json!(folder);
        if let Err(e) = engine.save_settings() {
            engine.settings.lock().unwrap()["instances"][id]["worldFolder"] = json!(old);
            return Err(e);
        }
    }
    Ok(json!({"folder":folder,"message":"已导入；旧世界与源存档均保留"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path) -> Result<()> {
        fs::create_dir_all(path.join("region"))?;
        fs::write(path.join("region/r.0.0.mca"), b"independent terrain data")?;
        let mut player = HashMap::new();
        player.insert("Health".into(), Nbt::Float(17.0));
        player.insert(
            "Pos".into(),
            Nbt::List(vec![
                Nbt::Double(12.0),
                Nbt::Double(65.0),
                Nbt::Double(-2.0),
            ]),
        );
        let mut version = HashMap::new();
        version.insert("Name".into(), Nbt::String("1.21.1".into()));
        let mut data = HashMap::new();
        data.insert("Version".into(), Nbt::Compound(version));
        data.insert("LevelName".into(), Nbt::String("迁移测试".into()));
        data.insert("DataVersion".into(), Nbt::Int(3955));
        data.insert("Player".into(), Nbt::Compound(player));
        let root = Nbt::Compound(HashMap::from([("Data".into(), Nbt::Compound(data))]));
        let mut gzip = GzEncoder::new(
            fs::File::create(path.join("level.dat"))?,
            Compression::default(),
        );
        gzip.write_all(&fastnbt::to_bytes(&root)?)?;
        gzip.finish()?;
        Ok(())
    }
    fn instance(e: &Arc<Engine>, server: bool, version: &str) -> Result<String> {
        let r = e.execute("create",&json!({"name":"World QA","minecraft":version,"loader":"vanilla","server":server,"install":false}),Arc::new(|_|{}))?;
        Ok(field(&r, "id")?.into())
    }
    #[test]
    fn import_copy_version_and_restore() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let src = temp.path().join("external/.minecraft/saves/My World");
        fixture(&src)?;
        let e = Arc::new(Engine::new(&temp.path().join("blocklink"))?);
        let id = instance(&e, false, "1.21.1")?;
        let server = instance(&e, true, "1.21.1")?;
        assert_eq!(
            scan(&temp.path().join("external"))?["worlds"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let payload = json!({"id":id,"path":src,"sourceStopped":true,"environmentConfirmed":true});
        let imported = execute(&e, "world-import", &payload, Arc::new(|_| {}))?;
        let imported_path =
            e.ws.instance_dir(&id)?
                .join("game/saves")
                .join(field(&imported, "folder")?);
        assert!(imported_path.join("level.dat").exists());
        fs::write(imported_path.join("region/r.0.0.mca"), b"changed target")?;
        assert_eq!(
            fs::read(src.join("region/r.0.0.mca"))?,
            b"independent terrain data"
        );
        let old = e.ws.instance_dir(&server)?.join("game/world");
        fixture(&old)?;
        let imported = execute(
            &e,
            "world-import",
            &json!({"id":server,"path":src,"sourceStopped":true,"environmentConfirmed":true}),
            Arc::new(|_| {}),
        )?;
        assert!(old.join("level.dat").exists());
        assert_eq!(active_folder(&e, &server)?, field(&imported, "folder")?);
        execute(
            &e,
            "world-activate",
            &json!({"id":server,"folder":"world"}),
            Arc::new(|_| {}),
        )?;
        assert_eq!(active_folder(&e, &server)?, "world");
        assert_eq!(list(&e, &server)?["worlds"].as_array().unwrap().len(), 2);
        drop(e);
        let reopened = Engine::new(&temp.path().join("blocklink"))?;
        assert_eq!(active_folder(&reopened, &server)?, "world");
        Ok(())
    }
    #[test]
    fn backup_restore_and_duplicate_are_independent() -> Result<()> {
        let temp=tempfile::tempdir()?;
        let e=Arc::new(Engine::new(&temp.path().join("app"))?);
        let id=instance(&e,false,"1.21.1")?;
        let source=e.ws.instance_dir(&id)?.join("game/saves/original");
        fixture(&source)?;
        let report:game::Reporter=Arc::new(|_|{});
        let saved=backup_all(&e,&id,"test",&report)?;
        let bid=field(&saved["items"][0],"id")?;
        fs::write(source.join("region/r.0.0.mca"),b"new progress")?;
        let restored=restore_backup(&e,&id,bid,&report)?;
        let target=e.ws.instance_dir(&id)?.join("game/saves").join(field(&restored,"folder")?);
        assert_eq!(fs::read(target.join("region/r.0.0.mca"))?,b"independent terrain data");
        assert_eq!(fs::read(source.join("region/r.0.0.mca"))?,b"new progress");
        assert!(restore_backup(&e,&id,"../bad",&report).is_err());
        let duplicate=e.execute("duplicate",&json!({"id":id,"includeWorlds":true}),report.clone())?;
        let new_id=field(&duplicate,"id")?;
        let copied=e.ws.instance_dir(new_id)?.join("game/saves/original/region/r.0.0.mca");
        fs::write(&copied,b"copy changed")?;
        assert_eq!(fs::read(source.join("region/r.0.0.mca"))?,b"new progress");
        assert!(e.config(new_id)["lobbyInvitation"].is_null());
        assert_eq!(e.config(new_id)["deploymentStatus"],"ready");
        fs::write(source.join("session.lock"),b"lock")?;
        let _lock=source_lock(&source)?;
        assert!(backup_all(&e,&id,"locked",&report).is_err());
        Ok(())
    }
    #[test]
    fn rejects_unsafe_incompatible_and_incomplete() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let src = temp.path().join("source");
        fixture(&src)?;
        let e = Arc::new(Engine::new(&temp.path().join("app"))?);
        let id = instance(&e, false, "1.20.1")?;
        let payload = json!({"id":id,"path":src,"sourceStopped":true,"environmentConfirmed":true});
        assert!(execute(&e, "world-import", &payload, Arc::new(|_| {}))
            .unwrap_err()
            .to_string()
            .contains("版本"));
        assert!(execute(
            &e,
            "world-import",
            &json!({"id":id,"path":src}),
            Arc::new(|_| {})
        )
        .is_err());
        assert!(child_name("../world").is_err());
        assert!(child_name("C:\\world").is_err());
        fs::write(src.join("session.lock"), [0u8])?;
        let lock = source_lock(&src)?;
        assert!(source_lock(&src).is_err());
        drop(lock);
        e.settings.lock().unwrap()["instances"][&id]["deploymentStatus"] = json!("preparing");
        assert!(e
            .execute("launch", &json!({"id":id}), Arc::new(|_| {}))
            .unwrap_err()
            .to_string()
            .contains("部署"));
        assert!(list(&e, &id)?["worlds"].as_array().unwrap().is_empty());
        Ok(())
    }
    #[test]
    fn player_nbt_preserves_position_and_health() -> Result<()> {
        let temp = tempfile::tempdir()?;
        fixture(temp.path())?;
        let source = fs::read(temp.path().join("level.dat"))?;
        let id = uuid::Uuid::new_v4();
        player_transfer(temp.path(), &id.to_string())?;
        let mut bytes = Vec::new();
        GzDecoder::new(fs::File::open(
            temp.path().join(format!("playerdata/{id}.dat")),
        )?)
        .read_to_end(&mut bytes)?;
        let nbt: Nbt = fastnbt::from_bytes(&bytes)?;
        let player = compound(&nbt)?;
        assert_eq!(player["Health"], Nbt::Float(17.0));
        assert_eq!(player["DataVersion"], Nbt::Int(3955));
        assert_eq!(
            player["Pos"],
            Nbt::List(vec![
                Nbt::Double(12.0),
                Nbt::Double(65.0),
                Nbt::Double(-2.0)
            ])
        );
        assert_eq!(fs::read(temp.path().join("level.dat"))?, source);
        Ok(())
    }

    #[test]
    #[ignore = "real Java and Cloudflare acceptance in an explicitly prepared isolated root"]
    fn real_import_deploy_and_join() -> Result<()> {
        let root = PathBuf::from(std::env::var("BLOCKLINK_WORLD_TEST_ROOT")?).canonicalize()?;
        ensure!(
            root.file_name().and_then(|n| n.to_str()) == Some("turn-world-acceptance"),
            "Requires isolated acceptance root"
        );
        ensure!(
            std::env::var("BLOCKLINK_TEST_RELAY_ONLY").as_deref() == Ok("1"),
            "Requires forced TURN"
        );
        let original = root.join("instances/f8bf5361-db55-438c-a47d-1728176e3ccb/game");
        ensure!(
            fs::read_to_string(original.join("eula.txt"))?
                .lines()
                .any(|l| l.trim() == "eula=true"),
            "Existing test EULA approval required"
        );
        let e = Arc::new(Engine::new(&root)?);
        let report: game::Reporter = Arc::new(|m| eprintln!("{m}"));
        e.execute(
            "offline-profile",
            &json!({"name":"BlocklinkTest"}),
            report.clone(),
        )?;
        let created=e.execute("create",&json!({"name":"存档迁移验收","minecraft":"1.21.1","loader":"fabric","loaderVersion":"0.19.5","install":false}),report.clone())?;
        let id = field(&created, "id")?;
        let imported = execute(
            &e,
            "world-import",
            &json!({"id":id,"path":original.join("world"),"sourceStopped":true,"environmentConfirmed":true}),
            report.clone(),
        )?;
        let deployed = execute(
            &e,
            "world-deploy",
            &json!({"id":id,"folder":imported["folder"],"name":"部署世界验收","port":25576,"offline":true,"sourceStopped":true}),
            report.clone(),
        )?;
        let sid = field(&deployed, "id")?;
        let server_dir = e.ws.instance_dir(sid)?;
        assert_eq!(
            fs::read(original.join("world/level.dat"))?,
            fs::read(server_dir.join("game/world/level.dat"))?
        );
        eprintln!("PASS: original world imported, deployed, level.dat preserved byte-for-byte");
        let mut client = None;
        let result = (|| -> Result<()> {
            e.execute("launch", &json!({"id":sid,"eula":true}), report.clone())?;
            wait_log(&server_dir.join("latest.log"), "Done (", 150)?;
            let properties = fs::read_to_string(server_dir.join("game/server.properties"))?;
            ensure!(
                properties.contains("server-ip=127.0.0.1")
                    && properties.contains("online-mode=false"),
                "Offline listener must be loopback"
            );
            eprintln!("PASS: deployed Fabric world loaded through production launch");
            let room = lobby::create(&e, sid)?;
            let joined = lobby::join(
                &e,
                &json!({"invitation":room["invitation"],"name":"迁移世界入服验收"}),
                &report,
            )?;
            let cid = field(&joined, "id")?.to_owned();
            client = Some(cid.clone());
            e.execute("launch", &json!({"id":cid}), report.clone())?;
            wait_log(
                &server_dir.join("latest.log"),
                "BlocklinkTest joined the game",
                180,
            )?;
            e.execute(
                "console",
                &json!({"id":sid,"command":"list"}),
                report.clone(),
            )?;
            wait_log(
                &server_dir.join("latest.log"),
                "players online: BlocklinkTest",
                20,
            )?;
            eprintln!(
                "PASS: real Minecraft player entered migrated world through forced public TURN"
            );
            lobby::close(&e, sid)?;
            wait_log(
                &server_dir.join("latest.log"),
                "BlocklinkTest lost connection",
                30,
            )?;
            Ok(())
        })();
        if let Some(cid) = client {
            let _ = e.execute("stop", &json!({"id":cid}), report.clone());
            let _ = lobby::close(&e, &cid);
        }
        let _ = lobby::close(&e, sid);
        let _ = e.execute("stop", &json!({"id":sid}), report);
        let deadline = std::time::Instant::now() + Duration::from_secs(45);
        while e.is_running(sid) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(250));
        }
        if e.is_running(sid) {
            e.execute("stop", &json!({"id":sid,"force":true}), Arc::new(|_| {}))?;
            bail!("Server required forced shutdown");
        }
        result?;
        wait_log(
            &server_dir.join("latest.log"),
            "All dimensions are saved",
            5,
        )?;
        eprintln!("PASS: room closure disconnected player; normal server stop saved world");
        Ok(())
    }
    fn wait_log(path: &Path, marker: &str, seconds: u64) -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
        loop {
            if fs::read_to_string(path)
                .unwrap_or_default()
                .contains(marker)
            {
                return Ok(());
            }
            ensure!(std::time::Instant::now() < deadline, "Timed out: {marker}");
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}
