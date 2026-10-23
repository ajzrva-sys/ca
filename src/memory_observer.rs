//! Bounded payload observations. No serialization, payload logging, or reclamation of essential state.
use crate::cache_memory::Essential;
use std::cell::RefCell;
use std::mem::size_of;

const MESSAGES_PER_TICK: usize = 64;
const NODES_PER_TICK: usize = 8192;

#[derive(Debug, Default)]
struct Scan {
    cursor: usize,
    total: usize,
    history: usize,
    replay: usize,
    published_history: usize,
    published_replay: usize,
    complete: bool,
    bounded: bool,
}
#[derive(Debug)]
pub(crate) struct Observer {
    scan: RefCell<Scan>,
    history: Essential,
    replay: Essential,
    streams: Essential,
    catalogs: Essential,
}
impl Default for Observer {
    fn default() -> Self {
        Self {
            scan: RefCell::new(Scan::default()),
            history: Essential::new("conversation history"),
            replay: Essential::new("provider replay state"),
            streams: Essential::new("active stream buffers"),
            catalogs: Essential::new("model catalogs"),
        }
    }
}
impl Observer {
    pub(crate) fn observe(&self, state: &crate::app::AppState) {
        let saved = state
            .current_session
            .as_ref()
            .map_or(&[][..], |session| session.messages.as_slice());
        let total = state.messages.len().saturating_add(saved.len());
        let mut scan = self.scan.borrow_mut();
        if scan.total != total {
            scan.complete = false;
        }
        if scan.total != total || scan.cursor >= total {
            scan.total = total;
            scan.cursor = 0;
            scan.history = state.messages.capacity() * size_of::<crate::app::Message>()
                + std::mem::size_of_val(saved);
            scan.replay = 0;
            scan.bounded = false;
        }
        let mut nodes = NODES_PER_TICK;
        for index in scan.cursor..total.min(scan.cursor + MESSAGES_PER_TICK) {
            let message = if index < state.messages.len() {
                &state.messages[index]
            } else {
                &saved[index - state.messages.len()]
            };
            scan.history = scan.history.saturating_add(
                message.content.capacity() + message.thinking.as_ref().map_or(0, String::capacity),
            );
            if let Some(calls) = &message.tool_calls {
                scan.history = scan
                    .history
                    .saturating_add(calls.capacity() * size_of::<crate::app::ToolCall>());
                for call in calls.iter().take(512) {
                    scan.history = scan.history.saturating_add(
                        call.id.capacity()
                            + call.name.capacity()
                            + call.arguments.capacity()
                            + call.output.as_ref().map_or(0, String::capacity),
                    );
                }
                scan.bounded |= calls.len() > 512;
            }
            scan.replay = scan.replay.saturating_add(
                message.response_items.capacity()
                    * size_of::<crate::agent::stream::ProviderResponseItem>(),
            );
            for item in message.response_items.iter().take(128) {
                let (bytes, bounded) = replay_bytes(item, &mut nodes);
                scan.replay = scan.replay.saturating_add(bytes);
                scan.bounded |= bounded;
            }
            scan.bounded |= message.response_items.len() > 128;
            scan.cursor = index + 1;
            if nodes == 0 {
                scan.bounded = true;
                break;
            }
        }
        if scan.cursor == total {
            scan.complete = true;
            scan.published_history = scan.history;
            scan.published_replay = scan.replay;
        }
        if !scan.complete {
            scan.published_history = scan.history;
            scan.published_replay = scan.replay;
        }
        self.history.set(scan.published_history);
        self.replay.set(scan.published_replay);
        let mut stream_bytes = 0usize;
        if let Some(stream) = &state.streaming {
            stream_bytes = stream.content.capacity()
                + stream.thinking.as_ref().map_or(0, String::capacity)
                + stream
                    .provider_text
                    .content
                    .as_ref()
                    .map_or(0, String::capacity)
                + stream
                    .provider_text
                    .thinking
                    .as_ref()
                    .map_or(0, String::capacity);
            for call in stream.tool_calls.iter().take(512) {
                stream_bytes = stream_bytes.saturating_add(
                    size_of::<crate::app::PartialToolCall>()
                        + call.arguments.capacity()
                        + call.id.as_ref().map_or(0, String::capacity)
                        + call.name.as_ref().map_or(0, String::capacity),
                );
            }
            for arguments in stream.provider_text.tool_arguments.values().take(512) {
                stream_bytes = stream_bytes.saturating_add(arguments.capacity());
            }
            for item in stream.response_items.iter().take(128) {
                stream_bytes = stream_bytes.saturating_add(replay_bytes(item, &mut nodes).0);
            }
        }
        self.streams.set(stream_bytes);
        let mut catalog_bytes = 0usize;
        for model in state.model_list.iter().take(2048) {
            catalog_bytes = catalog_bytes.saturating_add(
                size_of::<crate::app::ModelInfo>()
                    + model.id.capacity()
                    + model.provider.capacity()
                    + model.display_name.capacity(),
            );
        }
        for (provider, models) in state
            .discovered_models
            .iter()
            .chain(state.codex_catalogs.iter())
            .take(128)
        {
            catalog_bytes = catalog_bytes.saturating_add(provider.capacity());
            for model in models.iter().take(2048) {
                catalog_bytes =
                    catalog_bytes.saturating_add(size_of::<String>() + model.capacity());
            }
            scan.bounded |= models.len() > 2048;
        }
        scan.bounded |= state.model_list.len() > 2048
            || state.discovered_models.len() + state.codex_catalogs.len() > 128;
        self.catalogs.set(catalog_bytes);
    }
    pub(crate) fn history_estimate(&self) -> usize {
        let scan = self.scan.borrow();
        scan.published_history.saturating_add(scan.published_replay)
    }
    pub(crate) fn progress(&self) -> String {
        let scan = self.scan.borrow();
        format!("Essential payload observations: {}/{} messages in the current bounded scan; {}. Estimates refresh in bounded chunks on idle ticks.{}", scan.cursor, scan.total, if scan.complete { "a full scan is available" } else { "initial scan in progress" }, if scan.bounded { " Deep or oversized collections are lower bounds." } else { "" })
    }
}
fn source_bytes(source: &crate::agent::stream::ProviderStateSourceV1) -> usize {
    source.provider.capacity()
        + source.endpoint_sha256.capacity()
        + source
            .credential_sha256
            .as_ref()
            .map_or(0, String::capacity)
}
fn replay_bytes(
    item: &crate::agent::stream::ProviderResponseItem,
    nodes: &mut usize,
) -> (usize, bool) {
    use crate::agent::stream::ProviderResponseItem;
    match item {
        ProviderResponseItem::NativeContent {
            source,
            model,
            prefix_sha256,
            content,
        } => {
            let mut bytes = source_bytes(source)
                + model.capacity()
                + prefix_sha256.capacity()
                + content.capacity() * size_of::<serde_json::Value>();
            let mut bounded = false;
            for value in content.iter().take(128) {
                let (size, limit) = value_bytes(value, nodes);
                bytes = bytes.saturating_add(size);
                bounded |= limit;
            }
            (bytes, bounded || content.len() > 128)
        }
        ProviderResponseItem::ResponsesOutput { source, item, .. } => {
            let (bytes, bounded) = value_bytes(item, nodes);
            (bytes + source_bytes(source), bounded)
        }
        ProviderResponseItem::ChatReasoning { source, content } => {
            (source_bytes(source) + content.capacity(), false)
        }
    }
}
fn value_bytes(value: &serde_json::Value, nodes: &mut usize) -> (usize, bool) {
    let mut stack = vec![value];
    let mut bytes = 0usize;
    let mut bounded = false;
    while let Some(value) = stack.pop() {
        if *nodes == 0 {
            bounded = true;
            break;
        }
        *nodes -= 1;
        match value {
            serde_json::Value::String(value) => bytes = bytes.saturating_add(value.capacity()),
            serde_json::Value::Array(values) => {
                bytes = bytes.saturating_add(values.capacity() * size_of::<serde_json::Value>());
                let available = (*nodes).saturating_sub(stack.len());
                bounded |= values.len() > available;
                stack.extend(values.iter().take(available));
            }
            serde_json::Value::Object(values) => {
                bytes =
                    bytes.saturating_add(values.len() * size_of::<(String, serde_json::Value)>());
                let available = (*nodes).saturating_sub(stack.len());
                bounded |= values.len() > available;
                for (key, value) in values.iter().take(available) {
                    bytes = bytes.saturating_add(key.capacity());
                    stack.push(value);
                }
            }
            _ => {}
        }
    }
    (bytes, bounded)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ocx_memory_observation_is_bounded_and_preserves_essential_payloads() {
        let mut state = crate::tui::test_support::test_state();
        state.messages = (0..130)
            .map(|_| crate::app::Message {
                role: crate::app::Role::Assistant,
                content: "preserved history".into(),
                thinking: None,
                tool_calls: None,
                response_items: Vec::new(),
                timestamp: chrono::Utc::now(),
            })
            .collect();
        let messages = state.messages.clone();
        state.memory_observer.observe(&state);
        assert_eq!(
            state.memory_observer.scan.borrow().cursor,
            MESSAGES_PER_TICK
        );
        state.memory_observer.observe(&state);
        state.memory_observer.observe(&state);
        assert!(state.memory_observer.scan.borrow().complete);
        assert!(
            state.memory_observer.history_estimate()
                >= state
                    .messages
                    .iter()
                    .map(|message| message.content.len())
                    .sum()
        );
        assert_eq!(state.messages, messages);
    }
    #[test]
    fn ocx_memory_replay_counts_opaque_bytes_without_rendering_their_values() {
        let value = serde_json::json!({"signature": "OPAQUE_MEMORY_SIGNATURE", "parts": [{"text": ""}, {"data": "HIDDEN"}]});
        let mut nodes = 32;
        let (bytes, bounded) = value_bytes(&value, &mut nodes);
        assert!(bytes >= "OPAQUE_MEMORY_SIGNATURE".len());
        assert!(!bounded);
        let mut nodes = 1;
        assert!(value_bytes(&value, &mut nodes).1);
    }
}
