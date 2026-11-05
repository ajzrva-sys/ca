//! Omit only opaque protocol payloads from the local request comparison.
//!
//! A property named `data` or `signature` inside tool arguments is ordinary
//! displayable data. Follow request-envelope paths instead of recursively
//! interpreting user documents as provider protocol blocks.

use serde_json::Value;

const OPAQUE: &str = "[opaque provider block; transmitted unchanged]";

pub(super) fn redact_opaque(value: &mut Value) -> usize {
    let Some(body) = value.as_object_mut() else {
        return 0;
    };
    let mut count = 0;

    // Responses replay items live directly in input, alongside message and
    // function-call items. Message content and tool arguments are not items.
    if let Some(items) = body.get_mut("input").and_then(Value::as_array_mut) {
        for item in items {
            if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                count += redact_field(item, "encrypted_content");
            }
        }
    }

    // Anthropic signed thinking and redacted thinking are content blocks, not
    // arbitrary objects nested inside a tool_use input or a tool result.
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            if let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("thinking") => count += redact_field(block, "signature"),
                        Some("redacted_thinking") => {
                            count += redact_field(block, "data");
                            count += redact_field(block, "signature");
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // Gemini parts can carry a signature with either a text or functionCall
    // part. Stay outside functionCall.args and functionResponse.response.
    if let Some(contents) = body.get_mut("contents").and_then(Value::as_array_mut) {
        for content in contents {
            count += redact_google_content(content);
        }
    }
    for key in ["systemInstruction", "system_instruction"] {
        if let Some(content) = body.get_mut(key) {
            count += redact_google_content(content);
        }
    }
    count
}

fn redact_google_content(content: &mut Value) -> usize {
    let Some(parts) = content.get_mut("parts").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut count = 0;
    for part in parts {
        count += redact_field(part, "thoughtSignature");
        count += redact_field(part, "thought_signature");
        for key in ["inlineData", "inline_data"] {
            if let Some(blob) = part.get_mut(key) {
                count += redact_field(blob, "data");
            }
        }
    }
    count
}

fn redact_field(value: &mut Value, key: &str) -> usize {
    let Some(field) = value.get_mut(key).filter(|field| field.is_string()) else {
        return 0;
    };
    *field = Value::String(OPAQUE.into());
    1
}

use serde_json::value::RawValue;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

const CHUNK_SAMPLE_BYTES: usize = 64 * 1024;
const PANE_SAMPLE_BYTES: usize = 512 * 1024;
const CHANGE_BYTES: usize = 64 * 1024;

// Deserialize conversation arrays directly instead of first borrowing the whole
// array as RawValue and scanning it a second time to discover its items.
enum RawField<'a> {
    Array(Vec<&'a RawValue>),
    Scalar(&'a RawValue),
    Owned(Value),
}
struct RawEnvelope<'a>(BTreeMap<String, RawField<'a>>);
struct ConversationField;

impl<'de> serde::de::DeserializeSeed<'de> for ConversationField {
    type Value = RawField<'de>;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = RawField<'de>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a provider conversation value")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = sequence.next_element::<&'de RawValue>()? {
                    items.push(item);
                }
                Ok(RawField::Array(items))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                <Value as serde::Deserialize>::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )
                .map(RawField::Owned)
            }
            fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<Self::Value, E> {
                Ok(RawField::Owned(Value::String(text.into())))
            }
            fn visit_string<E: serde::de::Error>(self, text: String) -> Result<Self::Value, E> {
                Ok(RawField::Owned(Value::String(text)))
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(RawField::Owned(Value::Bool(value)))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(RawField::Owned(Value::Number(value.into())))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(RawField::Owned(Value::Number(value.into())))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|number| RawField::Owned(Value::Number(number)))
                    .ok_or_else(|| E::custom("invalid JSON number"))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(RawField::Owned(Value::Null))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

