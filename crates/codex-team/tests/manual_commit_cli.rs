//! Public-CLI coverage for the supervised manual-commit lifecycle.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod support;

fn binary() -> PathBuf {
    support::codex_team_binary()
}

struct TestRepo(PathBuf);

impl TestRepo {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock must be after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "codex-team-it-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create disposable repository");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn path_text(&self) -> &str {
        self.0.to_str().expect("temporary path must be UTF-8")
    }

    fn run(&self, program: &str, args: &[&str]) -> Output {
        Command::new(program)
            .args(args)
            .current_dir(self.path())
            .output()
            .unwrap_or_else(|error| panic!("run {program} {args:?}: {error}"))
    }

    fn team(&self, args: &[&str]) -> Output {
        Command::new(binary())
            .args(args)
            .env("CODEX_TEAM_TEST_ISOLATION_READY", "1")
            .current_dir(self.path())
            .output()
            .unwrap_or_else(|error| panic!("run codex-team {args:?}: {error}"))
    }

    fn git(&self, args: &[&str]) -> String {
        let output = self.run("git", args);
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("git stdout must be UTF-8")
            .trim()
            .to_string()
    }

    fn assert_clean_with_empty_index(&self) {
        assert_eq!(self.git(&["status", "--porcelain=v1"]), "");
        assert_eq!(self.git(&["diff", "--cached", "--name-only"]), "");
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn success_json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "codex-team failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "codex-team stdout was not one JSON object ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn occurrence_count(text: &str, needle: &str) -> usize {
    text.match_indices(needle).count()
}

#[cfg(unix)]
fn wait_for_path(path: &Path) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn manual_pending_commit_requires_exact_separate_cli_approval() {
    let repo = TestRepo::new("manual-commit-cli");
    repo.git(&["init", "-q"]);
    repo.git(&["config", "user.email", "test@example.invalid"]);
    repo.git(&["config", "user.name", "CLI Integration Test"]);
    repo.git(&["commit", "--allow-empty", "-qm", "initial"]);

    let init = repo.team(&[
        "init",
        "--repo",
        repo.path_text(),
        "--mission",
        "exercise supervised manual commit",
        "--commit-mode",
        "manual",
        "--require-clean-start",
    ]);
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    #[cfg(unix)]
    let save_failure_release = {
        use std::os::unix::fs::PermissionsExt as _;

        let release = std::env::temp_dir().join(format!(
            "codex-team-manual-recovery-release-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock must be after epoch")
                .as_nanos()
        ));
        let filter = repo.path().join(".orchestrator/manual-recovery-filter.sh");
        fs::write(
            &filter,
            format!(
                "#!/bin/sh\nif test -f .orchestrator/arm-state-save-failure && test -f .orchestrator/pending-commit-recovery.json; then\n  : > .orchestrator/filter-entered\n  while test ! -f '{}'; do sleep 0.01; done\nfi\ncat\n",
                release.display()
            ),
        )
        .expect("write delayed clean filter");
        let mut permissions = fs::metadata(&filter)
            .expect("filter metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&filter, permissions).expect("make filter executable");
        fs::write(
            repo.path().join(".gitattributes"),
            "NEXT_STEPS.md filter=codex-team-manual-recovery\n",
        )
        .expect("write test attributes");
        repo.git(&[
            "config",
            "filter.codex-team-manual-recovery.clean",
            filter.to_str().expect("filter path must be UTF-8"),
        ]);
        repo.git(&[
            "config",
            "filter.codex-team-manual-recovery.required",
            "true",
        ]);
        release
    };

    fs::write(
        repo.path().join("NEXT_STEPS.md"),
        r#"# NEXT_STEPS

### CLI-MANUAL-001 - Capture Read-Only Evidence
- Task ID: `CLI-MANUAL-001`
- Title: Capture Read-Only Evidence
- Objective: Capture command-only verification evidence without provider traffic.
- Why: It proves the public supervised commit boundary end to end.
- Status: Approved.
- Task type: verification
- Review tier: ai_review_on_failure
- Automation level: ai_worker_rust_review
- Allowed files:
  - `NEXT_STEPS.md`
- Forbidden files/actions:
  - Do not edit source files.
- Exact steps:
  1. Run the command-only verification recipe.
- Verification commands:
  - `git diff --check`
- Success criteria:
  - Rust records a passing evidence receipt.
- Stop conditions:
  - Stop if the command cannot run without provider traffic.
"#,
    )
    .expect("write deterministic task");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-qm", "manual flow fixture"]);

    let base_head = repo.git(&["rev-parse", "HEAD"]);
    let base_commit_count: usize = repo
        .git(&["rev-list", "--count", "HEAD"])
        .parse()
        .expect("commit count must be numeric");
    repo.assert_clean_with_empty_index();

    let cycle = success_json(repo.team(&["step", "--repo", repo.path_text(), "--json"]));
    assert_eq!(cycle["outcome"], "paused");
    assert_eq!(cycle["commit_mode"], "manual");
    assert_eq!(cycle["require_clean_start"], true);
    assert_eq!(cycle["pending_commit"]["task_id"], "CLI-MANUAL-001");
    assert_eq!(
        cycle["pending_commit"]["dirty_paths"],
        serde_json::json!(["NEXT_STEPS.md"])
    );
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), base_head);
    assert_eq!(
        repo.git(&["rev-list", "--count", "HEAD"]),
        base_commit_count.to_string()
    );
    repo.assert_clean_with_empty_index();

    let reviewed =
        success_json(repo.team(&["status", "--repo", repo.path_text(), "--json", "--observe"]));
    let pending_id = reviewed["pending_commit"]["id"]
        .as_str()
        .expect("pending ID")
        .to_string();
    let pending_digest = reviewed["pending_commit"]["digest"]
        .as_str()
        .expect("pending digest")
        .to_string();
    let reviewed_revision = reviewed["revision"]
        .as_str()
        .expect("reviewed revision")
        .to_string();

    let stale_revision = "0".repeat(64);
    let stale = repo.team(&[
        "commit-pending",
        "--repo",
        repo.path_text(),
        "--id",
        &pending_id,
        "--expected-digest",
        &pending_digest,
        "--expected-revision",
        &stale_revision,
        "--json",
    ]);
    assert!(
        !stale.status.success(),
        "stale approval unexpectedly succeeded"
    );
    assert!(String::from_utf8_lossy(&stale.stderr).contains("repository revision is stale"));
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), base_head);
    assert_eq!(
        repo.git(&["rev-list", "--count", "HEAD"]),
        base_commit_count.to_string()
    );
    repo.assert_clean_with_empty_index();
    let after_stale =
        success_json(repo.team(&["status", "--repo", repo.path_text(), "--json", "--observe"]));
    assert_eq!(after_stale, reviewed);

    #[cfg(unix)]
    let committed = {
        use std::os::unix::fs::PermissionsExt as _;

        let orch = repo.path().join(".orchestrator");
        let entered = orch.join("filter-entered");
        fs::write(orch.join("arm-state-save-failure"), b"armed\n").expect("arm state-save failure");
        let child = Command::new(binary())
            .args([
                "commit-pending",
                "--repo",
                repo.path_text(),
                "--id",
                &pending_id,
                "--expected-digest",
                &pending_digest,
                "--expected-revision",
                &reviewed_revision,
                "--json",
            ])
            .current_dir(repo.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn first pending approval");
        wait_for_path(&entered);
        let original_permissions = fs::metadata(&orch)
            .expect("orchestrator directory metadata")
            .permissions();
        let mut read_only = original_permissions.clone();
        read_only.set_mode(0o555);
        fs::set_permissions(&orch, read_only).expect("make state directory read-only");
        fs::write(&save_failure_release, b"continue\n").expect("release clean filter");
        let failed = child
            .wait_with_output()
            .expect("wait for failed state save");
        fs::set_permissions(&orch, original_permissions)
            .expect("restore orchestrator directory permissions");
        let _ = fs::remove_file(&save_failure_release);
        let _ = fs::remove_file(orch.join("arm-state-save-failure"));
        let _ = fs::remove_file(&entered);

        assert!(
            !failed.status.success(),
            "state-save fault unexpectedly reported success"
        );
        assert!(
            String::from_utf8_lossy(&failed.stderr).contains("state could not be saved"),
            "unexpected state-save failure: {}",
            String::from_utf8_lossy(&failed.stderr)
        );
        assert_ne!(repo.git(&["rev-parse", "HEAD"]), base_head);
        assert_eq!(
            repo.git(&["rev-list", "--count", "HEAD"]),
            (base_commit_count + 1).to_string()
        );

        // A fresh observation after Git advanced is not the approval that
        // authorized the commit. It must not be able to consume recovery,
        // even though it matches the repository's current revision.
        let post_commit_status =
            success_json(repo.team(&["status", "--repo", repo.path_text(), "--json", "--observe"]));
        let post_commit_revision = post_commit_status["revision"]
            .as_str()
            .expect("post-commit status revision");
        assert_ne!(post_commit_revision, reviewed_revision);
        let fresh_but_unapproved = repo.team(&[
            "commit-pending",
            "--repo",
            repo.path_text(),
            "--id",
            &pending_id,
            "--expected-digest",
            &pending_digest,
            "--expected-revision",
            post_commit_revision,
            "--json",
        ]);
        assert!(
            !fresh_but_unapproved.status.success(),
            "fresh post-commit revision consumed the original approval"
        );
        assert_eq!(
            repo.git(&["rev-list", "--count", "HEAD"]),
            (base_commit_count + 1).to_string()
        );

        // Repeat the exact original approval. The current repository revision
        // is now different because Git advanced, so success proves the narrow
        // durable-recovery exception ran before ordinary stale rejection.
        success_json(repo.team(&[
            "commit-pending",
            "--repo",
            repo.path_text(),
            "--id",
            &pending_id,
            "--expected-digest",
            &pending_digest,
            "--expected-revision",
            &reviewed_revision,
            "--json",
        ]))
    };

    #[cfg(not(unix))]
    let committed = success_json(repo.team(&[
        "commit-pending",
        "--repo",
        repo.path_text(),
        "--id",
        &pending_id,
        "--expected-digest",
        &pending_digest,
        "--expected-revision",
        &reviewed_revision,
        "--json",
    ]));
    assert_eq!(committed["outcome"], "committed");
    let committed_head = committed["commit_sha"].as_str().expect("committed SHA");
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), committed_head);
    assert_eq!(repo.git(&["rev-parse", "HEAD^"]), base_head);
    assert_eq!(
        repo.git(&["rev-list", "--count", "HEAD"]),
        (base_commit_count + 1).to_string()
    );
    repo.assert_clean_with_empty_index();

    let completed =
        success_json(repo.team(&["status", "--repo", repo.path_text(), "--json", "--observe"]));
    assert!(completed["pending_commit"].is_null());
    assert_eq!(completed["queue"], serde_json::json!([]));
    let next_steps =
        fs::read_to_string(repo.path().join("NEXT_STEPS.md")).expect("read completed task record");
    assert!(next_steps.contains("- Status: Done."));
    assert_eq!(occurrence_count(&next_steps, "- Status: Done."), 1);
    assert!(next_steps.contains("Rust evidence-job notes"));

    let replay = repo.team(&[
        "commit-pending",
        "--repo",
        repo.path_text(),
        "--id",
        &pending_id,
        "--expected-digest",
        &pending_digest,
        "--expected-revision",
        committed["revision"]
            .as_str()
            .expect("post-commit revision"),
        "--json",
    ]);
    assert!(
        !replay.status.success(),
        "cleared intent was committed twice"
    );
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), committed_head);
    assert_eq!(
        repo.git(&["rev-list", "--count", "HEAD"]),
        (base_commit_count + 1).to_string()
    );
    repo.assert_clean_with_empty_index();
}
