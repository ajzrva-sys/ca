use crate::agent::stream::{ProviderError, ProviderEvent};

pub(crate) const MAX_STREAM_ANOMALY_STATE_BYTES: usize = 64 * 1024;
const MAX_TOOL_NAME_STATE_BYTES: usize = 4 * 1024;
const MAX_CHANNEL_STATE_BYTES: usize =
    (MAX_STREAM_ANOMALY_STATE_BYTES - MAX_TOOL_NAME_STATE_BYTES) / 2;
const MIN_REPEATED_SPAN_WORDS: usize = 10;
const MAX_REPEATED_SPAN_WORDS: usize = 96;
const MIN_PARAGRAPH_WORDS: usize = 12;
const MAX_PARAGRAPH_WORDS: usize = 192;

#[derive(Debug, Default)]
pub(crate) struct StreamAnomalyGuard {
    text: RollingText,
    reasoning: RollingText,
    advertised_tools: Vec<String>,
    advertised_tool_bytes: usize,
    saw_tool_event: bool,
}

impl StreamAnomalyGuard {
    pub(crate) fn new<'a>(
        advertised_tools: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, ProviderError> {
        let mut guard = Self::default();
        for name in advertised_tools {
            let normalized = name.trim().to_ascii_lowercase();
            if normalized.is_empty()
                || normalized.len() > 128
                || !normalized
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(ProviderError::Protocol(
                    "advertised tool names exceed stream anomaly guard limits".into(),
                ));
            }
            if guard.advertised_tools.contains(&normalized) {
                continue;
            }
            let Some(next) = guard.advertised_tool_bytes.checked_add(normalized.len()) else {
                return Err(ProviderError::Protocol(
                    "advertised tool names exceed stream anomaly guard limits".into(),
                ));
            };
            if next > MAX_TOOL_NAME_STATE_BYTES {
                return Err(ProviderError::Protocol(
                    "advertised tool names exceed stream anomaly guard limits".into(),
                ));
            }
            guard.advertised_tool_bytes = next;
            guard.advertised_tools.push(normalized);
        }
        Ok(guard)
    }

    pub(crate) fn inspect(&mut self, event: &ProviderEvent) -> Result<(), ProviderError> {
        use crate::agent::stream::StreamAnomalyKind;

        let repetition = match event {
            ProviderEvent::Text(text) => {
                self.text.append(text);
                detects_repetition(&self.text.visible_text())
            }
            ProviderEvent::Reasoning(reasoning) => {
                self.reasoning.append(reasoning);
                detects_repetition(&self.reasoning.visible_text())
            }
            ProviderEvent::ToolCallStart { .. } | ProviderEvent::ToolCallArg { .. } => {
                self.saw_tool_event = true;
                false
            }
            ProviderEvent::Finish { .. } | ProviderEvent::Done => {
                if !self.saw_tool_event
                    && !self.advertised_tools.is_empty()
                    && self
                        .text
                        .full_text()
                        .is_some_and(|text| suspected_tool_envelope(text, &self.advertised_tools))
                {
                    return Err(ProviderError::StreamAnomaly(
                        StreamAnomalyKind::SuspectedToolEnvelope,
                    ));
                }
                false
            }
            ProviderEvent::ResponseItem(_) | ProviderEvent::Usage(_) => false,
        };
        if repetition {
            return Err(ProviderError::StreamAnomaly(
                StreamAnomalyKind::RepetitionLoop,
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    fn state_bytes(&self) -> usize {
        self.text.raw.len() + self.reasoning.raw.len() + self.advertised_tool_bytes
    }
}

#[derive(Debug, Default)]
struct RollingText {
    raw: String,
    total_bytes: usize,
    start_in_fence: bool,
    start_at_line_start: bool,
}

impl RollingText {
    fn append(&mut self, text: &str) {
        if self.raw.is_empty() && self.total_bytes == 0 {
            self.start_at_line_start = true;
        }
        self.total_bytes = self.total_bytes.saturating_add(text.len());
        self.raw.push_str(text);
        if self.raw.len() <= MAX_CHANNEL_STATE_BYTES {
            return;
        }

        let excess = self.raw.len() - MAX_CHANNEL_STATE_BYTES;
        let mut boundary = excess;
        while !self.raw.is_char_boundary(boundary) {
            boundary += 1;
        }
        if let Some(newline) = self.raw[boundary..].find('\n') {
            boundary += newline + 1;
        }
        let removed = self.raw[..boundary].to_string();
        let state = scan_fences(
            &removed,
            self.start_in_fence,
            self.start_at_line_start,
            false,
        );
        self.start_in_fence = state.in_fence;
        self.start_at_line_start = state.at_line_start;
        self.raw.drain(..boundary);
        self.raw.shrink_to_fit();
    }

    fn visible_text(&self) -> String {
        scan_fences(
            &self.raw,
            self.start_in_fence,
            self.start_at_line_start,
            true,
        )
        .visible
    }

    fn full_text(&self) -> Option<&str> {
        (self.total_bytes == self.raw.len()).then_some(self.raw.as_str())
    }
}

#[derive(Debug, Default)]
struct FenceScan {
    in_fence: bool,
    at_line_start: bool,
    visible: String,
}

fn scan_fences(
    input: &str,
    mut in_fence: bool,
    mut at_line_start: bool,
    retain_visible: bool,
) -> FenceScan {
    let mut visible = String::new();
    for line in input.split_inclusive('\n') {
        let marker = at_line_start && is_fence_marker(line);
        if marker {
            in_fence = !in_fence;
        } else if !in_fence && retain_visible {
            visible.push_str(line);
        }
        at_line_start = line.ends_with('\n');
    }
    FenceScan {
        in_fence,
        at_line_start,
        visible,
    }
}

fn is_fence_marker(line: &str) -> bool {
    let line = line.trim_end_matches(['\r', '\n']);
    let leading_spaces = line.bytes().take_while(|byte| *byte == b' ').count();
    if leading_spaces > 3 {
        return false;
    }
    let marker = &line[leading_spaces..];
    marker.starts_with("```") || marker.starts_with("~~~")
}

#[derive(Debug)]
struct WordToken {
    normalized: String,
    start: usize,
    end: usize,
}

fn tokenize_words(input: &str) -> Vec<WordToken> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut normalized = String::new();
    for (index, character) in input.char_indices() {
        if character.is_alphanumeric() {
            start.get_or_insert(index);
            normalized.extend(character.to_lowercase());
        } else if let Some(token_start) = start.take() {
            tokens.push(WordToken {
                normalized: std::mem::take(&mut normalized),
                start: token_start,
                end: index,
            });
        }
    }
    if let Some(token_start) = start {
        tokens.push(WordToken {
            normalized,
            start: token_start,
            end: input.len(),
        });
    }
    tokens
}

fn detects_repetition(visible: &str) -> bool {
    detects_three_repeated_spans(visible) || detects_four_similar_paragraphs(visible)
}

fn detects_three_repeated_spans(visible: &str) -> bool {
    let tokens = tokenize_words(visible);
    let maximum = MAX_REPEATED_SPAN_WORDS.min(tokens.len() / 3);
    for span_words in MIN_REPEATED_SPAN_WORDS..=maximum {
        let end = tokens.len();
        let first = end - span_words * 3;
        let second = end - span_words * 2;
        let third = end - span_words;
        if token_values_equal(&tokens[first..second], &tokens[second..third])
            && token_values_equal(&tokens[second..third], &tokens[third..end])
        {
            let candidate = &visible[tokens[first].start..tokens[end - 1].end];
            if candidate.len() >= 144 && !looks_like_code(candidate) {
                return true;
            }
        }
    }
    false
}

fn token_values_equal(left: &[WordToken], right: &[WordToken]) -> bool {
    left.iter()
        .zip(right)
        .all(|(left, right)| left.normalized == right.normalized)
}

fn detects_four_similar_paragraphs(visible: &str) -> bool {
    let paragraphs = prose_paragraphs(visible);
    let Some(recent) = paragraphs.get(paragraphs.len().saturating_sub(4)..) else {
        return false;
    };
    if recent.len() != 4 || recent.iter().any(|paragraph| looks_like_code(paragraph)) {
        return false;
    }
    let normalized = recent
        .iter()
        .map(|paragraph| {
            tokenize_words(paragraph)
                .into_iter()
                .map(|token| token.normalized)
                .take(MAX_PARAGRAPH_WORDS + 1)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if normalized
        .iter()
        .any(|words| words.len() < MIN_PARAGRAPH_WORDS || words.len() > MAX_PARAGRAPH_WORDS)
    {
        return false;
    }
    normalized[1..]
        .iter()
        .all(|candidate| word_similarity(&normalized[0], candidate) >= 0.82)
}

fn prose_paragraphs(input: &str) -> Vec<&str> {
    let mut paragraphs = Vec::new();
    let mut start = 0usize;
    let mut cursor = 0usize;
    for line in input.split_inclusive('\n') {
        let next = cursor + line.len();
        if line.trim().is_empty() {
            let paragraph = input[start..cursor].trim();
            if !paragraph.is_empty() {
                paragraphs.push(paragraph);
            }
            start = next;
        }
        cursor = next;
    }
    let paragraph = input[start..].trim();
    if !paragraph.is_empty() {
        paragraphs.push(paragraph);
    }
    paragraphs
}

fn word_similarity(left: &[String], right: &[String]) -> f64 {
    let maximum = left.len().max(right.len());
    if maximum == 0 {
        return 1.0;
    }
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0usize; right.len() + 1];
    for (left_index, left_word) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_word) in right.iter().enumerate() {
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + usize::from(left_word != right_word));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    1.0 - previous[right.len()] as f64 / maximum as f64
}

fn looks_like_code(input: &str) -> bool {
    let non_whitespace = input
        .chars()
        .filter(|character| !character.is_whitespace())
        .count();
    if non_whitespace == 0 {
        return false;
    }
    let code_punctuation = input
        .chars()
        .filter(|character| {
            matches!(
                character,
                '{' | '}' | '[' | ']' | '(' | ')' | ';' | '=' | '|' | '`'
            )
        })
        .count();
    if code_punctuation * 100 >= non_whitespace * 8 {
        return true;
    }
    let lines = input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let code_lines = lines
        .iter()
        .filter(|line| {
            let line = line.trim();
            line.ends_with(';')
                || line.contains("=>")
                || line.contains("::")
                || line.starts_with("let ")
                || line.starts_with("fn ")
                || line.starts_with("{")
                || line.starts_with("<")
        })
        .count();
    lines.len() >= 2 && code_lines * 2 >= lines.len()
}

fn suspected_tool_envelope(text: &str, advertised_tools: &[String]) -> bool {
    let visible = scan_fences(text, false, true, true).visible;
    let trimmed = visible.trim();
    if trimmed.is_empty() {
        return false;
    }
    json_tool_envelope(trimmed, advertised_tools) || xml_tool_envelope(trimmed, advertised_tools)
}

fn json_tool_envelope(text: &str, advertised_tools: &[String]) -> bool {
    let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) else {
        return false;
    };
    let candidate = &text[start..=end];
    if !is_primary_envelope(text, start, end + 1) {
        return false;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(candidate) else {
        return false;
    };
    json_names_advertised_tool(&value, advertised_tools) && json_has_arguments(&value)
}

fn json_names_advertised_tool(value: &serde_json::Value, advertised_tools: &[String]) -> bool {
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
            (matches!(key.as_str(), "name" | "tool" | "tool_name")
                && value.as_str().is_some_and(|name| {
                    advertised_tools
                        .iter()
                        .any(|tool| name.eq_ignore_ascii_case(tool))
                }))
                || json_names_advertised_tool(value, advertised_tools)
        }),
        serde_json::Value::Array(items) => items
            .iter()
            .any(|item| json_names_advertised_tool(item, advertised_tools)),
        _ => false,
    }
}

