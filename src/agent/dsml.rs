use crate::agent::stream::{FinishReason, ProviderError, ProviderEvent, StreamAnomalyKind};
use crate::config::ToolCallFormat;
use serde_json::{Map, Value};

const MAX_DSML_BYTES: usize = 64 * 1024;
const MAX_DSML_CALLS: usize = 32;
const MAX_DSML_NAME_BYTES: usize = 128;

const ASCII_TOOL_CALLS_OPEN: &str = "<|DSML|tool_calls>";
const FULLWIDTH_TOOL_CALLS_OPEN: &str = "<｜DSML｜tool_calls>";
const ASCII_DSML_PREFIX: &str = "<|DSML|";
const FULLWIDTH_DSML_PREFIX: &str = "<｜DSML｜";

#[derive(Debug)]
pub(crate) struct DsmlStreamNormalizer {
    format: Option<ToolCallFormat>,
    text: TextState,
    buffer: String,
    passthrough_in_fence: bool,
    passthrough_at_line_start: bool,
    saw_native_tool: bool,
    converted: bool,
}

#[derive(Debug, Default)]
enum TextState {
    #[default]
    Prefix,
    Passthrough,
    Capturing,
}

impl DsmlStreamNormalizer {
    pub(crate) fn new(format: Option<ToolCallFormat>) -> Self {
        Self {
            format,
            text: TextState::Prefix,
            buffer: String::new(),
            passthrough_in_fence: false,
            passthrough_at_line_start: true,
            saw_native_tool: false,
            converted: false,
        }
    }

    pub(crate) fn normalize(
        &mut self,
        event: ProviderEvent,
    ) -> Result<Vec<ProviderEvent>, ProviderError> {
        match event {
            ProviderEvent::Text(text) => self.normalize_text(text),
            ProviderEvent::ToolCallStart { .. } | ProviderEvent::ToolCallArg { .. } => {
                if matches!(self.text, TextState::Capturing) || self.converted {
                    return Err(protocol_error("mixed native and DSML tool calls"));
                }
                self.saw_native_tool = true;
                if matches!(self.text, TextState::Prefix) {
                    let buffered = std::mem::take(self.buffer_mut());
                    if looks_like_incomplete_dsml(buffered.trim_start()) {
                        return Err(protocol_error("mixed native and DSML tool calls"));
                    }
                    if !buffered.is_empty() {
                        return Ok(vec![ProviderEvent::Text(buffered), event]);
                    }
                } else if matches!(self.text, TextState::Passthrough) && !self.buffer().is_empty() {
                    if !self.passthrough_in_fence && could_start_dsml(self.buffer()) {
                        return Err(protocol_error("mixed native and DSML tool calls"));
                    }
                    let buffered = std::mem::take(self.buffer_mut());
                    return Ok(vec![ProviderEvent::Text(buffered), event]);
                }
                Ok(vec![event])
            }
            ProviderEvent::Finish { reason, usage } => {
                if matches!(self.text, TextState::Prefix) {
                    let buffered = std::mem::take(self.buffer_mut());
                    if looks_like_incomplete_dsml(buffered.trim_start()) {
                        return Err(protocol_error("incomplete DSML tool wrapper"));
                    }
                    self.text = TextState::Passthrough;
                    let mut events = Vec::with_capacity(2);
                    if !buffered.is_empty() {
                        events.push(ProviderEvent::Text(buffered));
                    }
                    events.push(ProviderEvent::Finish { reason, usage });
                    return Ok(events);
                }
                if matches!(self.text, TextState::Passthrough) {
                    if !self.passthrough_in_fence && could_start_dsml(self.buffer()) {
                        return Err(protocol_error("incomplete DSML tool wrapper"));
                    }
                    let buffered = std::mem::take(self.buffer_mut());
                    let mut events = Vec::with_capacity(2);
                    if !buffered.is_empty() {
                        events.push(ProviderEvent::Text(buffered));
                    }
                    events.push(ProviderEvent::Finish { reason, usage });
                    return Ok(events);
                }
                if self.saw_native_tool || reason != FinishReason::Stop {
                    return Err(protocol_error("mixed native and DSML tool calls"));
                }
                if self.format != Some(ToolCallFormat::DeepseekDsml) {
                    return Err(ProviderError::StreamAnomaly(
                        StreamAnomalyKind::SuspectedToolEnvelope,
                    ));
                }
                let payload = std::mem::take(self.buffer_mut());
                let calls = parse_dsml(&payload)?;
                let mut events = Vec::with_capacity(calls.len() * 2 + 1);
                for (index, call) in calls.into_iter().enumerate() {
                    events.push(ProviderEvent::ToolCallStart {
                        index,
                        id: format!("call_dsml_{}", uuid::Uuid::new_v4().simple()),
                        name: call.name,
                    });
                    events.push(ProviderEvent::ToolCallArg {
                        index,
                        arg: call.arguments,
                    });
                }
                events.push(ProviderEvent::Finish {
                    reason: FinishReason::ToolCalls,
                    usage,
                });
                self.converted = true;
                self.text = TextState::Passthrough;
                Ok(events)
            }
            ProviderEvent::Done if matches!(self.text, TextState::Capturing) => {
                Err(protocol_error("incomplete DSML tool wrapper"))
            }
            _ => Ok(vec![event]),
        }
    }

