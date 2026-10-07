//! Frozen RND-097 arm and phase construction.

use crate::artifact::{
    parse_and_bind_planner_proposal, ExecutorCompletionV1, HandoffAuthorityV1, ValidatedHandoffV1,
    EXECUTOR_SYSTEM_PROMPT_V1, PLANNER_SYSTEM_PROMPT_V1,
};
use crate::controller::ControllerBinaryIdentityV1;
use crate::reasoning_experiment_protocol::{
    ExperimentArm, ExperimentControllerChannelV1, ExperimentDeclaredFileV1, ExperimentFileRoot,
    ExperimentHandoffV1, ExperimentModelRouteV1, ExperimentPhase, ExperimentRequestLimitsV1,
    ExperimentWorkerSpecV1, CHEAP_MODEL, STRONG_MODEL, WORKER_SPEC_SCHEMA_V1,
};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

const FIXTURE_SCHEMA_V1: &str = "reasoning_pilot_fixture.v1";
const MAX_FIXTURE_FILE_BYTES: usize = 256 * 1024;
const MAX_FIXTURE_TREE_BYTES: usize = 2 * 1024 * 1024;
const MAX_FIXTURE_FILES: usize = 64;
const MAX_PROMPT_BYTES: usize = 512 * 1024;
const HANDOFF_DIRECTORY: &str = "handoffs-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FixtureManifestV1 {
    schema_version: String,
    id: String,
    package: String,
    task_file: String,
    allowed_paths: Vec<PathBuf>,
    public_test_argv: Vec<String>,
    stdlib_only: bool,
    network_allowed: bool,
}

pub struct FixtureInputsV1 {
    pub fixture_id: String,
    pub task_path: PathBuf,
    pub task_sha256: String,
    task: String,
    snapshot: String,
    pub allowed_read_paths: Vec<PathBuf>,
    pub allowed_write_paths: Vec<PathBuf>,
    pub required_verifier_commands: Vec<Vec<String>>,
}

impl std::fmt::Debug for FixtureInputsV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FixtureInputsV1")
            .field("fixture_id", &self.fixture_id)
            .field("task_path", &self.task_path)
            .field("task_sha256", &self.task_sha256)
            .field("task", &"[REDACTED]")
            .field("snapshot", &"[REDACTED]")
            .field("allowed_read_paths", &"[DECLARED]")
            .field("allowed_write_paths", &"[DECLARED]")
            .field("required_verifier_commands", &"[DECLARED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct TrialContextV1 {
    pub experiment_id: String,
    pub preregistration_sha256: String,
    pub trial_id: String,
    pub slot_index: u16,
    pub fixture_id: String,
    pub arm: ExperimentArm,
    pub repetition: u8,
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub controller_root: PathBuf,
    pub cargo_target_dir: PathBuf,
    pub source_manifest_sha256: String,
    pub destination_manifest_sha256: String,
    pub planner_schema_sha256: String,
    pub executor_schema_sha256: String,
    pub binary_identity: ControllerBinaryIdentityV1,
    pub toolchain: Vec<crate::pilot::PilotToolchainExecutableV1>,
}

impl std::fmt::Debug for TrialContextV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrialContextV1")
            .field("experiment_id", &self.experiment_id)
            .field("trial_id", &self.trial_id)
            .field("slot_index", &self.slot_index)
            .field("fixture_id", &self.fixture_id)
            .field("arm", &self.arm)
            .field("repetition", &self.repetition)
            .field("workspace_id", &self.workspace_id)
            .field("workspace_root", &"[PRIVATE]")
            .field("controller_root", &"[PRIVATE]")
            .field("cargo_target_dir", &"[PRIVATE]")
            .field("toolchain", &self.toolchain)
            .finish_non_exhaustive()
    }
}

pub struct PreparedPhaseV1 {
    pub worker_spec: ExperimentWorkerSpecV1,
    binary_identity: ControllerBinaryIdentityV1,
    system_prompt: String,
    task_prompt: String,
    output_schema_name: String,
    output_schema: Value,
}

impl std::fmt::Debug for PreparedPhaseV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedPhaseV1")
            .field("worker_spec", &self.worker_spec)
            .field("binary_identity", &self.binary_identity)
            .field("system_prompt", &"[REDACTED]")
            .field("task_prompt", &"[REDACTED]")
            .field("output_schema", &"[DECLARED]")
            .finish()
    }
}

impl PreparedPhaseV1 {
    pub fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    pub fn task_prompt(&self) -> &str {
        &self.task_prompt
    }

