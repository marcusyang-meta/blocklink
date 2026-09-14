//! Opt-in real game acceptance; never changes authentication in a production instance.
use super::*;
use anyhow::ensure;

#[test]
#[ignore = "requires temporary isolated invitation and cloud Xvfb"]
fn cloud_client_enters_remote_world() -> Result<()> {
    ensure!(test_relay_only(), "Cloud acceptance must force relay");
    ensure!(std::env::var("BLOCKLINK_TEST_TURN_TLS_ONLY").as_deref()==Ok("1"), "Cloud acceptance must force TLS");
    let root=tempfile::tempdir()?;
    let engine=Arc::new(Engine::new(root.path())?);
    let report: game::Reporter=Arc::new(|m|eprintln!("{m}"));
    engine.execute("offline-profile",&json!({"name":"CloudCheck"}),report.clone())?;
    let joined=join(&engine,&json!({"invitation":std::env::var("BLOCKLINK_TEST_INVITATION")?,"name":"TLS acceptance"}),&report)?;
    let id=field(&joined,"id")?;
    // The unattended fixture has already chosen its accessibility preferences.
    // Production instances retain Minecraft's first-run accessibility screen.
    fs::write(engine.ws.instance_dir(id)?.join("game/options.txt"), "onboardAccessibility:false\npauseOnLostFocus:false\nautoJump:false\n")?;
    let outcome=(|| -> Result<()> {
        let launched=engine.execute("launch",&json!({"id":id}),report.clone())?;
        let log=engine.ws.instance_dir(id)?.join("game/logs/latest.log");
        let deadline=std::time::Instant::now()+Duration::from_secs(150);
        let joined=regex::Regex::new(r"Loaded \d+ advancements")?;
        loop {
            let text=fs::read_to_string(&log).unwrap_or_default();
            if joined.is_match(&text) {break;}
            ensure!(std::time::Instant::now()<deadline,"Client did not receive world advancements: {}",text.chars().rev().take(1500).collect::<String>().chars().rev().collect::<String>());
            ensure!(engine.is_running(id),"Minecraft exited before joining");
            std::thread::sleep(Duration::from_secs(1));
        }
        eprintln!("PASS: real client received world data over forced TLS relay");
        if std::env::var("BLOCKLINK_TEST_GAMEPLAY").as_deref()==Ok("1") {
            let status=Command::new("python3").arg("scripts/cloud-gameplay-input.py")
                .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
                .env("BLOCKLINK_TEST_GAME_LOG",&log)
                .env("BLOCKLINK_TEST_GAME_PID",launched["pid"].to_string()).status()?;
            ensure!(status.success(),"Cloud keyboard/mouse acceptance failed");
            std::thread::sleep(Duration::from_secs(15));
        } else {std::thread::sleep(Duration::from_secs(85));}
        Ok(())
    })();
    if std::env::var("GITHUB_ACTIONS").as_deref()==Ok("true") {
        let evidence=Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cloud-checks");
        let _=fs::create_dir_all(&evidence);
        let _=fs::copy(engine.ws.instance_dir(id)?.join("game/logs/latest.log"),evidence.join("minecraft.log"));
        let _=Command::new("scrot").arg(evidence.join("minecraft.png")).status();
    }
    let _=engine.execute("stop",&json!({"id":id,"force":true}),report);
    let _=close(&engine,id);
    outcome
}