impl<'de> serde::Deserialize<'de> for RawEnvelope<'de> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = RawEnvelope<'de>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a provider request object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut fields = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    let value = if matches!(key.as_str(), "messages" | "input" | "contents") {
                        map.next_value_seed(ConversationField)?
                    } else {
                        RawField::Scalar(map.next_value::<&'de RawValue>()?)
                    };
                    // Match serde_json::Value: a duplicate key replaces the
                    // earlier complete value rather than creating ambiguous spans.
                    fields.insert(key, value);
                }
                Ok(RawEnvelope(fields))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

#[derive(Hash, PartialEq, Eq)]
struct ChunkKey {
    field: String,
    array_item: bool,
    len: usize,
    edges: [u8; 32],
}
impl ChunkKey {
    fn new(field: &str, array_item: bool, raw: &str) -> Self {
        let mut edges = [0; 32];
        let first = raw.len().min(16);
        edges[..first].copy_from_slice(&raw.as_bytes()[..first]);
        let last = raw.len().min(16);
        edges[16..16 + last].copy_from_slice(&raw.as_bytes()[raw.len() - last..]);
        Self {
            field: field.into(),
            array_item,
            len: raw.len(),
            edges,
        }
    }
}

struct Chunk {
    input: Arc<String>,
    full: Arc<String>,
    sample: Value,
    sample_bytes: usize,
    latest: Arc<str>,
    opaque: usize,
    ticket: Option<crate::cache_memory::Ticket>,
}

enum Field {
    Array(Vec<Arc<Chunk>>),
    Scalar(Arc<Chunk>),
}
struct Envelope {
    fields: BTreeMap<String, Field>,
    full: Arc<String>,
    latest: Arc<str>,
    pane: Arc<str>,
    opaque: usize,
}

pub(super) struct Rendered {
    pub original: Arc<String>,
    pub sent: Arc<String>,
    pub changes: Arc<str>,
    pub latest: Arc<str>,
    pub pane: Arc<str>,
    pub opaque_blocks: usize,
}

#[derive(Default)]
pub(super) struct Cache {
    chunks: HashMap<ChunkKey, Vec<Arc<Chunk>>>,
}

impl Cache {
    pub(super) fn render(&mut self, original: &[u8], sent: &[u8]) -> Result<Rendered, String> {
        self.chunks.retain(|_, entries| {
            entries.retain(|entry| entry.ticket.as_ref().is_some_and(|ticket| ticket.valid()));
            !entries.is_empty()
        });
        let original = self.envelope(original)?;
        let sent = self.envelope(sent)?;
        let changes = changes(&original, &sent);
        Ok(Rendered {
            original: original.full,
            sent: sent.full,
            changes: Arc::from(changes),
            latest: sent.latest,
            pane: sent.pane,
            opaque_blocks: original.opaque.max(sent.opaque),
        })
    }

