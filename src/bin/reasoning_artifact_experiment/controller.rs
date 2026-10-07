//! Controller-owned execution loop for one RND-097 phase.
//!
//! The loop is deliberately transport-agnostic: callers provide one-call
//! provider and tool dispatch closures. This keeps credential access in the
//! broker and workspace authority in the tool worker while making the arm
//! logic deterministic and fully testable without traffic.

use crate::conversation::{PhaseConversation, TurnDisposition};
use crate::reasoning_experiment_protocol::{
    ExperimentPhase, ExperimentUsageV1, ExperimentWorkerSpecV1, ProviderCallOutcomeV1,
    ProviderCallReceiptV1, ProviderCallSpecV1, ToolCallOutcomeV1, ToolCallReceiptV1,
    ToolCallSpecV1, MAX_INPUT_TOKENS_PER_REQUEST, MAX_OUTPUT_TOKENS_PER_REQUEST,
    PROVIDER_CALL_SPEC_SCHEMA_V1, TOOL_CALL_SPEC_SCHEMA_V1,
};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerBinaryIdentityV1 {
    pub controller_sha256: String,
    pub worker_sha256: String,
}

impl ControllerBinaryIdentityV1 {
    pub fn validate(&self) -> Result<()> {
        validate_sha256("controller digest", &self.controller_sha256)?;
        validate_sha256("worker digest", &self.worker_sha256)
    }
}

pub struct PhaseRunResultV1 {
    content: String,
    pub semantic_requests: u16,
    pub physical_posts: u16,
    pub tool_calls: u16,
    pub usage: ExperimentUsageV1,
    pub elapsed_millis: u64,
}

impl std::fmt::Debug for PhaseRunResultV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PhaseRunResultV1")
            .field("content", &"[REDACTED]")
            .field("semantic_requests", &self.semantic_requests)
            .field("physical_posts", &self.physical_posts)
            .field("tool_calls", &self.tool_calls)
            .field("usage", &self.usage)
            .field("elapsed_millis", &self.elapsed_millis)
            .finish()
    }
}

impl PhaseRunResultV1 {
    pub fn content(&self) -> &str {
        &self.content
    }
}

pub fn run_phase_with<P, T>(
    worker_spec: &ExperimentWorkerSpecV1,
    identity: &ControllerBinaryIdentityV1,
    system_prompt: &str,
    task_prompt: &str,
    invoke_provider: P,
    invoke_tool: T,
) -> Result<PhaseRunResultV1>
where
    P: FnMut(&ProviderCallSpecV1) -> Result<ProviderCallReceiptV1>,
    T: FnMut(&ToolCallSpecV1) -> Result<ToolCallReceiptV1>,
{
    run_phase_with_output_schema(
        worker_spec,
        identity,
        system_prompt,
        task_prompt,
        None,
        invoke_provider,
        invoke_tool,
    )
}

