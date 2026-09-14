//! Real installers and game processes. Run explicitly in the prepared QA workspace.
use super::*;
use anyhow::ensure;
#[test]
#[ignore = "requires isolated neo-acceptance directory, network, Java and graphical desktop"]
fn install_and_join() -> Result<()> {
    let root = PathBuf::from(std::env::var("BLOCKLINK_LOADER_TEST_ROOT")?).canonicalize()?;
    ensure!(
        root.file_name().and_then(|v| v.to_str()) == Some("neo-acceptance"),
        "Requires isolated root"
    );
    let approval = root
        .parent()
        .unwrap()
        .join("app-test-data/instances/f8bf5361-db55-438c-a47d-1728176e3ccb/game/eula.txt");
    ensure!(
        fs::read_to_string(approval)?
            .lines()
            .any(|s| s.trim() == "eula=true"),
        "Existing test EULA approval required"
    );
    let mc = std::env::var("BLOCKLINK_LOADER_TEST_MC").unwrap_or("1.21.1".into());
    let kind = std::env::var("BLOCKLINK_LOADER_TEST_KIND").unwrap_or("neoforge".into());
    let engine = Arc::new(Engine::new(&root)?);
    let report: game::Reporter = Arc::new(|s| eprintln!("{s}"));
    engine.execute(
        "offline-profile",
        &json!({"name":"BlocklinkTest"}),
        report.clone(),
    )?;
    let resume = if std::env::var("BLOCKLINK_LOADER_TEST_RESUME").as_deref() == Ok("1") {
        engine.ws.instances()?.into_iter().find(|i| {
            i.minecraft == mc
                && i.loader.kind() == kind
                && engine.config(&i.instance_id)["serverId"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
        })
    } else {
        None
    };
    let created = if let Some(i) = &resume {
        json!({"id":i.instance_id})
    } else {
        engine.execute("create",&json!({"name":format!("{kind} {mc} client QA"),"minecraft":mc,"loader":kind,"memory":2048}),report.clone())?
    };
    let cid = field(&created, "id")?;
    if resume.is_some() && std::env::var("BLOCKLINK_LOADER_TEST_REINSTALL").as_deref() == Ok("1") {
        engine.execute("install", &json!({"id":cid}), report.clone())?;
    }
    let si = if resume.is_some() {
        json!({"id":engine.config(cid)["serverId"]})
    } else if mc == "1.21.1" && kind == "neoforge" {
        engine.execute(
            "mod-add",
            &json!({"id":cid,"project":"ferrite-core"}),
            report.clone(),
        )?;
        let src = root
            .parent()
            .unwrap()
            .join("app-test-data/instances/f8bf5361-db55-438c-a47d-1728176e3ccb/game/world");
        let world = engine.execute(
            "world-import",
            &json!({"id":cid,"path":src,"sourceStopped":true,"environmentConfirmed":true}),
            report.clone(),
        )?;
        engine.execute("world-deploy",&json!({"id":cid,"folder":world["folder"],"name":"NeoForge migrated server QA","port":25577,"offline":true,"sourceStopped":true}),report.clone())?
    } else {
        engine.execute("create",&json!({"name":format!("{kind} {mc} server QA"),"minecraft":mc,"loader":kind,"server":true,"port":25577,"memory":2048}),report.clone())?
    };
    let sid = field(&si, "id")?;
    let test_mod = std::env::var("BLOCKLINK_LOADER_TEST_MOD").ok();
    if let Some(project) = &test_mod {
        engine.execute(
            "mod-add",
            &json!({"id":sid,"project":project}),
            report.clone(),
        )?;
    }
    engine.settings.lock().unwrap()["instances"][sid]["onlineMode"] = json!(false);
    engine.save_settings()?;
    engine.execute(
        "configure",
        &json!({"id":cid,"serverId":sid,"memory":2048}),
        report.clone(),
    )?;
    let server_dir = engine.ws.instance_dir(sid)?;
    let result = (|| -> Result<()> {
        engine.execute("launch", &json!({"id":sid,"eula":true}), report.clone())?;
        wait(&engine, sid, &server_dir.join("latest.log"), "Done (", 180)?;
        eprintln!("PASS: {kind} {mc} installed and server loaded");
        engine.execute("launch", &json!({"id":cid}), report.clone())?;
        wait(
            &engine,
            cid,
            &server_dir.join("latest.log"),
            "BlocklinkTest joined the game",
            180,
        )?;
        let lock = engine
            .ws
            .read_lock(cid)?
            .context("Client sync receipt missing")?;
        ensure!(
            lock.environment.loader.kind() == kind,
            "Wrong synced loader"
        );
        if test_mod.is_some() {
            ensure!(!lock.mods.is_empty(), "Expected test Mod was not synced");
        }
        if mc == "1.21.1" && kind == "neoforge" {
            ensure!(
                lock.mods.iter().any(|m| m.mod_id == "ferritecore"),
                "NeoForge Mod not synced"
            );
        }
        eprintln!("PASS: real {kind} {mc} player joined, Mod sync and matching loader confirmed");
        Ok(())
    })();
    let _ = engine.execute("stop", &json!({"id":cid}), report.clone());
    let _ = engine.execute("stop", &json!({"id":sid}), report.clone());
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    while engine.is_running(sid) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    if engine.is_running(sid) {
        engine.execute("stop", &json!({"id":sid,"force":true}), report)?;
        bail!("Server did not stop normally")
    }
    result?;
    eprintln!("PASS: {kind} {mc} normal server shutdown");
    Ok(())
}
fn wait(engine: &Engine, id: &str, path: &Path, needle: &str, seconds: u64) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        if fs::read_to_string(path)
            .unwrap_or_default()
            .contains(needle)
        {
            return Ok(());
        }
        ensure!(
            engine.is_running(id),
            "Game process exited; see {}",
            engine.ws.instance_dir(id)?.join("latest.log").display()
        );
        ensure!(
            std::time::Instant::now() < deadline,
            "Timed out waiting for {needle}; log: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}
