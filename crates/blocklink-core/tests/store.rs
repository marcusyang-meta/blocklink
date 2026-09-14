use blocklink_core::*;
use blocklink_model::*;
use std::{fs, path::Path};

fn instance() -> Instance {
    Instance {
        schema_version: 1,
        instance_id: uuid::Uuid::new_v4().to_string(),
        name: "测试实例".into(),
        minecraft: "1.21.1".into(),
        loader: Loader::Fabric {
            version: "0.16.10".into(),
        },
        runtime: Runtime {
            java: JavaMode::Auto,
            memory_mi_b: 4096,
        },
        storage: Storage {
            link_mode: LinkMode::Auto,
        },
        mods: vec![],
        server: None,
    }
}
fn fixture(w: &Workspace, path: &Path, name: &str, body: &str) -> Artifact {
    let file = path.join(name);
    let bytes = [b"PK\x03\x04".as_slice(), body.as_bytes()].concat();
    fs::write(&file, bytes).unwrap();
    let b = w.import_jar(&file).unwrap();
    Artifact {
        mod_id: "example".into(),
        version: body.into(),
        file: name.into(),
        sha512: b.sha512,
        bytes: b.bytes,
        side: Side::Both,
        source: Source::Local,
        dependencies: vec![],
    }
}
fn lock(m: Artifact) -> Lockfile {
    Lockfile {
        content: None,
        schema_version: 1,
        environment: Environment {
            minecraft: "1.21.1".into(),
            loader: Loader::Fabric {
                version: "0.16.10".into(),
            },
        },
        mods: vec![m],
    }
}

#[test]
fn duplicate_import_reuses_one_blob_and_multiple_instances_link_it() {
    let tmp = tempfile::tempdir().unwrap();
    let w = Workspace::open(tmp.path().join("data")).unwrap();
    let m = fixture(&w, tmp.path(), "first.jar", "1.0");
    let second = w.import_jar(tmp.path().join("first.jar")).unwrap();
    assert!(second.reused);
    let a = instance();
    let b = instance();
    for i in [&a, &b] {
        w.create_instance(i).unwrap();
        let r = w
            .sync(&i.instance_id, &lock(m.clone()), LinkMode::Hardlink)
            .unwrap();
        assert_eq!(r.linked, 1);
        w.verify_instance(&i.instance_id).unwrap();
    }
    fs::write(
        w.instance_dir(&a.instance_id)
            .unwrap()
            .join("game/mods/first.jar"),
        b"changed",
    )
    .unwrap();
    assert_eq!(
        fs::read(
            w.instance_dir(&b.instance_id)
                .unwrap()
                .join("game/mods/first.jar")
        )
        .unwrap(),
        b"changed"
    );
    assert!(matches!(
        w.verify_instance(&b.instance_id),
        Err(Error::HashMismatch(_))
    ));
}

#[test]
fn upgrade_preserves_unmanaged_data_and_isolates_other_instances() {
    let tmp = tempfile::tempdir().unwrap();
    let w = Workspace::open(tmp.path().join("data")).unwrap();
    let old = fixture(&w, tmp.path(), "old.jar", "1.0");
    let new = fixture(&w, tmp.path(), "new.jar", "2.0");
    let a = instance();
    let b = instance();
    for i in [&a, &b] {
        w.create_instance(i).unwrap();
        w.sync(&i.instance_id, &lock(old.clone()), LinkMode::Hardlink)
            .unwrap();
    }
    let mods = w.instance_dir(&a.instance_id).unwrap().join("game/mods");
    fs::write(mods.join("personal.txt"), b"keep").unwrap();
    w.sync(&a.instance_id, &lock(new), LinkMode::Hardlink)
        .unwrap();
    assert!(!mods.join("old.jar").exists());
    assert_eq!(fs::read(mods.join("personal.txt")).unwrap(), b"keep");
    assert!(w
        .instance_dir(&b.instance_id)
        .unwrap()
        .join("game/mods/old.jar")
        .exists());
    w.verify_instance(&b.instance_id).unwrap();
}

#[test]
fn damaged_store_blocks_commit_and_copy_mode_does_not_share_mutations() {
    let tmp = tempfile::tempdir().unwrap();
    let w = Workspace::open(tmp.path().join("data")).unwrap();
    let old = fixture(&w, tmp.path(), "old.jar", "1.0");
    let new = fixture(&w, tmp.path(), "new.jar", "2.0");
    let i = instance();
    w.create_instance(&i).unwrap();
    let r = w
        .sync(&i.instance_id, &lock(old.clone()), LinkMode::Copy)
        .unwrap();
    assert_eq!(r.copied, 1);
    fs::write(w.blob_path(&old.sha512).unwrap(), b"broken").unwrap();
    assert_ne!(
        fs::read(
            w.instance_dir(&i.instance_id)
                .unwrap()
                .join("game/mods/old.jar")
        )
        .unwrap(),
        b"broken"
    );
    fs::write(w.blob_path(&new.sha512).unwrap(), b"broken").unwrap();
    assert!(matches!(
        w.sync(&i.instance_id, &lock(new), LinkMode::Copy),
        Err(Error::HashMismatch(_))
    ));
    assert!(w
        .instance_dir(&i.instance_id)
        .unwrap()
        .join("game/mods/old.jar")
        .exists());
}