    fn normalize_text(&mut self, text: String) -> Result<Vec<ProviderEvent>, ProviderError> {
        if self.converted {
            return Err(protocol_error("content followed a DSML tool wrapper"));
        }
        match self.text {
            TextState::Passthrough => self.normalize_passthrough(text),
            TextState::Capturing => {
                self.push_bounded(&text)?;
                Ok(Vec::new())
            }
            TextState::Prefix => {
                self.buffer_mut().push_str(&text);
                let candidate = self.buffer().trim_start();
                if starts_dsml_wrapper(candidate) {
                    if self.saw_native_tool {
                        return Err(protocol_error("mixed native and DSML tool calls"));
                    }
                    if self.buffer().len() > MAX_DSML_BYTES {
                        return Err(protocol_error("DSML tool wrapper exceeded its size bound"));
                    }
                    self.text = TextState::Capturing;
                    return Ok(Vec::new());
                }
                if candidate.is_empty() {
                    if self.buffer().len() <= MAX_DSML_BYTES {
                        return Ok(Vec::new());
                    }
                    let buffered = std::mem::take(self.buffer_mut());
                    self.text = TextState::Passthrough;
                    return self.normalize_passthrough(buffered);
                }
                if ASCII_TOOL_CALLS_OPEN.starts_with(candidate)
                    || FULLWIDTH_TOOL_CALLS_OPEN.starts_with(candidate)
                {
                    return Ok(Vec::new());
                }
                if candidate.starts_with("<|DSML|") || candidate.starts_with("<｜DSML｜") {
                    return Err(protocol_error("malformed DSML tool wrapper"));
                }
                let buffered = std::mem::take(self.buffer_mut());
                self.text = TextState::Passthrough;
                self.normalize_passthrough(buffered)
            }
        }
    }

    fn normalize_passthrough(&mut self, text: String) -> Result<Vec<ProviderEvent>, ProviderError> {
        self.buffer_mut().push_str(&text);
        let scan = scan_passthrough(
            self.buffer(),
            self.passthrough_in_fence,
            self.passthrough_at_line_start,
        );
        if scan.dsml {
            return Err(protocol_error(if self.saw_native_tool {
                "mixed native and DSML tool calls"
            } else {
                "DSML tool wrapper must be the complete assistant output"
            }));
        }
        self.passthrough_in_fence = scan.in_fence;
        self.passthrough_at_line_start = scan.at_line_start;
        if scan.emit_bytes == 0 {
            return Ok(Vec::new());
        }
        let retained = self.buffer_mut().split_off(scan.emit_bytes);
        let emitted = std::mem::replace(self.buffer_mut(), retained);
        Ok(vec![ProviderEvent::Text(emitted)])
    }

    fn push_bounded(&mut self, text: &str) -> Result<(), ProviderError> {
        if self.buffer().len().saturating_add(text.len()) > MAX_DSML_BYTES {
            return Err(protocol_error("DSML tool wrapper exceeded its size bound"));
        }
        self.buffer_mut().push_str(text);
        Ok(())
    }

    // The buffer is kept separately from the state discriminator so moving
    // between prefix/capture/pass-through states never copies private output.
    fn buffer(&self) -> &String {
        &self.buffer
    }

    fn buffer_mut(&mut self) -> &mut String {
        &mut self.buffer
    }
}

