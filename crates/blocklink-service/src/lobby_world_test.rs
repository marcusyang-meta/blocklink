//! Opt-in real game acceptance; never changes authentication in a production instance.
use super::*;
use anyhow::ensure;

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
            std::thread::sleep(Duration::from_secs(60));
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

fn wait_log(path: &Path, marker: &str, seconds: u64) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    loop {
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