    pub fn output_schema(&self) -> (&str, Value) {
        (&self.output_schema_name, self.output_schema.clone())
    }

    pub fn binary_identity(&self) -> &ControllerBinaryIdentityV1 {
        &self.binary_identity
    }

    pub fn validate_before_execution(&self) -> Result<()> {
        self.worker_spec
            .validate()
            .map_err(|error| anyhow!("prepared worker spec is invalid: {error}"))?;
        self.binary_identity.validate()?;
        crate::conversation::PhaseConversation::new_with_output_schema(
            self.worker_spec.route.clone(),
            &self.system_prompt,
            &self.task_prompt,
            self.worker_spec.phase != ExperimentPhase::Planner,
            Some((&self.output_schema_name, self.output_schema.clone())),
        )?
        .request_json()?;
        Ok(())
    }
}

pub struct PersistedHandoffV1 {
    pub handoff: ExperimentHandoffV1,
    visible_text: String,
    requires_strong_executor: bool,
}

impl std::fmt::Debug for PersistedHandoffV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PersistedHandoffV1")
            .field("handoff", &"[DECLARED]")
            .field("visible_text", &"[REDACTED]")
            .field("requires_strong_executor", &self.requires_strong_executor)
            .finish()
    }
}

pub fn load_fixture_inputs(
    workspace_root: &Path,
    expected_fixture_id: &str,
) -> Result<FixtureInputsV1> {
    require_exact_directory(workspace_root)?;
    let manifest_bytes =
        read_bounded_regular_file(&workspace_root.join("fixture.json"), MAX_FIXTURE_FILE_BYTES)?;
    let manifest: FixtureManifestV1 =
        serde_json::from_slice(&manifest_bytes).context("fixture manifest is not strict JSON")?;
    if manifest.schema_version != FIXTURE_SCHEMA_V1
        || manifest.id != expected_fixture_id
        || manifest.task_file != "TASK.md"
        || !manifest.stdlib_only
        || manifest.network_allowed
        || manifest.public_test_argv != ["cargo", "test", "--quiet"]
        || manifest.allowed_paths.is_empty()
        || manifest.allowed_paths.len() > MAX_FIXTURE_FILES
    {
        bail!("fixture manifest differs from the frozen pilot contract");
    }
    validate_identifier("fixture id", &manifest.id)?;
    validate_identifier("fixture package", &manifest.package)?;
    let mut allowed_writes = BTreeSet::new();
    for path in &manifest.allowed_paths {
        validate_relative_path(path)?;
        if !allowed_writes.insert(path.clone()) {
            bail!("fixture manifest has duplicate allowed paths");
        }
    }

    let mut files = Vec::new();
    collect_regular_files(workspace_root, workspace_root, 0, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    if files.is_empty() || files.len() > MAX_FIXTURE_FILES {
        bail!("fixture file count is invalid");
    }
    let mut aggregate = 0usize;
    let mut snapshot = String::new();
    let mut allowed_reads = Vec::with_capacity(files.len());
    let mut task = None;
    let mut task_sha256 = None;
    for (relative, absolute) in files {
        let bytes = read_bounded_regular_file(&absolute, MAX_FIXTURE_FILE_BYTES)?;
        aggregate = aggregate
            .checked_add(bytes.len())
            .ok_or_else(|| anyhow!("fixture byte count overflow"))?;
        if aggregate > MAX_FIXTURE_TREE_BYTES {
            bail!("fixture tree exceeds its byte bound");
        }
        let text = String::from_utf8(bytes).context("fixture contains a non-UTF-8 regular file")?;
        if relative == Path::new("TASK.md") {
            task_sha256 = Some(sha256_hex(text.as_bytes()));
            task = Some(text.clone());
        }
        allowed_reads.push(relative.clone());
        snapshot.push_str("\n<file path=\"");
        snapshot.push_str(&relative.to_string_lossy());
        snapshot.push_str("\">\n");
        snapshot.push_str(&text);
        snapshot.push_str("\n</file>\n");
    }
    if snapshot.len() > MAX_PROMPT_BYTES {
        bail!("fixture snapshot exceeds the planner prompt bound");
    }
    for path in &allowed_writes {
        if !allowed_reads.contains(path) {
            bail!("fixture write authority names a missing regular file");
        }
    }
    Ok(FixtureInputsV1 {
        fixture_id: manifest.id,
        task_path: PathBuf::from("TASK.md"),
        task_sha256: task_sha256.ok_or_else(|| anyhow!("fixture task file is missing"))?,
        task: task.ok_or_else(|| anyhow!("fixture task file is missing"))?,
        snapshot,
        allowed_read_paths: allowed_reads,
        allowed_write_paths: allowed_writes.into_iter().collect(),
        required_verifier_commands: vec![manifest.public_test_argv],
    })
}

pub fn prepare_planner_phase(
    context: &TrialContextV1,
    fixture: &FixtureInputsV1,
) -> Result<PreparedPhaseV1> {
    validate_context(context, fixture)?;
    let schema = load_declared_schema(
        &context
            .controller_root
            .join("schemas-v1/planner-proposal.schema.json"),
        &context.planner_schema_sha256,
    )?;
    let task_prompt = format!(
        "Plan this task from the bounded untrusted repository snapshot. Return only ca.rnd097.planner_proposal.v1 JSON.\n\n<task>\n{}\n</task>\n<repository_snapshot>{}\n</repository_snapshot>",
        fixture.task, fixture.snapshot
    );
    if task_prompt.len() > MAX_PROMPT_BYTES {
        bail!("planner task prompt exceeds its bound");
    }
    let worker_spec = worker_spec(
        context,
        fixture,
        ExperimentPhase::Planner,
        ExperimentModelRouteV1::openai(STRONG_MODEL),
        ExperimentHandoffV1::None,
        Vec::new(),
        Vec::new(),
        "schemas-v1/planner-proposal.schema.json",
        &context.planner_schema_sha256,
        serde_json::to_vec(&schema)?.len() as u64,
    )?;
    Ok(PreparedPhaseV1 {
        worker_spec,
        binary_identity: context.binary_identity.clone(),
        system_prompt: PLANNER_SYSTEM_PROMPT_V1.into(),
        task_prompt,
        output_schema_name: "planner_proposal".into(),
        output_schema: schema,
    })
}

pub fn bind_and_persist_planner_handoff(
    context: &TrialContextV1,
    fixture: &FixtureInputsV1,
    proposal_json: &str,
) -> Result<PersistedHandoffV1> {
    validate_context(context, fixture)?;
    if matches!(context.arm, ExperimentArm::S0 | ExperimentArm::C0) {
        bail!("direct arms may not create a planner handoff");
    }
    let validated = parse_and_bind_planner_proposal(
        proposal_json,
        HandoffAuthorityV1 {
            experiment_id: context.experiment_id.clone(),
            trial_id: context.trial_id.clone(),
            fixture_id: context.fixture_id.clone(),
            task_input_sha256: fixture.task_sha256.clone(),
            source_manifest_sha256: context.source_manifest_sha256.clone(),
            planner_route: ExperimentModelRouteV1::openai(STRONG_MODEL),
            allowed_read_paths: fixture.allowed_read_paths.clone(),
            allowed_write_paths: fixture.allowed_write_paths.clone(),
            required_verifier_commands: fixture.required_verifier_commands.clone(),
        },
        format!("{}-plan", context.trial_id),
    )?;
    persist_handoff(context, validated)
}

pub fn prepare_executor_phase(
    context: &TrialContextV1,
    fixture: &FixtureInputsV1,
    handoff: Option<&PersistedHandoffV1>,
) -> Result<PreparedPhaseV1> {
    validate_context(context, fixture)?;
    let (route, handoff_contract, visible_handoff) = match context.arm {
        ExperimentArm::S0 => (ExperimentModelRouteV1::openai(STRONG_MODEL), None, None),
        ExperimentArm::C0 => (ExperimentModelRouteV1::openai(CHEAP_MODEL), None, None),
        ExperimentArm::Ss => structured_executor(handoff, STRONG_MODEL)?,
        ExperimentArm::Sc => structured_executor(handoff, CHEAP_MODEL)?,
        ExperimentArm::ScG => {
            let handoff = handoff.ok_or_else(|| anyhow!("SC-G requires a planner handoff"))?;
            let model = if handoff.requires_strong_executor {
                STRONG_MODEL
            } else {
                CHEAP_MODEL
            };
            structured_executor(Some(handoff), model)?
        }
        ExperimentArm::Sp => {
            let handoff = handoff.ok_or_else(|| anyhow!("SP requires its own prose handoff"))?;
            match &handoff.handoff {
                ExperimentHandoffV1::SizeMatchedProse { .. } => (
                    ExperimentModelRouteV1::openai(CHEAP_MODEL),
                    Some(handoff.handoff.clone()),
                    Some(handoff.visible_text.as_str()),
                ),
                _ => bail!("SP requires a size-matched prose handoff"),
            }
        }
    };
    if matches!(context.arm, ExperimentArm::S0 | ExperimentArm::C0) && handoff.is_some() {
        bail!("direct arms may not receive a planner handoff");
    }
    let schema = load_declared_schema(
        &context
            .controller_root
            .join("schemas-v1/executor-completion.schema.json"),
        &context.executor_schema_sha256,
    )?;
    let mut task_prompt = format!(
        "Complete the following fixture task. Use only declared tools and paths. The external verifier, not your claim, determines success. Return only ca.rnd097.executor_completion.v1 JSON when finished.\n\n<task>\n{}\n</task>",
        fixture.task
    );
    let cargo = context
        .toolchain
        .iter()
        .find(|tool| tool.name == "cargo")
        .ok_or_else(|| anyhow!("trial context lacks its frozen Cargo executable"))?;
    let rustc = context
        .toolchain
        .iter()
        .find(|tool| tool.name == "rustc")
        .ok_or_else(|| anyhow!("trial context lacks its frozen Rustc executable"))?;
    task_prompt.push_str(&format!(
        "\nFor run_command, use only these preregistered absolute executables: cargo={} (sha256 {}), rustc={} (sha256 {}). Never invoke a shell or an env wrapper.",
        cargo.path.display(), cargo.sha256, rustc.path.display(), rustc.sha256
    ));
    if let Some(visible_handoff) = visible_handoff {
        task_prompt.push_str("\n<handoff>\n");
        task_prompt.push_str(visible_handoff);
        task_prompt.push_str("\n</handoff>");
    }
    if task_prompt.len() > MAX_PROMPT_BYTES {
        bail!("executor task prompt exceeds its bound");
    }
    let worker_spec = worker_spec(
        context,
        fixture,
        ExperimentPhase::Executor,
        route,
        handoff_contract.unwrap_or(ExperimentHandoffV1::None),
        vec![
            "read_file".into(),
            "run_command".into(),
            "write_file".into(),
        ],
        fixture.allowed_write_paths.clone(),
        "schemas-v1/executor-completion.schema.json",
        &context.executor_schema_sha256,
        serde_json::to_vec(&schema)?.len() as u64,
    )?;
    Ok(PreparedPhaseV1 {
        worker_spec,
        binary_identity: context.binary_identity.clone(),
        system_prompt: EXECUTOR_SYSTEM_PROMPT_V1.into(),
        task_prompt,
        output_schema_name: "executor_completion".into(),
        output_schema: schema,
    })
}

pub fn validate_executor_completion(value: &str) -> Result<ExecutorCompletionV1> {
    crate::artifact::parse_executor_completion(value)
}

pub fn prepare_recovery_phase(
    context: &TrialContextV1,
    fixture: &FixtureInputsV1,
    handoff: &PersistedHandoffV1,
    verifier_failure_code: &str,
) -> Result<PreparedPhaseV1> {
    validate_context(context, fixture)?;
    if context.arm != ExperimentArm::ScG
        || verifier_failure_code.is_empty()
        || verifier_failure_code.len() > 128
        || verifier_failure_code
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        bail!("recovery phase is outside the frozen SC-G contract");
    }
    let visible_handoff = match &handoff.handoff {
        ExperimentHandoffV1::StructuredArtifact { .. } => handoff.visible_text.as_str(),
        _ => bail!("SC-G recovery requires its structured planner handoff"),
    };
    let schema = load_declared_schema(
        &context
            .controller_root
            .join("schemas-v1/executor-completion.schema.json"),
        &context.executor_schema_sha256,
    )?;
    let task_prompt = format!(
        "The first executor failed deterministic verification with bounded failure class `{verifier_failure_code}`. Repair the same fixture using only declared tools and paths. Hidden verifier source and commands are unavailable. Return only ca.rnd097.executor_completion.v1 JSON when finished.\n\n<task>\n{}\n</task>\n<handoff>\n{}\n</handoff>",
        fixture.task, visible_handoff
    );
    if task_prompt.len() > MAX_PROMPT_BYTES {
        bail!("recovery task prompt exceeds its bound");
    }
    let worker_spec = worker_spec(
        context,
        fixture,
        ExperimentPhase::Recovery,
        ExperimentModelRouteV1::openai(STRONG_MODEL),
        handoff.handoff.clone(),
        vec![
            "read_file".into(),
            "run_command".into(),
            "write_file".into(),
        ],
        fixture.allowed_write_paths.clone(),
        "schemas-v1/executor-completion.schema.json",
        &context.executor_schema_sha256,
        serde_json::to_vec(&schema)?.len() as u64,
    )?;
    Ok(PreparedPhaseV1 {
        worker_spec,
        binary_identity: context.binary_identity.clone(),
        system_prompt: EXECUTOR_SYSTEM_PROMPT_V1.into(),
        task_prompt,
        output_schema_name: "executor_completion".into(),
        output_schema: schema,
    })
}