fn starts_dsml_wrapper(value: &str) -> bool {
    value.starts_with(ASCII_TOOL_CALLS_OPEN) || value.starts_with(FULLWIDTH_TOOL_CALLS_OPEN)
}

fn could_start_dsml(value: &str) -> bool {
    !value.is_empty()
        && (ASCII_DSML_PREFIX.starts_with(value) || FULLWIDTH_DSML_PREFIX.starts_with(value))
}

fn looks_like_incomplete_dsml(value: &str) -> bool {
    value.starts_with("<|DSML") || value.starts_with("<｜DSML")
}

#[derive(Debug)]
struct PassthroughScan {
    emit_bytes: usize,
    in_fence: bool,
    at_line_start: bool,
    dsml: bool,
}

fn scan_passthrough(input: &str, mut in_fence: bool, mut at_line_start: bool) -> PassthroughScan {
    let mut cursor = 0usize;
    while cursor < input.len() {
        let remaining = &input[cursor..];
        if at_line_start {
            let leading_spaces = remaining.bytes().take_while(|byte| *byte == b' ').count();
            if leading_spaces <= 3 {
                let marker = &remaining[leading_spaces..];
                if marker.starts_with("```") || marker.starts_with("~~~") {
                    in_fence = !in_fence;
                } else if marker.is_empty()
                    || "```".starts_with(marker)
                    || "~~~".starts_with(marker)
                {
                    return PassthroughScan {
                        emit_bytes: cursor,
                        in_fence,
                        at_line_start,
                        dsml: false,
                    };
                }
            }
        }
        if !in_fence {
            if remaining.starts_with(ASCII_DSML_PREFIX)
                || remaining.starts_with(FULLWIDTH_DSML_PREFIX)
            {
                return PassthroughScan {
                    emit_bytes: cursor,
                    in_fence,
                    at_line_start,
                    dsml: true,
                };
            }
            if could_start_dsml(remaining) {
                return PassthroughScan {
                    emit_bytes: cursor,
                    in_fence,
                    at_line_start,
                    dsml: false,
                };
            }
        }
        let character = remaining
            .chars()
            .next()
            .expect("cursor remains on a UTF-8 boundary");
        cursor += character.len_utf8();
        at_line_start = character == '\n';
    }
    PassthroughScan {
        emit_bytes: cursor,
        in_fence,
        at_line_start,
        dsml: false,
    }
}

#[derive(Debug)]
struct ParsedCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Copy)]
struct Syntax {
    tool_calls_open: &'static str,
    tool_calls_close: &'static str,
    invoke_open: &'static str,
    invoke_close: &'static str,
    parameter_open: &'static str,
    parameter_close: &'static str,
}

const ASCII: Syntax = Syntax {
    tool_calls_open: ASCII_TOOL_CALLS_OPEN,
    tool_calls_close: "</|DSML|tool_calls>",
    invoke_open: "<|DSML|invoke name=\"",
    invoke_close: "</|DSML|invoke>",
    parameter_open: "<|DSML|parameter name=\"",
    parameter_close: "</|DSML|parameter>",
};

const FULLWIDTH: Syntax = Syntax {
    tool_calls_open: FULLWIDTH_TOOL_CALLS_OPEN,
    tool_calls_close: "</｜DSML｜tool_calls>",
    invoke_open: "<｜DSML｜invoke name=\"",
    invoke_close: "</｜DSML｜invoke>",
    parameter_open: "<｜DSML｜parameter name=\"",
    parameter_close: "</｜DSML｜parameter>",
};

fn parse_dsml(payload: &str) -> Result<Vec<ParsedCall>, ProviderError> {
    let payload = payload.trim();
    let syntax = if payload.starts_with(ASCII.tool_calls_open) {
        ASCII
    } else if payload.starts_with(FULLWIDTH.tool_calls_open) {
        FULLWIDTH
    } else {
        return Err(protocol_error("malformed DSML tool wrapper"));
    };
    let body = payload
        .strip_prefix(syntax.tool_calls_open)
        .and_then(|payload| payload.strip_suffix(syntax.tool_calls_close))
        .ok_or_else(|| protocol_error("incomplete DSML tool wrapper"))?;
    let mut body = body;
    let mut calls = Vec::new();
    loop {
        body = body.trim_start();
        if body.is_empty() {
            break;
        }
        if calls.len() >= MAX_DSML_CALLS {
            return Err(protocol_error("DSML tool wrapper contains too many calls"));
        }
        let (name, rest) = take_named_open(body, syntax.invoke_open, "\">")?;
        validate_tool_name(name)?;
        let close = rest
            .find(syntax.invoke_close)
            .ok_or_else(|| protocol_error("incomplete DSML invocation"))?;
        let invocation = &rest[..close];
        body = &rest[close + syntax.invoke_close.len()..];
        calls.push(ParsedCall {
            name: name.to_string(),
            arguments: parse_parameters(invocation, syntax)?,
        });
    }
    if calls.is_empty() {
        return Err(protocol_error("DSML tool wrapper contains no calls"));
    }
    Ok(calls)
}