    fn chunk(
        &mut self,
        field: &str,
        array_item: bool,
        raw: &RawValue,
    ) -> Result<Arc<Chunk>, String> {
        let raw = raw.get();
        let key = ChunkKey::new(field, array_item, raw);
        if let Some(found) = self.chunks.get(&key).and_then(|entries| {
            entries.iter().find(|entry| {
                entry.ticket.as_ref().is_some_and(|ticket| ticket.valid())
                    && entry.input.as_str() == raw
            })
        }) {
            found
                .ticket
                .as_ref()
                .expect("cached chunk has a ticket")
                .touch();
            return Ok(found.clone());
        }
        // The standard JSON parser establishes all structure, including its
        // last-key-wins semantics. Only byte-identical, context-identical
        // chunks can reuse that result; edge bytes merely narrow the lookup.
        let value: Value =
            serde_json::from_str(raw).map_err(|_| "Invalid request preview chunk")?;
        let mut wrapper = serde_json::Map::new();
        wrapper.insert(
            field.into(),
            if array_item {
                Value::Array(vec![value])
            } else {
                value
            },
        );
        let mut wrapper = Value::Object(wrapper);
        let opaque = redact_opaque(&mut wrapper);
        let visible = crate::provider_filter::outbound_pane_value(&wrapper, b"");
        let value = wrapper
            .as_object_mut()
            .expect("wrapper is an object")
            .remove(field)
            .unwrap();
        let value = if array_item {
            match value {
                Value::Array(mut items) => items.pop().expect("one wrapped item"),
                _ => unreachable!("array wrapper"),
            }
        } else {
            value
        };
        let mut encoded = Vec::with_capacity(raw.len());
        serde_json::to_writer(&mut encoded, &value).map_err(|_| "Invalid request preview chunk")?;
        let full = Arc::new(String::from_utf8(encoded).expect("JSON serializer emits UTF-8"));
        let latest: Arc<str> = Arc::from(if visible.is_empty() {
            bounded_pretty(&value, 8192)
        } else {
            bounded_text(visible, 8192)
        });
        let mut budget = CHUNK_SAMPLE_BYTES;
        let sample = sample(&value, &mut budget, 0);
        let sample_bytes = retained_value_bytes(&sample);
        let identical = full.as_str() == raw;
        let bytes = (if identical { 0 } else { raw.len() })
            .saturating_add(full.capacity())
            .saturating_add(sample_bytes)
            .saturating_add(latest.len())
            .saturating_add(512);
        let ticket = crate::cache_memory::reserve("privacy_previews", bytes);
        let chunk = Arc::new(Chunk {
            input: if ticket.is_some() {
                if identical {
                    full.clone()
                } else {
                    Arc::new(raw.to_owned())
                }
            } else {
                Arc::new(String::new())
            },
            full,
            sample,
            sample_bytes,
            latest,
            opaque,
            ticket,
        });
        if chunk.ticket.is_some() {
            self.chunks.entry(key).or_default().push(chunk.clone());
        }
        Ok(chunk)
    }

    fn envelope(&mut self, body: &[u8]) -> Result<Envelope, String> {
        // RawValue borrows exact complete spans while serde validates the full
        // envelope. Old long string payloads are not decoded/copied on hits.
        let RawEnvelope(raw) =
            serde_json::from_slice(body).map_err(|_| "Invalid request preview")?;
        let mut fields = BTreeMap::new();
        let mut opaque = 0;
        for (field, value) in raw {
            let rendered = match value {
                RawField::Array(items) => {
                    let mut chunks = Vec::with_capacity(items.len());
                    for item in items {
                        let chunk = self.chunk(&field, true, item)?;
                        opaque += chunk.opaque;
                        chunks.push(chunk);
                    }
                    Field::Array(chunks)
                }
                RawField::Scalar(value) => {
                    let chunk = self.chunk(&field, false, value)?;
                    opaque += chunk.opaque;
                    Field::Scalar(chunk)
                }
                RawField::Owned(value) => {
                    let raw = RawValue::from_string(
                        serde_json::to_string(&value).map_err(|_| "Invalid preview value")?,
                    )
                    .map_err(|_| "Invalid preview value")?;
                    let chunk = self.chunk(&field, false, &raw)?;
                    opaque += chunk.opaque;
                    Field::Scalar(chunk)
                }
            };
            fields.insert(field, rendered);
        }
        let mut full = String::with_capacity(body.len());
        full.push('{');
        let mut first = true;
        for (field, value) in &fields {
            if !first {
                full.push(',');
            }
            first = false;
            full.push_str(&serde_json::to_string(field).expect("JSON key serializes"));
            full.push(':');
            match value {
                Field::Scalar(chunk) => full.push_str(&chunk.full),
                Field::Array(chunks) => {
                    full.push('[');
                    for (index, chunk) in chunks.iter().enumerate() {
                        if index > 0 {
                            full.push(',');
                        }
                        full.push_str(&chunk.full);
                    }
                    full.push(']');
                }
            }
        }
        full.push('}');
        let newest =
            ["messages", "input", "contents"]
                .iter()
                .find_map(|key| match fields.get(*key) {
                    Some(Field::Array(chunks)) => chunks.last(),
                    _ => None,
                });
        let latest = newest.map_or_else(|| Arc::from(""), |chunk| chunk.latest.clone());
        // Feed the existing pane renderer bounded samples of the exact
        // redacted chunks, newest-first at the request level as before. Full
        // Original/Sent retain every byte of all older content.
        let mut pane_fields = serde_json::Map::new();
        let mut pane_budget = PANE_SAMPLE_BYTES;
        let mut omitted = false;
        for (field, value) in &fields {
            match value {
                Field::Scalar(chunk) => {
                    pane_fields.insert(field.clone(), chunk.sample.clone());
                }
                Field::Array(chunks) => {
                    let mut visible = Vec::new();
                    for chunk in chunks.iter().rev() {
                        if pane_budget == 0 {
                            omitted = true;
                            break;
                        }
                        visible.push(chunk.sample.clone());
                        pane_budget = pane_budget.saturating_sub(chunk.sample_bytes.max(1));
                    }
                    visible.reverse();
                    pane_fields.insert(field.clone(), Value::Array(visible));
                }
            }
        }
        let mut pane = crate::provider_filter::outbound_pane_value(
            &Value::Object(pane_fields),
            full.as_bytes(),
        );
        if omitted {
            pane.push_str("\n\n[older pane history omitted; Full contains the complete request]");
        }
        Ok(Envelope {
            fields,
            full: Arc::new(full),
            latest,
            pane: Arc::from(pane),
            opaque,
        })
    }
}