fn structured_executor<'a>(
    handoff: Option<&'a PersistedHandoffV1>,
    model: &str,
) -> Result<(
    ExperimentModelRouteV1,
    Option<ExperimentHandoffV1>,
    Option<&'a str>,
)> {
    let handoff = handoff.ok_or_else(|| anyhow!("structured arm requires a planner handoff"))?;
    match &handoff.handoff {
        ExperimentHandoffV1::StructuredArtifact { .. } => Ok((
            ExperimentModelRouteV1::openai(model),
            Some(handoff.handoff.clone()),
            Some(handoff.visible_text.as_str()),
        )),
        _ => bail!("structured arm requires a structured planner handoff"),
    }
}

fn persist_handoff(
    context: &TrialContextV1,
    validated: ValidatedHandoffV1,
) -> Result<PersistedHandoffV1> {
    let directory = create_private_directory(&context.controller_root, HANDOFF_DIRECTORY)?;
    let structured_name = format!("{}-structured.json", context.trial_id);
    let structured_path = directory.join(&structured_name);
    write_new_private_file(&structured_path, validated.bytes())?;
    let structured_relative = structured_path
        .strip_prefix(&context.controller_root)?
        .to_path_buf();
    let requires_strong_executor = validated.requires_strong_executor();
    if context.arm == ExperimentArm::Sp {
        let prose = validated.size_matched_prose()?;
        let prose_name = format!("{}-prose.txt", context.trial_id);
        let prose_path = directory.join(&prose_name);
        write_new_private_file(&prose_path, prose.as_bytes())?;
        let prose_relative = prose_path
            .strip_prefix(&context.controller_root)?
            .to_path_buf();
        return Ok(PersistedHandoffV1 {
            handoff: ExperimentHandoffV1::SizeMatchedProse {
                file: ExperimentDeclaredFileV1 {
                    root: ExperimentFileRoot::Controller,
                    relative_path: prose_relative,
                    sha256: sha256_hex(prose.as_bytes()),
                    max_bytes: prose.len() as u64,
                },
                source_artifact_sha256: validated.sha256().into(),
                source_artifact_bytes: validated.bytes().len() as u64,
                prose_bytes: prose.len() as u64,
            },
            visible_text: prose,
            requires_strong_executor,
        });
    }
    Ok(PersistedHandoffV1 {
        handoff: ExperimentHandoffV1::StructuredArtifact {
            file: ExperimentDeclaredFileV1 {
                root: ExperimentFileRoot::Controller,
                relative_path: structured_relative,
                sha256: validated.sha256().into(),
                max_bytes: validated.bytes().len() as u64,
            },
        },
        visible_text: String::from_utf8(validated.bytes().to_vec())?,
        requires_strong_executor,
    })
}

