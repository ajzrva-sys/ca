//! Explicit, process-local history supplied by the user. These items are data,
//! not provider-issued replay receipts or authorization to execute old calls.

use serde::Deserialize;
use serde_json::{value::RawValue, Value};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

pub(crate) const SCHEMA: &str = "ca.native-history.v1";
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ITEMS: usize = 65_536;
const MAX_ITEM_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema: String,
    provider: String,
    protocol: String,
    model: String,
    input: Vec<Box<RawValue>>,
}

pub(crate) struct NativeHistory {
    provider: String,
    protocol: String,
    model: String,
    input: Vec<Box<RawValue>>,
    text: Vec<String>,
}

impl std::fmt::Debug for NativeHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeHistory")
            .field("provider", &self.provider)
            .field("protocol", &self.protocol)
            .field("model", &self.model)
            .field("items", &self.input.len())
            .finish()
    }
}

impl NativeHistory {
    pub(crate) fn text_chunks(&self) -> impl Iterator<Item = &str> {
        self.text.iter().map(String::as_str)
    }

    pub(crate) fn items(&self) -> &[Box<RawValue>] {
        &self.input
    }

    pub(crate) fn validate_route(
        &self,
        provider: &str,
        protocol: &str,
        model: &str,
    ) -> Result<(), String> {
        if self.provider == provider && self.protocol == protocol && self.model == model {
            Ok(())
        } else {
            Err("Imported native history requires its declared provider, protocol and model".into())
        }
    }
}

/// Recognize native history without changing the portable-session import path.
/// A recognized native envelope either loads completely or fails completely.
pub(crate) fn load_native(path: &Path) -> Result<Option<Arc<NativeHistory>>, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "Conversation history could not be opened".to_string())?;
    let metadata = file
        .metadata()
        .map_err(|_| "Conversation history metadata is unavailable".to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
        return Err("Conversation history exceeds the regular-file or 64 MiB bound".into());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Conversation history could not be read".to_string())?;
    parse_native(&bytes)
}

fn parse_native(bytes: &[u8]) -> Result<Option<Arc<NativeHistory>>, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Conversation history exceeds 64 MiB".into());
    }
    let probe: Value = serde_json::from_slice(bytes)
        .map_err(|_| "Conversation history is not valid JSON".to_string())?;
    if probe.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
        return Ok(None);
    }
    let envelope: Envelope = serde_json::from_slice(bytes)
        .map_err(|_| "Native conversation history has invalid envelope fields".to_string())?;
    if envelope.schema != SCHEMA
        || envelope.provider != "openai-codex"
        || envelope.protocol != "openai_responses"
        || envelope.model.is_empty()
        || envelope.model.len() > 256
        || envelope.model.chars().any(char::is_control)
    {
        return Err("Native conversation history requires openai-codex/openai_responses".into());
    }
    if envelope.input.is_empty() || envelope.input.len() > MAX_ITEMS {
        return Err("Native conversation history has an invalid item count".into());
    }
    let mut calls = HashMap::<String, &'static str>::new();
    let mut outputs = HashSet::new();
    let mut text = Vec::new();
    for (index, raw) in envelope.input.iter().enumerate() {
        if raw.get().len() > MAX_ITEM_BYTES {
            return Err(format!("Native history item {} exceeds 8 MiB", index + 1));
        }
        let value: Value = serde_json::from_str(raw.get())
            .map_err(|_| format!("Native history item {} is invalid JSON", index + 1))?;
        validate_item(&value, &mut calls, &mut outputs, &mut text)
            .map_err(|error| format!("Native history item {}: {error}", index + 1))?;
    }
    if calls.len() != outputs.len() {
        return Err("Native conversation history contains unresolved tool calls".into());
    }
    Ok(Some(Arc::new(NativeHistory {
        provider: envelope.provider,
        protocol: envelope.protocol,
        model: envelope.model,
        input: envelope.input,
        text,
    })))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be text"))
}

fn identity<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    let value = string(value, key)?;
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(format!("{key} is not a valid identifier"));
    }
    Ok(value)
}

