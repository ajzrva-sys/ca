#[path = "../build_support/harness_identity.rs"]
mod identity;
use std::collections::BTreeMap;
use std::path::Path;

fn fixture(root: &Path) {
    for (name, content) in [
        ("build.rs", "build"),
        ("build_support/identity.rs", "helper"),
        ("Cargo.toml", "manifest"),
        ("Cargo.lock", "lock"),
        ("src/main.rs", "main"),
        ("prompts/system.md", "instructions"),
        ("tests/fixtures/codex-model-catalog.json", "models"),
        ("tests/reasoning_pilot_hidden/src/lib.rs", "verifier"),
        ("crates/member/Cargo.toml", "member"),
        ("crates/member/src/lib.rs", "code"),
        ("crates/member/prompts/worker.md", "worker"),
        ("crates/member/schemas/task.json", "schema"),
    ] {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
}
fn digest(root: &Path, settings: &BTreeMap<String, String>, compiler: &str) -> String {
    identity::digest(
        root,
        &identity::input_roots(root).unwrap(),
        settings,
        compiler.as_bytes(),
    )
    .unwrap()
}

#[test]
fn hrs253_embedded_assets_manifests_and_verifier_change_build_identity() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path());
    let settings = BTreeMap::new();
    let before = digest(temp.path(), &settings, "rustc-version");
    for name in [
        "prompts/system.md",
        "crates/member/Cargo.toml",
        "crates/member/prompts/worker.md",
        "crates/member/schemas/task.json",
        "tests/fixtures/codex-model-catalog.json",
        "tests/reasoning_pilot_hidden/src/lib.rs",
        "build_support/identity.rs",
    ] {
        let path = temp.path().join(name);
        let original = std::fs::read(&path).unwrap();
        std::fs::write(&path, b"changed").unwrap();
        assert_ne!(
            before,
            digest(temp.path(), &settings, "rustc-version"),
            "{name}"
        );
        std::fs::write(path, original).unwrap();
    }
}

#[test]
fn hrs253_build_identity_tracks_settings_and_excludes_runtime_and_outputs() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    fixture(first.path());
    fixture(second.path());
    let settings = BTreeMap::new();
    let before = digest(first.path(), &settings, "rustc-version");
    assert_eq!(before, digest(second.path(), &settings, "rustc-version"));
    assert_ne!(before, digest(first.path(), &settings, "other-compiler"));
    for key in [
        "CARGO_FEATURE_EXPERIMENT",
        "CARGO_CFG_TARGET_ARCH",
        "TARGET",
        "OPT_LEVEL",
        "PROFILE",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        assert!(identity::relevant_setting(key));
        assert_ne!(
            before,
            digest(
                first.path(),
                &BTreeMap::from([(key.into(), "changed".into())]),
                "rustc-version"
            )
        );
    }
    assert!(!identity::relevant_setting("API_KEY"));
    assert!(!identity::relevant_setting("HOME"));
    for name in [
        "target/output",
        "crates/member/target/output",
        "crates/member/README.md",
        "runtime/config.toml",
    ] {
        let path = first.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "not a build input").unwrap();
    }
    assert_eq!(before, digest(first.path(), &settings, "rustc-version"));
}

#[test]
fn hrs253_build_identity_tracks_additions_and_rejects_symlink_inputs() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path());
    let settings = BTreeMap::new();
    let before = digest(temp.path(), &settings, "rustc-version");
    let added = temp.path().join("prompts/new.md");
    std::fs::write(&added, "new instructions").unwrap();
    assert_ne!(before, digest(temp.path(), &settings, "rustc-version"));
    std::fs::remove_file(&added).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("../Cargo.toml", &added).unwrap();
        assert!(identity::digest(
            temp.path(),
            &identity::input_roots(temp.path()).unwrap(),
            &settings,
            b"rustc-version"
        )
        .is_err());
    }
}