#[allow(clippy::too_many_arguments)]
fn worker_spec(
    context: &TrialContextV1,
    fixture: &FixtureInputsV1,
    phase: ExperimentPhase,
    route: ExperimentModelRouteV1,
    handoff: ExperimentHandoffV1,
    allowed_tools: Vec<String>,
    allowed_write_paths: Vec<PathBuf>,
    schema_relative: &str,
    schema_sha256: &str,
    schema_bytes: u64,
) -> Result<ExperimentWorkerSpecV1> {
    let worker_spec = ExperimentWorkerSpecV1 {
        schema_version: WORKER_SPEC_SCHEMA_V1.into(),
        experiment_id: context.experiment_id.clone(),
        preregistration_sha256: context.preregistration_sha256.clone(),
        trial_id: context.trial_id.clone(),
        slot_index: context.slot_index,
        fixture_id: context.fixture_id.clone(),
        arm: context.arm,
        repetition: context.repetition,
        phase,
        route,
        limits: ExperimentRequestLimitsV1::for_phase(phase),
        workspace_id: context.workspace_id.clone(),
        workspace_root: context.workspace_root.clone(),
        controller_root: context.controller_root.clone(),
        source_manifest_sha256: context.source_manifest_sha256.clone(),
        destination_manifest_sha256: context.destination_manifest_sha256.clone(),
        cargo_target_dir: context.cargo_target_dir.clone(),
        task_input_sha256: fixture.task_sha256.clone(),
        task_input: ExperimentDeclaredFileV1 {
            root: ExperimentFileRoot::Workspace,
            relative_path: fixture.task_path.clone(),
            sha256: fixture.task_sha256.clone(),
            max_bytes: fixture.task.len() as u64,
        },
        handoff,
        output_schema: ExperimentDeclaredFileV1 {
            root: ExperimentFileRoot::Controller,
            relative_path: PathBuf::from(schema_relative),
            sha256: schema_sha256.into(),
            max_bytes: schema_bytes,
        },
        allowed_tools,
        allowed_read_paths: fixture.allowed_read_paths.clone(),
        allowed_write_paths,
        controller_channel: ExperimentControllerChannelV1 {
            channel_id: format!("{}-{phase:?}", context.trial_id).to_ascii_lowercase(),
            nonce_sha256: sha256_hex(
                format!("{}:{phase:?}", context.preregistration_sha256).as_bytes(),
            ),
            inherited_fd: 9,
        },
        network_allowed: false,
    };
    worker_spec
        .validate()
        .map_err(|error| anyhow!("prepared worker spec is invalid: {error}"))?;
    Ok(worker_spec)
}

