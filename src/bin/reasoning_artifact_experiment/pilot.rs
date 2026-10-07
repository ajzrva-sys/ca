//! Offline immutable RND-097 preregistration.
//!
//! This command deliberately stops before config, provider, tool, sandbox, or
//! credential use.  It freezes the exact six fixtures, 72 arm slots, binaries,
//! and pending paid-run requirements that a later controller must verify again
//! before it can ask the broker to make even one provider request.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::arm::{PreparedPhaseV1, TrialContextV1};
use crate::artifact::{
    controller_contract_sha256, executor_completion_schema, executor_completion_schema_sha256,
    planner_proposal_schema, planner_proposal_schema_sha256,
};
use crate::controller::{ControllerBinaryIdentityV1, PhaseRunResultV1};
use crate::fixtures::{
    validate_fixture_check_receipt, FixtureCheckReceiptV1, FIXTURE_CHECK_SCHEMA_VERSION,
    FIXTURE_IDS,
};
use crate::reasoning_experiment_ledger::{AttoUsd, FrozenModelPriceV1, FrozenPriceCardV1};
use crate::reasoning_experiment_protocol::{
    ExperimentArm, ExperimentModelRouteV1, ProviderCallReceiptV1, ProviderCallSpecV1,
    ToolCallOutcomeV1, ToolCallReceiptV1, ToolCallSpecV1, CHEAP_MODEL,
    MAX_INPUT_TOKENS_PER_REQUEST, MAX_OUTPUT_TOKENS_PER_REQUEST, REASONING_EFFORT, SERVICE_TIER,
    STRONG_MODEL,
};
use crate::receipt_store::{ProviderReceiptStore, StoredReceiptHandleV1};
use crate::recovery::{
    prove_recovery_bundle, write_recovery_bundle, ProvenRecoveryBundle, RecoveryCheckpointStore,
};
use crate::trial_state::{TrialPhaseV1, TrialStateJournal};
use crate::workspace_materialization::{
    materialize_workspace, ClonePreference, DisposableWorkspace, MaterializationPolicy,
};
use std::time::Duration;

const PREREGISTRATION_SCHEMA: &str = "ca.rnd097.preregistration.v1";
const MAX_TREE_ENTRIES: usize = 128;
const MAX_TREE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_PREREGISTRATION_BYTES: usize = 128 * 1024;
const PREREGISTRATION_FILE: &str = "preregistration.v1.json";
const FIXTURE_RECEIPT_FILE: &str = "fixture-check.v1.json";
const OFFICIAL_PRICE_CARD_FILE: &str = "official-price-card.v1.json";
const OFFICIAL_PRICE_CARD_SCHEMA: &str = "ca.rnd097.official_price_card.v1";
const PAID_AUTHORIZATION_FILE: &str = "paid-authorization.v1.json";
const PAID_AUTHORIZATION_SCHEMA: &str = "ca.rnd097.paid_authorization.v1";
const PROVIDER_BUDGET_ATTESTATION_FILE: &str = "provider-budget-attestation.v1.json";
const PROVIDER_BUDGET_ATTESTATION_SCHEMA: &str = "ca.rnd097.provider_budget_attestation.v1";
const OFFICIAL_COMPARE_URL: &str = "https://developers.openai.com/api/docs/models/compare";
const OFFICIAL_SOL_URL: &str = "https://developers.openai.com/api/docs/models/gpt-5.6-sol";
const OFFICIAL_LUNA_URL: &str = "https://developers.openai.com/api/docs/models/gpt-5.6-luna";
const MAX_OFFICIAL_PRICE_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const OFFICIAL_PRICE_MAX_AGE_HOURS: i64 = 24;
const CONTROLLER_ROOT_DIRECTORY: &str = "controller-v1";
const CONTROLLER_SCHEMA_DIRECTORY: &str = "schemas-v1";
const PLANNER_SCHEMA_FILE: &str = "planner-proposal.schema.json";
const EXECUTOR_SCHEMA_FILE: &str = "executor-completion.schema.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OfficialPriceSourceV1 {
    pub url: String,
    pub sha256: String,
    pub bytes: u32,
}

/// Immutable evidence of the public rate card that a later paid controller
/// must bind into its ledger.  Source bodies are used only while validating a
/// deliberate fetch and are never retained with experiment artifacts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OfficialPriceCardReceiptV1 {
    pub schema_version: String,
    pub preregistration_sha256: String,
    pub frozen_at: String,
    pub sources: Vec<OfficialPriceSourceV1>,
    pub price_card: FrozenPriceCardV1,
    pub price_card_sha256: String,
}

struct FetchedOfficialPriceSource {
    url: &'static str,
    body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PilotTrialSlotV1 {
    pub slot_index: u16,
    pub fixture_id: String,
    pub arm: ExperimentArm,
    pub repetition: u8,
    pub included_calibration: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PilotTreeDigestV1 {
    pub label: String,
    pub sha256: String,
    pub entries: u32,
    pub logical_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PilotToolchainExecutableV1 {
    pub name: String,
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PilotPreregistrationV1 {
    pub schema_version: String,
    pub experiment_id: Uuid,
    pub created_at: String,
    pub repository_root: PathBuf,
    pub worker_path: PathBuf,
    pub controller_schema_directory: PathBuf,
    pub controller_binary_sha256: String,
    pub worker_binary_sha256: String,
    pub protocol_source_sha256: String,
    pub planner_proposal_schema_sha256: String,
    pub executor_completion_schema_sha256: String,
    pub controller_contract_sha256: String,
    pub hidden_verifier_sha256: String,
    pub toolchain: Vec<PilotToolchainExecutableV1>,
    pub fixtures: Vec<PilotTreeDigestV1>,
    pub slots: Vec<PilotTrialSlotV1>,
    pub strong_route: ExperimentModelRouteV1,
    pub cheap_route: ExperimentModelRouteV1,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub price_card_state: String,
    pub paid_run_authorization_state: String,
}

impl std::fmt::Debug for PilotPreregistrationV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PilotPreregistrationV1")
            .field("schema_version", &self.schema_version)
            .field("experiment_id", &self.experiment_id)
            .field("created_at", &self.created_at)
            .field("repository_root", &"[PRIVATE]")
            .field("worker_path", &"[PRIVATE]")
            .field("controller_schema_directory", &"[PRIVATE]")
            .field("controller_binary_sha256", &self.controller_binary_sha256)
            .field("worker_binary_sha256", &self.worker_binary_sha256)
            .field("protocol_source_sha256", &self.protocol_source_sha256)
            .field(
                "planner_proposal_schema_sha256",
                &self.planner_proposal_schema_sha256,
            )
            .field(
                "executor_completion_schema_sha256",
                &self.executor_completion_schema_sha256,
            )
            .field(
                "controller_contract_sha256",
                &self.controller_contract_sha256,
            )
            .field("hidden_verifier_sha256", &self.hidden_verifier_sha256)
            .field("toolchain", &self.toolchain)
            .field("fixtures", &self.fixtures)
            .field("slots", &self.slots)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PilotPreregistrationReceiptV1 {
    pub schema_version: String,
    pub experiment_id: Uuid,
    pub preregistration_sha256: String,
    pub slots: u16,
    pub calibration_slots: u16,
    pub price_card_state: String,
    pub paid_run_authorization_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PilotFixtureReceiptV1 {
    pub schema_version: String,
    pub preregistration_sha256: String,
    pub checked_at: String,
    pub fixture_check: FixtureCheckReceiptV1,
}

/// Durable, provider-free view of one preregistered slot.  It deliberately
/// contains only state-journal metadata: never workspace paths, prompts,
/// response bodies, opaque provider state, or tool output.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PilotSlotProgressV1 {
    pub slot: PilotTrialSlotV1,
    pub trial_id: Option<String>,
    pub phase: Option<TrialPhaseV1>,
    pub reason_code: Option<String>,
}

/// Offline controller status for `report` and the read-only half of `resume`.
/// A pending price card is an explicit safety state, not a zero-cost default.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PilotProgressReportV1 {
    pub schema_version: String,
    pub experiment_id: Uuid,
    pub preregistration_sha256: String,
    pub fixture_receipt_verified: bool,
    pub price_card_state: String,
    pub paid_run_authorization_state: String,
    pub event_count: u32,
    pub slots: Vec<PilotSlotProgressV1>,
    pub pending_slots: u16,
    pub completed_slots: u16,
    pub failed_slots: u16,
    pub interrupted_slots: u16,
    /// `false` means a later `resume` must refuse to replay an operation.  A
    /// nonterminal post-execution phase is ambiguous by design.
    pub resume_safe: bool,
}

/// The fail-closed gate evaluated before `run` can locate a configuration,
/// spawn a broker, or reserve money.  It intentionally has no provider or
/// credential fields, making it safe to print and retain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PaidRunPreflightV1 {
    pub schema_version: String,
    pub experiment_id: Uuid,
    pub authorized_max_usd: String,
    pub ready: bool,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PaidAuthorizationReceiptV1 {
    pub schema_version: String,
    pub preregistration_sha256: String,
    pub price_card_sha256: String,
    pub authorized_max_usd: String,
    pub authorized_at: String,
}

/// Owner-supplied evidence that the OpenAI project itself has a hard budget
/// backstop. This is intentionally separate from the harness authorization:
/// ca cannot inspect or grant a provider-side budget and never infers this
/// receipt from local configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderBudgetAttestationV1 {
    pub schema_version: String,
    pub preregistration_sha256: String,
    pub price_card_sha256: String,
    pub provider: String,
    pub project_budget_usd: String,
    pub attestation: String,
    pub attested_at: String,
}

/// Offline owner for one trial workspace through recovery proof.  It contains
/// no provider configuration or tool authority; a future execution controller
/// must obtain those separately through the strict broker/tool protocols.
pub struct TrialWorkspaceLifecycle {
    journal: TrialStateJournal,
    experiment_dir: PathBuf,
    preregistration_sha256: String,
    trial_id: String,
    slot_index: u16,
    source: PathBuf,
    private_root: PathBuf,
    artifact_root: PathBuf,
    workspace: DisposableWorkspace,
}

impl std::fmt::Debug for TrialWorkspaceLifecycle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrialWorkspaceLifecycle")
            .field("trial_id", &self.trial_id)
            .field("slot_index", &self.slot_index)
            .field("source", &"[PRIVATE]")
            .field("workspace", &self.workspace)
            .finish_non_exhaustive()
    }
}

/// Creates a new owner-only experiment directory and an immutable preregistration.
/// No existing directory is accepted, preventing an offline planning command
/// from accidentally overwriting a prior run or its evidence.
pub fn preregister(
    repository_root: &Path,
    experiment_dir: &Path,
    worker: &Path,
) -> Result<PilotPreregistrationReceiptV1> {
    require_unix()?;
    let repository_root = repository_root
        .canonicalize()
        .context("cannot canonicalize repository root")?;
    if !repository_root.is_dir() {
        bail!("repository root is not a directory");
    }
    if !worker.is_absolute()
        || fs::symlink_metadata(worker)
            .context("cannot inspect worker binary")?
            .file_type()
            .is_symlink()
    {
        bail!("worker binary must be an absolute non-symlink path");
    }
    let worker = worker
        .canonicalize()
        .context("cannot canonicalize worker binary")?;
    let worker_metadata = fs::metadata(&worker)?;
    if !worker_metadata.is_file() {
        bail!("worker path is not a regular file");
    }
    #[cfg(unix)]
    if worker_metadata.mode() & 0o111 == 0 {
        bail!("worker binary is not executable");
    }

    let experiment_dir = create_experiment_directory(experiment_dir)?;
    let controller = std::env::current_exe().context("cannot resolve experiment controller")?;
    let controller = controller
        .canonicalize()
        .context("cannot canonicalize experiment controller")?;
    let mut fixtures = Vec::with_capacity(FIXTURE_IDS.len() * 2);
    for (kind, relative_root) in [
        ("baseline", "tests/fixtures/reasoning_pilot"),
        ("golden", "tests/reasoning_pilot_golden"),
    ] {
        for fixture_id in FIXTURE_IDS {
            fixtures.push(hash_tree(
                &repository_root.join(relative_root).join(fixture_id),
                &format!("{kind}:{fixture_id}"),
            )?);
        }
    }
    let hidden_verifier = hash_tree(
        &repository_root.join("tests/reasoning_pilot_hidden"),
        "hidden-verifier",
    )?;
    let toolchain = ["cargo", "rustc"]
        .into_iter()
        .map(resolve_toolchain_executable)
        .collect::<Result<Vec<_>>>()?;
    let controller_directory =
        create_owner_subdirectory(&experiment_dir, CONTROLLER_ROOT_DIRECTORY)?;
    let schema_directory =
        create_owner_subdirectory(&controller_directory, CONTROLLER_SCHEMA_DIRECTORY)?;
    let planner_schema = planner_proposal_schema()?;
    let executor_schema = executor_completion_schema()?;
    write_immutable_file(&schema_directory.join(PLANNER_SCHEMA_FILE), &planner_schema)?;
    write_immutable_file(
        &schema_directory.join(EXECUTOR_SCHEMA_FILE),
        &executor_schema,
    )?;
    let experiment_id = Uuid::new_v4();
    let preregistration = PilotPreregistrationV1 {
        schema_version: PREREGISTRATION_SCHEMA.into(),
        experiment_id,
        created_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        repository_root,
        worker_path: worker.clone(),
        controller_schema_directory: schema_directory,
        controller_binary_sha256: hash_regular_file(&controller)?,
        worker_binary_sha256: hash_regular_file(&worker)?,
        protocol_source_sha256: hash_regular_file(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/reasoning_experiment_protocol.rs"),
        )?,
        planner_proposal_schema_sha256: planner_proposal_schema_sha256()?,
        executor_completion_schema_sha256: executor_completion_schema_sha256()?,
        controller_contract_sha256: controller_contract_sha256()?,
        hidden_verifier_sha256: hidden_verifier.sha256,
        toolchain,
        fixtures,
        slots: deterministic_slots(experiment_id),
        strong_route: ExperimentModelRouteV1::openai(STRONG_MODEL),
        cheap_route: ExperimentModelRouteV1::openai(CHEAP_MODEL),
        max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
        max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
        price_card_state: "requires_official_freeze_before_paid_run".into(),
        paid_run_authorization_state: "requires_explicit_authorize_max_usd_50".into(),
    };
    validate_preregistration(&preregistration)?;
    let bytes = serde_json::to_vec(&preregistration)?;
    if bytes.len() > MAX_PREREGISTRATION_BYTES {
        bail!("preregistration exceeds the bound");
    }
    write_immutable_file(&experiment_dir.join(PREREGISTRATION_FILE), &bytes)?;
    let preregistration_sha256 = hash_bytes(&bytes);
    Ok(PilotPreregistrationReceiptV1 {
        schema_version: "ca.rnd097.preregistration_receipt.v1".into(),
        experiment_id,
        preregistration_sha256,
        slots: preregistration.slots.len() as u16,
        calibration_slots: preregistration
            .slots
            .iter()
            .filter(|slot| slot.included_calibration)
            .count() as u16,
        price_card_state: preregistration.price_card_state,
        paid_run_authorization_state: preregistration.paid_run_authorization_state,
    })
}

