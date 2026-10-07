use std::path::PathBuf;

/// Resolve this build's binary without retaining a previous workspace location.
pub fn codex_team_binary() -> PathBuf {
    let test_binary = std::env::current_exe().expect("locate integration test artifact");
    let dependencies = test_binary.parent().expect("test artifact directory");
    assert_eq!(
        dependencies.file_name(),
        Some(std::ffi::OsStr::new("deps")),
        "integration test must run from its Cargo artifact directory"
    );
    let binary = dependencies
        .parent()
        .expect("Cargo profile directory")
        .join(format!("codex-team{}", std::env::consts::EXE_SUFFIX));
    assert!(
        binary.is_file(),
        "missing Cargo binary artifact {}: build the workspace binary targets before running this integration test",
        binary.display()
    );
    binary
}
