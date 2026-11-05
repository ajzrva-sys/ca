//! Host-owned provenance for retrieved text; never inferred from ordinary prose.
use crate::agent::stream::ProviderError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievedSpan {
    pub start: usize,
    pub end: usize,
    /// Host-issued source receipt. Never derived from equal text or model input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

pub fn full_span(text: &str) -> Vec<RetrievedSpan> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![RetrievedSpan {
            source: None,
            start: 0,
            end: text.len(),
        }]
    }
}

pub fn model_with_evidence(prefix: &str, evidence: &str) -> crate::image_content::ModelContent {
    let text = format!("{prefix}{evidence}");
    let spans = if evidence.is_empty() {
        Vec::new()
    } else {
        vec![RetrievedSpan {
            source: None,
            start: prefix.len(),
            end: text.len(),
        }]
    };
    crate::image_content::ModelContent::text(text).with_retrieved_spans(spans)
}

pub fn without_retrieved(text: &str, spans: &[RetrievedSpan]) -> Result<String, ProviderError> {
    validate_spans(text, spans)?;
    let mut authored = String::new();
    let mut offset = 0;
    for span in spans {
        authored.push_str(&text[offset..span.start]);
        offset = span.end;
    }
    authored.push_str(&text[offset..]);
    Ok(authored)
}

pub fn validate_spans(text: &str, spans: &[RetrievedSpan]) -> Result<(), ProviderError> {
    let mut end = 0;
    for span in spans {
        if span.start < end
            || span.start >= span.end
            || span.end > text.len()
            || !text.is_char_boundary(span.start)
            || !text.is_char_boundary(span.end)
        {
            return Err(ProviderError::Protocol(
                "invalid retrieved-content provenance; request withheld".into(),
            ));
        }
        end = span.end;
    }
    Ok(())
}

fn json_string_boundaries(text: &str) -> Vec<(usize, usize)> {
    let mut boundaries = vec![(0, 1)];
    let mut quoted_offset = 1;
    for (offset, character) in text.char_indices() {
        quoted_offset += serde_json::to_string(&character.to_string()).unwrap().len() - 2;
        boundaries.push((offset + character.len_utf8(), quoted_offset));
    }
    boundaries
}

/// Restore JSON string values while retaining the exact quoted source ranges.
/// Property names are protocol/schema identifiers and are never restored.
pub(crate) fn restore_argument_json(raw: &str) -> Option<(String, Vec<RetrievedSpan>)> {
    fn write(value: &serde_json::Value, text: &mut String, spans: &mut Vec<RetrievedSpan>) {
        match value {
            serde_json::Value::String(value) => {
                let mut restored = value.clone();
                let source = crate::provider_filter::restore_view_with_spans(&mut restored);
                let offset = text.len();
                text.push_str(&serde_json::to_string(&restored).expect("string serializes"));
                let boundaries = json_string_boundaries(&restored);
                for span in source {
                    let quoted = |position| {
                        let index = boundaries
                            .binary_search_by_key(&position, |(decoded, _)| *decoded)
                            .expect("restored ranges are character boundaries");
                        offset + boundaries[index].1
                    };
                    spans.push(RetrievedSpan {
                        source: None,
                        start: quoted(span.start),
                        end: quoted(span.end),
                    });
                }
            }
            serde_json::Value::Array(values) => {
                text.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        text.push(',');
                    }
                    write(value, text, spans);
                }
                text.push(']');
            }
            serde_json::Value::Object(values) => {
                text.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if index > 0 {
                        text.push(',');
                    }
                    text.push_str(&serde_json::to_string(key).expect("key serializes"));
                    text.push(':');
                    write(value, text, spans);
                }
                text.push('}');
            }
            _ => text.push_str(&value.to_string()),
        }
    }
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let mut text = String::new();
    let mut spans = Vec::new();
    write(&value, &mut text, &mut spans);
    if spans.is_empty()
        && serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .as_ref()
            == Some(&value)
    {
        text = raw.to_string();
    }
    Some((text, spans))
}