#[test]
#[ignore = "requires prepared isolated directory and explicit offline acceptance approval"]
fn real_game_enters_world_over_turn() -> Result<()> {
    ensure!(
        std::env::var("BLOCKLINK_OFFLINE_WORLD_APPROVED").as_deref() == Ok("1"),
        "Offline acceptance not approved"
    );
    ensure!(test_relay_only(), "Must force TURN for this acceptance");
    let root = PathBuf::from(std::env::var("BLOCKLINK_WORLD_TEST_ROOT")?).canonicalize()?;
    ensure!(
        root.file_name().and_then(|v| v.to_str()) == Some("turn-world-acceptance"),
        "Not the isolated acceptance directory"
    );
    let engine = Arc::new(Engine::new(&root)?);
    let id = "f8bf5361-db55-438c-a47d-1728176e3ccb";
    let dir = engine.ws.instance_dir(id)?;
    ensure!(
        fs::read_to_string(dir.join("game/eula.txt"))?.contains("eula=true"),
        "Isolated EULA approval missing"
    );
    let installed = read_json(&dir.join("installed.json"))?;
    let properties = dir.join("game/server.properties");
    let original = fs::read_to_string(&properties)?;
    let filtered: Vec<_> = original
        .lines()
        .filter(|l| {
            !["online-mode=", "server-ip=", "server-port=", "white-list="]
                .iter()
                .any(|p| l.starts_with(p))
        })
        .collect();
    fs::write(
        &properties,
        format!(
            "{}\nonline-mode=false\nserver-ip=127.0.0.1\nserver-port=25576\nwhite-list=false\n",
            filtered.join("\n")
        ),
    )?;
    let report: game::Reporter = Arc::new(|m| eprintln!("{m}"));
    let mut client_id = None;
    let outcome = (|| -> Result<()> {
        let log = fs::File::create(dir.join("latest.log"))?;
        let child = hidden(
            Command::new(game::process_path(Path::new(field(&installed, "java")?)))
                .args(["-Xmx2048M", "-jar"])
                .arg(game::process_path(Path::new(field(
                    &installed,
                    "serverJar",
                )?)))
                .arg("nogui")
                .current_dir(game::process_path(&dir.join("game")))
                .stdin(Stdio::piped())
                .stdout(log.try_clone()?)
                .stderr(log),
        )
        .spawn()?;
        engine.children.lock().unwrap().insert(id.into(), child);
        wait_log(&dir.join("latest.log"), "Done (", 120)?;
        eprintln!("PASS: real Fabric world loaded on loopback");
        let room = create(&engine, id)?;
        if let Ok(invite_file) = std::env::var("BLOCKLINK_WORLD_REMOTE_INVITE_FILE") {
            fs::write(invite_file, field(&room, "invitation")?)?;
            eprintln!("READY: isolated invitation saved; waiting for remote CloudCheck client");
            wait_log(&dir.join("latest.log"), "CloudCheck joined the game", 1200)?;
            engine.execute("console", &json!({"id":id,"command":"list"}), report.clone())?;
            wait_log(&dir.join("latest.log"), "players online: CloudCheck", 15)?;
            eprintln!("PASS: remote real Minecraft client joined through forced TURN; player list confirmed");
            if std::env::var("BLOCKLINK_TEST_GAMEPLAY").as_deref()==Ok("1") {
                gameplay_actions(&engine,id,&dir.join("latest.log"),&report)?;
                std::thread::sleep(Duration::from_secs(5));
            } else {std::thread::sleep(Duration::from_secs(60));}
            ensure!(!fs::read_to_string(dir.join("latest.log"))?.contains("CloudCheck lost connection"), "Player disconnected before room closure");
            close(&engine, id)?;
            wait_log(&dir.join("latest.log"), "CloudCheck lost connection", 30)?;
            eprintln!("PASS: room closure disconnected remote real Minecraft client");
            return Ok(());
        }
        let joined = join(
            &engine,
            &json!({"invitation":room["invitation"],"name":"TURN world acceptance"}),
            &report,
        )?;
        let cid = field(&joined, "id")?.to_owned();
        client_id = Some(cid.clone());
        engine.execute("launch", &json!({"id":cid}), report.clone())?;
        wait_log(
            &dir.join("latest.log"),
            "BlocklinkTest joined the game",
            150,
        )?;
        engine.execute(
            "console",
            &json!({"id":id,"command":"list"}),
            report.clone(),
        )?;
        wait_log(&dir.join("latest.log"), "players online: BlocklinkTest", 15)?;
        eprintln!("PASS: real Minecraft client joined world through forced Cloudflare TURN; server player list confirmed");
        close(&engine, id)?;
        wait_log(&dir.join("latest.log"), "BlocklinkTest lost connection", 30)?;
        eprintln!("PASS: closing lobby disconnected the real player");
        Ok(())
    })();
    if let Some(cid) = client_id {
        let _ = engine.execute("stop", &json!({"id":cid}), report.clone());
        let _ = close(&engine, &cid);
    }
    let _ = close(&engine, id);
    let _ = engine.execute("stop", &json!({"id":id}), report);
    let deadline = std::time::Instant::now() + Duration::from_secs(45);
    if let Some(mut child) = engine.children.lock().unwrap().remove(id) {
        while child.try_wait()?.is_none() {
            if std::time::Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    fs::write(properties, original)?;
    outcome
}

fn gameplay_actions(engine: &Arc<Engine>, id: &str, log: &Path, report: &game::Reporter) -> Result<()> {
    let command=|text: &str| -> Result<()> {engine.execute("console",&json!({"id":id,"command":text}),report.clone())?;Ok(())};
    let notify=|marker: &str|command(&format!("tellraw CloudCheck {{\"text\":\"{marker}\"}}"));
    let check=|setup: &str, condition: &str, marker: &str| -> Result<()> {
        let deadline=std::time::Instant::now()+Duration::from_secs(25);
        loop {
            if !setup.is_empty() {command(setup)?;}
            command(&format!("execute {condition} run say {marker}"))?;
            std::thread::sleep(Duration::from_millis(100));
            if fs::read_to_string(log)?.contains(marker) { eprintln!("PASS: server verified {marker}");return Ok(()); }
            ensure!(std::time::Instant::now()<deadline,"Server did not verify {marker}");
        }
    };
    command("gamemode creative CloudCheck")?;
    command("difficulty peaceful")?;
    command("forceload add -16 -16 16 16")?;
    command("tp CloudCheck 0.5 101 0.5 0 0")?;
    std::thread::sleep(Duration::from_secs(3));
    command("fill -6 100 -6 6 100 12 minecraft:stone")?;
    command("fill -6 101 -6 6 106 12 minecraft:air")?;
    command("scoreboard objectives add bl_accept dummy")?;
    command("tp CloudCheck 0.5 101 0.5 0 0")?;
    std::thread::sleep(Duration::from_secs(1));
    notify("BL_MOVE_READY")?;
    check("execute store result score #z bl_accept run data get entity CloudCheck Pos[2] 1000", "if score #z bl_accept matches 3000..10000", "BL_MOVE_PASS")?;
    command("tp CloudCheck 0.5 101 0.5 0 0")?;
    std::thread::sleep(Duration::from_secs(2));
    notify("BL_JUMP_READY")?;
    check("execute store result score #y bl_accept run data get entity CloudCheck Pos[1] 1000", "if score #y bl_accept matches 101200..104000", "BL_JUMP_PASS")?;
    std::thread::sleep(Duration::from_secs(2));
    command("tp CloudCheck 0.5 101 0.5 0 45")?;
    command("item replace entity CloudCheck hotbar.0 with minecraft:stone 64")?;
    command("setblock 0 101 2 minecraft:air")?;
    std::thread::sleep(Duration::from_secs(1));
    notify("BL_PLACE_READY")?;
    check("", "if block 0 101 2 minecraft:stone", "BL_PLACE_PASS")?;
    notify("BL_BREAK_READY")?;
    check("", "if block 0 101 2 minecraft:air", "BL_BREAK_PASS")?;
    notify("BL_ACTIONS_PASS")?;
    eprintln!("PASS: remote keyboard movement, jump, placement and mining verified against server state");
    Ok(())
}

fn wait_log(path: &Path, marker: &str, seconds: u64) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        if let Ok(path) = std::env::var("BLOCKLINK_WORLD_REMOTE_INVITE_FILE") {
            ensure!(!PathBuf::from(path).with_extension("cancel").exists(), "Acceptance cancelled; cleaning up isolated host");
        }
        if fs::read_to_string(path)
            .unwrap_or_default()
            .contains(marker)
        {
            return Ok(());
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "Timed out waiting for {marker}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}
