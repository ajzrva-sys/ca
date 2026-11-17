//! One persistent browser session, several steps per call.
//! The first `#` line is the label the terminal shows.

use crate::agent::registry::ToolContext;
use crate::agent::registry::ToolExecutionResult;
use crate::browser::BrowserManager;

const MAX_CODE_BYTES: usize = 16 * 1024;
const MAX_STEPS: usize = 40;
const MAX_LABEL_CHARS: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserScript {
    pub label: Option<String>,
    pub steps: Vec<BrowserStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserStep {
    Goto(String),
    Wait,
    Page,
    Controls,
    Html(String),
    Click(String),
    ClickText(String),
    Fill { selector: String, text: String },
    Scroll(String),
    Press(String),
    Js(String),
    ClickAt { x: u32, y: u32 },
    Shot,
}

pub fn parse_script(code: &str) -> Result<BrowserScript, String> {
    if code.len() > MAX_CODE_BYTES {
        return Err("browser script exceeds 16 KiB".into());
    }
    if code.chars().any(char::is_control)
        && code
            .chars()
            .any(|ch| ch != '\n' && ch != '\r' && ch != '\t' && ch.is_control())
    {
        return Err("browser script contains a control character".into());
    }
    let mut label = None;
    let mut steps = Vec::new();
    let mut saw_command = false;
    for raw in code.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            if saw_command || label.is_some() {
                return Err("a browser step comment must be the first line".into());
            }
            let comment = comment.trim();
            if comment.is_empty() || comment.chars().count() > MAX_LABEL_CHARS {
                return Err("a browser step comment must be 1 to 60 characters".into());
            }
            label = Some(comment.to_string());
            continue;
        }
        saw_command = true;
        steps.push(parse_step(line)?);
        if steps.len() > MAX_STEPS {
            return Err("a browser script may contain at most 40 steps".into());
        }
    }
    if steps.is_empty() {
        return Err("browser script needs at least one step".into());
    }
    Ok(BrowserScript { label, steps })
}

fn parse_step(line: &str) -> Result<BrowserStep, String> {
    let (command, rest) = line
        .split_once(char::is_whitespace)
        .map(|(command, rest)| (command, rest.trim()))
        .unwrap_or((line, ""));
    match command {
        "goto" => require_rest(rest, "goto").map(BrowserStep::Goto),
        "wait" => empty(rest, "wait").map(|()| BrowserStep::Wait),
        "page" => empty(rest, "page").map(|()| BrowserStep::Page),
        "controls" => empty(rest, "controls").map(|()| BrowserStep::Controls),
        "html" => require_rest(rest, "html").map(BrowserStep::Html),
        "click" => parse_click(rest),
        "fill" => parse_fill(rest),
        "scroll" => require_rest(rest, "scroll").map(BrowserStep::Scroll),
        "press" => require_rest(rest, "press").map(BrowserStep::Press),
        "js" => require_rest(rest, "js").map(BrowserStep::Js),
        "click_at" => parse_click_at(rest),
        "shot" => empty(rest, "shot").map(|()| BrowserStep::Shot),
        other => Err(format!("unknown browser step {other}")),
    }
}

fn parse_click(rest: &str) -> Result<BrowserStep, String> {
    if let Some(label) = rest.strip_prefix("text ") {
        let label = label.trim();
        if label.is_empty() {
            return Err("click text needs the visible label".into());
        }
        return Ok(BrowserStep::ClickText(label.to_string()));
    }
    require_rest(rest, "click").map(BrowserStep::Click)
}

fn parse_fill(rest: &str) -> Result<BrowserStep, String> {
    let (selector, text) = rest
        .split_once(char::is_whitespace)
        .ok_or("fill needs a selector and text")?;
    let text = text.trim();
    if selector.is_empty() || text.is_empty() {
        return Err("fill needs a selector and text".into());
    }
    Ok(BrowserStep::Fill {
        selector: selector.to_string(),
        text: text.to_string(),
    })
}

fn parse_click_at(rest: &str) -> Result<BrowserStep, String> {
    let mut parts = rest.split_whitespace();
    let x = parts
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or("click_at needs x and y")?;
    let y = parts
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or("click_at needs x and y")?;
    if parts.next().is_some() || x > 4000 || y > 4000 {
        return Err("click_at coordinates must be within 4000".into());
    }
    Ok(BrowserStep::ClickAt { x, y })
}

