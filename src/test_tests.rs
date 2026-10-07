use super::{run_agent_turn, run_script_with_backend};
use crate::agent::client::{ChatBackend, ChatDelta, ChatMessage, ToolDef};
use crate::agent::stream::{FinishReason, ProviderError, ProviderEvent};
use crate::app::AppState;
use crate::context::ShellHistory;
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::sync::mpsc;

struct ScriptedBackend {
    requests: Mutex<Vec<Vec<ChatMessage>>>,
    prepared: Mutex<Vec<(crate::request::RequestKind, String, String)>>,
    scripts: Mutex<VecDeque<Vec<ChatDelta>>>,
}

impl ScriptedBackend {
    fn new(scripts: Vec<Vec<ChatDelta>>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            prepared: Mutex::new(Vec::new()),
            scripts: Mutex::new(scripts.into()),
        }
    }
}

fn selected_tools(names: &[&str]) -> Vec<ToolDef> {
    crate::tool_selection::resolve_tools(
        crate::agent::registry::ToolRegistry::builtins(),
        crate::agent::registry::ToolProfile::Main,
        &crate::tool_selection::ToolSelectionPolicy::replacing(names),
        &crate::tool_selection::ToolSelectionConstraints::default(),
    )
    .unwrap()
    .into_definitions()
}

#[async_trait::async_trait]
impl ChatBackend for ScriptedBackend {
    async fn stream_chat(
        &self,
        messages: &[ChatMessage],
        _tools: &[ToolDef],
    ) -> std::result::Result<
        mpsc::UnboundedReceiver<Result<ProviderEvent, ProviderError>>,
        ProviderError,
    > {
        self.requests.lock().unwrap().push(messages.to_vec());
        let script = self
            .scripts
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| ProviderError::Protocol("no scripted response remaining".into()))?;
        let (tx, rx) = mpsc::unbounded_channel();
        for delta in script {
            tx.send(Ok(delta))
                .map_err(|_| ProviderError::Protocol("scripted receiver closed".into()))?;
        }
        tx.send(Ok(ProviderEvent::Done))
            .map_err(|_| ProviderError::Protocol("scripted receiver closed".into()))?;
        Ok(rx)
    }

    async fn stream_prepared(
        &self,
        request: &crate::request::PreparedModelRequest,
    ) -> std::result::Result<
        mpsc::UnboundedReceiver<Result<ProviderEvent, ProviderError>>,
        ProviderError,
    > {
        self.prepared.lock().unwrap().push((
            request.kind,
            request.provider.clone(),
            request.model.clone(),
        ));
        self.stream_chat(&request.messages, &request.tools).await
    }
}

#[tokio::test]
async fn scripted_backend_completes_a_text_turn_without_http() {
    let backend = ScriptedBackend::new(vec![vec![
        ChatDelta::Text("hello".to_string()),
        ChatDelta::Finish {
            reason: FinishReason::Stop,
            usage: None,
        },
    ]]);
    let mut state = AppState::new(ShellHistory::in_memory());
    let mut api_messages = vec![ChatMessage {
        role: "user".to_string(),
        content: crate::image_content::ModelContent::text("hi"),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }];

    run_agent_turn(
        &mut state,
        &backend,
        &[],
        &mut api_messages,
        5,
        ("fake", "model"),
        ".",
    )
    .await
    .unwrap();

    assert_eq!(backend.requests.lock().unwrap().len(), 1);
    assert_eq!(
        backend.prepared.lock().unwrap().as_slice(),
        &[(
            crate::request::RequestKind::Scripted,
            "fake".into(),
            "model".into()
        )]
    );
    assert!(state
        .messages
        .iter()
        .any(|message| message.content == "hello"));
}

