use crate::config::{
    ConfigStore, ProviderApiKeySource, ProviderManagementEntry, ProviderManagementSnapshot,
};
use crate::provider_probe::{ProviderCheckStatus, ProviderProbeRequest};
use crate::runtime::RuntimePaths;
use serde::Serialize;
use std::collections::BTreeSet;
use std::io::Write;

pub const SCHEMA: &str = "ca.doctor.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckLevel {
    Pass,
    Warning,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub level: CheckLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorProvider {
    pub id: String,
    pub configured_models: usize,
    pub available_models: usize,
    pub credential: String,
    pub connection: String,
    pub price_freshness: String,
    pub billing_readiness: String,
    pub capability: String,
    pub route_eligibility: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tested_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub schema: &'static str,
    pub profile: String,
    pub ready: bool,
    pub checks: Vec<DoctorCheck>,
    pub providers: Vec<DoctorProvider>,
}

impl DoctorReport {
    pub fn render_text(&self) -> String {
        let heading = if self.profile == "default" {
            "ca doctor".to_string()
        } else {
            format!("ca doctor · profile {}", self.profile)
        };
        let mut lines = vec![heading, String::new()];
        for check in &self.checks {
            let label = match check.level {
                CheckLevel::Pass => "PASS",
                CheckLevel::Warning => "WARN",
                CheckLevel::Fail => "FAIL",
            };
            lines.push(format!("[{label}] {} — {}", check.name, check.message));
        }
        lines.push(String::new());
        lines.push("Providers".into());
        if self.providers.is_empty() {
            lines.push("(none configured)".into());
        } else {
            for provider in &self.providers {
                let tested = provider
                    .tested_model
                    .as_deref()
                    .map(|model| format!(" · model {model}"))
                    .unwrap_or_default();
                lines.push(format!(
                    "- {}: {} configured · {} available · {} · {}{}",
                    provider.id,
                    provider.configured_models,
                    provider.available_models,
                    provider.credential,
                    provider.connection,
                    tested
                ));
                lines.push(format!(
                    "  pricing {} · billing {} · capability {} · routing {}",
                    provider.price_freshness,
                    provider.billing_readiness,
                    provider.capability,
                    provider.route_eligibility
                ));
            }
        }
        lines.push(String::new());
        lines.push(if self.ready {
            "Ready".into()
        } else {
            "Not ready — fix the failed checks above".into()
        });
        lines.join("\n")
    }
}

pub async fn diagnose(
    paths: &RuntimePaths,
    config_store: &ConfigStore,
    connect: bool,
) -> DoctorReport {
    let (mut report, snapshot) = diagnose_offline(paths, config_store);
    if connect {
        add_connection_checks(&mut report, snapshot.as_ref(), config_store).await;
    }
    report.ready = !report
        .checks
        .iter()
        .any(|check| check.level == CheckLevel::Fail);
    report
}

