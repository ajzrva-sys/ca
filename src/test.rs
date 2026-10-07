use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::backend::TestBackend;
use std::time::Duration;

use crate::agent;
use crate::app::{AppState, ApprovalMode, Message, ModelInfo, Role, ToolCall, ToolStatus};
use crate::config::CaConfig;

#[derive(Debug)]
enum TestCmd {
    Type(String),
    Key(KeyCode),
    WaitSecs(f32),
    SleepMs(u64),
    AssertContains(String),
    AssertNotContains(String),
    AssertTool(String),
}

fn parse_script(path: &str) -> Result<Vec<TestCmd>> {
    let content = std::fs::read_to_string(path)?;
    let mut cmds = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("type:") {
            cmds.push(TestCmd::Type(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix("key:") {
            let code = match rest {
                "enter" => KeyCode::Enter,
                "esc" => KeyCode::Esc,
                "tab" => KeyCode::Tab,
                "up" => KeyCode::Up,
                "down" => KeyCode::Down,
                "left" => KeyCode::Left,
                "right" => KeyCode::Right,
                "backspace" => KeyCode::Backspace,
                "space" => KeyCode::Char(' '),
                s if s.starts_with("char:") => {
                    let ch = s[5..].chars().next().unwrap_or(' ');
                    KeyCode::Char(ch)
                }
                _ => anyhow::bail!("Unknown key: {}", rest),
            };
            cmds.push(TestCmd::Key(code));
        } else if let Some(rest) = line.strip_prefix("wait:") {
            let secs: f32 = rest.trim_end_matches('s').parse()?;
            cmds.push(TestCmd::WaitSecs(secs));
        } else if let Some(rest) = line.strip_prefix("sleep:") {
            let ms: u64 = rest.trim_end_matches("ms").parse()?;
            cmds.push(TestCmd::SleepMs(ms));
        } else if let Some(rest) = line.strip_prefix("assert:contains:") {
            cmds.push(TestCmd::AssertContains(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix("assert:not_contains:") {
            cmds.push(TestCmd::AssertNotContains(rest.to_string()));
        } else if let Some(rest) = line.strip_prefix("assert:tool:") {
            cmds.push(TestCmd::AssertTool(rest.to_string()));
        } else {
            anyhow::bail!("Unknown command: {}", line);
        }
    }
    Ok(cmds)
}

#[allow(clippy::field_reassign_with_default)]
pub async fn run_test(
    script_path: &str,
    tool_policy: &crate::tool_selection::ToolSelectionPolicy,
) -> Result<()> {
    let mut config = CaConfig::default_config();
    // Script tests use only the caller's explicitly supplied environment.
    for pc in config.providers.values_mut() {
        pc.api_key = crate::config::expand_env_vars(&pc.api_key);
    }
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".into());

    let mut state = AppState::default();
    state.cwd = cwd.clone();
    state.model_list = crate::tui::input::build_model_list(&config.providers);
    state.current_model = Some(ModelInfo {
        id: "deepseek-chat".into(),
        provider: "deepseek".into(),
        display_name: "deepseek-chat (deepseek)".into(),
    });
    state.approval = ApprovalMode::RunEverything;

    let (provider_cfg, model_id) = crate::resolve_provider(&state, &config);
    let client = agent::AgentClient::with_request_store(
        "deepseek",
        &provider_cfg,
        &model_id,
        state.request_store.clone(),
    );
    let registry = agent::registry::ToolRegistry::builtins();
    let scripted_allowed = registry.specs().iter().filter_map(|spec| {
        (!spec.capabilities.iter().any(|capability| {
            matches!(
                capability,
                agent::registry::ToolCapability::Interaction
                    | agent::registry::ToolCapability::Subagent
            )
        }))
        .then_some(spec.name.as_str())
    });
    let tools = crate::tool_selection::resolve_tools(
        registry,
        agent::registry::ToolProfile::Main,
        tool_policy,
        &crate::tool_selection::ToolSelectionConstraints {
            skills_enabled: false,
            mcp_enabled: false,
            ..crate::tool_selection::ToolSelectionConstraints::default()
        }
        .restricted_to(scripted_allowed),
    )
    .map_err(|error| anyhow::anyhow!("tool selection failed: {error}"))?
    .into_definitions();

    run_script_with_backend(
        script_path,
        state,
        &client,
        &tools,
        ("deepseek", &model_id),
        &cwd,
        (120, 40),
    )
    .await?;
    Ok(())
}

async fn run_script_with_backend<B: agent::client::ChatBackend + ?Sized>(
    script_path: &str,
    mut state: AppState,
    client: &B,
    tools: &[agent::client::ToolDef],
    request_target: (&str, &str),
    cwd: &str,
    terminal_size: (u16, u16),
) -> Result<AppState> {
    let (provider, model) = request_target;
    let (width, height) = terminal_size;
    let cmds = parse_script(script_path)?;
    let backend = TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend)?;
    let system = crate::build_system_prompt(cwd, false);
    let mut api_messages =
        agent::AgentClient::build_messages(Some(&system), &state.messages, None, &[])?;
    let max_turns = 5;

    for cmd in &cmds {
        match cmd {
            TestCmd::Type(text) => {
                for ch in text.chars() {
                    state.input.push(ch);
                }
                state.input_cursor = state.input.len();
            }
            TestCmd::Key(code) => {
                // Dispatch through the input handler for proper slash command processing
                let event =
                    crate::tui::event::Event::Key(*code, crossterm::event::KeyModifiers::NONE);
                crate::tui::input::handle_event(&mut state, &event, cwd);

                if *code == KeyCode::Enter && !state.input.is_empty() {
                    let input = std::mem::take(&mut state.input);
                    state.input_cursor = 0;
                    state.input_history.push(input.clone());
                    state.messages.push(Message {
                        role: Role::User,
                        content: input.clone(),
                        tool_calls: None,
                        thinking: None,
                        timestamp: chrono::Utc::now(),
                        response_items: Vec::new(),
                    });
                    // Push user message to API messages
                    api_messages.push(agent::client::ChatMessage {
                        role: "user".into(),
                        content: crate::image_content::ModelContent::text(input),
                        tool_calls: None,
                        tool_call_id: None,
                        response_items: Vec::new(),
                    });
                    run_agent_turn(
                        &mut state,
                        client,
                        tools,
                        &mut api_messages,
                        max_turns,
                        (provider, model),
                        cwd,
                    )
                    .await?;
                }
            }
            TestCmd::WaitSecs(secs) => {
                tokio::time::sleep(Duration::from_secs_f32(*secs)).await;
            }
            TestCmd::SleepMs(ms) => {
                tokio::time::sleep(Duration::from_millis(*ms)).await;
            }
            TestCmd::AssertContains(text) => {
                let rendered = render_state(&mut terminal, &state)?;
                if !rendered.contains(text.as_str()) {
                    println!("FAIL: '{}' not found", text);
                    println!("Output:\n{}", rendered);
                    anyhow::bail!("assert:contains failed");
                }
                println!("  ok assert:contains:{}", text);
            }
            TestCmd::AssertNotContains(text) => {
                let rendered = render_state(&mut terminal, &state)?;
                if rendered.contains(text.as_str()) {
                    println!("FAIL: '{}' found (should not be)", text);
                    anyhow::bail!("assert:not_contains failed");
                }
                println!("  ok assert:not_contains:{}", text);
            }
            TestCmd::AssertTool(name) => {
                let found = state.messages.iter().any(|m| {
                    m.tool_calls
                        .as_ref()
                        .map(|tcs| tcs.iter().any(|tc| tc.name == *name))
                        .unwrap_or(false)
                });
                if !found {
                    anyhow::bail!("assert:tool '{}' was not called", name);
                }
                println!("  ok assert:tool:{}", name);
            }
        }
    }

    println!("all assertions passed");
    Ok(state)
}

async fn run_agent_turn<B: agent::client::ChatBackend + ?Sized>(
    state: &mut AppState,
    client: &B,
    tools: &[agent::client::ToolDef],
    api_messages: &mut Vec<agent::client::ChatMessage>,
    max_turns: usize,
    request_target: (&str, &str),
    cwd: &str,
) -> Result<()> {
    let (provider, model) = request_target;
    let advertised_tool_names = tools
        .iter()
        .map(|tool| tool.function.name.clone())
        .collect::<std::collections::HashSet<_>>();
    for _ in 0..max_turns {
        let prepared = crate::request::PreparedModelRequest::new(
            crate::request::RequestKind::Scripted,
            provider,
            model,
            api_messages.clone(),
            tools.to_vec(),
            crate::agent::client::ChatRequestOptions::default(),
        );
        let mut stream = client.stream_prepared(&prepared).await?;
        let turn = agent::stream::consume_stream(&mut stream, |_| {})
            .await
            .map_err(anyhow::Error::msg)?;
        let content = turn.content;
        let reasoning = turn.reasoning;
        let response_items = turn.response_items;
        let tool_calls: Vec<(usize, String, String, String)> = turn
            .tool_calls
            .into_iter()
            .map(|call| (call.index, call.id, call.name, call.arguments))
            .collect();

        if tool_calls.is_empty() {
            let message = Message {
                role: Role::Assistant,
                content,
                tool_calls: None,
                thinking: reasoning,
                response_items,
                timestamp: chrono::Utc::now(),
            };
            agent::client::append_provider_messages(api_messages, &message)?;
            state.messages.push(message);
            return Ok(());
        }

        let mut state_tcs = Vec::new();
        for tc in &tool_calls {
            if !advertised_tool_names.contains(&tc.2) {
                anyhow::bail!("provider attempted unavailable scripted tool: {}", tc.2);
            }
            let args: serde_json::Value =
                serde_json::from_str(&tc.3).unwrap_or(serde_json::json!({}));
            let execution = agent::tools::execute_tool(&tc.2, &args, cwd, 30).await;
            state_tcs.push(ToolCall {
                id: tc.1.clone(),
                name: tc.2.clone(),
                arguments: tc.3.clone(),
                status: if execution.is_success() {
                    ToolStatus::Done
                } else {
                    ToolStatus::Error
                },
                output: Some(execution.model_text().to_string()),
            });
        }

        let message = Message {
            role: Role::Assistant,
            content,
            tool_calls: Some(state_tcs),
            thinking: reasoning,
            response_items,
            timestamp: chrono::Utc::now(),
        };
        agent::client::append_provider_messages(api_messages, &message)?;
        state.messages.push(message);
    }

    anyhow::bail!("maximum agent turns reached before a final response")
}

fn render_state(terminal: &mut ratatui::Terminal<TestBackend>, state: &AppState) -> Result<String> {
    terminal.draw(|f| crate::tui::render(f, state))?;
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            text.push(cell.symbol().chars().next().unwrap_or(' '));
        }
        text.push('\n');
    }
    Ok(text)
}

#[cfg(test)]
#[path = "test_tests.rs"]
mod tests;