fn json_has_arguments(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
            matches!(key.as_str(), "arguments" | "input" | "parameters")
                || json_has_arguments(value)
        }),
        serde_json::Value::Array(items) => items.iter().any(json_has_arguments),
        _ => false,
    }
}

fn xml_tool_envelope(text: &str, advertised_tools: &[String]) -> bool {
    let (Some(start), Some(end)) = (text.find('<'), text.rfind('>')) else {
        return false;
    };
    if !is_primary_envelope(text, start, end + 1) {
        return false;
    }
    let candidate = text[start..=end].to_ascii_lowercase();
    let dsml_shape = (candidate.contains("<|dsml|tool_calls>")
        || candidate.contains("<｜dsml｜tool_calls>"))
        && (candidate.contains("<|dsml|invoke name=\"")
            || candidate.contains("<｜dsml｜invoke name=\""));
    let tool_shape = dsml_shape
        || candidate.contains("<tool")
        || candidate.contains("<function")
        || candidate.contains("<name>");
    let arguments = candidate.contains("<argument")
        || candidate.contains("<input")
        || candidate.contains("<parameter")
        || candidate.contains("<|dsml|parameter")
        || candidate.contains("<｜dsml｜parameter");
    tool_shape
        && arguments
        && advertised_tools.iter().any(|tool| {
            candidate.contains(&format!(">{tool}<"))
                || candidate.contains(&format!("name=\"{tool}\""))
                || candidate.contains(&format!("name='{tool}'"))
        })
}