pub fn diagnose_offline(
    paths: &RuntimePaths,
    config_store: &ConfigStore,
) -> (DoctorReport, Option<ProviderManagementSnapshot>) {
    let mut checks = Vec::new();
    checks.push(path_check("Runtime home", &paths.home, true));
    checks.push(path_check("Config directory", &paths.config_root, false));
    checks.push(path_check("Data directory", &paths.data_dir, false));

    let config_path = config_store.path();
    let snapshot = if !config_path.is_file() && config_store.creates_on_load() {
        checks.push(fail("Config", "config.toml is missing"));
        None
    } else {
        match config_store.provider_management_snapshot_offline() {
            Ok(snapshot) => {
                checks.push(if config_path.is_file() {
                    pass("Config", "configuration parsed successfully")
                } else {
                    warning(
                        "Config",
                        "profile file is missing; using in-memory defaults until the first save",
                    )
                });
                Some(snapshot)
            }
            Err(_) => {
                checks.push(fail("Config", "config.toml could not be parsed safely"));
                None
            }
        }
    };

    let mut providers = Vec::new();
    if let Some(snapshot) = snapshot.as_ref() {
        let mut entries = snapshot.providers.iter().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            let left_current = left.id == snapshot.default_provider;
            let right_current = right.id == snapshot.default_provider;
            right_current
                .cmp(&left_current)
                .then_with(|| left.id.cmp(&right.id))
        });
        let billing = config_store.provider_billing_metadata().unwrap_or_default();
        let catalog =
            crate::pricing::PricingStore::for_profile_label(paths, config_store.profile_label())
                .load();
        let now = chrono::Utc::now();
        providers = entries
            .iter()
            .map(|entry| doctor_provider(entry, catalog.as_ref().ok(), &billing, now))
            .collect();

        match &catalog {
            Ok(catalog) => {
                let fresh = catalog
                    .cards
                    .iter()
                    .filter(|card| card.fresh_at(now))
                    .count();
                checks.push(if fresh == catalog.cards.len() {
                    pass(
                        "Official pricing",
                        &format!("all {} delegation cards are fresh", catalog.cards.len()),
                    )
                } else {
                    warning(
                        "Official pricing",
                        &format!(
                            "{fresh} of {} delegation cards are fresh; stale cards cannot route",
                            catalog.cards.len()
                        ),
                    )
                });
            }
            Err(_) => checks.push(warning(
                "Official pricing",
                "catalog is invalid; automatic delegation is disabled",
            )),
        }
        let eligible = providers
            .iter()
            .filter(|provider| provider.route_eligibility == "eligible")
            .count();
        checks.push(if eligible == 0 {
            warning(
                "Delegation routes",
                "no provider has complete credentials, billing, capability, and fresh pricing",
            )
        } else {
            pass(
                "Delegation routes",
                &format!("{eligible} provider(s) have at least one eligible route"),
            )
        });

        if snapshot.providers.is_empty() {
            checks.push(fail("Providers", "no providers are configured"));
        } else {
            let invalid = snapshot
                .providers
                .iter()
                .filter(|entry| !valid_http_endpoint(&entry.base_url))
                .count();
            if invalid == 0 {
                checks.push(pass(
                    "Providers",
                    &format!("{} provider(s) have valid HTTP endpoints", providers.len()),
                ));
            } else {
                checks.push(fail(
                    "Providers",
                    &format!("{invalid} provider endpoint(s) are invalid"),
                ));
            }
        }
        checks.push(default_check(snapshot));
        checks.push(approval_check(&snapshot.approval_mode));
        checks.push(sandbox_check(&snapshot.sandbox_mode));
    }

    checks.push(match render_first_frame() {
        Ok(()) => pass("Terminal render", "first frame rendered in a test terminal"),
        Err(()) => fail("Terminal render", "first frame could not be rendered"),
    });

    let ready = !checks.iter().any(|check| check.level == CheckLevel::Fail);
    (
        DoctorReport {
            schema: SCHEMA,
            profile: config_store.profile_label().into(),
            ready,
            checks,
            providers,
        },
        snapshot,
    )
}

pub fn write_report(
    report: &DoctorReport,
    format: crate::headless::OutputFormat,
    writer: &mut dyn Write,
) -> std::io::Result<()> {
    let output = match format {
        crate::headless::OutputFormat::Text => report.render_text(),
        crate::headless::OutputFormat::Json => serde_json::to_string_pretty(report)
            .expect("doctor report contains only serializable values"),
        crate::headless::OutputFormat::Jsonl => unreachable!("JSONL is rejected during parsing"),
    };
    writeln!(writer, "{output}")
}

