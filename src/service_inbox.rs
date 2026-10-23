//! Service input admission is separate from execution. Loading never replays work.
use crate::agent::client::ChatMessage;
use crate::app::{Message, Role, ToolCall, ToolStatus};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    #[default]
    Steer,
    Queue,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputStatus {
    Pending,
    Promoted,
    Completed,
    Interrupted,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub id: String,
    pub text: String,
    pub delivery: Delivery,
    pub status: InputStatus,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Inbox {
    pub inputs: Vec<Input>,
    pub held: bool,
}
impl std::fmt::Debug for Inbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inbox")
            .field("inputs", &self.inputs.len())
            .field("held", &self.held)
            .finish()
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    pub input_id: String,
    pub delivery: Delivery,
    pub status: InputStatus,
    pub durable: bool,
}
impl Inbox {
    pub fn admit(&mut self, id: &str, text: &str, delivery: Delivery) -> Result<Input, String> {
        if let Some(existing) = self.inputs.iter().find(|input| input.id == id) {
            return if existing.text == text && existing.delivery == delivery {
                Ok(existing.clone())
            } else {
                Err("input ID already admitted with different text or delivery".into())
            };
        }
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
        {
            return Err("input ID must be 1-128 safe ASCII characters".into());
        }
        if text.trim().is_empty() || text.len() > 48 * 1024 || text.contains('\0') {
            return Err("input must be 1-49152 UTF-8 bytes without NUL".into());
        }
        let pending = self
            .inputs
            .iter()
            .filter(|i| i.status == InputStatus::Pending)
            .collect::<Vec<_>>();
        if pending.len() >= 64
            || pending
                .iter()
                .map(|i| i.text.len())
                .sum::<usize>()
                .saturating_add(text.len())
                > 512 * 1024
            || self.inputs.len() >= 4096
        {
            return Err("service inbox capacity reached".into());
        }
        let input = Input {
            id: id.into(),
            text: text.into(),
            delivery,
            status: InputStatus::Pending,
        };
        self.inputs.push(input.clone());
        Ok(input)
    }
    fn promote(&mut self, idle: bool) -> Vec<String> {
        if self.held {
            return vec![];
        }
        let mut texts = Vec::new();
        for input in &mut self.inputs {
            if input.status == InputStatus::Pending && input.delivery == Delivery::Steer {
                input.status = InputStatus::Promoted;
                texts.push(input.text.clone());
            }
        }
        if texts.is_empty() && idle {
            if let Some(input) = self
                .inputs
                .iter_mut()
                .find(|i| i.status == InputStatus::Pending)
            {
                input.status = InputStatus::Promoted;
                texts.push(input.text.clone());
            }
        }
        texts
    }
    pub fn stop(&mut self, success: bool) {
        for input in &mut self.inputs {
            if input.status == InputStatus::Promoted {
                input.status = if success {
                    InputStatus::Completed
                } else {
                    InputStatus::Interrupted
                };
            }
        }
        if !success {
            self.held = true;
        }
    }
    pub fn receipts(&self, durable: bool) -> Vec<Receipt> {
        self.inputs
            .iter()
            .map(|i| Receipt {
                input_id: i.id.clone(),
                delivery: i.delivery,
                status: i.status,
                durable,
            })
            .collect()
    }
}
struct NativeBinding {
    session: crate::session::SessionV2,
    store: crate::session::SessionStore,
    writer: crate::session_access::SessionWriterLease,
}
#[derive(Default)]
pub struct Runtime {
    pub inbox: Inbox,
    messages: Vec<ChatMessage>,
    native: Option<NativeBinding>,
}
impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceInboxRuntime")
            .field("inbox", &self.inbox)
            .field("durable", &self.native.is_some())
            .finish()
    }
}
impl Runtime {
    pub fn durable(&self) -> bool {
        self.native.is_some()
    }
    pub async fn attach(
        &mut self,
        mut session: crate::session::SessionV2,
        store: crate::session::SessionStore,
        messages: Vec<ChatMessage>,
    ) -> Result<(), String> {
        let access = store.clone();
        let id = session.id;
        let writer = tokio::task::spawn_blocking(move || access.try_writer_lease(id))
            .await
            .map_err(|_| "service session ownership worker stopped")?
            .map_err(|e| e.to_string())?;
        let mut inbox = session.service_inbox.clone().unwrap_or_default();
        inbox.stop(false);
        // A fresh persistent session may execute new submissions; loading holds saved work.
        if session.storage_binding.is_none() {
            inbox.held = false;
        }
        session.service_inbox = Some(inbox.clone());
        self.native = Some(NativeBinding {
            session,
            store,
            writer,
        });
        self.commit(inbox, messages).await
    }
    async fn commit(&mut self, inbox: Inbox, messages: Vec<ChatMessage>) -> Result<(), String> {
        if let Some(native) = &mut self.native {
            let mut next = native.session.clone();
            next.service_inbox = Some(inbox.clone());
            let mut persistent = messages.clone();
            crate::service::scrub_email_chat_messages(&mut persistent);
            let mut canonical = canonical(&persistent)?;
            for (old, message) in native.session.messages.iter().zip(&mut canonical) {
                if old.role == message.role
                    && old.content == message.content
                    && old.tool_calls == message.tool_calls
                    && old.response_items == message.response_items
                {
                    message.timestamp = old.timestamp;
                    message.thinking = old.thinking.clone();
                } else {
                    break;
                }
            }
            next.messages = canonical;
            let store = native.store.clone();
            let writer = native.writer.clone();
            let next = tokio::task::spawn_blocking(move || {
                store
                    .save_owned(&mut next, &writer)
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>(next)
            })
            .await
            .map_err(|_| "service session save worker stopped")??;
            native.session = next;
        }
        self.inbox = inbox;
        self.messages = messages;
        Ok(())
    }
    pub async fn admit(
        &mut self,
        id: &str,
        text: &str,
        delivery: Delivery,
    ) -> Result<Receipt, String> {
        let already_admitted = self.inbox.inputs.iter().any(|input| input.id == id);
        let mut inbox = self.inbox.clone();
        let input = inbox.admit(id, text, delivery)?;
        if !already_admitted {
            self.commit(inbox, self.messages.clone()).await?;
        }
        Ok(Receipt {
            input_id: input.id,
            // Admission receipts are immutable; inspect_inbox reports lifecycle status.
            status: InputStatus::Pending,
            delivery: input.delivery,
            durable: self.durable(),
        })
    }
    pub async fn promote(
        &mut self,
        messages: &mut Vec<ChatMessage>,
        idle: bool,
    ) -> Result<bool, String> {
        let mut inbox = self.inbox.clone();
        if idle {
            inbox.stop(true);
        }
        let texts = inbox.promote(idle);
        if texts.is_empty() {
            self.commit(inbox, messages.clone()).await?;
            return Ok(false);
        }
        let mut next = messages.clone();
        for text in texts {
            next.push(ChatMessage {
                role: "user".into(),
                content: crate::image_content::ModelContent::text(text),
                tool_calls: None,
                tool_call_id: None,
                response_items: vec![],
            });
        }
        self.commit(inbox, next.clone()).await?;
        *messages = next;
        Ok(true)
    }
    pub async fn checkpoint(&mut self, messages: Vec<ChatMessage>) -> Result<(), String> {
        self.commit(self.inbox.clone(), messages).await
    }
    pub async fn stop(&mut self, success: bool) -> Result<(), String> {
        let mut inbox = self.inbox.clone();
        inbox.stop(success);
        self.commit(inbox, self.messages.clone()).await
    }
    pub async fn resume(&mut self) -> Result<(), String> {
        let mut inbox = self.inbox.clone();
        inbox.held = false;
        self.commit(inbox, self.messages.clone()).await
    }
}