#[tokio::test]
async fn fake_backend_captures_tool_result_order() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("input.txt"), "hello").unwrap();
    let response_item = crate::agent::stream::ProviderResponseItem::ChatReasoning {
        source: crate::agent::stream::ProviderStateSourceV1::for_test(
            "fake",
            crate::agent::ProviderProtocol::ChatCompletions,
        ),
        content: "scripted opaque continuation".into(),
    };
    let backend = ScriptedBackend::new(vec![
        vec![
            ChatDelta::ToolCallStart {
                index: 0,
                id: "call-1".to_string(),
                name: "read_file".to_string(),
            },
            ChatDelta::ToolCallArg {
                index: 0,
                arg: r#"{"path":"input.txt"}"#.to_string(),
            },
            ChatDelta::ResponseItem(response_item.clone()),
            ChatDelta::Finish {
                reason: FinishReason::ToolCalls,
                usage: None,
            },
        ],
        vec![
            ChatDelta::Text("done".to_string()),
            ChatDelta::Finish {
                reason: FinishReason::Stop,
                usage: None,
            },
        ],
    ]);
    let mut state = AppState::new(ShellHistory::in_memory());
    let mut api_messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: crate::image_content::ModelContent::text("test system"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: crate::image_content::ModelContent::text("read input.txt"),
            tool_calls: None,
            tool_call_id: None,
            response_items: Vec::new(),
        },
    ];

    let tools = selected_tools(&["read_file"]);
    run_agent_turn(
        &mut state,
        &backend,
        &tools,
        &mut api_messages,
        5,
        ("fake", "model"),
        temp_dir.path().to_string_lossy().as_ref(),
    )
    .await
    .unwrap();

    let requests = backend.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let second = &requests[1];
    let roles: Vec<&str> = second.iter().map(|message| message.role.as_str()).collect();
    assert_eq!(roles, vec!["system", "user", "assistant", "tool"]);
    assert_eq!(second[2].tool_calls.as_ref().unwrap()[0].id, "call-1");
    assert_eq!(second[2].response_items, vec![response_item]);
    assert_eq!(second[3].tool_call_id.as_deref(), Some("call-1"));
    assert!(second[3].content.as_str().unwrap().contains("hello"));
    let stored_call = state
        .messages
        .iter()
        .flat_map(|message| message.tool_calls.iter().flatten())
        .find(|call| call.id == "call-1")
        .unwrap();
    assert_eq!(
        second[3].tool_call_id.as_deref(),
        Some(stored_call.id.as_str())
    );
    assert_eq!(second[3].content.as_str(), stored_call.output.as_deref());
    assert!(state
        .messages
        .iter()
        .any(|message| message.content == "done"));
}

#[tokio::test]
async fn scripted_provider_cannot_execute_a_tool_outside_the_advertised_set() {
    let temp_dir = tempfile::tempdir().unwrap();
    let target = temp_dir.path().join("must-not-exist");
    let backend = ScriptedBackend::new(vec![vec![
        ChatDelta::ToolCallStart {
            index: 0,
            id: "hidden-call".into(),
            name: "write_file".into(),
        },
        ChatDelta::ToolCallArg {
            index: 0,
            arg: serde_json::json!({"path":target, "content":"no"}).to_string(),
        },
        ChatDelta::Finish {
            reason: FinishReason::ToolCalls,
            usage: None,
        },
    ]]);
    let mut state = AppState::new(ShellHistory::in_memory());
    let mut api_messages = vec![ChatMessage {
        role: "user".into(),
        content: crate::image_content::ModelContent::text("try hidden tool"),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }];
    let tools = crate::tool_selection::resolve_tools(
        crate::agent::registry::ToolRegistry::builtins(),
        crate::agent::registry::ToolProfile::Main,
        &crate::tool_selection::ToolSelectionPolicy::replacing(&["read_file"]),
        &crate::tool_selection::ToolSelectionConstraints::default(),
    )
    .unwrap()
    .into_definitions();

    let error = run_agent_turn(
        &mut state,
        &backend,
        &tools,
        &mut api_messages,
        5,
        ("fake", "model"),
        temp_dir.path().to_str().unwrap(),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("unavailable scripted tool"));
    assert!(!target.exists());
}