fn validate_context(context: &TrialContextV1, fixture: &FixtureInputsV1) -> Result<()> {
    for id in [
        context.experiment_id.as_str(),
        context.trial_id.as_str(),
        context.fixture_id.as_str(),
        context.workspace_id.as_str(),
    ] {
        validate_identifier("trial context id", id)?;
    }
    if context.fixture_id != fixture.fixture_id
        || context.slot_index >= 72
        || !matches!(context.repetition, 1 | 2)
        || !context.workspace_root.is_absolute()
        || !context.controller_root.is_absolute()
        || !context.cargo_target_dir.is_absolute()
        || context.workspace_root.starts_with(&context.controller_root)
        || context.controller_root.starts_with(&context.workspace_root)
        || context
            .cargo_target_dir
            .starts_with(&context.workspace_root)
        || context
            .cargo_target_dir
            .starts_with(&context.controller_root)
    {
        bail!("trial context identity or roots are invalid");
    }
    for digest in [
        &context.preregistration_sha256,
        &context.source_manifest_sha256,
        &context.destination_manifest_sha256,
        &context.planner_schema_sha256,
        &context.executor_schema_sha256,
    ] {
        validate_sha256(digest)?;
    }
    if context.toolchain.len() != 2
        || context.toolchain[0].name != "cargo"
        || context.toolchain[1].name != "rustc"
        || context.toolchain.iter().any(|tool| {
            !tool.path.is_absolute()
                || validate_sha256(&tool.sha256).is_err()
                || tool.path.canonicalize().ok().as_ref() != Some(&tool.path)
                || read_bounded_regular_file(&tool.path, 128 * 1024 * 1024)
                    .map(|bytes| sha256_hex(&bytes) != tool.sha256)
                    .unwrap_or(true)
        })
    {
        bail!("trial context toolchain identity is invalid or drifted");
    }
    context.binary_identity.validate()
}