async fn add_connection_checks(
    report: &mut DoctorReport,
    snapshot: Option<&ProviderManagementSnapshot>,
    config_store: &ConfigStore,
) {
    let Some(snapshot) = snapshot else {
        report
            .checks
            .push(fail("Provider connections", "config is not usable"));
        return;
    };
    let runtime = match config_store.load() {
        Ok(config) => config,
        Err(_) => {
            report.checks.push(fail(
                "Provider connections",
                "runtime credentials could not be resolved",
            ));
            return;
        }
    };

    let mut failures = 0usize;
    for (index, provider) in report.providers.iter_mut().enumerate() {
        let entry = ordered_entry(snapshot, index);
        let Some(config) = runtime.providers.get(&entry.id).cloned() else {
            provider.connection = "provider unavailable".into();
            failures += 1;
            continue;
        };
        let model = representative_model(snapshot, entry, &config);
        let Some(model) = model else {
            provider.connection = status_text(&ProviderCheckStatus::NeedsModel);
            failures += 1;
            continue;
        };
        let result = crate::provider_probe::probe_provider(ProviderProbeRequest {
            provider: entry.id.clone(),
            model: model.clone(),
            config,
            usage: None,
            request_store: crate::request::RequestStore::default(),
        })
        .await;
        provider.connection = status_text(&result.status);
        provider.tested_model = Some(safe_text(&model));
        if result.status.working_model().is_none() {
            failures += 1;
        }
    }

    if failures == 0 {
        report.checks.push(pass(
            "Provider connections",
            "every configured provider passed one isolated request",
        ));
    } else {
        report.checks.push(fail(
            "Provider connections",
            &format!("{failures} provider(s) did not pass"),
        ));
    }
}

fn ordered_entry(snapshot: &ProviderManagementSnapshot, index: usize) -> &ProviderManagementEntry {
    let mut entries = snapshot.providers.iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        let left_current = left.id == snapshot.default_provider;
        let right_current = right.id == snapshot.default_provider;
        right_current
            .cmp(&left_current)
            .then_with(|| left.id.cmp(&right.id))
    });
    entries[index]
}

fn representative_model(
    snapshot: &ProviderManagementSnapshot,
    entry: &ProviderManagementEntry,
    config: &crate::config::ProviderConfig,
) -> Option<String> {
    if entry.id == snapshot.default_provider
        && snapshot.default_model != "auto"
        && !snapshot.default_model.is_empty()
    {
        return Some(snapshot.default_model.clone());
    }
    entry.configured_models.first().cloned().or_else(|| {
        crate::model_catalog::documented_chat_models(&entry.id, config)
            .into_iter()
            .next()
    })
}

fn doctor_provider(
    entry: &ProviderManagementEntry,
    catalog: Option<&crate::pricing::OfficialPriceCatalog>,
    billing: &std::collections::HashMap<String, crate::pricing::ProviderBillingConfig>,
    now: chrono::DateTime<chrono::Utc>,
) -> DoctorProvider {
    let provider = crate::config::ProviderConfig {
        base_url: entry.base_url.clone(),
        api_key: String::new(),
        protocol: crate::agent::ProviderProtocol::default(),
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        models: entry.configured_models.clone(),
        timeout: crate::config::ProviderTimeoutConfig::default(),
    };
    let mut available = entry
        .configured_models
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    available.extend(crate::model_catalog::documented_chat_models(
        &entry.id, &provider,
    ));
    let cards = catalog
        .into_iter()
        .flat_map(|catalog| catalog.cards.iter())
        .filter(|card| {
            card.provider == entry.id
                && entry
                    .configured_models
                    .iter()
                    .any(|model| model == &card.model)
        })
        .collect::<Vec<_>>();
    let fresh = cards.iter().filter(|card| card.fresh_at(now)).count();
    let capability = if cards.is_empty() {
        "unknown".into()
    } else {
        cards
            .iter()
            .map(|card| card.capability)
            .max()
            .map_or("unknown", crate::pricing::Complexity::label)
            .to_string()
    };
    let billing_readiness =
        crate::pricing::billing_readiness(&entry.id, &entry.base_url, billing.get(&entry.id))
            .to_string();
    let billing_ready =
        crate::pricing::supported_billing_route(&entry.id, &entry.base_url, billing.get(&entry.id));
    DoctorProvider {
        id: safe_text(&entry.id),
        configured_models: entry.configured_models.len(),
        available_models: available.len(),
        credential: credential_text(&entry.api_key_source),
        connection: "not checked".into(),
        price_freshness: if cards.is_empty() {
            "unknown".into()
        } else {
            format!("{fresh}/{} fresh", cards.len())
        },
        billing_readiness,
        capability,
        route_eligibility: if entry.connected && billing_ready && fresh > 0 {
            "eligible".into()
        } else {
            "ineligible".into()
        },
        tested_model: None,
    }
}