fn parse_parameters(mut input: &str, syntax: Syntax) -> Result<String, ProviderError> {
    let mut parameters = Map::new();
    loop {
        input = input.trim_start();
        if input.is_empty() {
            break;
        }
        let rest = input
            .strip_prefix(syntax.parameter_open)
            .ok_or_else(|| protocol_error("malformed DSML parameter"))?;
        let name_end = rest
            .find("\" string=\"")
            .ok_or_else(|| protocol_error("malformed DSML parameter"))?;
        let name = &rest[..name_end];
        validate_parameter_name(name)?;
        let rest = &rest[name_end + "\" string=\"".len()..];
        let (string_value, rest) = if let Some(rest) = rest.strip_prefix("true\">") {
            (true, rest)
        } else if let Some(rest) = rest.strip_prefix("false\">") {
            (false, rest)
        } else {
            return Err(protocol_error("invalid DSML parameter string mode"));
        };
        let close = rest
            .find(syntax.parameter_close)
            .ok_or_else(|| protocol_error("incomplete DSML parameter"))?;
        let raw = &rest[..close];
        input = &rest[close + syntax.parameter_close.len()..];
        let value = if string_value {
            Value::String(raw.to_string())
        } else {
            serde_json::from_str(raw.trim())
                .map_err(|_| protocol_error("DSML parameter contains invalid JSON"))?
        };
        if parameters.insert(name.to_string(), value).is_some() {
            return Err(protocol_error(
                "DSML invocation contains duplicate parameters",
            ));
        }
    }
    serde_json::to_string(&Value::Object(parameters))
        .map_err(|_| protocol_error("DSML arguments could not be encoded"))
}

fn take_named_open<'a>(
    input: &'a str,
    prefix: &str,
    suffix: &str,
) -> Result<(&'a str, &'a str), ProviderError> {
    let rest = input
        .strip_prefix(prefix)
        .ok_or_else(|| protocol_error("malformed DSML invocation"))?;
    let end = rest
        .find(suffix)
        .ok_or_else(|| protocol_error("malformed DSML invocation"))?;
    Ok((&rest[..end], &rest[end + suffix.len()..]))
}

fn validate_tool_name(name: &str) -> Result<(), ProviderError> {
    if name.is_empty()
        || name.len() > MAX_DSML_NAME_BYTES
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(protocol_error("DSML invocation has an invalid tool name"));
    }
    Ok(())
}

fn validate_parameter_name(name: &str) -> Result<(), ProviderError> {
    if name.is_empty()
        || name.len() > MAX_DSML_NAME_BYTES
        || name.chars().any(char::is_control)
        || name.contains(['"', '<', '>'])
    {
        return Err(protocol_error(
            "DSML invocation has an invalid parameter name",
        ));
    }
    Ok(())
}