pub fn run_phase_with_output_schema<P, T>(
    worker_spec: &ExperimentWorkerSpecV1,
    identity: &ControllerBinaryIdentityV1,
    system_prompt: &str,
    task_prompt: &str,
    output_schema: Option<(&str, Value)>,
    mut invoke_provider: P,
    mut invoke_tool: T,
) -> Result<PhaseRunResultV1>
where
    P: FnMut(&ProviderCallSpecV1) -> Result<ProviderCallReceiptV1>,
    T: FnMut(&ToolCallSpecV1) -> Result<ToolCallReceiptV1>,
{
    worker_spec
        .validate()
        .map_err(|error| anyhow!("worker spec is invalid: {error}"))?;
    identity.validate()?;
    validate_controller_tool_surface(worker_spec)?;
    let worker_spec_sha256 = canonical_sha256(worker_spec)?;
    let tools_enabled = worker_spec.phase != ExperimentPhase::Planner;
    let mut conversation = PhaseConversation::new_with_output_schema(
        worker_spec.route.clone(),
        system_prompt,
        task_prompt,
        tools_enabled,
        output_schema,
    )?;
    let started = Instant::now();
    let phase_timeout = Duration::from_secs(u64::from(worker_spec.limits.wall_time_secs));
    let mut semantic_requests = 0_u16;
    let mut physical_posts = 0_u16;
    let mut tool_calls = 0_u16;
    let mut usage = ExperimentUsageV1 {
        regular_input_tokens: 0,
        cached_input_tokens: 0,
        cache_write_tokens: 0,
        regular_output_tokens: 0,
        reasoning_output_tokens: 0,
    };

    while semantic_requests < worker_spec.limits.max_semantic_requests {
        ensure_within_deadline(started, phase_timeout)?;
        let request_json = conversation.request_json()?;
        let semantic_index = semantic_requests;
        semantic_requests = semantic_requests
            .checked_add(1)
            .ok_or_else(|| anyhow!("semantic request count overflow"))?;

        let mut completed = None;
        for physical_attempt in 1..=2_u8 {
            if physical_posts >= worker_spec.limits.max_physical_posts {
                bail!("phase exhausted its physical POST budget");
            }
            ensure_within_deadline(started, phase_timeout)?;
            let call = provider_call(
                worker_spec,
                identity,
                &worker_spec_sha256,
                semantic_index,
                physical_attempt,
                request_json.clone(),
            )?;
            physical_posts = physical_posts
                .checked_add(1)
                .ok_or_else(|| anyhow!("physical POST count overflow"))?;
            let receipt = invoke_provider(&call)?;
            receipt
                .validate_against(&call)
                .map_err(|error| anyhow!("provider receipt is invalid: {error}"))?;
            match receipt.outcome {
                ProviderCallOutcomeV1::Completed {
                    controller_response_json,
                    ..
                } => {
                    add_usage(
                        &mut usage,
                        receipt
                            .usage
                            .ok_or_else(|| anyhow!("completed provider receipt lacks usage"))?,
                    )?;
                    completed = Some(controller_response_json);
                    break;
                }
                ProviderCallOutcomeV1::VerifiedPreOutputFailure { .. } if physical_attempt == 1 => {
                }
                ProviderCallOutcomeV1::VerifiedPreOutputFailure { .. } => {
                    bail!("provider failed before output after its one allowed retry")
                }
                ProviderCallOutcomeV1::AmbiguousDelivery { .. } => {
                    bail!("provider delivery is ambiguous and cannot be replayed")
                }
            }
        }
        let response = completed.ok_or_else(|| anyhow!("provider request did not complete"))?;
        match conversation.accept_broker_turn(&response)? {
            TurnDisposition::Stop { content } => {
                return Ok(PhaseRunResultV1 {
                    content,
                    semantic_requests,
                    physical_posts,
                    tool_calls,
                    usage,
                    elapsed_millis: u64::try_from(started.elapsed().as_millis())
                        .unwrap_or(u64::MAX),
                });
            }
            TurnDisposition::ToolCalls(calls) => {
                for pending in calls {
                    if tool_calls >= worker_spec.limits.max_tool_calls {
                        bail!("phase exhausted its tool-call budget");
                    }
                    ensure_within_deadline(started, phase_timeout)?;
                    let call = tool_call(
                        worker_spec,
                        &worker_spec_sha256,
                        &pending.id,
                        &pending.name,
                        pending.arguments_json,
                        started,
                        phase_timeout,
                    )?;
                    tool_calls = tool_calls
                        .checked_add(1)
                        .ok_or_else(|| anyhow!("tool-call count overflow"))?;
                    let receipt = invoke_tool(&call)?;
                    receipt
                        .validate_against(&call)
                        .map_err(|error| anyhow!("tool receipt is invalid: {error}"))?;
                    if !matches!(receipt.outcome, ToolCallOutcomeV1::Completed) {
                        bail!("tool call did not complete and cannot be replayed");
                    }
                    conversation.accept_tool_result(
                        &pending.id,
                        receipt
                            .result_json
                            .as_deref()
                            .ok_or_else(|| anyhow!("completed tool receipt lacks a result"))?,
                    )?;
                }
            }
        }
    }
    bail!("phase exhausted its semantic-request budget before completion")
}