fn load_declared_schema(path: &Path, expected_sha256: &str) -> Result<Value> {
    let bytes = read_bounded_regular_file(path, MAX_FIXTURE_FILE_BYTES)?;
    if sha256_hex(&bytes) != expected_sha256 {
        bail!("controller output schema drifted");
    }
    let schema: Value = serde_json::from_slice(&bytes)?;
    if !schema.is_object() {
        bail!("controller output schema is not an object");
    }
    Ok(schema)
}

fn collect_regular_files(
    root: &Path,
    current: &Path,
    depth: usize,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<()> {
    if depth > 8 || files.len() > MAX_FIXTURE_FILES {
        bail!("fixture tree exceeds its structural bound");
    }
    let mut children = fs::read_dir(current)?.collect::<std::io::Result<Vec<_>>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let absolute = child.path();
        let metadata = fs::symlink_metadata(&absolute)?;
        if metadata.file_type().is_symlink() {
            bail!("fixture tree contains a symbolic link");
        }
        if metadata.is_dir() {
            collect_regular_files(root, &absolute, depth + 1, files)?;
        } else if metadata.is_file() {
            let relative = absolute.strip_prefix(root)?.to_path_buf();
            validate_relative_path(&relative)?;
            files.push((relative, absolute));
        } else {
            bail!("fixture tree contains a special file");
        }
    }
    Ok(())
}

