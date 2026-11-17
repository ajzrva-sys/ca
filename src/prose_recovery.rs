//! One retry when the model repeats prose and made no tool call.

pub const MAX_REASONING_BYTES: usize = 8 * 1024;
pub const MAX_TEXT_BYTES: usize = 4 * 1024;
pub const REPEAT_REMINDER: &str =
    "The previous reply repeated itself and called no tool. Continue with new text.";
pub const REPETITION_MIN_CHARS: usize = 400;
pub const REPETITION_WINDOW: usize = 60;
pub const REPETITION_MARKER: &str =
    "[the reply degenerated into a repetition loop and was interrupted]";

/// SSE event the xAI Responses stream uses for a server-side loop report.
pub const DOOM_LOOP_CHECK_EVENT: &str = "response.doom_loop_check";
/// Opt-in header. The value is the detector window, in tokens.
pub const DOOM_LOOP_CHECK_HEADER: &str = "x-grok-doom-loop-check";
/// Sibling header for exact-repetition labels. The value is the minimum token run.
pub const EXACT_REPETITION_CHECK_HEADER: &str = "x-grok-exact-repetition-check";
pub const DOOM_LOOP_WINDOW_TOKENS: &str = "1024";
pub const EXACT_REPETITION_MIN_TOKENS: &str = "64";
/// `tail_repetition` thresholds at or below this, on the thinking channel, abort the attempt.
pub const DOOM_LOOP_MAX_THRESHOLD: u32 = 64;

/// xAI Responses requests ask the server to report loop checks. Other providers do not.
#[must_use]
pub fn xai_requests_doom_loop_check(provider_id: &str, url: &str) -> bool {
    matches!(provider_id, "xai" | "xai-oauth") && url.trim_end_matches('/').ends_with("/responses")
}

/// A thinking-channel tail loop tight enough to abort. Other labels are recorded and ignored.
#[must_use]
pub fn confident_thinking_loop(label: &str) -> bool {
    let Some((head, channel)) = label.split_once('@') else {
        return false;
    };
    if channel != "thinking" {
        return false;
    }
    let Some(threshold) = head.strip_prefix("tail_repetition:") else {
        return false;
    };
    threshold
        .parse::<u32>()
        .is_ok_and(|threshold| threshold <= DOOM_LOOP_MAX_THRESHOLD)
}

/// True when any trigger on this event or completed response is a confident thinking loop.
#[must_use]
pub fn doom_loop_is_confident(value: &serde_json::Value) -> bool {
    let triggers = value
        .pointer("/doom_loop_check/triggers")
        .or_else(|| value.pointer("/response/doom_loop_check/triggers"));
    triggers
        .and_then(serde_json::Value::as_array)
        .is_some_and(|triggers| {
            triggers
                .iter()
                .filter_map(serde_json::Value::as_str)
                .any(confident_thinking_loop)
        })
}