fn canonical(messages: &[ChatMessage]) -> Result<Vec<Message>, String> {
    let mut result: Vec<Message> = Vec::new();
    for message in messages {
        if message.role == "user" && message.content.tool_result_success().is_some() {
            continue;
        }
        if message.role == "system" || message.role == "developer" {
            continue;
        }
        if message.role == "tool" {
            let id = message
                .tool_call_id
                .as_deref()
                .ok_or("tool result missing ID")?;
            let call = result
                .iter_mut()
                .rev()
                .filter_map(|m| m.tool_calls.as_mut())
                .flatten()
                .find(|c| c.id == id)
                .ok_or("tool result has no matching call")?;
            call.output = Some(
                message
                    .content
                    .as_str()
                    .unwrap_or("[Media unavailable after restart]")
                    .into(),
            );
            // Unknown restored outcomes remain protected from selective pruning.
            call.status = if message.content.tool_result_success() == Some(true) {
                ToolStatus::Done
            } else {
                ToolStatus::Error
            };
            continue;
        }
        let role = match message.role.as_str() {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            _ => return Err("unsupported service message role".into()),
        };
        result.push(Message {
            role,
            content: message
                .content
                .as_str()
                .unwrap_or("[Media unavailable after restart]")
                .into(),
            tool_calls: message.tool_calls.as_ref().map(|calls| {
                calls
                    .iter()
                    .map(|c| ToolCall {
                        id: c.id.clone(),
                        name: c.function.name.clone(),
                        arguments: c.function.arguments.clone(),
                        status: ToolStatus::Running,
                        output: None,
                    })
                    .collect()
            }),
            thinking: None,
            response_items: message.response_items.clone(),
            timestamp: chrono::Utc::now(),
        });
    }
    if result.iter().any(|m| {
        m.tool_calls
            .as_ref()
            .is_some_and(|calls| calls.iter().any(|c| c.output.is_none()))
    }) {
        return Err("service history has an incomplete tool exchange".into());
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn orc290_exact_retry_conflict_limits_and_order() {
        let mut inbox = Inbox::default();
        inbox.admit("queued", "later", Delivery::Queue).unwrap();
        inbox.admit("steer", "now", Delivery::Steer).unwrap();
        inbox.admit("steer", "now", Delivery::Steer).unwrap();
        assert_eq!(inbox.inputs.len(), 2);
        assert!(inbox.admit("steer", "different", Delivery::Steer).is_err());
        assert_eq!(inbox.promote(false), vec!["now"]);
        assert!(inbox.promote(false).is_empty());
        assert_eq!(inbox.promote(true), vec!["later"]);
        inbox.stop(false);
        assert!(inbox.held);
        assert!(inbox
            .inputs
            .iter()
            .all(|i| i.status == InputStatus::Interrupted));
        let mut full = Inbox::default();
        for i in 0..64 {
            full.admit(&format!("i{i}"), "x", Delivery::Queue).unwrap();
        }
        assert!(full.admit("overflow", "x", Delivery::Queue).is_err());
        assert_eq!(full.inputs.len(), 64);
    }
    #[tokio::test]
    async fn orc290_native_roundtrip_holds_inputs_and_never_promotes_on_load() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::session::SessionStore::new(temp.path());
        let session = crate::session::SessionV2::new(
            "service",
            temp.path().to_string_lossy(),
            "fake",
            "fake-model",
            vec![],
        );
        let id = session.id;
        let mut runtime = Runtime::default();
        runtime
            .attach(session, store.clone(), vec![])
            .await
            .unwrap();
        assert!(
            runtime
                .admit("one", "work", Delivery::Queue)
                .await
                .unwrap()
                .durable
        );
        drop(runtime);
        let saved = store.load(&id.to_string()).unwrap();
        let mut loaded = Runtime::default();
        loaded.attach(saved, store, vec![]).await.unwrap();
        assert!(loaded.inbox.held);
        let mut messages = vec![];
        assert!(!loaded.promote(&mut messages, true).await.unwrap());
        assert!(messages.is_empty());
        loaded.resume().await.unwrap();
        assert!(loaded.promote(&mut messages, true).await.unwrap());
        assert_eq!(messages.len(), 1);
    }
    #[test]
    fn orc290_aggregate_capacity_and_delivery_conflicts_do_not_change_admission() {
        let mut inbox = Inbox::default();
        for i in 0..10 {
            inbox
                .admit(
                    &format!("input{i}"),
                    &"x".repeat(48 * 1024),
                    Delivery::Queue,
                )
                .unwrap();
        }
        assert!(inbox
            .admit("overflow", &"x".repeat(48 * 1024), Delivery::Queue)
            .is_err());
        assert!(inbox
            .admit("input0", &"x".repeat(48 * 1024), Delivery::Steer)
            .is_err());
        assert_eq!(inbox.inputs.len(), 10);
    }
    #[tokio::test]
    async fn orc290_native_writer_conflict_and_failed_commit_never_acknowledge_input() {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::session::SessionStore::new(temp.path());
        let session = crate::session::SessionV2::new(
            "service",
            temp.path().to_string_lossy(),
            "fake",
            "model",
            vec![],
        );
        let id = session.id;
        let writer = store.try_writer_lease(id).unwrap();
        let mut runtime = Runtime::default();
        assert!(runtime
            .attach(session.clone(), store.clone(), vec![])
            .await
            .is_err());
        drop(writer);
        runtime
            .attach(session, store.clone(), vec![])
            .await
            .unwrap();
        std::fs::write(temp.path().join(format!("{id}.json")), b"{}").unwrap();
        assert!(runtime
            .admit("not-acknowledged", "work", Delivery::Steer)
            .await
            .is_err());
        assert!(runtime.inbox.inputs.is_empty());
    }
    #[test]
    fn orc290_native_fork_does_not_inherit_actionable_inbox() {
        let mut source =
            crate::session::SessionV2::new("service", "/workspace", "fake", "model", vec![]);
        let mut inbox = Inbox::default();
        inbox.admit("pending", "work", Delivery::Queue).unwrap();
        source.service_inbox = Some(inbox);
        let branch = crate::session::branch_in_memory(&source, 0, 0, "fork").unwrap();
        assert!(branch.service_inbox.is_none());
    }
}
