//! Full guidance lifecycle with real workers and a deterministic local provider.
//! Only the freezer's expected source identity is substituted for the tiny
//! engineering project. Production request, budget, trial and activation paths run.
use super::*;
use crate::agent::provider::ProviderProtocol;
use crate::agent::stream::ProviderStateSourceV1;
use codex_team::self_improvement::{
    load_harness_cycle, HarnessLane, HarnessPauseCause, HarnessStage,
};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
#[ignore = "seeds a disposable profile for the manual real-clock PTY integration test"]
fn hrs253_seed_idle_pty_profile() {
    let root = PathBuf::from(std::env::var_os("CA_HARNESS_PTY_FIXTURE").unwrap())
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert!(root
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("ca-hrs253-idle-pty-"));
    let url = std::env::var("CA_HARNESS_PTY_URL").unwrap();
    assert_eq!(url::Url::parse(&url).unwrap().host_str(), Some("127.0.0.1"));
    let home = root.join("home");
    assert!(!home.exists(), "fixture must use a new disposable home");
    std::fs::create_dir(&home).unwrap();
    let paths = crate::runtime::RuntimePaths::from_home(&home);
    let profile = crate::runtime::ActiveProfile::Default;
    let mut catalog = crate::pricing::OfficialPriceCatalog::seeded();
    for card in &mut catalog.cards {
        card.verified_at = chrono::Utc::now();
    }
    crate::pricing::PricingStore::for_paths(&paths, &profile)
        .save(&catalog)
        .unwrap();
    let mut config = crate::config::CaConfig::default();
    config.providers.clear();
    config.providers.insert(
        "openai".into(),
        crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: "synthetic-local-test-token".into(),
            protocol: ProviderProtocol::ChatCompletions,
            prompt_cache_mode: Default::default(),
            models: vec!["gpt-5.6-luna".into()],
            timeout: Default::default(),
        },
    );
    config.defaults.provider = "openai".into();
    config.defaults.model = "gpt-5.6-luna".into();
    config.model_context.insert(
        "openai/gpt-5.6-luna".into(),
        crate::config::ModelContextConfig {
            context_window_tokens: Some(32768),
            compact_at_tokens: None,
        },
    );
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    config.harness_improvement.enabled = true;
    config.harness_improvement.repository = Some(repository.clone());
    config
        .trusted_workspaces
        .push(repository.to_string_lossy().into());
    let directory = home.join(".config/ca");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("config.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    let provider = crate::harness_control::configured_provider(&config, &paths, &profile).unwrap();
    let request = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::Headless,
        "openai",
        &provider.model,
        Vec::new(),
        Vec::new(),
        provider.options.clone(),
    );
    let identity = crate::harness_registry::capture_prepared_on_route(&request, &provider.source)
        .outcome_snapshot();
    for mut outcome in super::tests::outcomes() {
        outcome.model = Some(provider.model.clone());
        outcome.provider = Some("openai".into());
        outcome.harness = identity.clone();
        outcome.harness_epochs = vec![identity.clone()];
        for event in &mut outcome.events {
            if event.kind == "tool_evidence" {
                event.excerpt = "truncate_utf8 panicked at char boundary".into();
            }
        }
        crate::session_outcome::save_outcome(&outcome, &paths.data_dir).unwrap();
    }
    let discovered = crate::harness_discovery::discover(&paths.data_dir, None).unwrap();
    assert!(crate::harness_discovery::select_for_provider(&discovered, &provider).is_some());
    std::fs::write(
        root.join("ready.json"),
        serde_json::to_vec(&serde_json::json!({
            "home":home, "data":paths.data_dir, "repository":repository,
            "build":env!("CA_BUILD_DIGEST"), "source":env!("CA_SOURCE_DIGEST"),
        }))
        .unwrap(),
    )
    .unwrap();
}

