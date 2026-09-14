use blocklink_model::*;
use serde_json::json;

fn lock() -> Lockfile {
    serde_json::from_value(json!({"schemaVersion":1,"environment":{"minecraft":"1.21.1","loader":{"kind":"fabric","version":"0.16.10"}},"mods":[{"modId":"test","version":"1.0.0","file":"test.jar","sha512":"a".repeat(128),"bytes":100,"side":"both","source":{"kind":"local"},"dependencies":[]}]})).unwrap()
}

#[test]
fn lock_requires_exact_versions() {
    let mut l = lock();
    l.environment.loader = Loader::Fabric {
        version: "recommended".into(),
    };
    assert!(l.validate().is_err());
    l.environment.loader = Loader::Fabric {
        version: "0.16.10".into(),
    };
    l.mods[0].version = "latest".into();
    assert!(l.validate().is_err());
}
#[test]
fn neoforge_manifest_roundtrip_and_loader_isolation() {
    let mut l = lock();
    l.environment.loader = Loader::NeoForge {
        version: "21.1.250".into(),
    };
    l.validate().unwrap();
    let encoded = serde_json::to_value(&l).unwrap();
    assert_eq!(encoded["environment"]["loader"]["kind"], "neoforge");
    let parsed: Lockfile = serde_json::from_value(encoded).unwrap();
    let mut i: Instance =
        serde_json::from_str(include_str!("../../../fixtures/instance.json")).unwrap();
    assert!(i.accepts(&parsed).is_err());
    i.loader = Loader::NeoForge {
        version: "21.1.250".into(),
    };
    assert!(i.accepts(&parsed).is_ok());
    i.loader = Loader::NeoForge {
        version: "21.1.249".into(),
    };
    assert!(i.accepts(&parsed).is_err());
    l.environment.loader = Loader::NeoForge {
        version: "recommended".into(),
    };
    assert!(l.validate().is_err());
}
#[test]
fn forge_and_quilt_remain_distinct_and_version_locked() {
    for kind in ["forge", "quilt"] {
        let mut l = lock();
        l.environment.loader =
            serde_json::from_value(json!({"kind":kind,"version":"1.2.3"})).unwrap();
        let encoded = serde_json::to_value(&l).unwrap();
        let parsed: Lockfile = serde_json::from_value(encoded).unwrap();
        let mut i: Instance =
            serde_json::from_str(include_str!("../../../fixtures/instance.json")).unwrap();
        assert!(i.accepts(&parsed).is_err());
        i.loader = parsed.environment.loader.clone();
        assert!(i.accepts(&parsed).is_ok());
        i.loader = serde_json::from_value(json!({"kind":kind,"version":"1.2.4"})).unwrap();
        assert!(i.accepts(&parsed).is_err());
    }
}
#[test]
fn dependency_graph_and_mod_identity_are_checked() {
    let mut l = lock();
    l.mods[0].dependencies.push(Dependency {
        mod_id: "missing".into(),
        version: "1.0".into(),
    });
    assert!(l.validate().is_err());
    l.mods[0].dependencies.clear();
    let mut duplicate = l.mods[0].clone();
    duplicate.file = "another-name.jar".into();
    l.mods.push(duplicate);
    assert!(l.validate().is_err());
}
#[test]
fn platform_paths_and_unicode_aliases_are_rejected() {
    for name in [
        "../bad.jar",
        "C:\\bad.jar",
        "CON.jar",
        "com1.jar",
        "LPT9.jar",
        ".hidden.jar",
        "a/b.jar",
        "bad\0.jar",
        "a：b.jar",
    ] {
        assert!(validate_filename(name).is_err(), "{name}");
    }
    assert!(validate_filename("中文模组.jar").is_ok());
    let mut l = lock();
    l.mods[0].file = "CAFÉ.jar".into();
    let mut other = l.mods[0].clone();
    other.mod_id = "other".into();
    other.file = "cafe\u{301}.jar".into();
    l.mods.push(other);
    assert!(l.validate().is_err());
}
#[test]
fn unknown_schema_fields_fail_closed() {
    let mut v = serde_json::to_value(lock()).unwrap();
    v["shellCommand"] = json!("execute this");
    assert!(serde_json::from_value::<Lockfile>(v).is_err());
    let mut l = lock();
    l.schema_version = 2;
    assert!(l.validate().is_err());
}
#[test]
fn contract_examples_parse_with_rust_models() {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let i: Instance =
        serde_json::from_slice(&std::fs::read(fixtures.join("instance.json")).unwrap()).unwrap();
    i.validate().unwrap();
    let l: Lockfile =
        serde_json::from_slice(&std::fs::read(fixtures.join("blocklink.lock.json")).unwrap())
            .unwrap();
    l.validate().unwrap();
    i.accepts(&l).unwrap();
    let m: ServerManifest =
        serde_json::from_slice(&std::fs::read(fixtures.join("server-manifest.json")).unwrap())
            .unwrap();
    m.validate().unwrap();
}
