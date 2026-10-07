//! Shared helpers for integration tests.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Create a temporary directory for a test repo and return its path.
///
/// The directory name includes a nanosecond nonce for uniqueness.
/// Also runs `git init` so `ensure_git_ready` passes with default config.
pub fn temp_repo(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("codex-team-it-{name}-{nonce}"));
    std::fs::create_dir_all(&dir).expect("failed to create temp repo dir");
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&dir)
        .output()
        .expect("failed to git init temp repo");
    dir
}

/// Initialize the orchestrator in the given repo directory.
pub fn init_orchestrator(repo: &std::path::Path, mission: &str) {
    codex_team::app::init(repo.to_path_buf(), mission.to_string(), None, false)
        .expect("init should succeed");
}

/// Patch `.orchestrator/config.toml` to use `ollama` for every role so tests
/// do not need a real `codex` binary on PATH.
pub fn patch_config_no_codex(repo: &std::path::Path) {
    let config_path = repo.join(".orchestrator/config.toml");
    let mut cfg: toml::Value =
        toml::from_str(&std::fs::read_to_string(&config_path).expect("read config"))
            .expect("parse config");
    // Set codex_bin to a sentinel (never actually invoked when no role uses codex).
    if let Some(table) = cfg.as_table_mut() {
        table.insert(
            "codex_bin".to_string(),
            toml::Value::String("codex-stub".to_string()),
        );
    }
    // Override every role's provider to "ollama".
    if let Some(roles) = cfg.get_mut("roles").and_then(|r| r.as_table_mut()) {
        for (_name, role) in roles.iter_mut() {
            if let Some(tbl) = role.as_table_mut() {
                tbl.insert(
                    "provider".to_string(),
                    toml::Value::String("ollama".to_string()),
                );
            }
        }
    }
    // Enable ollama so the doctor "ollama" check passes.
    if let Some(ollama) = cfg.get_mut("ollama").and_then(|o| o.as_table_mut()) {
        ollama.insert("enabled".to_string(), toml::Value::Boolean(true));
    }
    std::fs::write(
        &config_path,
        toml::to_string_pretty(&cfg).expect("serialize config"),
    )
    .expect("write config");
}

/// Patch `.orchestrator/state.json` to set `master_due` and `planner_due`
/// to false so `step()` does not attempt to spawn any AI role.
pub fn patch_state_quiesce(repo: &std::path::Path) {
    let state_path = repo.join(".orchestrator/state.json");
    let mut state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).expect("read state"))
            .expect("parse state");
    state["master_due"] = serde_json::Value::Bool(false);
    state["planner_due"] = serde_json::Value::Bool(false);
    state["needs_reconciliation"] = serde_json::Value::Bool(false);
    std::fs::write(
        &state_path,
        serde_json::to_string_pretty(&state).expect("serialize state"),
    )
    .expect("write state");
}

/// Remove the temp directory, ignoring errors (e.g. on Windows).
pub fn cleanup(repo: &PathBuf) {
    let _ = std::fs::remove_dir_all(repo);
}