fn provider_call(
    worker_spec: &ExperimentWorkerSpecV1,
    identity: &ControllerBinaryIdentityV1,
    worker_spec_sha256: &str,
    semantic_request_index: u16,
    physical_attempt: u8,
    request_json: String,
) -> Result<ProviderCallSpecV1> {
    let random = Uuid::new_v4().simple().to_string();
    let call = ProviderCallSpecV1 {
        schema_version: PROVIDER_CALL_SPEC_SCHEMA_V1.into(),
        call_id: format!("pc-{random}"),
        channel_id: format!("provider-{random}"),
        nonce: format!("nonce-{random}"),
        worker_spec_sha256: worker_spec_sha256.into(),
        controller_sha256: identity.controller_sha256.clone(),
        worker_sha256: identity.worker_sha256.clone(),
        experiment_id: worker_spec.experiment_id.clone(),
        preregistration_sha256: worker_spec.preregistration_sha256.clone(),
        trial_id: worker_spec.trial_id.clone(),
        slot_index: worker_spec.slot_index,
        phase: worker_spec.phase,
        semantic_request_index,
        physical_attempt,
        route: worker_spec.route.clone(),
        max_input_tokens: MAX_INPUT_TOKENS_PER_REQUEST,
        max_output_tokens: MAX_OUTPUT_TOKENS_PER_REQUEST,
        store: false,
        request_sha256: sha256_hex(request_json.as_bytes()),
        request_json,
        x_client_request_id: format!("rnd097-{random}"),
    };
    call.validate()
        .map_err(|error| anyhow!("constructed provider call is invalid: {error}"))?;
    Ok(call)
}

fn tool_call(
    worker_spec: &ExperimentWorkerSpecV1,
    worker_spec_sha256: &str,
    provider_tool_call_id: &str,
    tool_name: &str,
    arguments_json: String,
    started: Instant,
    phase_timeout: Duration,
) -> Result<ToolCallSpecV1> {
    if !worker_spec
        .allowed_tools
        .iter()
        .any(|tool| tool == tool_name)
    {
        bail!("provider requested a tool outside the frozen worker grant");
    }
    let remaining = phase_timeout
        .checked_sub(started.elapsed())
        .ok_or_else(|| anyhow!("phase exceeded its wall-time budget"))?;
    let timeout_millis = u64::try_from(remaining.as_millis())
        .unwrap_or(u64::MAX)
        .clamp(1, 60_000);
    let random = Uuid::new_v4().simple().to_string();
    let call = ToolCallSpecV1 {
        schema_version: TOOL_CALL_SPEC_SCHEMA_V1.into(),
        call_id: format!("tc-{random}"),
        channel_id: format!("tool-{random}"),
        nonce: format!("nonce-{random}"),
        worker_spec_sha256: worker_spec_sha256.into(),
        workspace_id: worker_spec.workspace_id.clone(),
        workspace_root: worker_spec.workspace_root.clone(),
        source_manifest_sha256: worker_spec.source_manifest_sha256.clone(),
        cargo_target_dir: worker_spec.cargo_target_dir.clone(),
        tool_name: tool_name.into(),
        tool_call_id: provider_tool_call_id.into(),
        arguments_sha256: sha256_hex(arguments_json.as_bytes()),
        arguments_json,
        allowed_read_paths: worker_spec.allowed_read_paths.clone(),
        allowed_write_paths: worker_spec.allowed_write_paths.clone(),
        timeout_millis,
    };
    call.validate()
        .map_err(|error| anyhow!("constructed tool call is invalid: {error}"))?;
    Ok(call)
}