fn require_rest(rest: &str, command: &str) -> Result<String, String> {
    if rest.is_empty() {
        Err(format!("{command} needs an argument"))
    } else {
        Ok(rest.to_string())
    }
}

fn empty(rest: &str, command: &str) -> Result<(), String> {
    if rest.is_empty() {
        Ok(())
    } else {
        Err(format!("{command} does not take an argument"))
    }
}

pub fn step_label_from_arguments(arguments: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let code = value.get("code").and_then(serde_json::Value::as_str)?;
    parse_script(code).ok()?.label
}

pub async fn run_script(
    manager: &BrowserManager,
    code: &str,
    session: Option<&str>,
    context: &ToolContext,
) -> ToolExecutionResult {
    let script = match parse_script(code) {
        Ok(script) => script,
        Err(error) => return ToolExecutionResult::Failure(error),
    };
    if let Err(error) = manager.claim_session(session).await {
        return ToolExecutionResult::Failure(error);
    }
    let started = manager
        .execute(&serde_json::json!({"action":"start"}), context)
        .await;
    if !started.is_success() {
        return started;
    }
    let mut log = Vec::new();
    let mut image = None;
    for step in &script.steps {
        if matches!(step, BrowserStep::Shot) {
            match manager.capture_image(context).await {
                Ok(captured) => {
                    log.push("shot: screenshot attached".to_string());
                    image = Some(captured);
                }
                Err(error) => {
                    log.push(format!("shot: {error}"));
                    return ToolExecutionResult::Failure(log.join("\n"));
                }
            }
            continue;
        }
        let args = match step_arguments(step) {
            Ok(args) => args,
            Err(error) => return ToolExecutionResult::Failure(error),
        };
        let result = manager.execute(&args, context).await;
        let text = result.model_text().to_string();
        if !result.is_success() {
            log.push(text);
            return ToolExecutionResult::Failure(log.join("\n"));
        }
        log.push(text);
    }
    let marker = log.join("\n");
    if let Some(image) = image {
        ToolExecutionResult::SuccessImage { marker, image }
    } else {
        ToolExecutionResult::Success(marker)
    }
}

fn step_arguments(step: &BrowserStep) -> Result<serde_json::Value, String> {
    Ok(match step {
        BrowserStep::Goto(url) => serde_json::json!({"action":"navigate","url":url}),
        BrowserStep::Wait => serde_json::json!({"action":"js","text":"document.readyState"}),
        BrowserStep::Page => serde_json::json!({"action":"snapshot"}),
        BrowserStep::Controls => serde_json::json!({"action":"controls"}),
        BrowserStep::Html(selector) => serde_json::json!({"action":"html","selector":selector}),
        BrowserStep::Click(selector) => serde_json::json!({"action":"click","selector":selector}),
        BrowserStep::ClickText(text) => serde_json::json!({"action":"click_text","text":text}),
        BrowserStep::Fill { selector, text } => {
            serde_json::json!({"action":"type","selector":selector,"text":text})
        }
        BrowserStep::Scroll(target) => {
            if matches!(target.as_str(), "up" | "down" | "top" | "bottom") {
                serde_json::json!({"action":"scroll","text":target})
            } else {
                serde_json::json!({"action":"scroll","selector":target})
            }
        }
        BrowserStep::Press(key) => serde_json::json!({"action":"press","text":key}),
        BrowserStep::Js(expression) => serde_json::json!({"action":"js","text":expression}),
        BrowserStep::ClickAt { x, y } => {
            serde_json::json!({"action":"click_at","text":format!("{x} {y}")})
        }
        BrowserStep::Shot => return Err("shot is handled by the script runner".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_comment_is_the_step_label_and_helpers_parse() {
        let script = parse_script(
            "# Open the case portal and accept terms\ngoto https://example.com/portal\nclick text Accept\nfill #user ada\nscroll down\npress Enter\njs document.title\nclick_at 12 40\nshot\n",
        )
        .unwrap();
        assert_eq!(
            script.label.as_deref(),
            Some("Open the case portal and accept terms")
        );
        assert!(matches!(script.steps[1], BrowserStep::ClickText(ref text) if text == "Accept"));
        assert!(matches!(
            &script.steps[2],
            BrowserStep::Fill { selector, text } if selector == "#user" && text == "ada"
        ));
        let arguments = serde_json::json!({
            "code": "# Open the case portal and accept terms\npage\n"
        })
        .to_string();
        assert_eq!(
            step_label_from_arguments(&arguments).as_deref(),
            Some("Open the case portal and accept terms")
        );
    }
}