fn argument_value_ranges(
    arguments: &str,
    spans: &[RetrievedSpan],
) -> Result<Vec<(String, Vec<RetrievedSpan>)>, ProviderError> {
    fn visit<'a>(
        value: &'a serde_json::Value,
        pointer: &str,
        offset: &mut usize,
        values: &mut Vec<(String, usize, &'a str)>,
    ) {
        match value {
            serde_json::Value::String(text) => {
                values.push((pointer.to_string(), *offset, text));
                *offset += serde_json::to_string(text).unwrap().len();
            }
            serde_json::Value::Array(items) => {
                *offset += 1;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        *offset += 1;
                    }
                    visit(item, &format!("{pointer}/{index}"), offset, values);
                }
                *offset += 1;
            }
            serde_json::Value::Object(items) => {
                *offset += 1;
                for (index, (key, item)) in items.iter().enumerate() {
                    if index > 0 {
                        *offset += 1;
                    }
                    *offset += serde_json::to_string(key).unwrap().len() + 1;
                    let key = key.replace('~', "~0").replace('/', "~1");
                    visit(item, &format!("{pointer}/{key}"), offset, values);
                }
                *offset += 1;
            }
            _ => *offset += value.to_string().len(),
        }
    }
    validate_spans(arguments, spans)?;
    if spans.is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value = serde_json::from_str(arguments).map_err(|_| {
        ProviderError::Protocol("retrieved argument provenance requires valid JSON".into())
    })?;
    if serde_json::to_string(&value).unwrap() != arguments {
        return Err(ProviderError::Protocol(
            "retrieved argument provenance requires canonical JSON".into(),
        ));
    }
    let mut values = Vec::new();
    let mut offset = 0;
    visit(&value, "", &mut offset, &mut values);
    let mut mapped = 0;
    let mut result = Vec::new();
    for (pointer, offset, text) in values {
        let end = offset + serde_json::to_string(text).unwrap().len() - 1;
        let mut local = Vec::new();
        let boundaries = json_string_boundaries(text);
        for span in spans {
            if span.start > offset && span.end <= end {
                let boundary = |position: usize| {
                    boundaries
                        .binary_search_by_key(&(position - offset), |(_, quoted)| *quoted)
                        .ok()
                        .map(|index| boundaries[index].0)
                };
                let start = boundary(span.start).ok_or_else(|| {
                    ProviderError::Protocol("retrieved argument starts inside a JSON escape".into())
                })?;
                let end = boundary(span.end).ok_or_else(|| {
                    ProviderError::Protocol("retrieved argument ends inside a JSON escape".into())
                })?;
                // JSON decoding changes the exact receipt bytes; derived
                // fragments retain retrieved status with opaque projection.
                local.push(RetrievedSpan {
                    source: None,
                    start,
                    end,
                });
                mapped += 1;
            }
        }
        if !local.is_empty() {
            result.push((pointer, local));
        }
    }
    if mapped != spans.len() {
        return Err(ProviderError::Protocol(
            "retrieved argument provenance is not a string value".into(),
        ));
    }
    Ok(result)
}

/// Validate that source ranges cover complete decoded string-value boundaries.
pub fn validate_argument_spans(
    arguments: &str,
    spans: &[RetrievedSpan],
) -> Result<(), ProviderError> {
    argument_value_ranges(arguments, spans).map(|_| ())
}

/// Recover decoded string-value ranges from canonical serialized arguments.
pub fn json_string_spans(
    arguments: &str,
    spans: &[RetrievedSpan],
    pointer: &str,
) -> Result<Vec<RetrievedSpan>, ProviderError> {
    Ok(argument_value_ranges(arguments, spans)?
        .into_iter()
        .find_map(|(path, ranges)| (path == pointer).then_some(ranges))
        .unwrap_or_default())
}

/// Re-project only restored source values; authored argument text stays readable.
pub fn project_argument_string(
    arguments: &str,
    spans: &[RetrievedSpan],
    filter: &crate::provider_filter::ProviderFilter,
) -> Result<String, ProviderError> {
    let ranges = argument_value_ranges(arguments, spans)?;
    if ranges.is_empty() {
        return Ok(arguments.to_string());
    }
    let mut value: serde_json::Value = serde_json::from_str(arguments)
        .map_err(|_| ProviderError::Protocol("retrieved arguments are not JSON".into()))?;
    for (pointer, spans) in ranges {
        let field = value
            .pointer_mut(&pointer)
            .ok_or_else(|| ProviderError::Protocol("retrieved argument value is missing".into()))?;
        let text = field.as_str().ok_or_else(|| {
            ProviderError::Protocol("retrieved argument value is not text".into())
        })?;
        let projected = crate::image_content::ModelContent::text(text)
            .with_retrieved_spans(spans)
            .project_retrieved(filter)?;
        *field =
            serde_json::Value::String(projected.as_str().expect("text stays text").to_string());
    }
    Ok(value.to_string())
}

pub fn append(text: &mut String, spans: &mut Vec<RetrievedSpan>, retrieved: &str) {
    if !retrieved.is_empty() {
        let start = text.len();
        text.push_str(retrieved);
        spans.push(RetrievedSpan {
            source: None,
            start,
            end: text.len(),
        });
    }
}

/// Rebase unchanged retrieved slices through a host-authored wrapper change.
/// Missing or duplicated evidence cannot silently become authored text.
pub fn rebase_spans(
    old: &str,
    new: &str,
    spans: &[RetrievedSpan],
) -> Result<Vec<RetrievedSpan>, ProviderError> {
    validate_spans(old, spans)?;
    let mut result = Vec::new();
    let mut after = 0;
    for span in spans {
        let slice = &old[span.start..span.end];
        let mut matches = new[after..].match_indices(slice);
        let Some((offset, _)) = matches.next() else {
            return Err(ProviderError::Protocol(
                "retrieved context changed without provenance; request withheld".into(),
            ));
        };
        if matches.next().is_some() {
            return Err(ProviderError::Protocol(
                "ambiguous retrieved context provenance; request withheld".into(),
            ));
        }
        let start = after + offset;
        after = start + slice.len();
        result.push(RetrievedSpan {
            source: span.source.clone(),
            start,
            end: after,
        });
    }
    Ok(result)
}