fn bounded_text(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[… newest content truncated; Full contains the complete request]");
    }
    text
}

fn bounded_pretty(value: &Value, limit: usize) -> String {
    struct Writer {
        bytes: Vec<u8>,
        limit: usize,
        cut: bool,
    }
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let count = bytes.len().min(self.limit.saturating_sub(self.bytes.len()));
            self.bytes.extend_from_slice(&bytes[..count]);
            if count < bytes.len() {
                self.cut = true;
                return Err(std::io::Error::other("bounded preview complete"));
            }
            Ok(count)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        bytes: Vec::with_capacity(limit),
        limit,
        cut: false,
    };
    let _ = serde_json::to_writer_pretty(&mut writer, value);
    if let Err(error) = std::str::from_utf8(&writer.bytes) {
        writer.bytes.truncate(error.valid_up_to());
    }
    let mut text = String::from_utf8(writer.bytes).expect("bounded serializer prefix is UTF-8");
    if writer.cut {
        text.push_str("\n[… newest content truncated; Full contains the complete request]");
    }
    text
}

fn sample(value: &Value, budget: &mut usize, depth: usize) -> Value {
    if depth > 128 || *budget == 0 {
        return Value::String("[pane content omitted; see Full]".into());
    }
    *budget = budget.saturating_sub(32);
    match value {
        Value::String(text) => {
            let mut end = text.len().min(*budget);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            *budget = budget.saturating_sub(end);
            let mut result = text[..end].to_owned();
            if end < text.len() {
                result.push_str("[… pane content truncated; see Full]");
            }
            Value::String(result)
        }
        Value::Array(items) => {
            let mut result = Vec::new();
            for item in items.iter().take(4096) {
                if *budget == 0 {
                    break;
                }
                result.push(sample(item, budget, depth + 1));
            }
            if result.len() < items.len() {
                result.push(Value::String("[pane items omitted; see Full]".into()));
            }
            Value::Array(result)
        }
        Value::Object(items) => {
            let mut result = serde_json::Map::new();
            for (key, value) in items.iter().take(4096) {
                let control = matches!(
                    key.as_str(),
                    "role" | "type" | "name" | "id" | "tool_use_id"
                ) && value.as_str().is_some_and(|text| text.len() <= 1024);
                if control {
                    result.insert(key.clone(), value.clone());
                } else if *budget > 0 {
                    *budget = budget.saturating_sub(key.len().saturating_add(32));
                    result.insert(key.clone(), sample(value, budget, depth + 1));
                }
            }
            Value::Object(result)
        }
        _ => value.clone(),
    }
}

fn retained_value_bytes(value: &Value) -> usize {
    std::mem::size_of::<Value>().saturating_add(match value {
        Value::String(text) => text.capacity(),
        Value::Array(items) => items
            .capacity()
            .saturating_mul(std::mem::size_of::<Value>())
            .saturating_add(items.iter().map(retained_value_bytes).sum::<usize>()),
        Value::Object(items) => items
            .iter()
            .map(|(key, value)| {
                key.capacity()
                    .saturating_add(retained_value_bytes(value))
                    .saturating_add(96)
            })
            .sum(),
        _ => 0,
    })
}