fn validate_controller_tool_surface(worker_spec: &ExperimentWorkerSpecV1) -> Result<()> {
    let actual = worker_spec
        .allowed_tools
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = if worker_spec.phase == ExperimentPhase::Planner {
        BTreeSet::new()
    } else {
        ["read_file", "run_command", "write_file"]
            .into_iter()
            .collect()
    };
    if actual != expected {
        bail!("worker tool grant differs from the frozen controller surface");
    }
    Ok(())
}

fn add_usage(total: &mut ExperimentUsageV1, value: ExperimentUsageV1) -> Result<()> {
    value
        .validate()
        .map_err(|error| anyhow!("provider usage is invalid: {error}"))?;
    total.regular_input_tokens = total
        .regular_input_tokens
        .checked_add(value.regular_input_tokens)
        .ok_or_else(|| anyhow!("usage overflow"))?;
    total.cached_input_tokens = total
        .cached_input_tokens
        .checked_add(value.cached_input_tokens)
        .ok_or_else(|| anyhow!("usage overflow"))?;
    total.cache_write_tokens = total
        .cache_write_tokens
        .checked_add(value.cache_write_tokens)
        .ok_or_else(|| anyhow!("usage overflow"))?;
    total.regular_output_tokens = total
        .regular_output_tokens
        .checked_add(value.regular_output_tokens)
        .ok_or_else(|| anyhow!("usage overflow"))?;
    total.reasoning_output_tokens = total
        .reasoning_output_tokens
        .checked_add(value.reasoning_output_tokens)
        .ok_or_else(|| anyhow!("usage overflow"))?;
    Ok(())
}

fn ensure_within_deadline(started: Instant, timeout: Duration) -> Result<()> {
    if started.elapsed() >= timeout {
        bail!("phase exceeded its wall-time budget");
    }
    Ok(())
}