pub fn load_preregistration(experiment_dir: &Path) -> Result<PilotPreregistrationV1> {
    require_unix()?;
    if fs::symlink_metadata(experiment_dir)
        .context("cannot inspect experiment directory")?
        .file_type()
        .is_symlink()
    {
        bail!("experiment directory must not be a symbolic link");
    }
    let experiment_dir = experiment_dir
        .canonicalize()
        .context("cannot canonicalize experiment directory")?;
    require_owner_directory(&experiment_dir)?;
    let path = experiment_dir.join(PREREGISTRATION_FILE);
    let mut file = open_owner_regular_file(&path)?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_PREREGISTRATION_BYTES as u64 {
        bail!("preregistration exceeds the bound");
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    let preregistration: PilotPreregistrationV1 =
        serde_json::from_slice(&bytes).context("preregistration is not valid strict JSON")?;
    validate_preregistration(&preregistration)?;
    Ok(preregistration)
}

/// Binds a completed offline fixture check to the one immutable
/// preregistration it actually checked.  It is intentionally unavailable
/// without an existing preregistration and never invokes providers or tools.
pub fn persist_fixture_receipt(
    experiment_dir: &Path,
    repository_root: &Path,
    fixture_check: &FixtureCheckReceiptV1,
) -> Result<PilotFixtureReceiptV1> {
    let preregistration = load_preregistration(experiment_dir)?;
    let repository_root = repository_root.canonicalize()?;
    if preregistration.repository_root != repository_root {
        bail!("fixture receipt repository does not match preregistration");
    }
    verify_preregistered_inputs(&preregistration)?;
    validate_fixture_check_receipt(fixture_check)?;
    let experiment_dir = experiment_dir.canonicalize()?;
    let preregistration_sha256 = hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?;
    let receipt = PilotFixtureReceiptV1 {
        schema_version: "ca.rnd097.fixture_receipt.v1".into(),
        preregistration_sha256,
        checked_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        fixture_check: fixture_check.clone(),
    };
    let bytes = serde_json::to_vec(&receipt)?;
    if bytes.len() > MAX_PREREGISTRATION_BYTES {
        bail!("fixture receipt exceeds the bound");
    }
    write_immutable_file(&experiment_dir.join(FIXTURE_RECEIPT_FILE), &bytes)?;
    Ok(receipt)
}

/// Fetches the three fixed public OpenAI pages needed to freeze the pilot's
/// current rate card. This is an explicit public-document operation: it uses
/// no provider configuration, account, API key, or model endpoint. A fetch
/// error, redirect, oversized body, or changed required pricing text prevents
/// the immutable receipt from being created.
pub async fn freeze_official_price_card(
    experiment_dir: &Path,
) -> Result<OfficialPriceCardReceiptV1> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .user_agent("ca-rnd097-price-evidence/0.1")
        .build()
        .context("cannot create public price-evidence client")?;
    let mut sources = Vec::with_capacity(3);
    for url in [OFFICIAL_COMPARE_URL, OFFICIAL_SOL_URL, OFFICIAL_LUNA_URL] {
        sources.push(fetch_official_price_source(&client, url).await?);
    }
    persist_official_price_card_from_documents(experiment_dir, Utc::now(), &sources)
}

/// Creates the immutable receipt from already fetched public source bytes.
/// It exists so the parser and evidence binding can be exercised entirely
/// offline; normal callers should use `freeze_official_price_card` above.
fn persist_official_price_card_from_documents(
    experiment_dir: &Path,
    frozen_at: chrono::DateTime<Utc>,
    sources: &[FetchedOfficialPriceSource],
) -> Result<OfficialPriceCardReceiptV1> {
    let preregistration = load_preregistration(experiment_dir)?;
    verify_preregistered_inputs(&preregistration)?;
    let experiment_dir = experiment_dir.canonicalize()?;
    let preregistration_sha256 = hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?;
    let fixture_receipt = load_fixture_receipt(&experiment_dir)?;
    if fixture_receipt.preregistration_sha256 != preregistration_sha256 {
        bail!("fixture receipt is not bound to the current preregistration");
    }
    let source_receipts = validate_official_price_sources(sources)?;
    let frozen_at = frozen_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let price_card = current_official_price_card(&frozen_at)?;
    let price_card_sha256 = price_card
        .sha256()
        .map_err(|error| anyhow!("official price card is invalid: {error}"))?;
    let receipt = OfficialPriceCardReceiptV1 {
        schema_version: OFFICIAL_PRICE_CARD_SCHEMA.into(),
        preregistration_sha256,
        frozen_at,
        sources: source_receipts,
        price_card,
        price_card_sha256,
    };
    validate_official_price_card_receipt(&receipt, &receipt.preregistration_sha256)?;
    let bytes = serde_json::to_vec(&receipt)?;
    if bytes.len() > MAX_PREREGISTRATION_BYTES {
        bail!("official price card receipt exceeds the bound");
    }
    write_immutable_file(&experiment_dir.join(OFFICIAL_PRICE_CARD_FILE), &bytes)?;
    Ok(receipt)
}

