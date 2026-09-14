use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
};
fn command(root: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_blocklink"));
    c.arg("--root").arg(root);
    c
}
fn run(root: &Path, args: &[&str]) -> Value {
    let out = command(root).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn real_cli_supports_independent_instances_without_node() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("根目录 with spaces");
    let id = run(
        &root,
        &[
            "create",
            "--name",
            "测试",
            "--loader",
            "fabric",
            "--loader-version",
            "0.16.10",
        ],
    )["instanceId"]
        .as_str()
        .unwrap()
        .to_owned();
    let file = tmp.path().join("测试.jar");
    fs::write(&file, b"PK\x03\x04cli-test").unwrap();
    run(
        &root,
        &[
            "add",
            &id,
            file.to_str().unwrap(),
            "--mod-id",
            "cli-test",
            "--version",
            "1.0",
        ],
    );
    run(&root, &["verify", &id]);
    let list = run(&root, &["list"]);
    assert_eq!(list[0]["name"], "测试");
    assert_eq!(list[0]["runtime"]["memoryMiB"], 4096);
}

#[test]
fn externally_killed_process_is_recovered_at_each_commit_boundary() {
    for point in [
        "prepared",
        "old-moved",
        "new-moved",
        "lock-written",
        "receipt-written",
        "committed",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        let id = run(
            &root,
            &[
                "create",
                "--name",
                "kill-test",
                "--loader",
                "fabric",
                "--loader-version",
                "0.16.10",
            ],
        )["instanceId"]
            .as_str()
            .unwrap()
            .to_owned();
        let old = tmp.path().join("old.jar");
        fs::write(&old, b"PK\x03\x04old").unwrap();
        run(
            &root,
            &[
                "add",
                &id,
                old.to_str().unwrap(),
                "--mod-id",
                "test",
                "--version",
                "1.0",
            ],
        );
        let new = tmp.path().join("new.jar");
        fs::write(&new, b"PK\x03\x04new").unwrap();
        let blob = run(&root, &["import", new.to_str().unwrap()]);
        let lock = json!({"schemaVersion":1,"environment":{"minecraft":"1.21.1","loader":{"kind":"fabric","version":"0.16.10"}},"mods":[{"modId":"test","version":"2.0","file":"new.jar","sha512":blob["sha512"],"bytes":blob["bytes"],"side":"both","source":{"kind":"local"},"dependencies":[]}]});
        let lockfile = tmp.path().join("new-lock.json");
        fs::write(&lockfile, serde_json::to_vec(&lock).unwrap()).unwrap();
        let mut child = command(&root)
            .args([
                "apply",
                &id,
                lockfile.to_str().unwrap(),
                "--test-pause-at",
                point,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if line.starts_with("TEST_CHECKPOINT:") {
                    let _ = send.send(line);
                    break;
                }
            }
        });
        let paused = receive.recv_timeout(std::time::Duration::from_secs(15));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(paused.is_ok(), "Failed to reach {point}");
        let result = run(&root, &["recover"]);
        assert_eq!(result["recovered"].as_array().unwrap().len(), 1);
        run(&root, &["verify", &id]);
        let lock: Value = serde_json::from_slice(
            &fs::read(root.join("instances").join(&id).join("blocklink.lock.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            lock["mods"][0]["version"],
            if point == "committed" { "2.0" } else { "1.0" },
            "{point}"
        );
    }
}