fn is_primary_envelope(text: &str, start: usize, end: usize) -> bool {
    let outside = text[..start].trim().len() + text[end..].trim().len();
    let candidate = end.saturating_sub(start);
    outside <= 128 && candidate.saturating_mul(4) >= text.len().saturating_mul(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::stream::{FinishReason, StreamAnomalyKind};

    fn finish() -> ProviderEvent {
        ProviderEvent::Finish {
            reason: FinishReason::Stop,
            usage: None,
        }
    }

    fn new_guard(tools: &[&str]) -> StreamAnomalyGuard {
        StreamAnomalyGuard::new(tools.iter().copied()).unwrap()
    }

    fn expect_kind(error: ProviderError, kind: StreamAnomalyKind) {
        assert_eq!(error, ProviderError::StreamAnomaly(kind));
        assert!(!error.to_string().contains("sentinel"));
    }

    #[test]
    fn fragmented_unicode_three_span_loop_is_stopped() {
        let mut guard = new_guard(&[]);
        let span = "A bounded explanation with Unicode 🙂 repeats because generation is stuck. ";
        let repeated = format!("{span}{span}{span}");
        let mut outcome = Ok(());
        for chunk in repeated.as_bytes().chunks(7) {
            if let Ok(text) = std::str::from_utf8(chunk) {
                outcome = guard.inspect(&ProviderEvent::Text(text.into()));
            } else {
                // Provider decoders only emit valid UTF-8 strings. Reassemble an
                // arbitrary byte split before presenting it to the guard.
                continue;
            }
            if outcome.is_err() {
                break;
            }
        }
        if outcome.is_ok() {
            let mut guard = new_guard(&[]);
            for fragment in [
                "A bounded explanation with Unicode ",
                "🙂 repeats because generation is stuck. ",
                span,
                span,
            ] {
                outcome = guard.inspect(&ProviderEvent::Text(fragment.into()));
                if outcome.is_err() {
                    break;
                }
            }
        }
        expect_kind(outcome.unwrap_err(), StreamAnomalyKind::RepetitionLoop);
    }

    #[test]
    fn four_near_identical_paragraphs_are_stopped() {
        let mut guard = new_guard(&[]);
        for paragraph in [
            "The provider keeps restating this detailed explanation about a stalled generation path and never reaches a conclusion.",
            "The provider keeps restating this detailed explanation about the stalled generation path and never reaches a conclusion.",
            "The provider keeps restating this detailed explanation about a stalled generation path, but never reaches a conclusion.",
            "The provider keeps restating this detailed explanation about a stalled generation path and never reaches its conclusion.",
        ] {
            let event = ProviderEvent::Text(format!("{paragraph}\n\n"));
            match guard.inspect(&event) {
                Ok(()) => {}
                Err(error) => {
                    expect_kind(error, StreamAnomalyKind::RepetitionLoop);
                    return;
                }
            }
        }
        panic!("four near-identical prose paragraphs were not stopped");
    }

    #[test]
    fn repeated_reasoning_is_guarded_independently_from_text() {
        let mut guard = new_guard(&[]);
        let span = "This internal analysis is cycling through the same bounded rationale without making any meaningful progress. ";
        guard
            .inspect(&ProviderEvent::Text("A safe partial answer. ".into()))
            .unwrap();
        guard
            .inspect(&ProviderEvent::Reasoning(format!("{span}{span}")))
            .unwrap();
        expect_kind(
            guard
                .inspect(&ProviderEvent::Reasoning(span.into()))
                .unwrap_err(),
            StreamAnomalyKind::RepetitionLoop,
        );
    }

    #[test]
    fn short_repetition_fences_and_ordinary_code_are_not_flagged() {
        let mut guard = new_guard(&[]);
        for text in [
            "no no no\n\n",
            "```json\n{\"name\":\"read_file\",\"arguments\":{}}\n```\n",
            "let result = values.iter().map(|value| value + 1).collect();\nlet result = values.iter().map(|value| value + 1).collect();\nlet result = values.iter().map(|value| value + 1).collect();\n",
        ] {
            guard.inspect(&ProviderEvent::Text(text.into())).unwrap();
        }
        guard.inspect(&finish()).unwrap();
    }

    #[test]
    fn advertised_json_and_xml_pseudo_tools_are_typed_anomalies() {
        for payload in [
            r#"{"name":"read_file","arguments":{"path":"sentinel"}}"#,
            r#"<tool_call><name>read_file</name><arguments>{"path":"sentinel"}</arguments></tool_call>"#,
        ] {
            let mut guard = new_guard(&["read_file"]);
            guard.inspect(&ProviderEvent::Text(payload.into())).unwrap();
            expect_kind(
                guard.inspect(&finish()).unwrap_err(),
                StreamAnomalyKind::SuspectedToolEnvelope,
            );
        }
    }

    #[test]
    fn non_normalized_dsml_tool_wrapper_is_a_typed_anomaly() {
        let payload = concat!(
            "<|DSML|tool_calls>\n",
            "<|DSML|invoke name=\"propose_plan\">\n",
            "<|DSML|parameter name=\"title\" string=\"true\">",
            "sentinel",
            "</|DSML|parameter>\n",
            "</|DSML|invoke>\n",
            "</|DSML|tool_calls>",
        );
        let mut guard = new_guard(&["propose_plan"]);
        guard.inspect(&ProviderEvent::Text(payload.into())).unwrap();
        expect_kind(
            guard.inspect(&finish()).unwrap_err(),
            StreamAnomalyKind::SuspectedToolEnvelope,
        );
    }

    #[test]
    fn prose_and_fenced_tool_examples_are_not_pseudo_tools() {
        for payload in [
            "A model might describe read_file in ordinary prose without calling it.",
            "```json\n{\"name\":\"read_file\",\"arguments\":{}}\n```",
            "Here is an example:\n\n{\"name\":\"read_file\",\"arguments\":{}}\n\nDo not execute it.",
            "{\"name\":\"other_tool\",\"arguments\":{}}",
            "{\"name\":\"read_file\",\"arguments\":",
        ] {
            let mut guard = new_guard(&["read_file"]);
            guard.inspect(&ProviderEvent::Text(payload.into())).unwrap();
            guard.inspect(&finish()).unwrap();
        }
    }

    #[test]
    fn an_unfenced_envelope_after_a_fenced_example_is_still_stopped() {
        let mut guard = new_guard(&["read_file"]);
        guard
            .inspect(&ProviderEvent::Text(
                "```text\nThis harmless fenced note is ignored.\n```\n".into(),
            ))
            .unwrap();
        guard
            .inspect(&ProviderEvent::Text(
                r#"{"name":"read_file","arguments":{}}"#.into(),
            ))
            .unwrap();
        expect_kind(
            guard.inspect(&finish()).unwrap_err(),
            StreamAnomalyKind::SuspectedToolEnvelope,
        );
    }

    #[test]
    fn real_provider_tool_event_disables_pseudo_tool_classification() {
        let mut guard = new_guard(&["read_file"]);
        guard
            .inspect(&ProviderEvent::ToolCallStart {
                index: 0,
                id: "call_1".into(),
                name: "read_file".into(),
            })
            .unwrap();
        guard
            .inspect(&ProviderEvent::Text(
                r#"{"name":"read_file","arguments":{}}"#.into(),
            ))
            .unwrap();
        guard.inspect(&finish()).unwrap();
    }

    #[test]
    fn detector_state_remains_bounded_for_large_streams() {
        let mut guard = new_guard(&["read_file"]);
        for index in 0..2_000 {
            guard
                .inspect(&ProviderEvent::Text(format!(
                    "let distinct_record_{index} = \"bounded-state-{index:08x}\";\n"
                )))
                .unwrap();
            assert!(guard.state_bytes() <= MAX_STREAM_ANOMALY_STATE_BYTES);
        }
    }

    #[test]
    fn oversized_advertised_tool_names_fail_closed_without_retaining_names() {
        let names = (0..128)
            .map(|index| format!("tool_{index:03}_{}", "x".repeat(40)))
            .collect::<Vec<_>>();
        let error = StreamAnomalyGuard::new(names.iter().map(String::as_str)).unwrap_err();
        assert_eq!(
            error,
            ProviderError::Protocol(
                "advertised tool names exceed stream anomaly guard limits".into()
            )
        );
        assert!(!error.to_string().contains("tool_"));
    }
}