fn text_blocks(value: &Value, text: &mut Vec<String>, images: bool) -> Result<(), String> {
    if let Some(value) = value.as_str() {
        text.push(value.to_string());
        return Ok(());
    }
    let values = value
        .as_array()
        .ok_or("Content must be text or an ordered block array")?;
    for block in values {
        match string(block, "type")? {
            "input_text" | "output_text" | "summary_text" => {
                text.push(string(block, "text")?.to_string());
            }
            "refusal" => text.push(string(block, "refusal")?.to_string()),
            "input_image" if images => {
                if block.get("image_url").and_then(Value::as_str).is_none()
                    && block.get("file_id").and_then(Value::as_str).is_none()
                {
                    return Err("Image block needs image_url or file_id".into());
                }
            }
            _ => return Err("Unsupported native content block".into()),
        }
    }
    Ok(())
}

fn validate_item(
    value: &Value,
    calls: &mut HashMap<String, &'static str>,
    outputs: &mut HashSet<String>,
    text: &mut Vec<String>,
) -> Result<(), String> {
    match string(value, "type")? {
        "message" => {
            if !matches!(
                string(value, "role")?,
                "system" | "developer" | "user" | "assistant"
            ) {
                return Err("Unsupported native message role".into());
            }
            text_blocks(
                value.get("content").ok_or("Message content is missing")?,
                text,
                true,
            )?;
        }
        "function_call" | "custom_tool_call" => {
            let id = identity(value, "call_id")?;
            identity(value, "name")?;
            let (field, kind) = if value["type"] == "function_call" {
                ("arguments", "function")
            } else {
                ("input", "custom")
            };
            text.push(string(value, field)?.to_string());
            if calls.insert(id.to_string(), kind).is_some() {
                return Err("Duplicate native tool call identifier".into());
            }
        }
        "function_call_output" | "custom_tool_call_output" => {
            let id = identity(value, "call_id")?;
            let expected = if value["type"] == "function_call_output" {
                "function"
            } else {
                "custom"
            };
            if calls.get(id).copied() != Some(expected) {
                return Err("Tool output has no preceding matching call".into());
            }
            if !outputs.insert(id.to_string()) {
                return Err("Duplicate native tool output".into());
            }
            text_blocks(
                value.get("output").ok_or("Tool output is missing")?,
                text,
                true,
            )?;
        }
        "reasoning" => {
            if let Some(summary) = value.get("summary") {
                text_blocks(summary, text, false)?;
            }
            if let Some(content) = value.get("content") {
                text_blocks(content, text, false)?;
            }
            if let Some(encrypted) = value.get("encrypted_content") {
                if !encrypted.is_string() {
                    return Err("Encrypted reasoning must be text".into());
                }
            }
        }
        "compaction" => {
            identity(value, "id")?;
            if string(value, "encrypted_content")?.is_empty() {
                return Err("Compaction state is empty".into());
            }
        }
        _ => return Err("Unsupported native history item type".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(items: Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema": SCHEMA, "provider": "openai-codex",
            "protocol": "openai_responses", "model": "fixture", "input": items,
        }))
        .unwrap()
    }

    #[test]
    fn native_history_preserves_full_order_and_opaque_data_without_scanning_ciphertext() {
        let items = serde_json::json!([
            {"type":"message","role":"developer","content":[{"type":"input_text","text":"kernel EPT"}]},
            {"type":"compaction","id":"comp1","encrypted_content":"opaque EPT not plaintext"},
            {"type":"custom_tool_call","call_id":"custom1","name":"exec","input":"native code"},
            {"type":"custom_tool_call_output","call_id":"custom1","output":[{"type":"input_text","text":"tool result"},{"type":"input_image","image_url":"data:image/png;base64,eA=="}]},
            {"type":"reasoning","id":"rs1","summary":[{"type":"summary_text","text":"visible summary"}],"encrypted_content":"opaque2"},
            {"type":"function_call","call_id":"f1","name":"read_file","arguments":"not-valid-json historical arguments"},
            {"type":"function_call_output","call_id":"f1","output":"historical error"},
        ]);
        let history = parse_native(&envelope(items.clone())).unwrap().unwrap();
        let restored: Vec<Value> = history
            .items()
            .iter()
            .map(|item| serde_json::from_str(item.get()).unwrap())
            .collect();
        assert_eq!(Value::Array(restored), items);
        assert_eq!(
            history.text_chunks().collect::<Vec<_>>(),
            [
                "kernel EPT",
                "native code",
                "tool result",
                "visible summary",
                "not-valid-json historical arguments",
                "historical error"
            ]
        );
        assert!(history
            .validate_route("openai-codex", "openai_responses", "fixture")
            .is_ok());
        assert!(history
            .validate_route("other", "openai_responses", "fixture")
            .is_err());
        assert!(history
            .validate_route("openai-codex", "chat_completions", "fixture")
            .is_err());
        assert!(history
            .validate_route("openai-codex", "openai_responses", "other-model")
            .is_err());
    }

    #[test]
    fn native_history_fails_closed_on_unknown_items_and_broken_call_graphs() {
        for items in [
            serde_json::json!([{"type":"future_item","text":"must not disappear"}]),
            serde_json::json!([{"type":"function_call","call_id":"f1","name":"read","arguments":"{}"}]),
            serde_json::json!([{"type":"function_call_output","call_id":"missing","output":"result"}]),
            serde_json::json!([{"type":"custom_tool_call","call_id":"c1","name":"exec","input":"x"},{"type":"function_call_output","call_id":"c1","output":"wrong kind"}]),
            serde_json::json!([{"type":"message","role":"user","content":[{"type":"future_block","text":"must not disappear"}]}]),
        ] {
            assert!(parse_native(&envelope(items)).is_err());
        }
        assert!(parse_native(br#"{"schema":"ca.session-export.v1"}"#)
            .unwrap()
            .is_none());
    }

    #[test]
    fn native_history_regular_file_loader_keeps_item_bytes_and_rejects_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("history.json");
        let raw = r#"{ "role":"user", "type":"message", "content":"line\ntext" }"#;
        std::fs::write(&path, format!(r#"{{"schema":"{SCHEMA}","provider":"openai-codex","protocol":"openai_responses","model":"fixture","input":[{raw}]}}"#)).unwrap();
        let history = load_native(&path).unwrap().unwrap();
        assert_eq!(history.items()[0].get(), raw);
        #[cfg(unix)]
        {
            let link = root.path().join("link.json");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(load_native(&link).is_err());
        }
    }

    #[test]
    fn native_history_store_scopes_imports_and_adapter_keeps_every_item_without_replay_exemption() {
        use crate::agent::client::{ChatMessage, ToolDef};
        use crate::agent::provider::ProviderProtocol;
        use crate::agent::stream::ProviderStateSourceV1;
        use crate::request::{PreparedModelRequest, PromptCacheScope, RequestKind, RequestStore};

        let input = serde_json::json!([
            {"type":"message","role":"developer","content":[{"type":"input_text","text":"first privateword"}]},
            {"type":"compaction","id":"comp1","encrypted_content":"ciphertext privateword"},
            {"type":"custom_tool_call","call_id":"c1","name":"exec","input":"privateword"},
            {"type":"custom_tool_call_output","call_id":"c1","output":"privateword"},
            {"type":"function_call","call_id":"f1","name":"read_file","arguments":"{\"path\":\"privateword\"}"},
            {"type":"function_call_output","call_id":"f1","output":[{"type":"input_text","text":"last privateword"}]}
        ]);
        let history = parse_native(&envelope(input.clone())).unwrap().unwrap();
        let store = RequestStore::default();
        let owner = uuid::Uuid::new_v4();
        store.set_imported_history(owner, Some(history));
        let tool = ToolDef {
            def_type: "function".into(),
            function: crate::agent::client::ToolFunction {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            },
        };
        let mut request = PreparedModelRequest::new(
            RequestKind::MainTurn,
            "openai-codex",
            "fixture",
            vec![ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text("continue now"),
                tool_calls: None,
                tool_call_id: None,
                response_items: Vec::new(),
            }],
            vec![tool],
            Default::default(),
        )
        .with_prompt_cache(PromptCacheScope::conversation(owner), 0);
        store.prepare_host_contexts(&mut request).unwrap();
        assert!(request.imported_history.is_some());
        let source =
            ProviderStateSourceV1::for_test("openai-codex", ProviderProtocol::OpenAiResponses);
        let prepared = ProviderProtocol::OpenAiResponses
            .adapter()
            .prepare(&request, &source, None, None, None)
            .unwrap();
        assert!(
            prepared.replay.responses_items.is_empty(),
            "imported data has no provider-issued receipt"
        );
        let body: Value = serde_json::from_slice(&prepared.body).unwrap();
        assert_eq!(
            &body["input"].as_array().unwrap()[..6],
            input.as_array().unwrap()
        );
        assert_eq!(body["input"][6]["content"], "continue now");
        assert_eq!(body["tools"][0]["name"], "read_file");
        assert_eq!(
            body["tools"][0]["parameters"]["properties"]["path"]["type"],
            "string"
        );
        let filtered = crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig {
                enabled: true,
                hide: crate::provider_filter::FilterHide {
                    words: vec!["privateword".into()],
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap()
        .prepare_request(&prepared.body)
        .unwrap();
        let filtered: Value = serde_json::from_slice(&filtered).unwrap();
        assert!(!filtered["input"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("privateword"));
        assert_eq!(
            filtered["input"][1]["encrypted_content"],
            "ciphertext privateword"
        );
        assert_eq!(filtered["input"][4]["name"], "read_file");
        assert_eq!(filtered["input"][4]["call_id"], "f1");
        assert_eq!(filtered["tools"], body["tools"]);
        assert!(ProviderProtocol::ChatCompletions
            .adapter()
            .prepare(&request, &source, None, None, None)
            .is_err());
        request.provider = "other".into();
        assert!(ProviderProtocol::OpenAiResponses
            .adapter()
            .prepare(&request, &source, None, None, None)
            .is_err());
        request.provider = "openai-codex".into();
        request.model = "other-model".into();
        assert!(ProviderProtocol::OpenAiResponses
            .adapter()
            .prepare(&request, &source, None, None, None)
            .is_err());

        let mut fresh = request
            .clone()
            .with_prompt_cache(PromptCacheScope::conversation(uuid::Uuid::new_v4()), 0);
        store.prepare_imported_history(&mut fresh).unwrap();
        assert!(
            fresh.imported_history.is_none(),
            "another conversation cannot inherit the prefix"
        );
        assert!(
            store.imported_history(owner).is_some(),
            "returning to the original conversation retains its prefix"
        );
        let mut unscoped = request.clone();
        unscoped.prompt_cache = None;
        assert!(store.prepare_imported_history(&mut unscoped).is_err());
        assert!(unscoped.imported_history.is_none());
        let mut agent_scoped = request
            .clone()
            .with_prompt_cache(PromptCacheScope::agent(owner), 0);
        assert!(store.prepare_imported_history(&mut agent_scoped).is_err());

        for kind in [
            RequestKind::SideQuestion,
            RequestKind::Subagent,
            RequestKind::SessionTitle,
            RequestKind::ContextCheckpoint,
        ] {
            let mut isolated = PreparedModelRequest::new(
                kind,
                "openai-codex",
                "fixture",
                Vec::new(),
                Vec::new(),
                Default::default(),
            );
            store.prepare_imported_history(&mut isolated).unwrap();
            assert!(
                isolated.imported_history.is_none(),
                "{kind:?} must not inherit the conversation"
            );
        }
        store.set_imported_history(owner, None);
        assert!(store.imported_history(owner).is_none());
    }

    #[test]
    fn native_history_requires_model_and_rejects_whole_file_or_item_overflow() {
        let missing_model = serde_json::json!({"schema":SCHEMA,"provider":"openai-codex","protocol":"openai_responses","input":[{"type":"message","role":"user","content":"x"}]});
        assert!(parse_native(&serde_json::to_vec(&missing_model).unwrap()).is_err());
        let oversized_item = envelope(
            serde_json::json!([{"type":"message","role":"user","content":"x".repeat(MAX_ITEM_BYTES)}]),
        );
        assert!(parse_native(&oversized_item).unwrap_err().contains("8 MiB"));
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("huge.json");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_BYTES as u64 + 1)
            .unwrap();
        assert!(load_native(&path).unwrap_err().contains("64 MiB"));
    }
}