/// A streaming fragment of at least 400 characters whose trailing 60-character
/// window covers at least half the fragment.
pub fn streaming_repetition(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < REPETITION_MIN_CHARS {
        return false;
    }
    let window = REPETITION_WINDOW;
    let needle = &chars[chars.len() - window..];
    let mut covered = 0usize;
    let mut index = 0;
    while index + window <= chars.len() {
        if &chars[index..index + window] == needle {
            covered += window;
            index += window;
        } else {
            index += 1;
        }
    }
    covered * 2 >= chars.len()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub text: String,
    pub reasoning: String,
    pub repeated: bool,
    pub called_tool: bool,
    pub already_retried: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryOnce {
    Retry {
        reminder: &'static str,
        reasoning: String,
        text: String,
    },
    Drop,
}

/// One retry with a short reminder and capped reasoning and text.
/// A tool call is dropped so reasoning is not sent without its call.
/// `already_retried` is dropped so a second repeat is not another retry.
pub fn retry_once(turn: &Turn) -> RetryOnce {
    if turn.called_tool || turn.already_retried || !turn.repeated {
        return RetryOnce::Drop;
    }
    RetryOnce::Retry {
        reminder: REPEAT_REMINDER,
        reasoning: cap_bytes(&turn.reasoning, MAX_REASONING_BYTES),
        text: cap_bytes(&turn.text, MAX_TEXT_BYTES),
    }
}

fn cap_bytes(text: &str, max_bytes: usize) -> String {
    let mut end = text.len().min(max_bytes);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(text: &str, reasoning: &str, called_tool: bool, already_retried: bool) -> Turn {
        Turn {
            text: text.to_string(),
            reasoning: reasoning.to_string(),
            repeated: true,
            called_tool,
            already_retried,
        }
    }

    #[test]
    fn a_repeat_without_a_tool_retries_once_inside_the_byte_caps() {
        let text = "t".repeat(MAX_TEXT_BYTES + 512);
        let reasoning = "r".repeat(MAX_REASONING_BYTES + 512);
        let outcome = retry_once(&turn(&text, &reasoning, false, false));
        match outcome {
            RetryOnce::Retry {
                reminder,
                reasoning: kept_reasoning,
                text: kept_text,
            } => {
                assert_eq!(reminder, REPEAT_REMINDER);
                assert!(reminder.len() < 160, "{}", reminder.len());
                assert_eq!(kept_text.len(), MAX_TEXT_BYTES);
                assert!(kept_text.len() < text.len());
                assert_eq!(&text[..kept_text.len()], kept_text.as_str());
                assert_eq!(kept_reasoning.len(), MAX_REASONING_BYTES);
                assert!(kept_reasoning.len() < reasoning.len());
            }
            RetryOnce::Drop => panic!("expected one retry"),
        }
    }

    #[test]
    fn a_turn_that_called_a_tool_is_dropped() {
        let outcome = retry_once(&turn("same prose", "because", true, false));
        assert_eq!(outcome, RetryOnce::Drop);
    }

    #[test]
    fn a_second_repeat_is_not_another_retry() {
        let outcome = retry_once(&turn("same prose", "because", false, true));
        assert_eq!(outcome, RetryOnce::Drop);
    }

    #[test]
    fn a_streaming_loop_is_interrupted_and_a_long_reply_is_kept() {
        let unit = "abcdefghij".repeat(6);
        assert_eq!(unit.chars().count(), REPETITION_WINDOW);
        let looped = unit.repeat(8);
        assert!(looped.chars().count() >= REPETITION_MIN_CHARS);
        assert!(streaming_repetition(&looped));
        let unique = (0..80)
            .map(|index| format!("unique-word-{index}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(unique.chars().count() >= REPETITION_MIN_CHARS);
        assert!(!streaming_repetition(&unique));
        assert!(!streaming_repetition("short"));
    }

    #[test]
    fn a_tight_thinking_tail_is_confident_and_other_labels_are_not() {
        assert!(confident_thinking_loop("tail_repetition:8@thinking"));
        assert!(confident_thinking_loop("tail_repetition:64@thinking"));
        assert!(!confident_thinking_loop("tail_repetition:65@thinking"));
        assert!(!confident_thinking_loop("tail_repetition:4@response"));
        assert!(!confident_thinking_loop("exact_repetition:42x3@thinking"));
        assert!(!confident_thinking_loop("low_logprob@thinking"));
        assert!(!confident_thinking_loop("tail_repetition:4"));
        assert!(xai_requests_doom_loop_check(
            "xai-oauth",
            "https://api.x.ai/v1/responses"
        ));
        assert!(xai_requests_doom_loop_check(
            "xai",
            "https://api.x.ai/v1/responses/"
        ));
        assert!(!xai_requests_doom_loop_check(
            "openai",
            "https://api.openai.com/v1/responses"
        ));
        assert!(!xai_requests_doom_loop_check(
            "xai",
            "https://api.x.ai/v1/chat/completions"
        ));
        let check = serde_json::json!({
            "type": "response.doom_loop_check",
            "doom_loop_check": {"triggers": ["tail_repetition:4@response", "tail_repetition:8@thinking"]}
        });
        assert!(doom_loop_is_confident(&check));
        let loose = serde_json::json!({
            "response": {"doom_loop_check": {"triggers": "tail_repetition:8@thinking"}}
        });
        assert!(!doom_loop_is_confident(&loose));
    }
}