#[test]
fn reimport_repairs_store_by_replacing_inode_not_overwriting_links() {
    let tmp = tempfile::tempdir().unwrap();
    let w = Workspace::open(tmp.path().join("data")).unwrap();
    let m = fixture(&w, tmp.path(), "mod.jar", "1.0");
    let i = instance();
    w.create_instance(&i).unwrap();
    w.sync(&i.instance_id, &lock(m.clone()), LinkMode::Hardlink)
        .unwrap();
    let active = w
        .instance_dir(&i.instance_id)
        .unwrap()
        .join("game/mods/mod.jar");
    fs::write(&active, b"broken").unwrap();
    assert!(!w.import_jar(tmp.path().join("mod.jar")).unwrap().reused);
    assert_eq!(fs::read(&active).unwrap(), b"broken");
    w.sync(&i.instance_id, &lock(m), LinkMode::Hardlink)
        .unwrap();
    w.verify_instance(&i.instance_id).unwrap();
}

#[test]
fn collision_and_workspace_lease_fail_without_modification() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("data");
    let w = Workspace::open(&root).unwrap();
    assert!(matches!(Workspace::open(&root), Err(Error::Busy)));
    let m = fixture(&w, tmp.path(), "mod.jar", "1.0");
    let i = instance();
    w.create_instance(&i).unwrap();
    let mods = w.instance_dir(&i.instance_id).unwrap().join("game/mods");
    fs::create_dir(&mods).unwrap();
    fs::write(mods.join("mod.jar"), b"mine").unwrap();
    assert!(matches!(
        w.sync(&i.instance_id, &lock(m), LinkMode::Auto),
        Err(Error::Collision(_))
    ));
    assert_eq!(fs::read(mods.join("mod.jar")).unwrap(), b"mine");
    drop(w);
    assert!(Workspace::open(&root).is_ok());
}

#[test]
fn interrupted_transactions_recover_each_checkpoint() {
    for point in [
        Checkpoint::Prepared,
        Checkpoint::OldMoved,
        Checkpoint::NewMoved,
        Checkpoint::LockWritten,
        Checkpoint::ReceiptWritten,
        Checkpoint::Committed,
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let w = Workspace::open(tmp.path().join("data")).unwrap();
        let old = fixture(&w, tmp.path(), "old.jar", "1.0");
        let new = fixture(&w, tmp.path(), "new.jar", "2.0");
        let i = instance();
        w.create_instance(&i).unwrap();
        w.sync(&i.instance_id, &lock(old), LinkMode::Auto).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            w.sync_with_checkpoints(&i.instance_id, &lock(new), LinkMode::Auto, |p| {
                if p == point {
                    panic!("simulated interruption");
                }
            })
        }));
        assert!(result.is_err());
        assert!(matches!(
            w.verify_instance(&i.instance_id),
            Err(Error::RecoveryRequired(_))
        ));
        let events = w.recover_all().unwrap();
        assert_eq!(events.len(), 1);
        w.verify_instance(&i.instance_id).unwrap();
        let expected = if point == Checkpoint::Committed {
            "2.0"
        } else {
            "1.0"
        };
        assert_eq!(
            w.read_lock(&i.instance_id).unwrap().unwrap().mods[0].version,
            expected
        );
        assert!(w.recover_all().unwrap().is_empty());
    }
}

#[test]
fn initial_install_can_roll_back_to_no_lockfile() {
    let tmp = tempfile::tempdir().unwrap();
    let w = Workspace::open(tmp.path().join("data")).unwrap();
    let m = fixture(&w, tmp.path(), "mod.jar", "1.0");
    let i = instance();
    w.create_instance(&i).unwrap();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        w.sync_with_checkpoints(&i.instance_id, &lock(m), LinkMode::Auto, |p| {
            if p == Checkpoint::ReceiptWritten {
                panic!("stop");
            }
        })
    }));
    w.recover_all().unwrap();
    assert!(w.read_lock(&i.instance_id).unwrap().is_none());
    assert!(!w
        .instance_dir(&i.instance_id)
        .unwrap()
        .join("game/mods")
        .exists());
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed_during_preservation() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let w = Workspace::open(tmp.path().join("data")).unwrap();
    let m = fixture(&w, tmp.path(), "mod.jar", "1.0");
    let i = instance();
    w.create_instance(&i).unwrap();
    let mods = w.instance_dir(&i.instance_id).unwrap().join("game/mods");
    fs::create_dir(&mods).unwrap();
    symlink(tmp.path(), mods.join("outside")).unwrap();
    assert!(matches!(
        w.sync(&i.instance_id, &lock(m), LinkMode::Auto),
        Err(Error::UnsafePath(_))
    ));
    assert!(tmp.path().join("mod.jar").exists());
}