fn read_bounded_regular_file(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > maximum as u64 {
        bail!("declared file is not a bounded regular file");
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options.open(path)?;
    let opened = file.metadata()?;
    #[cfg(unix)]
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        bail!("declared file changed while it was opened");
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)?;
    if bytes.len() > maximum || bytes.len() as u64 != metadata.len() {
        bail!("declared file changed or exceeded its bound while read");
    }
    Ok(bytes)
}

fn create_private_directory(parent: &Path, name: &str) -> Result<PathBuf> {
    require_exact_directory(parent)?;
    let path = parent.join(name);
    match fs::create_dir(&path) {
        Ok(()) => {
            #[cfg(unix)]
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    require_exact_directory(&path)?;
    Ok(path)
}

fn write_new_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_FIXTURE_TREE_BYTES {
        bail!("handoff bytes are empty or oversized");
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("handoff has no parent"))?;
    require_exact_directory(parent)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn require_exact_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || path.canonicalize()? != path {
        bail!("directory identity is invalid");
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        bail!("directory is not owner-only");
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.to_string_lossy().len() > 4_096
    {
        bail!("fixture path is not normalized and relative");
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        bail!("{label} is invalid");
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("digest is invalid");
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::{
        executor_completion_schema, planner_proposal_schema, PlannerEvidenceV1, PlannerProposalV1,
        PlannerStepV1, PLANNER_PROPOSAL_SCHEMA_V1,
    };

    fn digest(byte: char) -> String {
        byte.to_string().repeat(64)
    }

    fn toolchain(name: &str) -> crate::pilot::PilotToolchainExecutableV1 {
        let path = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
            .unwrap()
            .canonicalize()
            .unwrap();
        crate::pilot::PilotToolchainExecutableV1 {
            name: name.into(),
            sha256: sha256_hex(&fs::read(&path).unwrap()),
            path,
        }
    }

    fn context(root: &Path, arm: ExperimentArm) -> TrialContextV1 {
        let root = root.canonicalize().unwrap();
        let workspace_root = root.join("workspace");
        let controller_root = root.join("controller");
        let cargo_target_dir = root.join("cargo-target");
        fs::create_dir(&controller_root).unwrap();
        fs::create_dir(&cargo_target_dir).unwrap();
        #[cfg(unix)]
        for directory in [&controller_root, &cargo_target_dir] {
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let schemas = create_private_directory(&controller_root, "schemas-v1").unwrap();
        let planner = planner_proposal_schema().unwrap();
        let executor = executor_completion_schema().unwrap();
        write_new_private_file(&schemas.join("planner-proposal.schema.json"), &planner).unwrap();
        write_new_private_file(&schemas.join("executor-completion.schema.json"), &executor)
            .unwrap();
        TrialContextV1 {
            experiment_id: "experiment-1".into(),
            preregistration_sha256: digest('a'),
            trial_id: format!("trial-{}", arm.id().to_ascii_lowercase()),
            slot_index: 0,
            fixture_id: "unicode_parser_boundary".into(),
            arm,
            repetition: 1,
            workspace_id: "workspace-1".into(),
            workspace_root,
            controller_root,
            cargo_target_dir,
            source_manifest_sha256: digest('b'),
            destination_manifest_sha256: digest('c'),
            planner_schema_sha256: sha256_hex(&planner),
            executor_schema_sha256: sha256_hex(&executor),
            binary_identity: ControllerBinaryIdentityV1 {
                controller_sha256: digest('d'),
                worker_sha256: digest('e'),
            },
            toolchain: vec![toolchain("cargo"), toolchain("rustc")],
        }
    }

    fn proposal() -> String {
        serde_json::to_string(&PlannerProposalV1 {
            schema_version: PLANNER_PROPOSAL_SCHEMA_V1.into(),
            objective: "Repair the Unicode boundary.".into(),
            evidence: vec![PlannerEvidenceV1 {
                path: PathBuf::from("src/lib.rs"),
                observation: "The byte slice may split a scalar.".into(),
            }],
            unresolved_decisions: Vec::new(),
            risks: Vec::new(),
            steps: vec![PlannerStepV1 {
                id: "edit".into(),
                instructions: "Move the index to a valid character boundary.".into(),
                depends_on: Vec::new(),
                affected_paths: vec![PathBuf::from("src/lib.rs")],
                verification_commands: vec![vec!["cargo".into(), "test".into(), "--quiet".into()]],
            }],
        })
        .unwrap()
    }

    #[test]
    fn fixture_inputs_are_strict_bounded_and_cover_exact_files() {
        let root = tempfile::tempdir().unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/reasoning_pilot/unicode_parser_boundary");
        let workspace = root.path().join("workspace");
        copy_fixture(&source, &workspace);
        let fixture = load_fixture_inputs(
            &workspace.canonicalize().unwrap(),
            "unicode_parser_boundary",
        )
        .unwrap();
        assert_eq!(fixture.allowed_write_paths, [PathBuf::from("src/lib.rs")]);
        assert!(fixture
            .allowed_read_paths
            .contains(&PathBuf::from("tests/public.rs")));
        assert!(!format!("{fixture:?}").contains("truncate_utf8"));
    }

    #[test]
    fn structured_and_prose_arms_are_fresh_visible_handoffs() {
        for arm in [ExperimentArm::Sc, ExperimentArm::Sp, ExperimentArm::ScG] {
            let root = tempfile::tempdir().unwrap();
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/reasoning_pilot/unicode_parser_boundary");
            let mut context = context(root.path(), arm);
            copy_fixture(&source, &context.workspace_root);
            context.workspace_root = context.workspace_root.canonicalize().unwrap();
            context.controller_root = context.controller_root.canonicalize().unwrap();
            context.cargo_target_dir = context.cargo_target_dir.canonicalize().unwrap();
            let fixture =
                load_fixture_inputs(&context.workspace_root, &context.fixture_id).unwrap();
            let planner = prepare_planner_phase(&context, &fixture).unwrap();
            assert_eq!(planner.worker_spec.phase, ExperimentPhase::Planner);
            assert!(planner.worker_spec.allowed_tools.is_empty());
            let handoff =
                bind_and_persist_planner_handoff(&context, &fixture, &proposal()).unwrap();
            let executor = prepare_executor_phase(&context, &fixture, Some(&handoff)).unwrap();
            assert_eq!(executor.worker_spec.phase, ExperimentPhase::Executor);
            assert!(!executor.worker_spec.allowed_tools.is_empty());
            assert!(executor.task_prompt().contains("<handoff>"));
            match arm {
                ExperimentArm::Sp => assert!(matches!(
                    executor.worker_spec.handoff,
                    ExperimentHandoffV1::SizeMatchedProse { .. }
                )),
                _ => assert!(matches!(
                    executor.worker_spec.handoff,
                    ExperimentHandoffV1::StructuredArtifact { .. }
                )),
            }
            if arm == ExperimentArm::ScG {
                let recovery =
                    prepare_recovery_phase(&context, &fixture, &handoff, "hidden_gate_failed")
                        .unwrap();
                assert_eq!(recovery.worker_spec.phase, ExperimentPhase::Recovery);
                assert_eq!(recovery.worker_spec.route.model, STRONG_MODEL);
                assert_eq!(recovery.worker_spec.limits.max_semantic_requests, 8);
                assert!(recovery.task_prompt().contains("hidden_gate_failed"));
                assert!(!recovery.task_prompt().contains("hidden verifier source"));
            }
        }
    }

    #[test]
    fn direct_arms_have_no_planner_handoff_and_exact_routes() {
        for (arm, model) in [
            (ExperimentArm::S0, STRONG_MODEL),
            (ExperimentArm::C0, CHEAP_MODEL),
        ] {
            let root = tempfile::tempdir().unwrap();
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/reasoning_pilot/unicode_parser_boundary");
            let mut context = context(root.path(), arm);
            copy_fixture(&source, &context.workspace_root);
            context.workspace_root = context.workspace_root.canonicalize().unwrap();
            context.controller_root = context.controller_root.canonicalize().unwrap();
            context.cargo_target_dir = context.cargo_target_dir.canonicalize().unwrap();
            let fixture =
                load_fixture_inputs(&context.workspace_root, &context.fixture_id).unwrap();
            let executor = prepare_executor_phase(&context, &fixture, None).unwrap();
            assert_eq!(executor.worker_spec.route.model, model);
            assert!(matches!(
                executor.worker_spec.handoff,
                ExperimentHandoffV1::None
            ));
        }
    }

    fn copy_fixture(source: &Path, destination: &Path) {
        fs::create_dir(destination).unwrap();
        #[cfg(unix)]
        fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).unwrap();
        copy_fixture_children(source, destination);
    }

    fn copy_fixture_children(source: &Path, destination: &Path) {
        let mut entries = fs::read_dir(source)
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                fs::create_dir(&target).unwrap();
                copy_fixture_children(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
}