fn changes(original: &Envelope, sent: &Envelope) -> String {
    let mut pairs = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for key in original.fields.keys().chain(sent.fields.keys()) {
        if !seen.insert(key) {
            continue;
        }
        match (original.fields.get(key), sent.fields.get(key)) {
            (Some(Field::Array(before)), Some(Field::Array(after))) => {
                for index in 0..before.len().max(after.len()) {
                    pairs.push((
                        format!("{key}[{index}]"),
                        before.get(index).map(|chunk| chunk.full.as_str()),
                        after.get(index).map(|chunk| chunk.full.as_str()),
                    ));
                }
            }
            (before, after) => {
                fn scalar(field: Option<&Field>) -> Option<&str> {
                    match field {
                        Some(Field::Scalar(chunk)) => Some(chunk.full.as_str()),
                        _ => None,
                    }
                }
                pairs.push((key.clone(), scalar(before), scalar(after)));
            }
        }
    }
    let total = pairs
        .iter()
        .filter(|(_, before, after)| before != after)
        .count();
    if total == 0 {
        return "No changes".into();
    }
    let mut out = String::new();
    let mut shown = 0;
    let mut omitted_bytes = 0usize;
    for (name, before, after) in pairs
        .into_iter()
        .rev()
        .filter(|(_, before, after)| before != after)
    {
        if out.len() >= CHANGE_BYTES {
            omitted_bytes = omitted_bytes
                .saturating_add(before.map_or(0, str::len))
                .saturating_add(after.map_or(0, str::len));
            continue;
        }
        shown += 1;
        out.push_str(&format!("{name}\n"));
        for (prefix, value) in [("− ", before), ("+ ", after)] {
            if let Some(value) = value {
                out.push_str(prefix);
                let mut end = value.len().min(CHANGE_BYTES.saturating_sub(out.len()));
                while !value.is_char_boundary(end) {
                    end -= 1;
                }
                out.push_str(&value[..end]);
                omitted_bytes = omitted_bytes.saturating_add(value.len() - end);
                out.push('\n');
            }
        }
    }
    if shown < total || omitted_bytes > 0 {
        out.push_str(&format!("\n[{} changed regions and {omitted_bytes} bytes omitted from Changes; Full Original/Sent contain the complete request]", total - shown));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn privacy_preview_single_scan_keeps_scalar_input_and_duplicate_key_semantics() {
        for raw in [
            r#"{"input":"complete scalar input","model":"m"}"#,
            r#"{"input":{"content":"object input"},"model":"m"}"#,
            r#"{"input":null,"model":"m"}"#,
            r#"{"input":17.5,"model":"m"}"#,
            r#"{"input":false,"model":"m"}"#,
            r#"{"input":[{"type":"reasoning","encrypted_content":"opaque discarded root"}],"input":[{"role":"user","content":"kept"}],"model":"m"}"#,
        ] {
            let mut cache = Cache::default();
            let rendered = cache.render(raw.as_bytes(), raw.as_bytes()).unwrap();
            let actual: Value = serde_json::from_str(&rendered.sent).unwrap();
            let expected: Value = serde_json::from_str(raw).unwrap();
            assert_eq!(actual, expected);
        }
        let raw = br#"{"input":[{"type":"reasoning","encrypted_content":"opaque first","encrypted_content":"opaque last"}]}"#;
        let mut cache = Cache::default();
        let rendered = cache.render(raw, raw).unwrap();
        assert!(!rendered.sent.contains("opaque first"));
        assert!(!rendered.sent.contains("opaque last"));
        assert!(rendered.sent.contains(OPAQUE));
    }

    #[test]
    fn privacy_preview_responses_omits_only_reasoning_ciphertext() {
        let mut body = json!({"input":[
            {"type":"reasoning","encrypted_content":"opaque","summary":[{"type":"summary_text","text":"readable summary"}]},
            {"type":"message","role":"user","content":[{"type":"input_text","text":"readable document"}]},
            {"type":"function_call","name":"custom","arguments":"{\"data\":\"public\",\"signature\":\"public\"}"}
        ]});
        assert_eq!(redact_opaque(&mut body), 1);
        assert_eq!(body["input"][0]["encrypted_content"], OPAQUE);
        assert_eq!(body["input"][0]["summary"][0]["text"], "readable summary");
        assert_eq!(
            body["input"][2]["arguments"],
            "{\"data\":\"public\",\"signature\":\"public\"}"
        );
    }

    #[test]
    fn privacy_preview_anthropic_keeps_native_tool_input_displayable() {
        let input = json!({
            "data":"public", "signature":"public", "encrypted_content":"public",
            "document":{"type":"redacted_thinking","data":"public","signature":"public"},
            "messages":[{"content":[{"type":"thinking","signature":"public"}]}]
        });
        let mut body = json!({"messages":[{"role":"assistant","content":[
            {"type":"thinking","thinking":"readable thinking","signature":"opaque signature"},
            {"type":"redacted_thinking","data":"opaque data","signature":"opaque signature"},
            {"type":"tool_use","id":"tool-1","name":"custom","input":input}
        ]}]});
        assert_eq!(redact_opaque(&mut body), 3);
        let blocks = &body["messages"][0]["content"];
        assert_eq!(blocks[0]["thinking"], "readable thinking");
        assert_eq!(blocks[0]["signature"], OPAQUE);
        assert_eq!(blocks[1]["data"], OPAQUE);
        assert_eq!(blocks[1]["signature"], OPAQUE);
        assert_eq!(blocks[2]["input"], input);
    }

    #[test]
    fn privacy_preview_google_omits_part_signatures_and_inline_blobs() {
        let arguments = json!({
            "data":"public", "signature":"public", "thoughtSignature":"public",
            "inlineData":{"mimeType":"image/png","data":"public"},
            "document":{"parts":[{"thoughtSignature":"public"}]}
        });
        let response = json!({"data":"public","signature":"public"});
        let mut body = json!({
            "systemInstruction":{"parts":[{"text":"rules","thought_signature":"opaque system"}]},
            "contents":[{"role":"model","parts":[
                {"text":"readable thinking","thought":true,"thoughtSignature":"opaque thought"},
                {"inlineData":{"mimeType":"image/png","data":"opaque image"}},
                {"inline_data":{"mime_type":"image/jpeg","data":"opaque image"}},
                {"functionCall":{"name":"custom","args":arguments},"thoughtSignature":"opaque call"},
                {"functionResponse":{"name":"custom","response":response}}
            ]}]
        });
        assert_eq!(redact_opaque(&mut body), 5);
        let parts = &body["contents"][0]["parts"];
        assert_eq!(
            body["systemInstruction"]["parts"][0]["thought_signature"],
            OPAQUE
        );
        assert_eq!(parts[0]["text"], "readable thinking");
        assert_eq!(parts[0]["thoughtSignature"], OPAQUE);
        assert_eq!(parts[1]["inlineData"]["data"], OPAQUE);
        assert_eq!(parts[2]["inline_data"]["data"], OPAQUE);
        assert_eq!(parts[3]["functionCall"]["args"], arguments);
        assert_eq!(parts[4]["functionResponse"]["response"], response);
    }

    #[test]
    fn privacy_preview_ordinary_data_and_signatures_are_not_protocol_blocks() {
        let original = json!({
            "data":"public", "signature":"public", "encrypted_content":"public",
            "type":"reasoning", "thoughtSignature":"public",
            "document":{"type":"redacted_thinking","data":"public"},
            "tools":[{"type":"function","function":{"name":"custom","parameters":{
                "properties":{"data":{"default":"public"},"signature":{"default":"public"}}
            }}}]
        });
        let mut body = original.clone();
        assert_eq!(redact_opaque(&mut body), 0);
        assert_eq!(body, original);
    }
}