async fn fetch_official_price_source(
    client: &reqwest::Client,
    url: &'static str,
) -> Result<FetchedOfficialPriceSource> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("cannot fetch public OpenAI price source {url}"))?;
    if response.url().as_str() != url {
        bail!("public OpenAI price source redirected or changed URL");
    }
    if !response.status().is_success() {
        bail!("public OpenAI price source returned a non-success status");
    }
    if let Some(length) = response.content_length() {
        if length > MAX_OFFICIAL_PRICE_SOURCE_BYTES as u64 {
            bail!("public OpenAI price source exceeds the byte bound");
        }
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body
            .len()
            .checked_add(chunk.len())
            .is_none_or(|length| length > MAX_OFFICIAL_PRICE_SOURCE_BYTES)
        {
            bail!("public OpenAI price source exceeds the byte bound");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(FetchedOfficialPriceSource { url, body })
}

fn validate_official_price_sources(
    sources: &[FetchedOfficialPriceSource],
) -> Result<Vec<OfficialPriceSourceV1>> {
    if sources.len() != 3 {
        bail!("official price evidence has an unexpected source count");
    }
    let expected = [OFFICIAL_COMPARE_URL, OFFICIAL_SOL_URL, OFFICIAL_LUNA_URL];
    let mut receipts = Vec::with_capacity(sources.len());
    for (source, expected_url) in sources.iter().zip(expected) {
        if source.url != expected_url || source.body.is_empty() {
            bail!("official price evidence source identity is invalid");
        }
        if source.body.len() > MAX_OFFICIAL_PRICE_SOURCE_BYTES {
            bail!("official price evidence source exceeds the byte bound");
        }
        let text = std::str::from_utf8(&source.body)
            .context("official price evidence is not valid UTF-8")?;
        let required = match source.url {
            OFFICIAL_COMPARE_URL => [
                "GPT-5.6 Sol",
                "$5.00",
                "$0.50",
                "$30.00",
                "GPT-5.6 Luna",
                "$0.20",
                "$0.02",
                "$1.20",
            ]
            .as_slice(),
            OFFICIAL_SOL_URL | OFFICIAL_LUNA_URL => [
                "GPT-5.6",
                "Cache writes are billed at 1.25x the uncached input token rate.",
                "272K",
            ]
            .as_slice(),
            _ => unreachable!("source URL was checked against a fixed list"),
        };
        if required.iter().any(|fragment| !text.contains(fragment)) {
            bail!("official price evidence no longer matches the frozen rate contract");
        }
        receipts.push(OfficialPriceSourceV1 {
            url: source.url.into(),
            sha256: hash_bytes(&source.body),
            bytes: u32::try_from(source.body.len())
                .map_err(|_| anyhow!("official price evidence length overflows u32"))?,
        });
    }
    Ok(receipts)
}

fn current_official_price_card(frozen_at: &str) -> Result<FrozenPriceCardV1> {
    let day = frozen_at
        .get(..10)
        .ok_or_else(|| anyhow!("official price freeze timestamp is invalid"))?;
    let price = |value| {
        AttoUsd::parse_usd(value).map_err(|error| anyhow!("invalid frozen price {value}: {error}"))
    };
    let card = FrozenPriceCardV1 {
        schema_version: crate::reasoning_experiment_ledger::FROZEN_PRICE_CARD_SCHEMA_V1.into(),
        price_card_id: format!("openai-gpt56-default-{day}"),
        effective_at_rfc3339: frozen_at.into(),
        service_tier: SERVICE_TIER.into(),
        sol: FrozenModelPriceV1 {
            regular_input_per_million_atto_usd: price("5")?,
            cached_input_per_million_atto_usd: price("0.5")?,
            cache_write_per_million_atto_usd: price("6.25")?,
            regular_output_per_million_atto_usd: price("30")?,
            reasoning_output_per_million_atto_usd: price("30")?,
        },
        luna: FrozenModelPriceV1 {
            regular_input_per_million_atto_usd: price("0.2")?,
            cached_input_per_million_atto_usd: price("0.02")?,
            cache_write_per_million_atto_usd: price("0.25")?,
            regular_output_per_million_atto_usd: price("1.2")?,
            reasoning_output_per_million_atto_usd: price("1.2")?,
        },
    };
    card.validate()
        .map_err(|error| anyhow!("official price card is invalid: {error}"))?;
    Ok(card)
}

pub fn load_official_price_card(experiment_dir: &Path) -> Result<OfficialPriceCardReceiptV1> {
    let preregistration = load_preregistration(experiment_dir)?;
    verify_preregistered_inputs(&preregistration)?;
    let experiment_dir = experiment_dir.canonicalize()?;
    let preregistration_sha256 = hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?;
    let mut file = open_owner_regular_file(&experiment_dir.join(OFFICIAL_PRICE_CARD_FILE))?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_PREREGISTRATION_BYTES as u64 {
        bail!("official price card receipt exceeds the bound");
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    let receipt: OfficialPriceCardReceiptV1 = serde_json::from_slice(&bytes)
        .context("official price card receipt is not valid strict JSON")?;
    if receipt.preregistration_sha256 != preregistration_sha256 {
        bail!("official price card receipt is not bound to this preregistration");
    }
    validate_official_price_card_receipt(&receipt, &preregistration_sha256)?;
    Ok(receipt)
}

fn validate_official_price_card_receipt(
    receipt: &OfficialPriceCardReceiptV1,
    preregistration_sha256: &str,
) -> Result<()> {
    if receipt.schema_version != OFFICIAL_PRICE_CARD_SCHEMA
        || receipt.preregistration_sha256 != preregistration_sha256
        || receipt.sources.len() != 3
        || !valid_sha256(&receipt.preregistration_sha256)
        || chrono::DateTime::parse_from_rfc3339(&receipt.frozen_at).is_err()
    {
        bail!("official price card receipt is invalid");
    }
    for (source, expected_url) in
        receipt
            .sources
            .iter()
            .zip([OFFICIAL_COMPARE_URL, OFFICIAL_SOL_URL, OFFICIAL_LUNA_URL])
    {
        if source.url != expected_url
            || !valid_sha256(&source.sha256)
            || source.bytes == 0
            || usize::try_from(source.bytes)
                .map_err(|_| anyhow!("official price evidence length is invalid"))?
                > MAX_OFFICIAL_PRICE_SOURCE_BYTES
        {
            bail!("official price source receipt is invalid");
        }
    }
    receipt
        .price_card
        .validate()
        .map_err(|error| anyhow!("official price card is invalid: {error}"))?;
    if receipt.price_card.service_tier != SERVICE_TIER
        || receipt.price_card_sha256
            != receipt
                .price_card
                .sha256()
                .map_err(|error| anyhow!("official price card is invalid: {error}"))?
    {
        bail!("official price card receipt digest or tier is invalid");
    }
    let expected = current_official_price_card(&receipt.frozen_at)?;
    if receipt.price_card != expected {
        bail!("official price card rates differ from the frozen RND-097 contract");
    }
    Ok(())
}

fn require_fresh_official_price_card_at(
    experiment_dir: &Path,
    now: chrono::DateTime<Utc>,
) -> Result<OfficialPriceCardReceiptV1> {
    let receipt = load_official_price_card(experiment_dir)?;
    let frozen_at = chrono::DateTime::parse_from_rfc3339(&receipt.frozen_at)?.with_timezone(&Utc);
    let age = now.signed_duration_since(frozen_at);
    if age < chrono::Duration::zero() || age > chrono::Duration::hours(OFFICIAL_PRICE_MAX_AGE_HOURS)
    {
        bail!("official price card is outside its 24-hour dispatch window");
    }
    Ok(receipt)
}

pub fn require_fresh_official_price_card(
    experiment_dir: &Path,
) -> Result<OfficialPriceCardReceiptV1> {
    require_fresh_official_price_card_at(experiment_dir, Utc::now())
}

fn official_price_card_state(
    experiment_dir: &Path,
    preregistration_sha256: &str,
    pending_state: &str,
) -> Result<String> {
    let path = experiment_dir.join(OFFICIAL_PRICE_CARD_FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            let receipt = load_official_price_card(experiment_dir)?;
            if receipt.preregistration_sha256 != preregistration_sha256 {
                bail!("official price card receipt is not bound to this preregistration");
            }
            let frozen_at =
                chrono::DateTime::parse_from_rfc3339(&receipt.frozen_at)?.with_timezone(&Utc);
            let age = Utc::now().signed_duration_since(frozen_at);
            if age < chrono::Duration::zero()
                || age > chrono::Duration::hours(OFFICIAL_PRICE_MAX_AGE_HOURS)
            {
                Ok("official_price_card_expired".into())
            } else {
                Ok("official_price_card_frozen".into())
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(pending_state.into()),
        Err(error) => Err(error.into()),
    }
}

/// Reads every immutable controller gate and journal event without creating a
/// journal, workspace, ledger, worker, or provider client.  This is the only
/// state view used by `report` and by the preflight portion of `resume`.
pub fn inspect_progress(experiment_dir: &Path) -> Result<PilotProgressReportV1> {
    let preregistration = load_preregistration(experiment_dir)?;
    verify_preregistered_inputs(&preregistration)?;
    let experiment_dir = experiment_dir.canonicalize()?;
    let preregistration_sha256 = hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?;
    let price_card_state = official_price_card_state(
        &experiment_dir,
        &preregistration_sha256,
        &preregistration.price_card_state,
    )?;
    let paid_run_authorization_state =
        match fs::symlink_metadata(experiment_dir.join(PAID_AUTHORIZATION_FILE)) {
            Ok(_) => {
                load_paid_authorization(&experiment_dir)?;
                "authorized_max_usd_50".into()
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                preregistration.paid_run_authorization_state.clone()
            }
            Err(error) => return Err(error.into()),
        };

    let fixture_receipt_path = experiment_dir.join(FIXTURE_RECEIPT_FILE);
    let fixture_receipt_verified = match fs::symlink_metadata(&fixture_receipt_path) {
        Ok(_) => {
            let receipt = load_fixture_receipt(&experiment_dir)?;
            if receipt.preregistration_sha256 != preregistration_sha256 {
                bail!("fixture receipt is not bound to the current preregistration");
            }
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };

    let events = if TrialStateJournal::exists(&experiment_dir)? {
        TrialStateJournal::open_existing(&experiment_dir, &preregistration_sha256)?.events()?
    } else {
        Vec::new()
    };
    let event_count = u32::try_from(events.len()).map_err(|_| anyhow!("event count overflow"))?;
    let mut per_slot =
        std::collections::BTreeMap::<u16, &crate::trial_state::TrialStateEventV1>::new();
    let mut trial_slots = std::collections::BTreeMap::<&str, u16>::new();
    for event in &events {
        if let Some(previous_slot) = trial_slots.insert(&event.trial_id, event.slot_index) {
            if previous_slot != event.slot_index {
                bail!("one trial ID is bound to multiple preregistered slots");
            }
        }
        if let Some(previous) = per_slot.insert(event.slot_index, event) {
            if previous.trial_id != event.trial_id {
                bail!("multiple trial IDs are bound to one preregistered slot");
            }
        }
    }

    let mut slots = Vec::with_capacity(preregistration.slots.len());
    let mut pending_slots = 0_u16;
    let mut completed_slots = 0_u16;
    let mut failed_slots = 0_u16;
    let mut interrupted_slots = 0_u16;
    // Resume safety is deliberately separate from fixture eligibility: a
    // newly preregistered pilot with no fixture receipt is not runnable, but
    // it has not performed an ambiguous operation and is still safe to inspect
    // or later start after its fixture receipt is persisted.
    let mut resume_safe = true;
    for slot in &preregistration.slots {
        let event = per_slot.get(&slot.slot_index).copied();
        let phase = event.map(|event| event.phase);
        match phase {
            Some(TrialPhaseV1::Settled) => completed_slots = completed_slots.saturating_add(1),
            Some(TrialPhaseV1::Failed) => failed_slots = failed_slots.saturating_add(1),
            Some(TrialPhaseV1::Interrupted) => {
                interrupted_slots = interrupted_slots.saturating_add(1);
                resume_safe = false;
            }
            Some(TrialPhaseV1::ExecutionStarted)
            | Some(TrialPhaseV1::RecoveryExported)
            | Some(TrialPhaseV1::RecoveryProven)
            | Some(TrialPhaseV1::VerifierCompleted) => {
                pending_slots = pending_slots.saturating_add(1);
                resume_safe = false;
            }
            None | Some(TrialPhaseV1::FixtureReady) | Some(TrialPhaseV1::WorkspaceMaterialized) => {
                pending_slots = pending_slots.saturating_add(1);
            }
        }
        slots.push(PilotSlotProgressV1 {
            slot: slot.clone(),
            trial_id: event.map(|event| event.trial_id.clone()),
            phase,
            reason_code: event.and_then(|event| event.reason_code.clone()),
        });
    }

    Ok(PilotProgressReportV1 {
        schema_version: "ca.rnd097.progress_report.v1".into(),
        experiment_id: preregistration.experiment_id,
        preregistration_sha256,
        fixture_receipt_verified,
        price_card_state,
        paid_run_authorization_state,
        event_count,
        slots,
        pending_slots,
        completed_slots,
        failed_slots,
        interrupted_slots,
        resume_safe,
    })
}

/// Evaluates the exact paid-run boundary without even locating a provider
/// configuration.  Future work may make `ready=true` only by loading a
/// separately immutable official price-card receipt; preregistration itself
/// remains immutable and cannot be edited to bypass this gate.
pub fn paid_run_preflight(
    experiment_dir: &Path,
    authorized_max_usd: &str,
) -> Result<PaidRunPreflightV1> {
    let progress = inspect_progress(experiment_dir)?;
    let mut blockers = Vec::new();
    if authorized_max_usd != "50.00" {
        blockers.push("authorized_max_usd_must_equal_50.00".into());
    }
    if !progress.fixture_receipt_verified {
        blockers.push("fixture_check_receipt_missing_or_unverified".into());
    }
    if progress.price_card_state != "official_price_card_frozen" {
        blockers.push("official_price_card_not_frozen".into());
    }
    if load_provider_budget_attestation(experiment_dir).is_err() {
        blockers.push("provider_project_budget_attestation_missing_or_stale".into());
    }
    if !progress.resume_safe {
        blockers.push("ambiguous_or_interrupted_trial_requires_manual_recovery".into());
    }
    Ok(PaidRunPreflightV1 {
        schema_version: "ca.rnd097.paid_run_preflight.v1".into(),
        experiment_id: progress.experiment_id,
        authorized_max_usd: authorized_max_usd.into(),
        ready: blockers.is_empty(),
        blockers,
    })
}

pub fn persist_paid_authorization(
    experiment_dir: &Path,
    authorized_max_usd: &str,
) -> Result<PaidAuthorizationReceiptV1> {
    let gate = paid_run_preflight(experiment_dir, authorized_max_usd)?;
    if !gate.ready {
        bail!("paid authorization cannot be persisted while preflight is blocked");
    }
    let experiment_dir = experiment_dir.canonicalize()?;
    let preregistration_sha256 = hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?;
    let price_card = load_official_price_card(&experiment_dir)?;
    let receipt = PaidAuthorizationReceiptV1 {
        schema_version: PAID_AUTHORIZATION_SCHEMA.into(),
        preregistration_sha256,
        price_card_sha256: price_card.price_card_sha256,
        authorized_max_usd: authorized_max_usd.into(),
        authorized_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    };
    write_immutable_file(
        &experiment_dir.join(PAID_AUTHORIZATION_FILE),
        &serde_json::to_vec(&receipt)?,
    )?;
    Ok(receipt)
}

pub fn load_paid_authorization(experiment_dir: &Path) -> Result<PaidAuthorizationReceiptV1> {
    let experiment_dir = experiment_dir.canonicalize()?;
    require_owner_directory(&experiment_dir)?;
    let mut file = open_owner_regular_file(&experiment_dir.join(PAID_AUTHORIZATION_FILE))?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_PREREGISTRATION_BYTES as u64 {
        bail!("paid authorization receipt exceeds the bound");
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    let receipt: PaidAuthorizationReceiptV1 =
        serde_json::from_slice(&bytes).context("paid authorization receipt is not strict JSON")?;
    let price_card = load_official_price_card(&experiment_dir)?;
    if receipt.schema_version != PAID_AUTHORIZATION_SCHEMA
        || receipt.preregistration_sha256
            != hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?
        || receipt.price_card_sha256 != price_card.price_card_sha256
        || receipt.authorized_max_usd != "50.00"
        || chrono::DateTime::parse_from_rfc3339(&receipt.authorized_at).is_err()
    {
        bail!("paid authorization receipt is invalid or stale");
    }
    Ok(receipt)
}

pub fn persist_provider_budget_attestation(
    experiment_dir: &Path,
    project_budget_usd: &str,
) -> Result<ProviderBudgetAttestationV1> {
    if project_budget_usd != "50.00" {
        bail!("provider project budget attestation must equal 50.00 USD");
    }
    let experiment_dir = experiment_dir.canonicalize()?;
    require_owner_directory(&experiment_dir)?;
    let price_card = require_fresh_official_price_card(&experiment_dir)?;
    let receipt = ProviderBudgetAttestationV1 {
        schema_version: PROVIDER_BUDGET_ATTESTATION_SCHEMA.into(),
        preregistration_sha256: hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?,
        price_card_sha256: price_card.price_card_sha256,
        provider: "openai".into(),
        project_budget_usd: project_budget_usd.into(),
        attestation: "owner_confirmed_provider_project_hard_budget_cap".into(),
        attested_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    };
    validate_provider_budget_attestation_at(&receipt, &experiment_dir, Utc::now())?;
    write_immutable_file(
        &experiment_dir.join(PROVIDER_BUDGET_ATTESTATION_FILE),
        &serde_json::to_vec(&receipt)?,
    )?;
    Ok(receipt)
}

pub fn ensure_provider_budget_attestation(
    experiment_dir: &Path,
    project_budget_usd: &str,
) -> Result<ProviderBudgetAttestationV1> {
    match fs::symlink_metadata(experiment_dir.join(PROVIDER_BUDGET_ATTESTATION_FILE)) {
        Ok(_) => {
            let receipt = load_provider_budget_attestation(experiment_dir)?;
            if receipt.project_budget_usd != project_budget_usd {
                bail!("stored provider project-budget attestation differs from this command");
            }
            Ok(receipt)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            persist_provider_budget_attestation(experiment_dir, project_budget_usd)
        }
        Err(error) => Err(error.into()),
    }
}

pub fn load_provider_budget_attestation(
    experiment_dir: &Path,
) -> Result<ProviderBudgetAttestationV1> {
    load_provider_budget_attestation_at(experiment_dir, Utc::now())
}

fn load_provider_budget_attestation_at(
    experiment_dir: &Path,
    now: chrono::DateTime<Utc>,
) -> Result<ProviderBudgetAttestationV1> {
    let experiment_dir = experiment_dir.canonicalize()?;
    require_owner_directory(&experiment_dir)?;
    let mut file = open_owner_regular_file(&experiment_dir.join(PROVIDER_BUDGET_ATTESTATION_FILE))?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_PREREGISTRATION_BYTES as u64 {
        bail!("provider project-budget attestation exceeds the bound");
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    let receipt: ProviderBudgetAttestationV1 = serde_json::from_slice(&bytes)
        .context("provider project-budget attestation is not strict JSON")?;
    validate_provider_budget_attestation_at(&receipt, &experiment_dir, now)?;
    Ok(receipt)
}

fn validate_provider_budget_attestation_at(
    receipt: &ProviderBudgetAttestationV1,
    experiment_dir: &Path,
    now: chrono::DateTime<Utc>,
) -> Result<()> {
    let price_card = require_fresh_official_price_card_at(experiment_dir, now)?;
    let attested_at = chrono::DateTime::parse_from_rfc3339(&receipt.attested_at)
        .map_err(|_| anyhow!("provider project-budget attestation timestamp is invalid"))?
        .with_timezone(&Utc);
    let age = now.signed_duration_since(attested_at);
    if receipt.schema_version != PROVIDER_BUDGET_ATTESTATION_SCHEMA
        || receipt.preregistration_sha256
            != hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?
        || receipt.price_card_sha256 != price_card.price_card_sha256
        || receipt.provider != "openai"
        || receipt.project_budget_usd != "50.00"
        || receipt.attestation != "owner_confirmed_provider_project_hard_budget_cap"
        || age < chrono::Duration::zero()
        || age > chrono::Duration::hours(OFFICIAL_PRICE_MAX_AGE_HOURS)
    {
        bail!("provider project-budget attestation is invalid or stale");
    }
    Ok(())
}

/// Final local gate used by resume and by every physical provider dispatch.
/// It cannot establish a provider-side budget: it only proves that the owner
/// explicitly attested one and that all three immutable receipts remain bound
/// to the current preregistration and fresh price evidence.
pub fn require_paid_dispatch_gates(experiment_dir: &Path) -> Result<()> {
    require_paid_dispatch_gates_at(experiment_dir, Utc::now())
}

fn require_paid_dispatch_gates_at(experiment_dir: &Path, now: chrono::DateTime<Utc>) -> Result<()> {
    require_fresh_official_price_card_at(experiment_dir, now)?;
    load_provider_budget_attestation_at(experiment_dir, now)?;
    load_paid_authorization(experiment_dir)?;
    Ok(())
}

/// Opens the only journal that a future controller may use.  This refuses to
/// create execution state unless the preregistration and its successful
/// revalidated fixture receipt already exist and are cryptographically bound.
pub fn open_trial_journal(experiment_dir: &Path) -> Result<TrialStateJournal> {
    let preregistration = load_preregistration(experiment_dir)?;
    verify_preregistered_inputs(&preregistration)?;
    let experiment_dir = experiment_dir.canonicalize()?;
    let preregistration_sha256 = hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?;
    let receipt = load_fixture_receipt(&experiment_dir)?;
    if receipt.preregistration_sha256 != preregistration_sha256 {
        bail!("fixture receipt is not bound to the current preregistration");
    }
    TrialStateJournal::create(&experiment_dir, &preregistration_sha256)
}

/// Creates an isolated WSP benchmark workspace after all immutable offline
/// gates pass, and records the materialization boundary durably.  The caller
/// may mutate only the returned workspace; the fixture source and normal
/// checkout are never execution targets.
pub fn materialize_trial_workspace(
    experiment_dir: &Path,
    trial_id: &str,
    slot_index: u16,
) -> Result<TrialWorkspaceLifecycle> {
    let preregistration = load_preregistration(experiment_dir)?;
    let slot = preregistration
        .slots
        .get(slot_index as usize)
        .ok_or_else(|| anyhow!("trial slot is outside preregistration"))?;
    let journal = open_trial_journal(experiment_dir)?;
    journal.append(
        trial_id,
        slot_index,
        TrialPhaseV1::FixtureReady,
        None,
        None,
        None,
    )?;
    let source = preregistration
        .repository_root
        .join("tests/fixtures/reasoning_pilot")
        .join(&slot.fixture_id);
    let experiment_dir = experiment_dir.canonicalize()?;
    let preregistration_sha256 = hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?;
    let private_root = create_owner_subdirectory(&experiment_dir, "workspaces-v1")?;
    let artifact_root = create_owner_subdirectory(&experiment_dir, "recovery-artifacts-v1")?;
    let workspace = materialize_workspace(
        &source,
        &private_root,
        &format!("trial-{slot_index:03}"),
        &MaterializationPolicy::benchmark(),
        ClonePreference::PreferClone,
    )
    .map_err(|error| anyhow!("cannot materialize trial workspace: {error}"))?;
    journal.append(
        trial_id,
        slot_index,
        TrialPhaseV1::WorkspaceMaterialized,
        Some(&workspace.workspace_id().to_string()),
        None,
        None,
    )?;
    Ok(TrialWorkspaceLifecycle {
        journal,
        experiment_dir,
        preregistration_sha256,
        trial_id: trial_id.into(),
        slot_index,
        source,
        private_root,
        artifact_root,
        workspace,
    })
}

/// Starts a fresh slot or safely resumes only its pre-execution WSP phases.
/// Once execution has started, callers must never reconstruct and replay it.
pub fn open_or_materialize_trial_workspace(
    experiment_dir: &Path,
    slot_index: u16,
) -> Result<TrialWorkspaceLifecycle> {
    let preregistration = load_preregistration(experiment_dir)?;
    let slot = preregistration
        .slots
        .get(slot_index as usize)
        .ok_or_else(|| anyhow!("trial slot is outside preregistration"))?;
    let journal = open_trial_journal(experiment_dir)?;
    let events = journal.events()?;
    let latest = events
        .iter()
        .rev()
        .find(|event| event.slot_index == slot_index)
        .cloned();
    let Some(latest) = latest else {
        let trial_id = format!("trial-{slot_index:03}-{}", Uuid::new_v4().simple());
        return materialize_trial_workspace(experiment_dir, &trial_id, slot_index);
    };
    if !matches!(
        latest.phase,
        TrialPhaseV1::FixtureReady | TrialPhaseV1::WorkspaceMaterialized
    ) {
        bail!("trial slot cannot be automatically resumed after execution began");
    }
    let experiment_dir = experiment_dir.canonicalize()?;
    let preregistration_sha256 = hash_regular_file(&experiment_dir.join(PREREGISTRATION_FILE))?;
    let private_root = create_owner_subdirectory(&experiment_dir, "workspaces-v1")?;
    let artifact_root = create_owner_subdirectory(&experiment_dir, "recovery-artifacts-v1")?;
    let source = preregistration
        .repository_root
        .join("tests/fixtures/reasoning_pilot")
        .join(&slot.fixture_id);
    let workspace = match latest.phase {
        TrialPhaseV1::FixtureReady => {
            let workspace = materialize_workspace(
                &source,
                &private_root,
                &format!("trial-{slot_index:03}"),
                &MaterializationPolicy::benchmark(),
                ClonePreference::PreferClone,
            )
            .map_err(|error| anyhow!("cannot resume trial materialization: {error}"))?;
            journal.append(
                &latest.trial_id,
                slot_index,
                TrialPhaseV1::WorkspaceMaterialized,
                Some(&workspace.workspace_id().to_string()),
                None,
                None,
            )?;
            workspace
        }
        TrialPhaseV1::WorkspaceMaterialized => {
            let workspace_id = latest
                .workspace_id
                .as_deref()
                .ok_or_else(|| anyhow!("materialized trial event lacks a workspace ID"))?
                .parse::<Uuid>()?;
            DisposableWorkspace::reopen(&private_root, workspace_id)
                .map_err(|error| anyhow!("cannot reopen pre-execution trial workspace: {error}"))?
        }
        _ => unreachable!("phase was checked above"),
    };
    Ok(TrialWorkspaceLifecycle {
        journal,
        experiment_dir,
        preregistration_sha256,
        trial_id: latest.trial_id.clone(),
        slot_index,
        source,
        private_root,
        artifact_root,
        workspace,
    })
}

impl TrialWorkspaceLifecycle {
    pub fn workspace_path(&self) -> &Path {
        self.workspace.path()
    }

    /// Reconstructs the exact controller-owned phase context from the durable
    /// preregistration and this WSP workspace. The external Cargo target is a
    /// separate owner-only directory and may not overlap controller or source
    /// data.
    pub fn trial_context(&self, cargo_target_dir: &Path) -> Result<TrialContextV1> {
        self.require_context_state()?;
        let preregistration = load_preregistration(&self.experiment_dir)?;
        verify_preregistered_inputs(&preregistration)?;
        let slot = preregistration
            .slots
            .get(self.slot_index as usize)
            .ok_or_else(|| anyhow!("trial slot is outside preregistration"))?;
        let cargo_target_dir = cargo_target_dir
            .canonicalize()
            .context("cannot canonicalize trial Cargo target")?;
        require_owner_directory(&cargo_target_dir)?;
        let controller_root = preregistration
            .controller_schema_directory
            .parent()
            .ok_or_else(|| anyhow!("controller schema directory has no root"))?
            .canonicalize()?;
        require_owner_directory(&controller_root)?;
        if cargo_target_dir.starts_with(self.workspace.path())
            || cargo_target_dir.starts_with(&controller_root)
            || self.workspace.path().starts_with(&cargo_target_dir)
            || controller_root.starts_with(&cargo_target_dir)
        {
            bail!("trial Cargo target overlaps a protected root");
        }
        Ok(TrialContextV1 {
            experiment_id: preregistration.experiment_id.to_string(),
            preregistration_sha256: self.preregistration_sha256.clone(),
            trial_id: self.trial_id.clone(),
            slot_index: self.slot_index,
            fixture_id: slot.fixture_id.clone(),
            arm: slot.arm,
            repetition: slot.repetition,
            workspace_id: self.workspace.workspace_id().to_string(),
            workspace_root: self.workspace.path().to_path_buf(),
            controller_root,
            cargo_target_dir,
            source_manifest_sha256: self.workspace.receipt().source_manifest_sha256.clone(),
            destination_manifest_sha256: self
                .workspace
                .receipt()
                .destination_manifest_sha256
                .clone(),
            planner_schema_sha256: preregistration.planner_proposal_schema_sha256,
            executor_schema_sha256: preregistration.executor_completion_schema_sha256,
            binary_identity: ControllerBinaryIdentityV1 {
                controller_sha256: preregistration.controller_binary_sha256,
                worker_sha256: preregistration.worker_binary_sha256,
            },
            toolchain: preregistration.toolchain,
        })
    }

    /// Durable precondition for any future planner/executor/provider dispatch.
    /// Once recorded, a crash before a later terminal state is intentionally
    /// ambiguous and `resume` must not replay the operation.
    pub fn mark_execution_started(&self) -> Result<()> {
        self.journal.append(
            &self.trial_id,
            self.slot_index,
            TrialPhaseV1::ExecutionStarted,
            Some(&self.workspace.workspace_id().to_string()),
            None,
            None,
        )?;
        Ok(())
    }

    /// Executes one prepared phase through caller-supplied one-call
    /// transports. The first phase starts the durable execution boundary;
    /// subsequent phases in the same trial share only the workspace and
    /// visible handoff, never the prior phase's provider conversation state.
    pub fn run_prepared_phase_with<P, T>(
        &self,
        prepared: &PreparedPhaseV1,
        mut invoke_provider: P,
        mut invoke_tool: T,
    ) -> Result<PhaseRunResultV1>
    where
        P: FnMut(&ProviderCallSpecV1) -> Result<ProviderCallReceiptV1>,
        T: FnMut(&ToolCallSpecV1) -> Result<ToolCallReceiptV1>,
    {
        prepared.validate_before_execution()?;
        self.validate_prepared_phase(prepared)?;
        match self.latest_trial_phase()? {
            TrialPhaseV1::WorkspaceMaterialized => self.mark_execution_started()?,
            TrialPhaseV1::ExecutionStarted => {}
            _ => bail!("prepared phase cannot run after the execution boundary closed"),
        }
        let (schema_name, schema) = prepared.output_schema();
        crate::controller::run_phase_with_output_schema(
            &prepared.worker_spec,
            prepared.binary_identity(),
            prepared.system_prompt(),
            prepared.task_prompt(),
            Some((schema_name, schema)),
            |call| self.dispatch_provider_with(call, |call| invoke_provider(call)),
            |call| self.dispatch_tool_with(call, |call| invoke_tool(call)),
        )
    }

    /// Persists the safe projection of a completed broker exchange only after
    /// the durable execution boundary has been recorded.  The raw Responses
    /// output remains only in the caller's in-memory IPC frame for the next
    /// semantic action; it cannot enter the trial receipt store.
    pub fn record_provider_result(
        &self,
        call: &ProviderCallSpecV1,
        receipt: &ProviderCallReceiptV1,
    ) -> Result<StoredReceiptHandleV1> {
        self.require_active_execution()?;
        self.validate_provider_call(call)?;
        ProviderReceiptStore::create(&self.experiment_dir, &self.preregistration_sha256)?
            .record_provider_receipt(call, receipt)
    }

    /// Persists only a digest-level tool receipt after checking that the exact
    /// call is scoped to this disposable WSP workspace.  Command JSON and tool
    /// output remain controller-memory data and never become trial evidence.
    pub fn record_tool_result(
        &self,
        call: &ToolCallSpecV1,
        receipt: &ToolCallReceiptV1,
    ) -> Result<StoredReceiptHandleV1> {
        self.require_active_execution()?;
        self.validate_tool_call(call)?;
        ProviderReceiptStore::create(&self.experiment_dir, &self.preregistration_sha256)?
            .record_tool_receipt(&self.trial_id, self.slot_index, call, receipt)
    }

    /// Runs one already-authorized broker exchange only while the durable
    /// execution boundary is active. A completed or verified-pre-output
    /// receipt remains available to the controller in memory; any transport,
    /// receipt-persistence, or ambiguous-delivery failure is recorded as a
    /// terminal interruption before the error returns. This prevents a later
    /// `resume` from silently replaying a physical request.
    pub fn dispatch_provider_with<F>(
        &self,
        call: &ProviderCallSpecV1,
        invoke: F,
    ) -> Result<ProviderCallReceiptV1>
    where
        F: FnOnce(&ProviderCallSpecV1) -> Result<ProviderCallReceiptV1>,
    {
        self.require_active_execution()?;
        self.validate_provider_call(call)?;
        let receipt = match invoke(call) {
            Ok(receipt) => receipt,
            Err(_) => return self.interrupt_after_dispatch_failure("provider_worker_failed"),
        };
        if self.record_provider_result(call, &receipt).is_err() {
            return self.interrupt_after_dispatch_failure("provider_receipt_unstored");
        }
        if matches!(
            &receipt.outcome,
            crate::reasoning_experiment_protocol::ProviderCallOutcomeV1::AmbiguousDelivery { .. }
        ) {
            return self.interrupt_after_dispatch_failure("provider_delivery_ambiguous");
        }
        Ok(receipt)
    }

    /// Dispatches one provider call through the separate credential-holding
    /// broker. This helper does not bypass the paid-run gate: it is inert
    /// until a future controller explicitly constructs and supplies a frozen
    /// one-call spec after that gate has passed.
    pub fn dispatch_provider_worker(
        &self,
        worker: &Path,
        call: &ProviderCallSpecV1,
        timeout: Duration,
    ) -> Result<ProviderCallReceiptV1> {
        self.dispatch_provider_with(call, |call| {
            crate::controller_ipc::invoke_provider_worker(
                worker,
                &self.experiment_dir,
                call,
                timeout,
            )
        })
    }

    /// Runs one exact local-tool exchange through the clean-environment
    /// worker. Any non-completed result is terminal because a local command
    /// might have changed the disposable workspace before it failed.
    pub fn dispatch_tool_with<F>(
        &self,
        call: &ToolCallSpecV1,
        invoke: F,
    ) -> Result<ToolCallReceiptV1>
    where
        F: FnOnce(&ToolCallSpecV1) -> Result<ToolCallReceiptV1>,
    {
        self.require_active_execution()?;
        self.validate_tool_call(call)?;
        let receipt = match invoke(call) {
            Ok(receipt) => receipt,
            Err(_) => return self.interrupt_after_dispatch_failure("tool_worker_failed"),
        };
        if self.record_tool_result(call, &receipt).is_err() {
            return self.interrupt_after_dispatch_failure("tool_receipt_unstored");
        }
        if !matches!(&receipt.outcome, ToolCallOutcomeV1::Completed) {
            return self.interrupt_after_dispatch_failure("tool_result_noncompleted");
        }
        Ok(receipt)
    }

    /// Dispatches one tool call through the dedicated clean-environment
    /// worker. As with the broker helper, it is not exposed by `run` yet.
    pub fn dispatch_tool_worker(
        &self,
        worker: &Path,
        call: &ToolCallSpecV1,
        timeout: Duration,
    ) -> Result<ToolCallReceiptV1> {
        self.dispatch_tool_with(call, |call| {
            crate::controller_ipc::invoke_tool_worker(worker, &self.experiment_dir, call, timeout)
        })
    }

    fn validate_provider_call(&self, call: &ProviderCallSpecV1) -> Result<()> {
        call.validate()
            .map_err(|error| anyhow!("provider call is invalid: {error}"))?;
        if call.trial_id != self.trial_id
            || call.slot_index != self.slot_index
            || call.preregistration_sha256 != self.preregistration_sha256
        {
            bail!("provider result is not bound to this trial lifecycle");
        }
        Ok(())
    }

    fn validate_tool_call(&self, call: &ToolCallSpecV1) -> Result<()> {
        call.validate()
            .map_err(|error| anyhow!("tool call is invalid: {error}"))?;
        if call.workspace_id != self.workspace.workspace_id().to_string()
            || call.workspace_root != self.workspace.path()
            || call.source_manifest_sha256 != self.workspace.receipt().source_manifest_sha256
        {
            bail!("tool result is not scoped to this disposable workspace");
        }
        Ok(())
    }

    fn interrupt_after_dispatch_failure<T>(&self, reason_code: &str) -> Result<T> {
        self.journal
            .append(
                &self.trial_id,
                self.slot_index,
                TrialPhaseV1::Interrupted,
                Some(&self.workspace.workspace_id().to_string()),
                None,
                Some(reason_code),
            )
            .context("cannot durably mark ambiguous trial dispatch interrupted")?;
        bail!("trial dispatch is ambiguous and has been marked interrupted")
    }

    fn latest_trial_phase(&self) -> Result<TrialPhaseV1> {
        let events = self.journal.events()?;
        events
            .iter()
            .rev()
            .find(|event| event.trial_id == self.trial_id)
            .map(|event| event.phase)
            .ok_or_else(|| anyhow!("trial lifecycle has no durable state"))
    }

    fn require_active_execution(&self) -> Result<()> {
        if self.latest_trial_phase()? != TrialPhaseV1::ExecutionStarted {
            bail!("trial lifecycle is not in the active execution phase");
        }
        Ok(())
    }

    fn require_context_state(&self) -> Result<()> {
        if !matches!(
            self.latest_trial_phase()?,
            TrialPhaseV1::WorkspaceMaterialized | TrialPhaseV1::ExecutionStarted
        ) {
            bail!("trial context may only be built during the execution lifecycle");
        }
        Ok(())
    }

    fn validate_prepared_phase(&self, prepared: &PreparedPhaseV1) -> Result<()> {
        let preregistration = load_preregistration(&self.experiment_dir)?;
        let slot = preregistration
            .slots
            .get(self.slot_index as usize)
            .ok_or_else(|| anyhow!("trial slot is outside preregistration"))?;
        let spec = &prepared.worker_spec;
        let controller_root = preregistration
            .controller_schema_directory
            .parent()
            .ok_or_else(|| anyhow!("controller schema directory has no root"))?
            .canonicalize()?;
        if spec.experiment_id != preregistration.experiment_id.to_string()
            || spec.preregistration_sha256 != self.preregistration_sha256
            || spec.trial_id != self.trial_id
            || spec.slot_index != self.slot_index
            || spec.fixture_id != slot.fixture_id
            || spec.arm != slot.arm
            || spec.repetition != slot.repetition
            || spec.workspace_id != self.workspace.workspace_id().to_string()
            || spec.workspace_root != self.workspace.path()
            || spec.controller_root != controller_root
            || spec.source_manifest_sha256 != self.workspace.receipt().source_manifest_sha256
            || spec.destination_manifest_sha256
                != self.workspace.receipt().destination_manifest_sha256
            || prepared.binary_identity().controller_sha256
                != preregistration.controller_binary_sha256
            || prepared.binary_identity().worker_sha256 != preregistration.worker_binary_sha256
        {
            bail!("prepared phase is not bound to this exact trial lifecycle");
        }
        Ok(())
    }

    /// Exports and proves the complete final tree.  The caller must only call
    /// this after a future tool worker has finished; doing so early is still
    /// safe but represents exactly the current workspace.
    pub fn export_and_prove(self) -> Result<ProvenRecoveryBundle> {
        let artifact = self.artifact_root.join(format!("{}.bundle", self.trial_id));
        let receipt = write_recovery_bundle(&self.workspace, &artifact)?;
        let checkpoint_store =
            RecoveryCheckpointStore::create(&self.artifact_root.join("recovery-checkpoints-v1"))?;
        let checkpoint = crate::recovery::RecoveryCheckpointV1 {
            schema_version: "ca.rnd097.recovery_checkpoint.v1".into(),
            trial_id: self.trial_id.clone(),
            private_root: self.private_root.clone(),
            source: self.source.clone(),
            workspace_id: self.workspace.workspace_id(),
            reconstructed_workspace_id: None,
            artifact: artifact.clone(),
            bundle: receipt.clone(),
        };
        checkpoint_store.persist_exported(&checkpoint)?;
        self.journal.append(
            &self.trial_id,
            self.slot_index,
            TrialPhaseV1::RecoveryExported,
            Some(&checkpoint.workspace_id.to_string()),
            Some(&receipt.recovery_artifact_sha256),
            None,
        )?;
        let proof = prove_recovery_bundle(
            self.workspace,
            &self.source,
            &self.private_root,
            &format!("trial-{:03}-reconstructed", self.slot_index),
            &artifact,
            receipt.clone(),
        )?;
        checkpoint_store.persist_proven(&checkpoint, proof.reconstructed_workspace_id())?;
        self.journal.append(
            &self.trial_id,
            self.slot_index,
            TrialPhaseV1::RecoveryProven,
            Some(&checkpoint.workspace_id.to_string()),
            Some(&receipt.recovery_artifact_sha256),
            None,
        )?;
        Ok(proof)
    }
}

pub fn load_fixture_receipt(experiment_dir: &Path) -> Result<PilotFixtureReceiptV1> {
    let experiment_dir = experiment_dir.canonicalize()?;
    require_owner_directory(&experiment_dir)?;
    let mut file = open_owner_regular_file(&experiment_dir.join(FIXTURE_RECEIPT_FILE))?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_PREREGISTRATION_BYTES as u64 {
        bail!("fixture receipt exceeds the bound");
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    let receipt: PilotFixtureReceiptV1 =
        serde_json::from_slice(&bytes).context("fixture receipt is not valid strict JSON")?;
    if receipt.schema_version != "ca.rnd097.fixture_receipt.v1"
        || !valid_sha256(&receipt.preregistration_sha256)
        || receipt.fixture_check.schema_version != FIXTURE_CHECK_SCHEMA_VERSION
        || !receipt.fixture_check.passed
        || receipt.fixture_check.candidates.len() != FIXTURE_IDS.len() * 2
    {
        bail!("fixture receipt is invalid");
    }
    Ok(receipt)
}

fn verify_preregistered_inputs(preregistration: &PilotPreregistrationV1) -> Result<()> {
    if preregistration.controller_schema_directory.canonicalize()?
        != preregistration.controller_schema_directory
    {
        bail!("preregistered controller schema directory identity drifted");
    }
    require_owner_directory(&preregistration.controller_schema_directory)?;
    let mut expected_fixtures = Vec::with_capacity(FIXTURE_IDS.len() * 2);
    for (kind, relative_root) in [
        ("baseline", "tests/fixtures/reasoning_pilot"),
        ("golden", "tests/reasoning_pilot_golden"),
    ] {
        for fixture_id in FIXTURE_IDS {
            expected_fixtures.push(hash_tree(
                &preregistration
                    .repository_root
                    .join(relative_root)
                    .join(fixture_id),
                &format!("{kind}:{fixture_id}"),
            )?);
        }
    }
    if expected_fixtures != preregistration.fixtures
        || hash_tree(
            &preregistration
                .repository_root
                .join("tests/reasoning_pilot_hidden"),
            "hidden-verifier",
        )?
        .sha256
            != preregistration.hidden_verifier_sha256
        || hash_regular_file(&std::env::current_exe()?.canonicalize()?)?
            != preregistration.controller_binary_sha256
        || hash_regular_file(&preregistration.worker_path)? != preregistration.worker_binary_sha256
        || hash_regular_file(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/reasoning_experiment_protocol.rs"),
        )? != preregistration.protocol_source_sha256
        || hash_regular_file(
            &preregistration
                .controller_schema_directory
                .join(PLANNER_SCHEMA_FILE),
        )? != preregistration.planner_proposal_schema_sha256
        || hash_regular_file(
            &preregistration
                .controller_schema_directory
                .join(EXECUTOR_SCHEMA_FILE),
        )? != preregistration.executor_completion_schema_sha256
        || preregistration.toolchain.len() != 2
        || preregistration
            .toolchain
            .iter()
            .any(|tool| verify_toolchain_executable(tool).is_err())
    {
        bail!("preregistered fixture, verifier, controller, worker, or protocol input drifted");
    }
    Ok(())
}

fn resolve_toolchain_executable(name: &str) -> Result<PilotToolchainExecutableV1> {
    if !matches!(name, "cargo" | "rustc") {
        bail!("toolchain executable name is outside the frozen pilot");
    }
    let path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| anyhow!("cannot resolve required {name} executable"))?
        .canonicalize()
        .with_context(|| format!("cannot canonicalize required {name} executable"))?;
    let tool = PilotToolchainExecutableV1 {
        name: name.into(),
        sha256: hash_regular_file(&path)?,
        path,
    };
    verify_toolchain_executable(&tool)?;
    Ok(tool)
}

fn verify_toolchain_executable(tool: &PilotToolchainExecutableV1) -> Result<()> {
    if !matches!(tool.name.as_str(), "cargo" | "rustc")
        || !tool.path.is_absolute()
        || tool.path.canonicalize()? != tool.path
        || hash_regular_file(&tool.path)? != tool.sha256
        || !valid_sha256(&tool.sha256)
    {
        bail!("preregistered toolchain executable drifted");
    }
    #[cfg(unix)]
    if fs::metadata(&tool.path)?.mode() & 0o111 == 0 {
        bail!("preregistered toolchain executable is not executable");
    }
    Ok(())
}

fn deterministic_slots(experiment_id: Uuid) -> Vec<PilotTrialSlotV1> {
    let mut slots = Vec::with_capacity(72);
    for arm in ExperimentArm::ALL {
        slots.push(PilotTrialSlotV1 {
            slot_index: slots.len() as u16,
            fixture_id: FIXTURE_IDS[0].into(),
            arm,
            repetition: 1,
            included_calibration: true,
        });
    }
    let mut remaining = Vec::with_capacity(66);
    for repetition in 1..=2 {
        for fixture_id in FIXTURE_IDS {
            if repetition == 1 && fixture_id == FIXTURE_IDS[0] {
                continue;
            }
            for arm in ExperimentArm::ALL {
                remaining.push((fixture_id, arm, repetition));
            }
        }
    }
    remaining.sort_by(|left, right| {
        slot_order_key(experiment_id, left.0, left.1, left.2).cmp(&slot_order_key(
            experiment_id,
            right.0,
            right.1,
            right.2,
        ))
    });
    for (fixture_id, arm, repetition) in remaining {
        slots.push(PilotTrialSlotV1 {
            slot_index: slots.len() as u16,
            fixture_id: fixture_id.into(),
            arm,
            repetition,
            included_calibration: false,
        });
    }
    slots
}

fn slot_order_key(
    experiment_id: Uuid,
    fixture_id: &str,
    arm: ExperimentArm,
    repetition: u8,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"ca-rnd097-slot-order-v1\0");
    hash.update(experiment_id.as_bytes());
    hash.update(fixture_id.as_bytes());
    hash.update(arm.id().as_bytes());
    hash.update([repetition]);
    hash.finalize().into()
}

fn validate_preregistration(value: &PilotPreregistrationV1) -> Result<()> {
    if value.schema_version != PREREGISTRATION_SCHEMA
        || value.slots.len() != 72
        || value.fixtures.len() != FIXTURE_IDS.len() * 2
        || value.max_input_tokens != MAX_INPUT_TOKENS_PER_REQUEST
        || value.max_output_tokens != MAX_OUTPUT_TOKENS_PER_REQUEST
        || value.strong_route != ExperimentModelRouteV1::openai(STRONG_MODEL)
        || value.cheap_route != ExperimentModelRouteV1::openai(CHEAP_MODEL)
        || value.price_card_state != "requires_official_freeze_before_paid_run"
        || value.paid_run_authorization_state != "requires_explicit_authorize_max_usd_50"
    {
        bail!("preregistration does not match the frozen RND-097 contract");
    }
    for (index, slot) in value.slots.iter().enumerate() {
        if slot.slot_index != index as u16
            || !FIXTURE_IDS.contains(&slot.fixture_id.as_str())
            || !(1..=2).contains(&slot.repetition)
        {
            bail!("preregistration has an invalid trial slot");
        }
        if index < 6 {
            if !slot.included_calibration
                || slot.fixture_id != FIXTURE_IDS[0]
                || slot.repetition != 1
                || slot.arm != ExperimentArm::ALL[index]
            {
                bail!("preregistration calibration slots are not exact");
            }
        } else if slot.included_calibration {
            bail!("preregistration has an unexpected calibration slot");
        }
    }
    for fixture_id in FIXTURE_IDS {
        for arm in ExperimentArm::ALL {
            let count = value
                .slots
                .iter()
                .filter(|slot| slot.fixture_id == fixture_id && slot.arm == arm)
                .count();
            if count != 2 {
                bail!("preregistration trial arms are not balanced");
            }
        }
    }
    for digest in &value.fixtures {
        if !valid_sha256(&digest.sha256)
            || digest.entries == 0
            || digest.entries as usize > MAX_TREE_ENTRIES
            || digest.logical_bytes > MAX_TREE_BYTES
        {
            bail!("preregistration fixture digest is invalid");
        }
    }
    if !valid_sha256(&value.controller_binary_sha256)
        || !valid_sha256(&value.worker_binary_sha256)
        || !valid_sha256(&value.protocol_source_sha256)
        || value.planner_proposal_schema_sha256 != planner_proposal_schema_sha256()?
        || value.executor_completion_schema_sha256 != executor_completion_schema_sha256()?
        || value.controller_contract_sha256 != controller_contract_sha256()?
        || !valid_sha256(&value.hidden_verifier_sha256)
        || value.strong_route.reasoning_effort != REASONING_EFFORT
        || value.cheap_route.reasoning_effort != REASONING_EFFORT
        || value.strong_route.service_tier != SERVICE_TIER
        || value.cheap_route.service_tier != SERVICE_TIER
        || !value.controller_schema_directory.is_absolute()
    {
        bail!("preregistration binary or route identity is invalid");
    }
    Ok(())
}

fn create_experiment_directory(path: &Path) -> Result<PathBuf> {
    require_unix()?;
    match fs::symlink_metadata(path) {
        Ok(_) => bail!("experiment directory must not already exist"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("experiment directory has no parent"))?
        .canonicalize()
        .context("cannot canonicalize experiment directory parent")?;
    require_owner_directory(&parent)?;
    fs::create_dir(path)
        .with_context(|| format!("cannot create experiment directory {}", path.display()))?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    path.canonicalize()
        .context("cannot canonicalize experiment directory")
}

fn create_owner_subdirectory(parent: &Path, name: &str) -> Result<PathBuf> {
    let path = parent.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            bail!("experiment subdirectory is invalid")
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&path)?;
            #[cfg(unix)]
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
        Err(error) => return Err(error.into()),
    }
    let path = path.canonicalize()?;
    require_owner_directory(&path)?;
    Ok(path)
}

fn write_immutable_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("file has no parent"))?;
    let temporary = parent.join(format!(".{}.{}.tmp", PREREGISTRATION_FILE, Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(error) = fs::hard_link(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(anyhow!("cannot publish preregistration: {error}"));
    }
    fs::remove_file(temporary)?;
    sync_directory(parent)?;
    Ok(())
}

fn hash_tree(root: &Path, label: &str) -> Result<PilotTreeDigestV1> {
    let root = root
        .canonicalize()
        .with_context(|| format!("cannot open {label}"))?;
    let mut files = Vec::new();
    collect_tree_files(&root, &root, 0, &mut files)?;
    files.sort_by(|left, right| os_bytes(left.0.as_os_str()).cmp(os_bytes(right.0.as_os_str())));
    if files.is_empty() || files.len() > MAX_TREE_ENTRIES {
        bail!("{label} tree entry count is invalid");
    }
    let entries = files.len() as u32;
    let mut hasher = Sha256::new();
    hasher.update(b"ca-rnd097-tree-digest-v1\0");
    let mut logical_bytes = 0_u64;
    for (relative, absolute, bytes) in files {
        logical_bytes = logical_bytes
            .checked_add(bytes)
            .ok_or_else(|| anyhow!("{label} byte count overflow"))?;
        if logical_bytes > MAX_TREE_BYTES {
            bail!("{label} exceeds tree byte bound");
        }
        let contents = read_regular_file(&absolute, bytes)?;
        hasher.update((os_bytes(relative.as_os_str()).len() as u64).to_be_bytes());
        hasher.update(os_bytes(relative.as_os_str()));
        hasher.update(bytes.to_be_bytes());
        hasher.update(Sha256::digest(&contents));
    }
    Ok(PilotTreeDigestV1 {
        label: label.into(),
        sha256: encode_hex(&hasher.finalize()),
        entries,
        logical_bytes,
    })
}

fn collect_tree_files(
    root: &Path,
    current: &Path,
    depth: usize,
    output: &mut Vec<(PathBuf, PathBuf, u64)>,
) -> Result<()> {
    if depth > 16 {
        bail!("preregistered tree exceeds depth bound");
    }
    let mut children = fs::read_dir(current)?.collect::<io::Result<Vec<_>>>()?;
    children.sort_by(|left, right| os_bytes(&left.file_name()).cmp(os_bytes(&right.file_name())));
    for child in children {
        let absolute = child.path();
        let metadata = fs::symlink_metadata(&absolute)?;
        if metadata.file_type().is_symlink() {
            bail!("preregistered tree contains a symbolic link");
        }
        if metadata.is_dir() {
            collect_tree_files(root, &absolute, depth + 1, output)?;
        } else if metadata.is_file() {
            if output.len() >= MAX_TREE_ENTRIES {
                bail!("preregistered tree exceeds entry bound");
            }
            let relative = absolute.strip_prefix(root)?.to_path_buf();
            validate_relative(&relative)?;
            output.push((relative, absolute, metadata.len()));
        } else {
            bail!("preregistered tree contains a special file");
        }
    }
    Ok(())
}

fn read_regular_file(path: &Path, expected_bytes: u64) -> Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != expected_bytes {
        bail!("preregistered tree changed while it was read");
    }
    let mut bytes = Vec::with_capacity(expected_bytes as usize);
    file.read_to_end(&mut bytes)?;
    if bytes.len() as u64 != expected_bytes || file.read(&mut [0_u8; 1])? != 0 {
        bail!("preregistered file changed while it was read");
    }
    Ok(bytes)
}

fn hash_regular_file(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("binary is not a regular file");
    }
    Ok(encode_hex(&Sha256::digest(read_regular_file(
        path,
        metadata.len(),
    )?)))
}

fn open_owner_regular_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        bail!("preregistration is not a regular file");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!("preregistration is not owner-only");
    }
    Ok(file)
}