fn protocol_error(detail: &str) -> ProviderError {
    ProviderError::Protocol(detail.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish() -> ProviderEvent {
        ProviderEvent::Finish {
            reason: FinishReason::Stop,
            usage: None,
        }
    }

    fn normalize(payload: &str) -> Result<Vec<ProviderEvent>, ProviderError> {
        let mut normalizer = DsmlStreamNormalizer::new(Some(ToolCallFormat::DeepseekDsml));
        assert!(normalizer
            .normalize(ProviderEvent::Text(payload.into()))?
            .is_empty());
        normalizer.normalize(finish())
    }

    #[test]
    fn screenshot_shaped_ascii_wrapper_becomes_one_structured_call() {
        let events = normalize(concat!(
            "<|DSML|tool_calls>\n",
            "<|DSML|invoke name=\"propose_plan\">\n",
            "<|DSML|parameter name=\"expected_revision\" string=\"false\">5</|DSML|parameter>\n",
            "<|DSML|parameter name=\"title\" string=\"true\">Protected-browser correctness</|DSML|parameter>\n",
            "</|DSML|invoke>\n",
            "</|DSML|tool_calls>",
        ))
        .unwrap();
        assert_eq!(events.len(), 3);
        let ProviderEvent::ToolCallStart { id, name, .. } = &events[0] else {
            panic!("missing tool start");
        };
        assert!(id.starts_with("call_dsml_"));
        assert_eq!(name, "propose_plan");
        let ProviderEvent::ToolCallArg { arg, .. } = &events[1] else {
            panic!("missing arguments");
        };
        assert_eq!(
            serde_json::from_str::<Value>(arg).unwrap(),
            serde_json::json!({
                "expected_revision": 5,
                "title": "Protected-browser correctness"
            })
        );
        assert!(matches!(
            events[2],
            ProviderEvent::Finish {
                reason: FinishReason::ToolCalls,
                ..
            }
        ));
    }

    #[test]
    fn fullwidth_markers_and_fragmented_prefix_normalize_in_order() {
        let payload = concat!(
            "<｜DSML｜tool_calls>",
            "<｜DSML｜invoke name=\"first\"><｜DSML｜parameter name=\"value\" string=\"true\">🧠</｜DSML｜parameter></｜DSML｜invoke>",
            "<｜DSML｜invoke name=\"second\"><｜DSML｜parameter name=\"items\" string=\"false\">[1,2]</｜DSML｜parameter></｜DSML｜invoke>",
            "</｜DSML｜tool_calls>",
        );
        let mut normalizer = DsmlStreamNormalizer::new(Some(ToolCallFormat::DeepseekDsml));
        for character in payload.chars() {
            assert!(normalizer
                .normalize(ProviderEvent::Text(character.to_string()))
                .unwrap()
                .is_empty());
        }
        let events = normalizer.normalize(finish()).unwrap();
        assert!(matches!(
            &events[0],
            ProviderEvent::ToolCallStart { name, .. } if name == "first"
        ));
        assert!(matches!(
            &events[2],
            ProviderEvent::ToolCallStart { name, .. } if name == "second"
        ));
        let ids = events
            .iter()
            .filter_map(|event| match event {
                ProviderEvent::ToolCallStart { id, .. } => Some(id),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn native_calls_and_fenced_examples_pass_through() {
        let fenced = "```text\n<|DSML|tool_calls></|DSML|tool_calls>\n```";
        let mut normalizer = DsmlStreamNormalizer::new(Some(ToolCallFormat::DeepseekDsml));
        assert_eq!(
            normalizer
                .normalize(ProviderEvent::Text(fenced.into()))
                .unwrap(),
            vec![ProviderEvent::Text(fenced.into())]
        );

        let mut native = DsmlStreamNormalizer::new(Some(ToolCallFormat::DeepseekDsml));
        let event = ProviderEvent::ToolCallStart {
            index: 0,
            id: "native".into(),
            name: "read_file".into(),
        };
        assert_eq!(native.normalize(event.clone()).unwrap(), vec![event]);
        assert!(native
            .normalize(ProviderEvent::Text(
                "<|DSML|tool_calls></|DSML|tool_calls>".into()
            ))
            .is_err());

        let mut native_after_ordinary_prefix =
            DsmlStreamNormalizer::new(Some(ToolCallFormat::DeepseekDsml));
        assert!(native_after_ordinary_prefix
            .normalize(ProviderEvent::Text("<".into()))
            .unwrap()
            .is_empty());
        let event = ProviderEvent::ToolCallStart {
            index: 0,
            id: "native-after-text".into(),
            name: "read_file".into(),
        };
        assert_eq!(
            native_after_ordinary_prefix
                .normalize(event.clone())
                .unwrap(),
            vec![ProviderEvent::Text("<".into()), event]
        );

        let mut ordinary_prefix = DsmlStreamNormalizer::new(Some(ToolCallFormat::DeepseekDsml));
        assert!(ordinary_prefix
            .normalize(ProviderEvent::Text("<".into()))
            .unwrap()
            .is_empty());
        assert!(matches!(
            ordinary_prefix
                .normalize(ProviderEvent::Finish {
                    reason: FinishReason::Stop,
                    usage: None,
                })
                .unwrap()
                .as_slice(),
            [ProviderEvent::Text(text), ProviderEvent::Finish { .. }] if text == "<"
        ));

        let ordinary = "x".repeat(MAX_DSML_BYTES + 1);
        let mut large = DsmlStreamNormalizer::new(Some(ToolCallFormat::DeepseekDsml));
        assert_eq!(
            large
                .normalize(ProviderEvent::Text(ordinary.clone()))
                .unwrap(),
            vec![ProviderEvent::Text(ordinary)]
        );
    }

    #[test]
    fn non_opted_in_malformed_duplicate_and_mixed_wrappers_fail_closed() {
        let payload = concat!(
            "<|DSML|tool_calls><|DSML|invoke name=\"read_file\">",
            "<|DSML|parameter name=\"path\" string=\"true\">a</|DSML|parameter>",
            "</|DSML|invoke></|DSML|tool_calls>",
        );
        let mut disabled = DsmlStreamNormalizer::new(None);
        assert!(disabled
            .normalize(ProviderEvent::Text(payload.into()))
            .unwrap()
            .is_empty());
        assert_eq!(
            disabled.normalize(finish()).unwrap_err(),
            ProviderError::StreamAnomaly(StreamAnomalyKind::SuspectedToolEnvelope)
        );

        for malformed in [
            "<|DSML|tool_calls><|DSML|invoke name=\"read_file\"></|DSML|invoke>",
            concat!(
                "<|DSML|tool_calls><|DSML|invoke name=\"read_file\">",
                "<|DSML|parameter name=\"path\" string=\"false\">not-json</|DSML|parameter>",
                "</|DSML|invoke></|DSML|tool_calls>"
            ),
            concat!(
                "<|DSML|tool_calls><|DSML|invoke name=\"read_file\">",
                "<|DSML|parameter name=\"path\" string=\"true\">a</|DSML|parameter>",
                "<|DSML|parameter name=\"path\" string=\"true\">b</|DSML|parameter>",
                "</|DSML|invoke></|DSML|tool_calls>"
            ),
        ] {
            assert!(normalize(malformed).is_err());
        }

        let mut mixed = DsmlStreamNormalizer::new(Some(ToolCallFormat::DeepseekDsml));
        mixed
            .normalize(ProviderEvent::Text(payload.into()))
            .unwrap();
        assert!(mixed
            .normalize(ProviderEvent::ToolCallStart {
                index: 0,
                id: "native".into(),
                name: "read_file".into(),
            })
            .is_err());
    }

    #[test]
    fn prose_mixed_dsml_is_withheld_and_fragmented_fenced_examples_remain_text() {
        let mut mixed = DsmlStreamNormalizer::new(Some(ToolCallFormat::DeepseekDsml));
        assert_eq!(
            mixed
                .normalize(ProviderEvent::Text("intro ".into()))
                .unwrap(),
            vec![ProviderEvent::Text("intro ".into())]
        );
        for character in "<|DSML".chars() {
            assert!(mixed
                .normalize(ProviderEvent::Text(character.to_string()))
                .unwrap()
                .is_empty());
        }
        let error = mixed
            .normalize(ProviderEvent::Text("|tool_calls>".into()))
            .unwrap_err();
        assert!(!error.to_string().contains("tool_calls"));

        let fenced = "```text\n<｜DSML｜tool_calls></｜DSML｜tool_calls>\n```";
        let mut example = DsmlStreamNormalizer::new(None);
        let mut rendered = String::new();
        for character in fenced.chars() {
            for event in example
                .normalize(ProviderEvent::Text(character.to_string()))
                .unwrap()
            {
                if let ProviderEvent::Text(text) = event {
                    rendered.push_str(&text);
                }
            }
        }
        for event in example.normalize(finish()).unwrap() {
            if let ProviderEvent::Text(text) = event {
                rendered.push_str(&text);
            }
        }
        assert_eq!(rendered, fenced);
    }

    #[test]
    fn oversized_wrapper_is_rejected_without_echoing_content() {
        let mut normalizer = DsmlStreamNormalizer::new(Some(ToolCallFormat::DeepseekDsml));
        assert!(normalizer
            .normalize(ProviderEvent::Text(ASCII_TOOL_CALLS_OPEN.into()))
            .unwrap()
            .is_empty());
        let error = normalizer
            .normalize(ProviderEvent::Text("sentinel".repeat(MAX_DSML_BYTES)))
            .unwrap_err();
        assert!(!error.to_string().contains("sentinel"));
    }
}