/// A complete host-style header is enough: old checkpoint quotes can end
/// before the closing delimiter. Unknown origin requires a fresh attachment.
pub fn has_legacy_attachment(text: &str) -> bool {
    const OPEN: &str = "\n\n--- Attached file: ";
    text.match_indices(OPEN).any(|(start, _)| {
        let tail = &text[start + OPEN.len()..];
        tail.find(" ---\n")
            .is_some_and(|end| end > 0 && !tail[..end].chars().any(char::is_control))
    })
}

pub fn checked_spans(
    text: &str,
    spans: Option<&[RetrievedSpan]>,
    enabled: bool,
) -> Result<Vec<RetrievedSpan>, ProviderError> {
    if let Some(spans) = spans {
        validate_spans(text, spans)?;
        return Ok(spans.to_vec());
    }
    if enabled && has_legacy_attachment(text) {
        return Err(ProviderError::Protocol("saved attachment has no source metadata; reattach its file before continuing (request withheld)".into()));
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argument_projection_preserves_json_escapes_and_rejects_invalid_origins() {
        let source = "Cobalt \"é\"\\\n雪";
        let quoted = serde_json::to_string(source).unwrap();
        let escaped = &quoted[1..quoted.len() - 1];
        let arguments = serde_json::json!({
            "task": format!("Authored before {source} authored after"),
            "nested": [{"a/b~": format!("Nested {source}")}]
        })
        .to_string();
        let spans: Vec<_> = arguments
            .match_indices(escaped)
            .map(|(start, value)| RetrievedSpan {
                source: None,
                start,
                end: start + value.len(),
            })
            .collect();
        assert_eq!(spans.len(), 2);
        let ranges = json_string_spans(&arguments, &spans, "/nested/0/a~1b~0").unwrap();
        let decoded: serde_json::Value = serde_json::from_str(&arguments).unwrap();
        let field = decoded
            .pointer("/nested/0/a~1b~0")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(&field[ranges[0].start..ranges[0].end], source);
        let filter = crate::provider_filter::ProviderFilter::compile(
            &crate::provider_filter::ProviderFilterConfig {
                enabled: true,
                page_stand_in: crate::provider_filter::PageStandIn::Latin,
                ..Default::default()
            },
        )
        .unwrap();
        let projected = project_argument_string(&arguments, &spans, &filter).unwrap();
        let projected: serde_json::Value = serde_json::from_str(&projected).unwrap();
        let task = projected["task"].as_str().unwrap();
        assert!(task.starts_with("Authored before "));
        assert!(task.ends_with(" authored after"));
        assert!(!task.contains("Cobalt"));
        let disabled =
            crate::provider_filter::ProviderFilter::compile(&Default::default()).unwrap();
        assert_eq!(
            project_argument_string(&arguments, &spans, &disabled).unwrap(),
            arguments
        );
        let key_start = arguments.find("nested").unwrap();
        assert!(project_argument_string(
            &arguments,
            &[RetrievedSpan {
                source: None,
                start: key_start,
                end: key_start + 6
            }],
            &disabled
        )
        .is_err());
        let escape_start = arguments.find("\\\"").unwrap() + 1;
        assert!(json_string_spans(
            &arguments,
            &[RetrievedSpan {
                source: None,
                start: escape_start,
                end: escape_start + 1
            }],
            "/task"
        )
        .is_err());
        assert_eq!(arguments, decoded.to_string());
    }

    #[test]
    fn spans_validate_boundaries_order_and_legacy_origins() {
        assert!(validate_spans(
            "éx",
            &[RetrievedSpan {
                source: None,
                start: 0,
                end: 2
            }]
        )
        .is_ok());
        for spans in [
            vec![RetrievedSpan {
                source: None,
                start: 1,
                end: 2,
            }],
            vec![RetrievedSpan {
                source: None,
                start: 0,
                end: 4,
            }],
            vec![
                RetrievedSpan {
                    source: None,
                    start: 0,
                    end: 2,
                },
                RetrievedSpan {
                    source: None,
                    start: 1,
                    end: 3,
                },
            ],
        ] {
            assert!(validate_spans("éx", &spans).is_err());
        }
        let old = "look\n\n--- Attached file: a ---\nbody\n--- End file ---";
        assert!(checked_spans(old, None, true).is_err());
        assert!(checked_spans(old, None, false).is_ok());
        assert!(checked_spans(old, Some(&[]), true).is_ok());
        let truncated = "look\n\n--- Attached file: a ---\nbody without its closing delimiter…";
        assert!(checked_spans(truncated, None, true).is_err());
        assert!(checked_spans(truncated, Some(&[]), true).is_ok());
        assert!(!has_legacy_attachment("\n\n--- Attached file: unfinished"));
        assert!(has_legacy_attachment(&format!(
            "\n\n--- Attached file: incomplete\n{truncated}"
        )));
    }
}