fn canonical_sha256<T: serde::Serialize>(value: &T) -> Result<String> {
    Ok(sha256_hex(
        &serde_json::to_vec(value).context("serialize digest-bound controller value")?,
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_sha256(label: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("{label} is not a lowercase SHA-256 digest");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning_experiment_protocol::{
        ExperimentArm, ExperimentControllerChannelV1, ExperimentDeclaredFileV1, ExperimentFileRoot,
        ExperimentHandoffV1, ExperimentModelRouteV1, ExperimentRequestLimitsV1, CHEAP_MODEL,
        PROVIDER_CALL_RECEIPT_SCHEMA_V1, TOOL_CALL_RECEIPT_SCHEMA_V1, WORKER_SPEC_SCHEMA_V1,
    };
    use serde_json::{json, Value};
    use std::path::PathBuf;

    fn digest(byte: char) -> String {
        byte.to_string().repeat(64)
    }

    fn spec() -> ExperimentWorkerSpecV1 {
        ExperimentWorkerSpecV1 {
            schema_version: WORKER_SPEC_SCHEMA_V1.into(),
            experiment_id: "experiment-1".into(),
            preregistration_sha256: digest('a'),
            trial_id: "trial-1".into(),
            slot_index: 0,
            fixture_id: "unicode_parser_boundary".into(),
            arm: ExperimentArm::C0,
            repetition: 1,
            phase: ExperimentPhase::Executor,
            route: ExperimentModelRouteV1::openai(CHEAP_MODEL),
            limits: ExperimentRequestLimitsV1::for_phase(ExperimentPhase::Executor),
            workspace_id: "workspace-1".into(),
            workspace_root: PathBuf::from("/private/tmp/rnd-workspace"),
            controller_root: PathBuf::from("/private/tmp/rnd-controller"),
            source_manifest_sha256: digest('b'),
            destination_manifest_sha256: digest('c'),
            cargo_target_dir: PathBuf::from("/private/tmp/rnd-target"),
            task_input_sha256: digest('d'),
            task_input: ExperimentDeclaredFileV1 {
                root: ExperimentFileRoot::Workspace,
                relative_path: PathBuf::from("TASK.md"),
                sha256: digest('d'),
                max_bytes: 64 * 1024,
            },
            handoff: ExperimentHandoffV1::None,
            output_schema: ExperimentDeclaredFileV1 {
                root: ExperimentFileRoot::Controller,
                relative_path: PathBuf::from("schemas/executor.json"),
                sha256: digest('e'),
                max_bytes: 64 * 1024,
            },
            allowed_tools: vec![
                "read_file".into(),
                "run_command".into(),
                "write_file".into(),
            ],
            allowed_read_paths: vec![PathBuf::from("src/lib.rs")],
            allowed_write_paths: vec![PathBuf::from("src/lib.rs")],
            controller_channel: ExperimentControllerChannelV1 {
                channel_id: "controller-1".into(),
                nonce_sha256: digest('f'),
                inherited_fd: 9,
            },
            network_allowed: false,
        }
    }

    fn identity() -> ControllerBinaryIdentityV1 {
        ControllerBinaryIdentityV1 {
            controller_sha256: digest('1'),
            worker_sha256: digest('2'),
        }
    }

    fn completed_receipt(call: &ProviderCallSpecV1, response: Value) -> ProviderCallReceiptV1 {
        let response = serde_json::to_string(&response).unwrap();
        ProviderCallReceiptV1 {
            schema_version: PROVIDER_CALL_RECEIPT_SCHEMA_V1.into(),
            call_id: call.call_id.clone(),
            channel_id: call.channel_id.clone(),
            nonce: call.nonce.clone(),
            worker_spec_sha256: call.worker_spec_sha256.clone(),
            request_sha256: call.request_sha256.clone(),
            reservation_id: format!("reservation-{}", call.semantic_request_index),
            x_client_request_id_sha256: sha256_hex(call.x_client_request_id.as_bytes()),
            x_request_id_sha256: Some(digest('3')),
            returned_model: Some(call.route.model.clone()),
            returned_service_tier: Some(call.route.service_tier.clone()),
            usage: Some(ExperimentUsageV1 {
                regular_input_tokens: 40_000,
                cached_input_tokens: 0,
                cache_write_tokens: 0,
                regular_output_tokens: 2,
                reasoning_output_tokens: 1,
            }),
            elapsed_millis: 1,
            outcome: ProviderCallOutcomeV1::Completed {
                controller_response_sha256: sha256_hex(response.as_bytes()),
                controller_response_json: response,
            },
        }
    }

    #[test]
    fn controller_replays_one_tool_turn_and_returns_sanitized_totals() {
        let spec = spec();
        let mut provider_calls = 0;
        let mut tool_calls = 0;
        let result = run_phase_with(
            &spec,
            &identity(),
            "Repair the fixture.",
            "Use the exact local tools and finish after verification.",
            |call| {
                provider_calls += 1;
                if provider_calls == 1 {
                    return Ok(completed_receipt(
                        call,
                        json!({
                            "content": "",
                            "reasoning": "opaque-reasoning",
                            "response_items": [
                                {"output_index": 1, "item": {
                                    "type": "function_call", "id": "fc-1",
                                    "call_id": "call-1", "name": "read_file",
                                    "arguments": "{\"path\":\"src/lib.rs\"}"
                                }},
                                {"output_index": 0, "item": {
                                    "type": "reasoning", "id": "rs-1",
                                    "encrypted_content": "opaque-state"
                                }}
                            ],
                            "tool_calls": [{
                                "index": 0, "id": "call-1", "name": "read_file",
                                "arguments": "{\"path\":\"src/lib.rs\"}"
                            }],
                            "finish_reason": "tool_calls"
                        }),
                    ));
                }
                let request: Value = serde_json::from_str(&call.request_json).unwrap();
                let input = request["input"].as_array().unwrap();
                assert_eq!(input[input.len() - 3]["type"], "reasoning");
                assert_eq!(input[input.len() - 2]["type"], "function_call");
                assert_eq!(input[input.len() - 1]["type"], "function_call_output");
                Ok(completed_receipt(
                    call,
                    json!({
                        "content": "done",
                        "reasoning": null,
                        "response_items": [{"output_index": 0, "item": {
                            "type": "message", "id": "msg-1", "status": "completed",
                            "role": "assistant", "content": [{"type": "output_text", "text": "done"}]
                        }}],
                        "tool_calls": [],
                        "finish_reason": "stop"
                    }),
                ))
            },
            |call| {
                tool_calls += 1;
                assert_eq!(call.tool_name, "read_file");
                let result_json = r#"{"kind":"read","content":"fixture"}"#.to_string();
                Ok(ToolCallReceiptV1 {
                    schema_version: TOOL_CALL_RECEIPT_SCHEMA_V1.into(),
                    call_id: call.call_id.clone(),
                    channel_id: call.channel_id.clone(),
                    nonce: call.nonce.clone(),
                    worker_spec_sha256: call.worker_spec_sha256.clone(),
                    arguments_sha256: call.arguments_sha256.clone(),
                    elapsed_millis: 1,
                    outcome: ToolCallOutcomeV1::Completed,
                    result_sha256: Some(sha256_hex(result_json.as_bytes())),
                    result_json: Some(result_json),
                })
            },
        )
        .unwrap();
        assert_eq!(provider_calls, 2);
        assert_eq!(tool_calls, 1);
        assert_eq!(result.content(), "done");
        assert_eq!(result.semantic_requests, 2);
        assert_eq!(result.physical_posts, 2);
        assert_eq!(result.tool_calls, 1);
        assert_eq!(result.usage.regular_input_tokens, 80_000);
        let debug = format!("{result:?}");
        assert!(!debug.contains("done"));
        assert!(!debug.contains("opaque"));
    }

    #[test]
    fn controller_retries_only_verified_pre_output_failure_once() {
        let spec = spec();
        let mut attempts = Vec::new();
        let result = run_phase_with(
            &spec,
            &identity(),
            "Repair the fixture.",
            "Finish without tools.",
            |call| {
                attempts.push((
                    call.semantic_request_index,
                    call.physical_attempt,
                    call.call_id.clone(),
                ));
                if call.physical_attempt == 1 {
                    return Ok(ProviderCallReceiptV1 {
                        schema_version: PROVIDER_CALL_RECEIPT_SCHEMA_V1.into(),
                        call_id: call.call_id.clone(),
                        channel_id: call.channel_id.clone(),
                        nonce: call.nonce.clone(),
                        worker_spec_sha256: call.worker_spec_sha256.clone(),
                        request_sha256: call.request_sha256.clone(),
                        reservation_id: "reservation-retry".into(),
                        x_client_request_id_sha256: sha256_hex(call.x_client_request_id.as_bytes()),
                        x_request_id_sha256: None,
                        returned_model: None,
                        returned_service_tier: None,
                        usage: None,
                        elapsed_millis: 1,
                        outcome: ProviderCallOutcomeV1::VerifiedPreOutputFailure {
                            reason: "connect_failed_before_write".into(),
                        },
                    });
                }
                Ok(completed_receipt(
                    call,
                    json!({
                        "content": "complete",
                        "reasoning": null,
                        "response_items": [{"output_index": 0, "item": {
                            "type": "message", "id": "msg-2", "role": "assistant",
                            "content": [{"type": "output_text", "text": "complete"}]
                        }}],
                        "tool_calls": [],
                        "finish_reason": "stop"
                    }),
                ))
            },
            |_| bail!("tool worker must not run"),
        )
        .unwrap();
        assert_eq!(result.physical_posts, 2);
        assert_eq!(result.semantic_requests, 1);
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].0, attempts[1].0);
        assert_eq!(attempts[0].1, 1);
        assert_eq!(attempts[1].1, 2);
        assert_ne!(attempts[0].2, attempts[1].2);
    }
}