fn default_check(snapshot: &ProviderManagementSnapshot) -> DoctorCheck {
    let Some(provider) = snapshot
        .providers
        .iter()
        .find(|provider| provider.id == snapshot.default_provider)
    else {
        return fail("Default model", "default provider is not configured");
    };
    let provider_config = crate::config::ProviderConfig {
        base_url: provider.base_url.clone(),
        api_key: String::new(),
        protocol: crate::agent::ProviderProtocol::default(),
        prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
        models: provider.configured_models.clone(),
        timeout: crate::config::ProviderTimeoutConfig::default(),
    };
    let mut available = provider
        .configured_models
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    available.extend(crate::model_catalog::documented_chat_models(
        &provider.id,
        &provider_config,
    ));
    if snapshot.default_model == "auto" {
        if available.is_empty() {
            fail("Default model", "default provider has no available model")
        } else {
            pass(
                "Default model",
                &format!("{} / auto", safe_text(&provider.id)),
            )
        }
    } else if available.contains(&snapshot.default_model) {
        pass(
            "Default model",
            &format!(
                "{} / {}",
                safe_text(&provider.id),
                safe_text(&snapshot.default_model)
            ),
        )
    } else {
        fail(
            "Default model",
            "default model is not configured or documented",
        )
    }
}

fn approval_check(value: &str) -> DoctorCheck {
    match value {
        "yolo" | "run-everything" | "everything" => warning(
            "Approval mode",
            "run-everything bypasses interactive approval prompts (CA boundaries still apply)",
        ),
        "auto-review" | "autoreview" | "auto" => pass(
            "Approval mode",
            "auto-review uses CA-local safe-tool/command evaluation (not a remote classifier)",
        ),
        "allowlist" | "" => pass(
            "Approval mode",
            "allowlist prompts for unmatched tools",
        ),
        other => warning(
            "Approval mode",
            &format!(
                "unrecognized approval_mode {other:?}; interactive launch treats unknown values as allowlist"
            ),
        ),
    }
}

fn sandbox_check(value: &str) -> DoctorCheck {
    match crate::sandbox::SandboxMode::parse(value) {
        crate::sandbox::SandboxMode::Required => {
            if cfg!(target_os = "macos") && std::path::Path::new("/usr/bin/sandbox-exec").is_file()
            {
                pass("Process sandbox", "required backend is available")
            } else {
                fail(
                    "Process sandbox",
                    "required backend is unavailable; sandboxed tools fail closed",
                )
            }
        }
        crate::sandbox::SandboxMode::PolicyOnly => warning(
            "Process sandbox",
            "policy-only mode does not provide OS process isolation",
        ),
        crate::sandbox::SandboxMode::Off => {
            warning("Process sandbox", "OS process isolation is disabled")
        }
    }
}

fn render_first_frame() -> Result<(), ()> {
    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).map_err(|_| ())?;
    let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
    state.cwd = "/doctor/check".into();
    terminal
        .draw(|frame| crate::tui::render(frame, &state))
        .map(|_| ())
        .map_err(|_| ())
}

fn path_check(name: &str, path: &std::path::Path, required: bool) -> DoctorCheck {
    if path.is_dir() {
        pass(name, "available")
    } else if required {
        fail(name, "missing or not a directory")
    } else {
        warning(name, "not created yet")
    }
}