#[tokio::test]
async fn fake_backend_preserves_multiple_tool_result_ids() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("one.txt"), "one").unwrap();
    std::fs::write(temp_dir.path().join("two.txt"), "two").unwrap();
    let backend = ScriptedBackend::new(vec![
        vec![
            ChatDelta::ToolCallStart {
                index: 0,
                id: "call-one".to_string(),
                name: "read_file".to_string(),
            },
            ChatDelta::ToolCallArg {
                index: 0,
                arg: r#"{"path":"one.txt"}"#.to_string(),
            },
            ChatDelta::ToolCallStart {
                index: 1,
                id: "call-two".to_string(),
                name: "read_file".to_string(),
            },
            ChatDelta::ToolCallArg {
                index: 1,
                arg: r#"{"path":"two.txt"}"#.to_string(),
            },
            ChatDelta::Finish {
                reason: FinishReason::ToolCalls,
                usage: None,
            },
        ],
        vec![
            ChatDelta::Text("finished".to_string()),
            ChatDelta::Finish {
                reason: FinishReason::Stop,
                usage: None,
            },
        ],
    ]);
    let mut state = AppState::new(ShellHistory::in_memory());
    let mut api_messages = vec![ChatMessage {
        role: "user".to_string(),
        content: crate::image_content::ModelContent::text("read both"),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }];

    let tools = selected_tools(&["read_file"]);
    run_agent_turn(
        &mut state,
        &backend,
        &tools,
        &mut api_messages,
        5,
        ("fake", "model"),
        temp_dir.path().to_string_lossy().as_ref(),
    )
    .await
    .unwrap();

    let requests = backend.requests.lock().unwrap();
    let second = &requests[1];
    assert_eq!(
        second
            .iter()
            .filter_map(|message| message.tool_call_id.as_deref())
            .collect::<Vec<_>>(),
        vec!["call-one", "call-two"]
    );
    let calls = second[1].tool_calls.as_ref().unwrap();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.id.as_str())
            .collect::<Vec<_>>(),
        vec!["call-one", "call-two"]
    );
}

#[tokio::test]
async fn agent_turn_errors_when_tool_rounds_exceed_limit() {
    let temp_dir = tempfile::tempdir().unwrap();
    std::fs::write(temp_dir.path().join("input.txt"), "hello").unwrap();
    let backend = ScriptedBackend::new(vec![vec![
        ChatDelta::ToolCallStart {
            index: 0,
            id: "call-limit".to_string(),
            name: "read_file".to_string(),
        },
        ChatDelta::ToolCallArg {
            index: 0,
            arg: r#"{"path":"input.txt"}"#.to_string(),
        },
        ChatDelta::Finish {
            reason: FinishReason::ToolCalls,
            usage: None,
        },
    ]]);
    let mut state = AppState::new(ShellHistory::in_memory());
    let mut api_messages = vec![ChatMessage {
        role: "user".to_string(),
        content: crate::image_content::ModelContent::text("keep using tools"),
        tool_calls: None,
        tool_call_id: None,
        response_items: Vec::new(),
    }];

    let tools = selected_tools(&["read_file"]);
    let error = run_agent_turn(
        &mut state,
        &backend,
        &tools,
        &mut api_messages,
        1,
        ("fake", "model"),
        temp_dir.path().to_string_lossy().as_ref(),
    )
    .await
    .expect_err("unfinished tool rounds must not silently succeed");

    assert!(error.to_string().contains("maximum agent turns"));
}

#[tokio::test]
async fn ca_script_runs_with_fake_backend() {
    let temp_dir = tempfile::tempdir().unwrap();
    let script_path = temp_dir.path().join("hermetic.ca");
    std::fs::write(
        &script_path,
        "type:hello\nkey:enter\nassert:contains:scripted response\n",
    )
    .unwrap();
    let backend = ScriptedBackend::new(vec![vec![
        ChatDelta::Text("scripted response".to_string()),
        ChatDelta::Finish {
            reason: FinishReason::Stop,
            usage: None,
        },
    ]]);
    let mut state = AppState::new(ShellHistory::in_memory());
    state.cwd = temp_dir.path().to_string_lossy().to_string();

    let state = run_script_with_backend(
        script_path.to_string_lossy().as_ref(),
        state,
        &backend,
        &[],
        ("fake", "model"),
        temp_dir.path().to_string_lossy().as_ref(),
        (120, 40),
    )
    .await
    .unwrap();

    assert_eq!(backend.requests.lock().unwrap().len(), 1);
    assert!(state
        .messages
        .iter()
        .any(|message| message.content == "scripted response"));
}