fn require_owner_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("directory is invalid");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!("directory is not owner-only");
    }
    Ok(())
}

fn validate_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("tree path is not relative");
    }
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("tree path is not normalized");
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hash_bytes(bytes: &[u8]) -> String {
    encode_hex(&Sha256::digest(bytes))
}

fn encode_hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(TABLE[(byte >> 4) as usize] as char);
        output.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> &[u8] {
    value.as_bytes()
}

#[cfg(not(unix))]
fn os_bytes(value: &OsStr) -> &[u8] {
    value.to_str().unwrap_or_default().as_bytes()
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    Ok(())
}

fn require_unix() -> Result<()> {
    #[cfg(not(unix))]
    bail!("RND-097 preregistration requires Unix owner-only files");
    #[cfg(unix)]
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning_experiment_protocol::{
        ExperimentModelRouteV1, ExperimentPhase, ExperimentUsageV1, ProviderCallOutcomeV1,
        ToolCallOutcomeV1, CHEAP_MODEL, MAX_INPUT_TOKENS_PER_REQUEST,
        MAX_OUTPUT_TOKENS_PER_REQUEST, PROVIDER_CALL_RECEIPT_SCHEMA_V1,
        PROVIDER_CALL_SPEC_SCHEMA_V1, TOOL_CALL_RECEIPT_SCHEMA_V1, TOOL_CALL_SPEC_SCHEMA_V1,
    };

    fn private_parent() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn private_child(parent: &Path, name: &str) -> PathBuf {
        let parent = parent.canonicalize().unwrap();
        let child = parent.join(name);
        fs::create_dir(&child).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&child, fs::Permissions::from_mode(0o700)).unwrap();
        child
    }

    fn digest(byte: char) -> String {
        byte.to_string().repeat(64)
    }

    fn raw_sha256(value: &str) -> String {
        let bytes = Sha256::digest(value.as_bytes());
        const TABLE: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            output.push(TABLE[(byte >> 4) as usize] as char);
            output.push(TABLE[(byte & 0x0f) as usize] as char);
        }
        output
    }

    fn provider_call(lifecycle: &TrialWorkspaceLifecycle) -> ProviderCallSpecV1 {
        let request_json = format!(
            r#"{{"model":"{CHEAP_MODEL}","input":[{{"role":"user","content":"private planner input"}}],"stream":true,"reasoning":{{"effort":"high"}},"include":["reasoning.encrypted_content"],"max_output_tokens":8192,"store":false,"service_tier":"default"}}"#
        );
        ProviderCallSpecV1 {
            schema_version: PROVIDER_CALL_SPEC_SCHEMA_V1.into(),
            call_id: "provider-call-1".into(),
            channel_id: "channel-1".into(),
            nonce: "nonce-1".into(),
            worker_spec_sha256: digest('a'),
            controller_sha256: digest('b'),
            worker_sha256: digest('c'),
            experiment_id: "pilot-1".into(),
            preregistration_sha256: lifecycle.preregistration_sha256.clone(),
            trial_id: lifecycle.trial_id.clone(),
            slot_index: lifecycle.slot_index,
            phase: ExperimentPhase::Executor,
            semantic_request_index: 0,
            physical_attempt: 1,
            route: ExperimentModelRouteV1::openai(CHEAP_MODEL),
            max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
            max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
            store: false,
            request_sha256: raw_sha256(&request_json),
            request_json,
            x_client_request_id: "private-client-request-id".into(),
        }
    }

    fn provider_receipt(call: &ProviderCallSpecV1) -> ProviderCallReceiptV1 {
        let response = r#"{"text":"private assistant response"}"#;
        ProviderCallReceiptV1 {
            schema_version: PROVIDER_CALL_RECEIPT_SCHEMA_V1.into(),
            call_id: call.call_id.clone(),
            channel_id: call.channel_id.clone(),
            nonce: call.nonce.clone(),
            worker_spec_sha256: call.worker_spec_sha256.clone(),
            request_sha256: call.request_sha256.clone(),
            reservation_id: "reservation-1".into(),
            x_client_request_id_sha256: raw_sha256(&call.x_client_request_id),
            x_request_id_sha256: Some(digest('e')),
            returned_model: Some(CHEAP_MODEL.into()),
            returned_service_tier: Some("default".into()),
            usage: Some(ExperimentUsageV1 {
                regular_input_tokens: 10,
                cached_input_tokens: 0,
                cache_write_tokens: 0,
                regular_output_tokens: 2,
                reasoning_output_tokens: 0,
            }),
            elapsed_millis: 1,
            outcome: ProviderCallOutcomeV1::Completed {
                controller_response_json: response.into(),
                controller_response_sha256: raw_sha256(response),
            },
        }
    }

    fn tool_call(lifecycle: &TrialWorkspaceLifecycle, target: &Path) -> ToolCallSpecV1 {
        let arguments_json =
            r#"{"executable":"/usr/bin/env","arguments":["private tool argument"]}"#;
        ToolCallSpecV1 {
            schema_version: TOOL_CALL_SPEC_SCHEMA_V1.into(),
            call_id: "tool-call-1".into(),
            channel_id: "tool-channel-1".into(),
            nonce: "tool-nonce-1".into(),
            worker_spec_sha256: digest('a'),
            workspace_id: lifecycle.workspace.workspace_id().to_string(),
            workspace_root: lifecycle.workspace.path().to_path_buf(),
            source_manifest_sha256: lifecycle.workspace.receipt().source_manifest_sha256.clone(),
            cargo_target_dir: target.to_path_buf(),
            tool_name: "run_command".into(),
            tool_call_id: "private-tool-call-id".into(),
            arguments_sha256: raw_sha256(arguments_json),
            arguments_json: arguments_json.into(),
            allowed_read_paths: vec![PathBuf::from("src")],
            allowed_write_paths: vec![],
            timeout_millis: 5_000,
        }
    }

    fn tool_receipt(call: &ToolCallSpecV1) -> ToolCallReceiptV1 {
        let output = r#"{"stdout":"private tool output"}"#;
        ToolCallReceiptV1 {
            schema_version: TOOL_CALL_RECEIPT_SCHEMA_V1.into(),
            call_id: call.call_id.clone(),
            channel_id: call.channel_id.clone(),
            nonce: call.nonce.clone(),
            worker_spec_sha256: call.worker_spec_sha256.clone(),
            arguments_sha256: call.arguments_sha256.clone(),
            elapsed_millis: 1,
            outcome: ToolCallOutcomeV1::Completed,
            result_sha256: Some(raw_sha256(output)),
            result_json: Some(output.into()),
        }
    }

    fn successful_fixture_check() -> FixtureCheckReceiptV1 {
        let candidates = FIXTURE_IDS
            .into_iter()
            .flat_map(|fixture_id| {
                [
                    crate::fixtures::FixtureCandidateReceiptV1 {
                        fixture_id: fixture_id.into(),
                        kind: "baseline".into(),
                        public_passed: true,
                        hidden_passed: false,
                        hidden_failure_code: Some(
                            crate::fixtures::expected_failure_code(fixture_id).into(),
                        ),
                    },
                    crate::fixtures::FixtureCandidateReceiptV1 {
                        fixture_id: fixture_id.into(),
                        kind: "golden".into(),
                        public_passed: true,
                        hidden_passed: true,
                        hidden_failure_code: None,
                    },
                ]
            })
            .collect();
        FixtureCheckReceiptV1 {
            schema_version: FIXTURE_CHECK_SCHEMA_VERSION.into(),
            passed: true,
            network_isolation: "sandbox-exec-deny-network".into(),
            candidates,
        }
    }

    fn ready_experiment(name: &str) -> (tempfile::TempDir, PathBuf) {
        let outer = private_parent();
        let parent = private_child(outer.path(), "experiments");
        let experiment = parent.join(name);
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        preregister(repository, &experiment, &std::env::current_exe().unwrap()).unwrap();
        persist_fixture_receipt(&experiment, repository, &successful_fixture_check()).unwrap();
        (outer, experiment)
    }

    fn ready_lifecycle(name: &str) -> (tempfile::TempDir, TrialWorkspaceLifecycle) {
        let (outer, experiment) = ready_experiment(name);
        let lifecycle = materialize_trial_workspace(&experiment, "trial-007", 0).unwrap();
        (outer, lifecycle)
    }

    fn official_sources() -> Vec<FetchedOfficialPriceSource> {
        vec![
            FetchedOfficialPriceSource {
                url: OFFICIAL_COMPARE_URL,
                body: b"GPT-5.6 Sol $5.00 $0.50 $30.00 GPT-5.6 Luna $0.20 $0.02 $1.20"
                    .to_vec(),
            },
            FetchedOfficialPriceSource {
                url: OFFICIAL_SOL_URL,
                body: b"GPT-5.6 Sol Cache writes are billed at 1.25x the uncached input token rate. Prompts with >272K input tokens"
                    .to_vec(),
            },
            FetchedOfficialPriceSource {
                url: OFFICIAL_LUNA_URL,
                body: b"GPT-5.6 Luna Cache writes are billed at 1.25x the uncached input token rate. Prompts with >272K input tokens"
                    .to_vec(),
            },
        ]
    }

    fn write_budget_attestation_at(
        experiment: &Path,
        attested_at: chrono::DateTime<Utc>,
    ) -> ProviderBudgetAttestationV1 {
        let experiment = experiment.canonicalize().unwrap();
        let price = load_official_price_card(&experiment).unwrap();
        let receipt = ProviderBudgetAttestationV1 {
            schema_version: PROVIDER_BUDGET_ATTESTATION_SCHEMA.into(),
            preregistration_sha256: hash_regular_file(&experiment.join(PREREGISTRATION_FILE))
                .unwrap(),
            price_card_sha256: price.price_card_sha256,
            provider: "openai".into(),
            project_budget_usd: "50.00".into(),
            attestation: "owner_confirmed_provider_project_hard_budget_cap".into(),
            attested_at: attested_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        };
        write_immutable_file(
            &experiment.join(PROVIDER_BUDGET_ATTESTATION_FILE),
            &serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        receipt
    }

    #[test]
    fn preregistration_has_exact_balanced_slots_and_pending_paid_gates() {
        let outer = private_parent();
        let parent = private_child(outer.path(), "experiments");
        let experiment = parent.join("pilot-one");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        let worker = std::env::current_exe().unwrap();
        let receipt = preregister(repository, &experiment, &worker).unwrap();
        assert_eq!(receipt.slots, 72);
        assert_eq!(receipt.calibration_slots, 6);
        assert_eq!(
            receipt.price_card_state,
            "requires_official_freeze_before_paid_run"
        );
        let preregistration = load_preregistration(&experiment).unwrap();
        assert_eq!(preregistration.slots.len(), 72);
        for (index, arm) in ExperimentArm::ALL.into_iter().enumerate() {
            assert_eq!(preregistration.slots[index].arm, arm);
            assert!(preregistration.slots[index].included_calibration);
        }
        assert!(preregister(repository, &experiment, &worker).is_err());
    }

    #[test]
    fn progress_and_paid_preflight_are_read_only_and_fail_closed() {
        let outer = private_parent();
        let parent = private_child(outer.path(), "experiments");
        let experiment = parent.join("pilot-preflight");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        preregister(repository, &experiment, &std::env::current_exe().unwrap()).unwrap();

        let report = inspect_progress(&experiment).unwrap();
        assert_eq!(report.event_count, 0);
        assert_eq!(report.pending_slots, 72);
        assert_eq!(report.completed_slots, 0);
        assert!(!report.fixture_receipt_verified);
        assert!(report.resume_safe);
        assert!(!experiment.join("trial-state-v1").exists());

        let gate = paid_run_preflight(&experiment, "49.99").unwrap();
        assert!(!gate.ready);
        assert!(gate
            .blockers
            .contains(&"authorized_max_usd_must_equal_50.00".into()));
        assert!(gate
            .blockers
            .contains(&"fixture_check_receipt_missing_or_unverified".into()));
        assert!(gate
            .blockers
            .contains(&"official_price_card_not_frozen".into()));
        assert!(gate
            .blockers
            .contains(&"provider_project_budget_attestation_missing_or_stale".into()));
        assert!(!experiment.join("trial-state-v1").exists());
    }

    #[test]
    fn official_price_card_is_immutable_bound_and_required_for_paid_preflight() {
        let (_outer, experiment) = ready_experiment("pilot-official-price-card");
        let sources = official_sources();
        let receipt =
            persist_official_price_card_from_documents(&experiment, Utc::now(), &sources).unwrap();
        assert_eq!(
            receipt.price_card.luna.regular_input_per_million_atto_usd,
            AttoUsd::parse_usd("0.2").unwrap()
        );
        assert_eq!(
            receipt.price_card.luna.cache_write_per_million_atto_usd,
            AttoUsd::parse_usd("0.25").unwrap()
        );
        assert!(
            persist_official_price_card_from_documents(&experiment, Utc::now(), &sources).is_err()
        );

        let progress = inspect_progress(&experiment).unwrap();
        assert_eq!(progress.price_card_state, "official_price_card_frozen");
        let gate = paid_run_preflight(&experiment, "50.00").unwrap();
        assert!(!gate.ready);
        let attestation = ensure_provider_budget_attestation(&experiment, "50.00").unwrap();
        assert_eq!(
            load_provider_budget_attestation(&experiment).unwrap(),
            attestation
        );
        let gate = paid_run_preflight(&experiment, "50.00").unwrap();
        assert!(gate.ready, "{gate:#?}");
        let authorization = persist_paid_authorization(&experiment, "50.00").unwrap();
        assert_eq!(load_paid_authorization(&experiment).unwrap(), authorization);
        assert_eq!(
            inspect_progress(&experiment)
                .unwrap()
                .paid_run_authorization_state,
            "authorized_max_usd_50"
        );
        assert!(persist_paid_authorization(&experiment, "50.00").is_err());
    }

    #[test]
    fn missing_or_stale_provider_budget_attestation_never_opens_paid_traffic() {
        let (_outer, experiment) = ready_experiment("pilot-budget-attestation-stale");
        persist_official_price_card_from_documents(&experiment, Utc::now(), &official_sources())
            .unwrap();
        let missing = paid_run_preflight(&experiment, "50.00").unwrap();
        assert!(missing
            .blockers
            .contains(&"provider_project_budget_attestation_missing_or_stale".into()));

        write_budget_attestation_at(
            &experiment,
            Utc::now() - chrono::Duration::hours(OFFICIAL_PRICE_MAX_AGE_HOURS + 1),
        );
        assert!(load_provider_budget_attestation(&experiment).is_err());
        let stale = paid_run_preflight(&experiment, "50.00").unwrap();
        assert!(!stale.ready);
        assert!(stale
            .blockers
            .contains(&"provider_project_budget_attestation_missing_or_stale".into()));
    }

    #[test]
    fn dispatch_gate_rechecks_price_when_a_run_crosses_twenty_four_hours() {
        let (_outer, experiment) = ready_experiment("pilot-cross-price-window");
        let frozen_at = Utc::now() - chrono::Duration::hours(23);
        persist_official_price_card_from_documents(&experiment, frozen_at, &official_sources())
            .unwrap();
        write_budget_attestation_at(&experiment, Utc::now());
        persist_paid_authorization(&experiment, "50.00").unwrap();
        require_paid_dispatch_gates_at(&experiment, Utc::now()).unwrap();
        let after_boundary = frozen_at
            + chrono::Duration::hours(OFFICIAL_PRICE_MAX_AGE_HOURS)
            + chrono::Duration::seconds(1);
        let error = require_paid_dispatch_gates_at(&experiment, after_boundary)
            .unwrap_err()
            .to_string();
        assert!(error.contains("24-hour dispatch window"));
    }

    #[test]
    fn price_evidence_with_stale_or_unexpected_luna_rates_never_opens_the_paid_gate() {
        let (_outer, experiment) = ready_experiment("pilot-stale-or-wrong-price-card");
        let mut sources = official_sources();
        sources[0].body = b"GPT-5.6 Sol $5.00 $0.50 $30.00 GPT-5.6 Luna $1.00 $0.10 $6.00".to_vec();
        assert!(
            persist_official_price_card_from_documents(&experiment, Utc::now(), &sources).is_err()
        );
        assert!(!experiment.join(OFFICIAL_PRICE_CARD_FILE).exists());

        let sources = official_sources();
        persist_official_price_card_from_documents(
            &experiment,
            Utc::now() - chrono::Duration::hours(OFFICIAL_PRICE_MAX_AGE_HOURS + 1),
            &sources,
        )
        .unwrap();
        assert_eq!(
            inspect_progress(&experiment).unwrap().price_card_state,
            "official_price_card_expired"
        );
        assert!(!paid_run_preflight(&experiment, "50.00").unwrap().ready);
    }

    #[test]
    fn progress_rejects_multiple_trial_ids_for_one_slot() {
        let outer = private_parent();
        let parent = private_child(outer.path(), "experiments");
        let experiment = parent.join("pilot-slot-collision");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        let worker = std::env::current_exe().unwrap();
        preregister(repository, &experiment, &worker).unwrap();
        persist_fixture_receipt(&experiment, repository, &successful_fixture_check()).unwrap();
        let journal = open_trial_journal(&experiment).unwrap();
        journal
            .append("trial-a", 0, TrialPhaseV1::FixtureReady, None, None, None)
            .unwrap();
        journal
            .append("trial-b", 0, TrialPhaseV1::FixtureReady, None, None, None)
            .unwrap();
        assert!(inspect_progress(&experiment).is_err());
    }

    #[test]
    fn successful_fixture_receipt_is_bound_to_exact_preregistration_once() {
        let outer = private_parent();
        let parent = private_child(outer.path(), "experiments");
        let experiment = parent.join("pilot-receipt");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        let worker = std::env::current_exe().unwrap();
        preregister(repository, &experiment, &worker).unwrap();
        let check = successful_fixture_check();
        let receipt = persist_fixture_receipt(&experiment, repository, &check).unwrap();
        assert_eq!(receipt.fixture_check, check);
        let journal = open_trial_journal(&experiment).unwrap();
        journal
            .append(
                "trial-000",
                0,
                crate::trial_state::TrialPhaseV1::FixtureReady,
                None,
                None,
                None,
            )
            .unwrap();
        assert!(persist_fixture_receipt(&experiment, repository, &receipt.fixture_check).is_err());
    }

    #[test]
    fn trial_workspace_lifecycle_records_wsp_export_and_reconstruction_proof() {
        let outer = private_parent();
        let parent = private_child(outer.path(), "experiments");
        let experiment = parent.join("pilot-lifecycle");
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
        let worker = std::env::current_exe().unwrap();
        preregister(repository, &experiment, &worker).unwrap();
        persist_fixture_receipt(&experiment, repository, &successful_fixture_check()).unwrap();
        let lifecycle = materialize_trial_workspace(&experiment, "trial-007", 0).unwrap();
        let target = outer.path().join("external-target");
        fs::create_dir(&target).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
        let context = lifecycle
            .trial_context(&target.canonicalize().unwrap())
            .unwrap();
        let fixture =
            crate::arm::load_fixture_inputs(&context.workspace_root, &context.fixture_id).unwrap();
        let prepared = crate::arm::prepare_executor_phase(&context, &fixture, None).unwrap();
        assert_eq!(prepared.worker_spec.arm, ExperimentArm::S0);
        assert_eq!(prepared.worker_spec.route.model, STRONG_MODEL);
        let provider_call = provider_call(&lifecycle);
        assert!(lifecycle
            .record_provider_result(&provider_call, &provider_receipt(&provider_call))
            .is_err());
        lifecycle.mark_execution_started().unwrap();
        let provider_handle = lifecycle
            .record_provider_result(&provider_call, &provider_receipt(&provider_call))
            .unwrap();
        let tool_call = tool_call(&lifecycle, &target.canonicalize().unwrap());
        let tool_handle = lifecycle
            .record_tool_result(&tool_call, &tool_receipt(&tool_call))
            .unwrap();
        let persisted = format!(
            "{}\n{}",
            fs::read_to_string(provider_handle.path).unwrap(),
            fs::read_to_string(tool_handle.path).unwrap()
        );
        assert!(!persisted.contains("private planner input"));
        assert!(!persisted.contains("private assistant response"));
        assert!(!persisted.contains("private tool argument"));
        assert!(!persisted.contains("private tool output"));
        fs::write(
            lifecycle.workspace.path().join("src/lib.rs"),
            b"pub fn trial_result() -> bool { true }\n",
        )
        .unwrap();
        let proof = lifecycle.export_and_prove().unwrap();
        proof.cleanup_after_persisting().unwrap();
        let events = open_trial_journal(&experiment).unwrap().events().unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(events[4].phase, TrialPhaseV1::RecoveryProven);
    }

    #[test]
    fn prepared_phase_runs_through_lifecycle_without_persisting_model_bodies() {
        let (outer, lifecycle) = ready_lifecycle("pilot-prepared-phase");
        let target = outer.path().join("external-target");
        fs::create_dir(&target).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
        let context = lifecycle
            .trial_context(&target.canonicalize().unwrap())
            .unwrap();
        let fixture =
            crate::arm::load_fixture_inputs(&context.workspace_root, &context.fixture_id).unwrap();
        let prepared = crate::arm::prepare_executor_phase(&context, &fixture, None).unwrap();
        let result = lifecycle
            .run_prepared_phase_with(
                &prepared,
                |call| {
                    let completion = r#"{"schema_version":"ca.rnd097.executor_completion.v1","summary":"private-completion-sentinel","required_verifier_attempted":true}"#;
                    let response = serde_json::json!({
                        "content": completion,
                        "reasoning": "private-reasoning-sentinel",
                        "response_items": [{"output_index": 0, "item": {
                            "type": "message", "id": "msg-1", "role": "assistant",
                            "content": [{"type": "output_text", "text": completion}]
                        }}],
                        "tool_calls": [],
                        "finish_reason": "stop"
                    });
                    let response = serde_json::to_string(&response).unwrap();
                    let mut receipt = provider_receipt(call);
                    receipt.returned_model = Some(call.route.model.clone());
                    receipt.outcome = ProviderCallOutcomeV1::Completed {
                        controller_response_sha256: raw_sha256(&response),
                        controller_response_json: response,
                    };
                    Ok(receipt)
                },
                |_| Err(anyhow!("tool worker must not run")),
            )
            .unwrap();
        let completion = crate::arm::validate_executor_completion(result.content()).unwrap();
        assert_eq!(completion.summary, "private-completion-sentinel");
        let receipts = fs::read_dir(lifecycle.experiment_dir.join("provider-receipts-v1"))
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(receipts.len(), 1);
        let retained = fs::read_to_string(receipts[0].path()).unwrap();
        assert!(!retained.contains("private-completion-sentinel"));
        assert!(!retained.contains("private-reasoning-sentinel"));
        let proof = lifecycle.export_and_prove().unwrap();
        proof.cleanup_after_persisting().unwrap();
    }

    #[test]
    fn preexecution_resume_reopens_or_finishes_the_exact_wsp_workspace() {
        let (_outer, experiment) = ready_experiment("pilot-preexecution-resume");
        let journal = open_trial_journal(&experiment).unwrap();
        journal
            .append(
                "trial-fixture-ready",
                0,
                TrialPhaseV1::FixtureReady,
                None,
                None,
                None,
            )
            .unwrap();
        let resumed = open_or_materialize_trial_workspace(&experiment, 0).unwrap();
        let first_id = resumed.workspace.workspace_id();
        let first_path = resumed.workspace.path().to_path_buf();
        drop(resumed);
        assert!(first_path.exists());
        let reopened = open_or_materialize_trial_workspace(&experiment, 0).unwrap();
        assert_eq!(reopened.workspace.workspace_id(), first_id);
        assert_eq!(reopened.workspace.path(), first_path);
        reopened.mark_execution_started().unwrap();
        let proof = reopened.export_and_prove().unwrap();
        proof.cleanup_after_persisting().unwrap();
        assert!(!first_path.exists());
    }

    #[test]
    fn ambiguous_broker_receipt_is_persisted_then_makes_the_trial_terminal() {
        let (_outer, lifecycle) = ready_lifecycle("pilot-ambiguous-broker");
        lifecycle.mark_execution_started().unwrap();
        let call = provider_call(&lifecycle);
        let result = lifecycle.dispatch_provider_with(&call, |call| {
            let mut receipt = provider_receipt(call);
            receipt.x_request_id_sha256 = None;
            receipt.returned_model = None;
            receipt.returned_service_tier = None;
            receipt.usage = None;
            receipt.outcome = ProviderCallOutcomeV1::AmbiguousDelivery {
                reason: "stream_incomplete".into(),
            };
            Ok(receipt)
        });
        assert!(result.is_err());

        let store = ProviderReceiptStore::create(
            &lifecycle.experiment_dir,
            &lifecycle.preregistration_sha256,
        )
        .unwrap();
        assert!(matches!(
            store
                .load(lifecycle.slot_index, &call.call_id)
                .unwrap()
                .outcome,
            crate::receipt_store::StoredProviderOutcomeV1::AmbiguousDelivery { .. }
        ));
        let events = lifecycle.journal.events().unwrap();
        assert_eq!(events.last().unwrap().phase, TrialPhaseV1::Interrupted);
        assert_eq!(
            events.last().unwrap().reason_code.as_deref(),
            Some("provider_delivery_ambiguous")
        );
        assert!(lifecycle
            .record_provider_result(&call, &provider_receipt(&call))
            .is_err());
    }

    #[test]
    fn broker_transport_failure_is_terminal_without_persisting_its_error_text() {
        let (_outer, lifecycle) = ready_lifecycle("pilot-broker-transport-failure");
        lifecycle.mark_execution_started().unwrap();
        let call = provider_call(&lifecycle);
        let result = lifecycle.dispatch_provider_with(&call, |_| {
            Err(anyhow!("private broker transport diagnostic"))
        });
        assert!(result.is_err());
        assert!(!lifecycle
            .experiment_dir
            .join("provider-receipts-v1")
            .exists());
        let events = lifecycle.journal.events().unwrap();
        assert_eq!(events.last().unwrap().phase, TrialPhaseV1::Interrupted);
        assert_eq!(
            events.last().unwrap().reason_code.as_deref(),
            Some("provider_worker_failed")
        );
        let bytes = fs::read_to_string(
            lifecycle
                .experiment_dir
                .join("trial-state-v1")
                .join("00000000000000000002.json"),
        )
        .unwrap();
        assert!(!bytes.contains("private broker transport diagnostic"));
    }

    #[test]
    fn noncompleted_tool_receipt_is_persisted_then_makes_the_trial_terminal() {
        let (outer, lifecycle) = ready_lifecycle("pilot-failed-tool");
        lifecycle.mark_execution_started().unwrap();
        let target = outer.path().join("external-target");
        fs::create_dir(&target).unwrap();
        let call = tool_call(&lifecycle, &target.canonicalize().unwrap());
        let result = lifecycle.dispatch_tool_with(&call, |call| {
            let mut receipt = tool_receipt(call);
            receipt.outcome = ToolCallOutcomeV1::Failed {
                reason: "command_failed".into(),
            };
            receipt.result_json = None;
            receipt.result_sha256 = None;
            Ok(receipt)
        });
        assert!(result.is_err());

        let store = ProviderReceiptStore::create(
            &lifecycle.experiment_dir,
            &lifecycle.preregistration_sha256,
        )
        .unwrap();
        assert!(matches!(
            store
                .load_tool(lifecycle.slot_index, &call.call_id)
                .unwrap()
                .outcome,
            crate::receipt_store::StoredToolOutcomeV1::Failed { .. }
        ));
        let events = lifecycle.journal.events().unwrap();
        assert_eq!(events.last().unwrap().phase, TrialPhaseV1::Interrupted);
        assert_eq!(
            events.last().unwrap().reason_code.as_deref(),
            Some("tool_result_noncompleted")
        );
    }

    #[test]
    fn deterministic_slot_order_is_balanced_but_experiment_specific() {
        let first = deterministic_slots(Uuid::from_u128(1));
        let second = deterministic_slots(Uuid::from_u128(2));
        assert_eq!(first.len(), 72);
        assert_eq!(first[..6], second[..6]);
        assert_ne!(first[6..], second[6..]);
        for fixture_id in FIXTURE_IDS {
            for arm in ExperimentArm::ALL {
                assert_eq!(
                    first
                        .iter()
                        .filter(|slot| slot.fixture_id == fixture_id && slot.arm == arm)
                        .count(),
                    2
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn preregistration_refuses_symlinked_experiment_directory() {
        use std::os::unix::fs::symlink;

        let outer = private_parent();
        let parent = private_child(outer.path(), "experiments");
        let existing = private_child(&parent, "existing");
        let linked = parent.join("linked");
        symlink(existing, &linked).unwrap();
        assert!(create_experiment_directory(&linked).is_err());
    }
}