fn valid_http_endpoint(value: &str) -> bool {
    reqwest::Url::parse(value)
        .ok()
        .is_some_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

fn credential_text(source: &ProviderApiKeySource) -> String {
    match source {
        ProviderApiKeySource::None => "credential none".into(),
        ProviderApiKeySource::Environment(variable) => {
            format!("credential environment {}", safe_text(variable))
        }
        ProviderApiKeySource::Keychain => "credential saved reference".into(),
        ProviderApiKeySource::Hidden => "credential configured".into(),
    }
}

fn status_text(status: &ProviderCheckStatus) -> String {
    match status {
        ProviderCheckStatus::Checking => "testing".into(),
        ProviderCheckStatus::Working { .. } => "working".into(),
        ProviderCheckStatus::MissingEnvironment { variable } => {
            format!("environment {} missing", safe_text(variable))
        }
        ProviderCheckStatus::CredentialRejected => "credential rejected (HTTP 401)".into(),
        ProviderCheckStatus::AccessDenied => "access denied (HTTP 403)".into(),
        ProviderCheckStatus::TimedOut => "timed out".into(),
        ProviderCheckStatus::CannotConnect => "cannot connect".into(),
        ProviderCheckStatus::EndpointNotFound => "endpoint not found (HTTP 404)".into(),
        ProviderCheckStatus::RateLimited => "rate limited (HTTP 429)".into(),
        ProviderCheckStatus::ProviderError {
            status: Some(status),
        } => {
            format!("provider error (HTTP {status})")
        }
        ProviderCheckStatus::ProviderError { status: None } => "provider error".into(),
        ProviderCheckStatus::InvalidResponse => "invalid or incomplete response".into(),
        ProviderCheckStatus::NeedsModel => "needs a model".into(),
        ProviderCheckStatus::AuthenticationRequired => "authentication required".into(),
    }
}

fn safe_text(value: &str) -> String {
    crate::terminal_text::sanitize_terminal_text(value)
}

fn pass(name: &str, message: &str) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        level: CheckLevel::Pass,
        message: message.into(),
    }
}

fn warning(name: &str, message: &str) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        level: CheckLevel::Warning,
        message: message.into(),
    }
}