async fn request(socket: &mut tokio::net::TcpStream) -> serde_json::Value {
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 8192];
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
        assert!(bytes.len() <= 4 * 1024 * 1024);
        if let Some(boundary) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..boundary]).unwrap();
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            if bytes.len() >= boundary + 4 + length {
                return serde_json::from_slice(&bytes[boundary + 4..boundary + 4 + length])
                    .unwrap();
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires fresh CA_HARNESS_TEST_BINARY and macOS sandbox; runs all 54 trials"]
async fn hrs253_discovered_guidance_generates_pauses_evaluates_promotes_and_rolls_back() {
    full_cycle(false, None).await;
}

#[tokio::test]
#[ignore = "requires fresh CA_HARNESS_TEST_BINARY and macOS sandbox; measures a hint across 54 trials"]
async fn hrs253_measured_hint_generates_evaluates_activates_and_rolls_back() {
    full_cycle(true, None).await;
}

#[tokio::test]
#[ignore = "requires fresh release ca and dedicated VM; freezes the image and runs all 54 VM trials"]
async fn hrs253_vm_guidance_cycle_evaluates_activates_and_rolls_back() {
    let binding = std::env::var_os("CA_HARNESS_VM_TEST_BINDING").expect("explicit VM binding");
    let frozen: codex_team::self_improvement::FrozenHarnessVm =
        serde_json::from_slice(&std::fs::read(binding).unwrap()).unwrap();
    // Pass only the selected profile. Production freezing rehashes the template
    // and binds the current controller; no saved plan or digest is rewritten.
    full_cycle(false, Some(frozen.profile)).await;
}

async fn full_cycle(hint_mode: bool, vm: Option<codex_team::self_improvement::HarnessVmProfile>) {
    let root = tempfile::tempdir().unwrap();
    let repository = root.path().join("repository");
    crate::harness_snapshot::tests::fixture(&repository);
    let real_repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cases = crate::harness_catalog::automatic_cases(real_repository, "unicode_parser_boundary")
        .unwrap();
    std::fs::create_dir_all(repository.join("tests/fixtures/reasoning_pilot")).unwrap();
    for case in cases.iter().take(6) {
        case.materialize(
            &repository
                .join("tests/fixtures/reasoning_pilot")
                .join(&case.fixture_id),
        )
        .unwrap();
    }
    let paths = [
        root.path().join("target"),
        root.path().join("held-a"),
        root.path().join("held-b"),
    ];
    for (case, path) in cases.iter().skip(6).zip(&paths) {
        case.materialize(path).unwrap();
    }
    let source_digest = crate::harness_snapshot::source_digest(&repository).unwrap();
    let binary =
        std::path::PathBuf::from(std::env::var_os("CA_HARNESS_TEST_BINARY").expect("fresh worker"));
    let binary_digest = crate::harness_engineering::artifact_digest(&binary).unwrap();
    let fixture_home = root.path().join("home");
    std::fs::create_dir_all(fixture_home.join(".codex-team")).unwrap();
    let vault = LearningVault::open_at(fixture_home.join(".codex-team/self-improvement")).unwrap();
    let data = root.path().join("data");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let mut provider = crate::harness_budget::tests::frozen();
    provider.context_tokens = 32768;
    provider.source =
        ProviderStateSourceV1::from_route("fake", ProviderProtocol::ChatCompletions, &url).unwrap();
    let prepared = crate::request::PreparedModelRequest::new(
        crate::request::RequestKind::Headless,
        "fake",
        &provider.model,
        Vec::new(),
        Vec::new(),
        provider.options.clone(),
    );
    let identity = crate::harness_registry::capture_prepared_on_route(&prepared, &provider.source)
        .outcome_snapshot();
    for mut record in super::tests::outcomes() {
        record.harness = identity.clone();
        record.harness_epochs = vec![identity.clone()];
        for event in &mut record.events {
            if event.kind == "tool_evidence" {
                event.excerpt = "truncate_utf8 panicked at char boundary".into();
            }
        }
        crate::session_outcome::save_outcome(&record, &data).unwrap();
    }
    let discovery = crate::harness_discovery::discover(&data, Some(&vault)).unwrap();
    let group = crate::harness_discovery::select_for_provider(&discovery, &provider).unwrap();
    assert_eq!(
        group.family_hypothesis.as_deref(),
        Some("unicode_parser_boundary")
    );
    let plan = create_with_source_digest(
        &vault,
        &repository,
        &binary,
        &data,
        &group.evidence_ids,
        provider.clone(),
        crate::harness_guidance::BASELINE.into(),
        "unicode_parser_boundary",
        &paths[0],
        [&paths[1], &paths[2]],
        &[],
        (&source_digest, &binary_digest),
        std::time::Instant::now(),
        true,
        vm.as_ref(),
    )
    .unwrap();
    assert_eq!(plan.cases.len(), 9);
    let candidate = if hint_mode {
        crate::harness_guidance::BASELINE.into()
    } else {
        format!(
            "{} Check Unicode boundaries with focused behavioral tests.",
            crate::harness_guidance::BASELINE
        )
    };
    let proposal = crate::harness_generation::GuidanceProposal {
        candidate_hints: hint_mode.then(|| {
            [crate::harness_hints::ExecutionHint::DependencyProbes]
                .into_iter()
                .collect()
        }),
        candidate_instructions: candidate.clone(),
        evidence_ids: plan.evidence_ids.clone(),
        root_cause: "Repeated Unicode boundary failures".into(),
        prediction: "Improve the frozen boundary target without regression".into(),
    };
    let foreground = Arc::new(AtomicBool::new(false));
    let signal = foreground.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let requests = count.clone();
    let store = vault.clone();
    let frozen = plan.clone();
    let schedule = codex_team::self_improvement::harness_trial_schedule(&plan).unwrap();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let body = request(&mut socket).await;
            let ordinal = requests.fetch_add(1, Ordering::SeqCst);
            let (_, state) = load_harness_cycle(&store, &frozen.id).unwrap();
            assert_eq!(state.reservations.len(), 1);
            assert_eq!(state.active_reservations.len(), 1);
            let (delta, finish) = if ordinal == 0 {
                signal.store(true, Ordering::Release);
                (
                    serde_json::json!({"content":serde_json::to_string(&proposal).unwrap()}),
                    "stop",
                )
            } else {
                let (case_id, _, lane) = &schedule[state.trials.len()];
                let case = cases.iter().find(|case| &case.case.id == case_id).unwrap();
                let messages = body["messages"].as_array().unwrap();
                let system = messages
                    .iter()
                    .filter(|m| m["role"] == "system")
                    .map(|m| m["content"].as_str().unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !hint_mode {
                    assert_eq!(system.contains(&candidate), *lane == HarnessLane::Candidate);
                }
                let calls = messages
                    .iter()
                    .filter_map(|m| m["tool_calls"].as_array())
                    .flatten()
                    .collect::<Vec<_>>();
                let probes = calls
                    .iter()
                    .filter(|call| call["function"]["name"] == "run_command")
                    .count();
                let repaired = calls
                    .iter()
                    .any(|call| call["function"]["name"] == "write_file");
                let no_fix = case.case.predicted_fix
                    && if hint_mode {
                        !system
                            .contains(crate::harness_hints::ExecutionHint::DependencyProbes.text())
                    } else {
                        *lane == HarnessLane::Baseline
                    };
                if hint_mode && case.case.predicted_fix && probes < 3 {
                    // Both lanes get identical probes. Only an actually delivered
                    // reminder changes the provider's subsequent repair behavior.
                    let command = ["which sh", "which ls", "which cat"][probes];
                    (
                        serde_json::json!({"tool_calls":[{"index":0,"id":format!("probe-{probes}"),"type":"function","function":{"name":"run_command","arguments":serde_json::json!({"command":command}).to_string()}}]}),
                        "tool_calls",
                    )
                } else if repaired || no_fix {
                    (
                        serde_json::json!({"content":serde_json::json!({"completed":repaired}).to_string()}),
                        "stop",
                    )
                } else {
                    let repair = std::fs::read_to_string(
                        real_repository
                            .join("tests/reasoning_pilot_golden")
                            .join(&case.fixture_id)
                            .join("src/lib.rs"),
                    )
                    .unwrap();
                    (
                        serde_json::json!({"tool_calls":[{"index":0,"id":"repair","type":"function","function":{"name":"write_file","arguments":serde_json::json!({"path":"src/lib.rs","content":repair}).to_string()}}]}),
                        "tool_calls",
                    )
                }
            };
            let response = format!(
                "data: {}\n\ndata: [DONE]\n\n",
                serde_json::json!({"choices":[{"index":0,"delta":delta,"finish_reason":finish}],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30,"prompt_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},"completion_tokens_details":{"reasoning_tokens":0}}})
            );
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).as_bytes()).await.unwrap();
        }
    });
    // Abort-on-drop owns the listener even when a runner assertion fails.
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let server = Server(server);
    let mut config = crate::config::CaConfig::default();
    config.providers.insert(
        "fake".into(),
        crate::config::ProviderConfig {
            google: None,
            tool_schema_policy: Default::default(),
            base_url: url,
            api_key: "synthetic-test-token".into(),
            protocol: ProviderProtocol::ChatCompletions,
            prompt_cache_mode: Default::default(),
            models: vec![provider.model.clone()],
            timeout: Default::default(),
        },
    );
    let run = || {
        crate::harness_control::run_background(
            vault.clone(),
            plan.id.clone(),
            repository.clone(),
            binary.clone(),
            config.clone(),
            None,
        )
    };
    assert!(
        crate::harness_idle::scope(&vault, &plan.id, foreground.clone(), run())
            .await
            .is_err()
    );
    let (_, paused) = load_harness_cycle(&vault, &plan.id).unwrap();
    assert_eq!(paused.stage, HarnessStage::Paused);
    assert_eq!(paused.pause_cause, Some(HarnessPauseCause::Foreground));
    assert_eq!(paused.charged_requests.len(), 1);
    assert!(paused.trials.is_empty());
    foreground.store(false, Ordering::Release);
    assert!(crate::harness_idle::resume_foreground_pause(&vault, &plan.id).unwrap());
    // Promotion must run in the evaluated executable, not the Rust test host.
    // Keep configuration and machine-vault discovery in this disposable home.
    let config_directory = fixture_home.join(".config/ca");
    std::fs::create_dir_all(&config_directory).unwrap();
    std::fs::write(
        config_directory.join("config.toml"),
        toml::to_string(&config).unwrap(),
    )
    .unwrap();
    let real_home = std::env::var_os("HOME").unwrap();
    let mut child = tokio::process::Command::new(&binary);
    child
        .args(["harness", "run", &plan.id, "--repository"])
        .arg(&repository)
        .arg("--baseline-binary")
        .arg(&binary)
        .arg("--json")
        .env_clear()
        .env("HOME", &fixture_home)
        .env("XDG_CONFIG_HOME", fixture_home.join(".config"))
        .env("XDG_DATA_HOME", fixture_home.join(".local/share"))
        .env("XDG_CACHE_HOME", fixture_home.join(".cache"))
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env(
            "CARGO_HOME",
            std::env::var_os("CARGO_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from(&real_home).join(".cargo")),
        )
        .env(
            "RUSTUP_HOME",
            std::env::var_os("RUSTUP_HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from(&real_home).join(".rustup")),
        )
        .current_dir(&repository)
        .kill_on_drop(true);
    let output = child.output().await.unwrap();
    assert!(
        output.status.success(),
        "full CLI runner failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!server.0.is_finished(), "fake provider assertions failed");
    let (_, state) = load_harness_cycle(&vault, &plan.id).unwrap();
    assert_eq!(state.stage, HarnessStage::Promoted);
    assert_eq!(state.trials.len(), 54);
    assert_eq!(
        state
            .trials
            .iter()
            .filter(|trial| trial.verified_pass)
            .count(),
        51
    );
    assert!(state
        .trials
        .iter()
        .all(|trial| !trial.false_completion && !trial.infrastructure_failure));
    let expected_requests = if hint_mode { 124 } else { 106 };
    assert_eq!(count.load(Ordering::SeqCst), expected_requests);
    assert_eq!(state.charged_requests.len(), expected_requests);
    assert!(state.reservations.is_empty() && state.active_reservations.is_empty());
    assert!(state.settled_microusd <= plan.settings.maximum_microusd);
    assert!(state.active_seconds <= plan.settings.maximum_active_seconds);
    let active_text = crate::harness_guidance::baseline_instructions(
        &vault,
        &provider.source,
        &provider.model,
        &provider.options,
    )
    .unwrap();
    assert_eq!(active_text == crate::harness_guidance::BASELINE, hint_mode);
    let active_hints = crate::harness_guidance::baseline_hints(
        &vault,
        &provider.source,
        &provider.model,
        &provider.options,
    )
    .unwrap();
    assert_eq!(
        active_hints.contains(&crate::harness_hints::ExecutionHint::DependencyProbes),
        hint_mode
    );
    assert!(crate::harness_discovery::discover(&data, Some(&vault))
        .unwrap()
        .groups
        .is_empty());
    crate::harness_guidance::rollback(vault.clone(), &plan.id)
        .await
        .unwrap();
    assert_eq!(
        load_harness_cycle(&vault, &plan.id).unwrap().1.stage,
        HarnessStage::RolledBack
    );
    assert_eq!(
        crate::harness_guidance::baseline_instructions(
            &vault,
            &provider.source,
            &provider.model,
            &provider.options
        )
        .unwrap(),
        crate::harness_guidance::BASELINE
    );
    assert!(crate::harness_guidance::baseline_hints(
        &vault,
        &provider.source,
        &provider.model,
        &provider.options
    )
    .unwrap()
    .is_empty());
}