fn fail(name: &str, message: &str) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        level: CheckLevel::Fail,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(home: &std::path::Path, api_key: &str) -> (RuntimePaths, ConfigStore) {
        let paths = RuntimePaths::from_home(home);
        std::fs::create_dir_all(&paths.config_root).unwrap();
        std::fs::write(
            &paths.config_file,
            format!(
                r#"[defaults]
provider = "safe"
model = "safe-model"

[agent]
sandbox_mode = "policy-only"
approval_mode = "auto-review"

[providers.safe]
base_url = "http://127.0.0.1:9/v1"
api_key = "{api_key}"
models = ["safe-model"]
"#
            ),
        )
        .unwrap();
        let store = ConfigStore::new(paths.config_file.clone());
        (paths, store)
    }

    #[tokio::test]
    async fn offline_report_is_safe_and_does_not_create_runtime_data() {
        let home = tempfile::tempdir().unwrap();
        let secret = "literal-secret-must-not-escape";
        let (paths, store) = write_config(home.path(), secret);

        let report = diagnose(&paths, &store, false).await;
        let text = report.render_text();
        let json = serde_json::to_string(&report).unwrap();

        assert!(report.ready);
        assert!(text.contains("credential configured"));
        assert!(text.contains("auto-review uses CA-local safe-tool/command evaluation"));
        assert!(text.contains("policy-only mode does not provide OS process isolation"));
        assert!(!text.contains(secret));
        assert!(!json.contains(secret));
        assert!(!paths.data_dir.exists());
    }

    #[tokio::test]
    async fn missing_config_is_a_failure_and_is_not_created() {
        let home = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(home.path());
        let store = ConfigStore::new(paths.config_file.clone());

        let report = diagnose(&paths, &store, false).await;

        assert!(!report.ready);
        assert!(report.render_text().contains("config.toml is missing"));
        assert!(!paths.config_file.exists());
    }

    #[tokio::test]
    async fn missing_named_profile_uses_defaults_without_creating_the_file() {
        let home = tempfile::tempdir().unwrap();
        let profile = crate::runtime::ActiveProfile::Named("work".into());
        let paths = RuntimePaths::from_home(home.path()).for_profile(&profile);
        let store = ConfigStore::for_profile(paths.config_file.clone(), profile);

        let report = diagnose(&paths, &store, false).await;

        assert_eq!(report.profile, "work");
        assert!(report.render_text().starts_with("ca doctor · profile work"));
        assert!(report
            .render_text()
            .contains("using in-memory defaults until the first save"));
        assert!(!paths.config_file.exists());
    }

    #[test]
    fn json_report_has_a_stable_schema() {
        let report = DoctorReport {
            schema: SCHEMA,
            profile: "default".into(),
            ready: true,
            checks: vec![pass("Config", "ready")],
            providers: Vec::new(),
        };
        let mut output = Vec::new();
        write_report(&report, crate::headless::OutputFormat::Json, &mut output).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(value["schema"], SCHEMA);
        assert_eq!(value["ready"], true);
    }

    #[test]
    fn offline_mimo_diagnostics_report_incomplete_token_plan_without_reading_secret() {
        let home = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(home.path());
        std::fs::create_dir_all(&paths.config_root).unwrap();
        let write = |key: &str| {
            std::fs::write(
                &paths.config_file,
                format!(
                    r#"[defaults]
provider = "mimo"
model = "mimo-v2.5-pro"

[providers.mimo]
base_url = "https://token-plan-sgp.xiaomimimo.com/v1"
api_key = "{key}"
models = ["mimo-v2.5-pro"]
"#,
                ),
            )
            .unwrap();
        };
        let store = ConfigStore::new(paths.config_file.clone());

        write("tp-token-plan-secret-never-render");
        let (incomplete, _) = diagnose_offline(&paths, &store);
        let mimo = incomplete
            .providers
            .iter()
            .find(|provider| provider.id == "mimo")
            .unwrap();
        assert_eq!(
            mimo.billing_readiness,
            "Token Plan endpoint detected; allocation metadata incomplete"
        );
        assert_eq!(mimo.route_eligibility, "ineligible");
        assert!(!incomplete
            .render_text()
            .contains("token-plan-secret-never-render"));
    }

    #[test]
    fn offline_deepseek_diagnostics_require_the_official_endpoint_without_reading_secret() {
        let home = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_home(home.path());
        std::fs::create_dir_all(&paths.config_root).unwrap();
        // Write a fresh pricing catalog so delegation routes pass the freshness check.
        let catalog_path = paths.data_dir.join("pricing/catalog.json");
        std::fs::create_dir_all(catalog_path.parent().unwrap()).unwrap();
        let mut catalog = crate::pricing::OfficialPriceCatalog::seeded();
        for card in &mut catalog.cards {
            card.verified_at = chrono::Utc::now();
        }
        std::fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog).unwrap()).unwrap();
        let write = |base_url: &str| {
            std::fs::write(
                &paths.config_file,
                format!(
                    r#"[defaults]
provider = "deepseek"
model = "deepseek-v4-pro"

[providers.deepseek]
base_url = "{base_url}"
api_key = "deepseek-secret-never-render"
models = ["deepseek-v4-flash", "deepseek-v4-pro"]
"#,
                ),
            )
            .unwrap();
        };
        let store = ConfigStore::new(paths.config_file.clone());

        write("https://api.deepseek.com/v1");
        let (official, _) = diagnose_offline(&paths, &store);
        let deepseek = official
            .providers
            .iter()
            .find(|provider| provider.id == "deepseek")
            .unwrap();
        assert_eq!(deepseek.billing_readiness, "metered USD ready");
        assert_eq!(deepseek.capability, "standard");
        assert_eq!(deepseek.route_eligibility, "eligible");
        assert!(!official
            .render_text()
            .contains("deepseek-secret-never-render"));

        write("https://proxy.example/v1");
        let (custom, _) = diagnose_offline(&paths, &store);
        let deepseek = custom
            .providers
            .iter()
            .find(|provider| provider.id == "deepseek")
            .unwrap();
        assert_eq!(
            deepseek.billing_readiness,
            "custom endpoint; delegation ineligible"
        );
        assert_eq!(deepseek.route_eligibility, "ineligible");
        assert!(!custom
            .render_text()
            .contains("deepseek-secret-never-render"));
    }
}
