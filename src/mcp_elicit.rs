//! Bounds for an MCP elicitation form. A server that keeps drawing refusals is cut off.

pub const MAX_ELICIT_FIELDS: usize = 16;
pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_SCHEMA_BYTES: usize = 8 * 1024;
pub const MAX_REFUSAL_NOTES: usize = 3;
pub const MAX_REASKS: u32 = 2;
const MAX_VISIBLE_OPTIONS: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    String,
    Number,
    Integer,
    Boolean,
    Enum,
    MultiSelect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitField {
    pub name: String,
    pub kind: FieldKind,
    pub options: Vec<String>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitForm {
    pub title: String,
    pub fields: Vec<ElicitField>,
    pub schema_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Refuse { note: String },
    CutOff,
}

/// Allow a form inside the caps. Refuse with a note, or cut the server off after
/// `MAX_REFUSAL_NOTES` earlier refusals.
pub fn decide(server_id: &str, form: &ElicitForm, prior_refusals: usize) -> Decision {
    if prior_refusals >= MAX_REFUSAL_NOTES {
        return Decision::CutOff;
    }
    if let Some(note) = bound_note(server_id, form) {
        return Decision::Refuse { note };
    }
    Decision::Allow
}

fn bound_note(server_id: &str, form: &ElicitForm) -> Option<String> {
    if form.fields.len() > MAX_ELICIT_FIELDS {
        return Some(format!(
            "server {server_id} sent {} fields; limit is {MAX_ELICIT_FIELDS}",
            form.fields.len()
        ));
    }
    let title_chars = form.title.chars().count();
    if title_chars > MAX_TITLE_CHARS {
        return Some(format!(
            "server {server_id} title is {title_chars} characters; limit is {MAX_TITLE_CHARS}"
        ));
    }
    if form.schema_bytes > MAX_SCHEMA_BYTES {
        return Some(format!(
            "server {server_id} schema is {} bytes; limit is {MAX_SCHEMA_BYTES}",
            form.schema_bytes
        ));
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub enum Conducted {
    Accept(serde_json::Value),
    Decline,
}

/// Ask each field, check the answer, and re-ask twice. A missing answer declines.
pub async fn conduct<F, Fut>(form: &ElicitForm, mut ask: F) -> Conducted
where
    F: FnMut(&ElicitField, u32) -> Fut,
    Fut: std::future::Future<Output = Option<crate::interaction::InteractionAnswer>>,
{
    if form
        .fields
        .iter()
        .any(|field| field.options.len() > MAX_VISIBLE_OPTIONS)
    {
        return Conducted::Decline;
    }
    let mut content = serde_json::Map::new();
    for field in &form.fields {
        let mut accepted = None;
        for attempt in 0..=MAX_REASKS {
            let Some(answer) = ask(field, attempt).await else {
                return Conducted::Decline;
            };
            match value_from_answer(field, &answer) {
                Ok(value) => {
                    accepted = Some(value);
                    break;
                }
                Err(_) if attempt == MAX_REASKS => return Conducted::Decline,
                Err(_) => {}
            }
        }
        if let Some(value) = accepted {
            content.insert(field.name.clone(), value);
        }
    }
    Conducted::Accept(serde_json::Value::Object(content))
}

pub fn form_from_schema(
    title: &str,
    schema: &serde_json::Value,
    schema_bytes: usize,
) -> Option<ElicitForm> {
    let properties = schema.get("properties")?.as_object()?;
    let required = schema
        .get("required")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut fields = Vec::new();
    for (name, spec) in properties {
        let (kind, options) = field_kind(spec)?;
        if options.len() > MAX_VISIBLE_OPTIONS {
            return None;
        }
        fields.push(ElicitField {
            name: name.clone(),
            kind,
            options,
            required: required.iter().any(|item| item == name),
        });
    }
    Some(ElicitForm {
        title: title.to_string(),
        fields,
        schema_bytes,
    })
}

fn field_kind(spec: &serde_json::Value) -> Option<(FieldKind, Vec<String>)> {
    let type_name = spec.get("type").and_then(serde_json::Value::as_str)?;
    match type_name {
        "string" => {
            let options = string_enum(spec);
            if options.is_empty() {
                Some((FieldKind::String, Vec::new()))
            } else {
                Some((FieldKind::Enum, options))
            }
        }
        "number" => Some((FieldKind::Number, Vec::new())),
        "integer" => Some((FieldKind::Integer, Vec::new())),
        "boolean" => Some((FieldKind::Boolean, Vec::new())),
        "array" => {
            let options = spec.get("items").map(string_enum).unwrap_or_default();
            if options.is_empty() {
                None
            } else {
                Some((FieldKind::MultiSelect, options))
            }
        }
        _ => None,
    }
}

fn string_enum(spec: &serde_json::Value) -> Vec<String> {
    let from_enum = spec
        .get("enum")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !from_enum.is_empty() {
        return from_enum;
    }
    for key in ["oneOf", "anyOf"] {
        let values = spec
            .get(key)
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("const").and_then(serde_json::Value::as_str))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !values.is_empty() {
            return values;
        }
    }
    Vec::new()
}

fn value_from_answer(
    field: &ElicitField,
    answer: &crate::interaction::InteractionAnswer,
) -> Result<serde_json::Value, ()> {
    match field.kind {
        FieldKind::String => {
            let text = answer.text.clone().unwrap_or_default();
            if field.required && text.trim().is_empty() {
                Err(())
            } else {
                Ok(serde_json::Value::String(text))
            }
        }
        FieldKind::Integer => {
            let text = answer.text.clone().unwrap_or_default();
            let number: i64 = text.trim().parse().map_err(|_| ())?;
            Ok(serde_json::json!(number))
        }
        FieldKind::Number => {
            let text = answer.text.clone().unwrap_or_default();
            let number: f64 = text.trim().parse().map_err(|_| ())?;
            if !number.is_finite() {
                return Err(());
            }
            Ok(serde_json::json!(number))
        }
        FieldKind::Boolean => {
            let selected = answer
                .selected
                .first()
                .map(|value| value.as_str())
                .or(answer.text.as_deref());
            match selected.map(str::to_ascii_lowercase).as_deref() {
                Some("yes" | "true") => Ok(serde_json::Value::Bool(true)),
                Some("no" | "false") => Ok(serde_json::Value::Bool(false)),
                _ => Err(()),
            }
        }
        FieldKind::Enum => {
            let value = answer
                .selected
                .first()
                .cloned()
                .or_else(|| answer.text.clone())
                .ok_or(())?;
            if field.options.iter().any(|option| option == &value) {
                Ok(serde_json::Value::String(value))
            } else {
                Err(())
            }
        }
        FieldKind::MultiSelect => {
            if answer.selected.is_empty() && field.required {
                return Err(());
            }
            if answer
                .selected
                .iter()
                .all(|value| field.options.iter().any(|option| option == value))
            {
                Ok(serde_json::Value::Array(
                    answer
                        .selected
                        .iter()
                        .cloned()
                        .map(serde_json::Value::String)
                        .collect(),
                ))
            } else {
                Err(())
            }
        }
    }
}

pub fn interaction_request(
    title: &str,
    field: &ElicitField,
    attempt: u32,
) -> Option<crate::interaction::InteractionRequest> {
    if field.options.len() > MAX_VISIBLE_OPTIONS {
        return None;
    }
    let question = if attempt == 0 {
        format!("{title}\n{}", field.name)
    } else {
        format!("{title}\n{} (try again)", field.name)
    };
    let (mode, choices) = match field.kind {
        FieldKind::String | FieldKind::Number | FieldKind::Integer => {
            (crate::interaction::InteractionMode::FreeText, Vec::new())
        }
        FieldKind::Boolean => (
            crate::interaction::InteractionMode::Confirmation,
            vec![choice("Yes", "yes"), choice("No", "no")],
        ),
        FieldKind::Enum => (
            crate::interaction::InteractionMode::SingleChoice,
            field
                .options
                .iter()
                .map(|option| choice(option, option))
                .collect(),
        ),
        FieldKind::MultiSelect => (
            crate::interaction::InteractionMode::MultiSelect,
            field
                .options
                .iter()
                .map(|option| choice(option, option))
                .collect(),
        ),
    };
    Some(crate::interaction::InteractionRequest {
        question,
        description: None,
        mode,
        choices,
        allow_other: false,
    })
}

fn choice(label: &str, value: &str) -> crate::interaction::InteractionChoice {
    crate::interaction::InteractionChoice {
        label: label.to_string(),
        value: Some(value.to_string()),
        description: None,
    }
}

struct PendingQuestion {
    title: String,
    field: ElicitField,
    attempt: u32,
    reply: tokio::sync::oneshot::Sender<Option<crate::interaction::InteractionAnswer>>,
}

struct Bridge {
    attached: std::sync::atomic::AtomicBool,
    notify: tokio::sync::Notify,
    idle: tokio::sync::Notify,
    slot: tokio::sync::Mutex<Option<PendingQuestion>>,
    shown: std::sync::Mutex<Option<crate::interaction::InteractionId>>,
}

fn bridge() -> &'static Bridge {
    static BRIDGE: std::sync::OnceLock<Bridge> = std::sync::OnceLock::new();
    BRIDGE.get_or_init(|| Bridge {
        attached: std::sync::atomic::AtomicBool::new(false),
        notify: tokio::sync::Notify::new(),
        idle: tokio::sync::Notify::new(),
        slot: tokio::sync::Mutex::new(None),
        shown: std::sync::Mutex::new(None),
    })
}

pub fn attach() {
    bridge()
        .attached
        .store(true, std::sync::atomic::Ordering::Release);
}

pub fn ui_attached() -> bool {
    bridge().attached.load(std::sync::atomic::Ordering::Acquire)
}

pub async fn ask_user(
    title: &str,
    field: &ElicitField,
    attempt: u32,
) -> Option<crate::interaction::InteractionAnswer> {
    if !ui_attached() {
        return None;
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut reply = Some(tx);
    loop {
        let notified = bridge().idle.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let mut slot = bridge().slot.lock().await;
        if slot.is_none() {
            *slot = Some(PendingQuestion {
                title: title.to_string(),
                field: field.clone(),
                attempt,
                reply: reply.take().expect("elicitation reply is queued once"),
            });
            drop(slot);
            bridge().notify.notify_one();
            break;
        }
        drop(slot);
        notified.await;
    }
    rx.await.ok().flatten()
}

pub async fn wait_for_question() {
    bridge().notify.notified().await;
}

pub async fn current_request() -> Option<crate::interaction::InteractionRequest> {
    let request = {
        let slot = bridge().slot.lock().await;
        let question = slot.as_ref()?;
        interaction_request(&question.title, &question.field, question.attempt)
    };
    if request.is_none() {
        submit(None);
    }
    request
}

pub fn arm_display(id: crate::interaction::InteractionId) {
    *bridge()
        .shown
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(id);
}

pub fn accepts(id: crate::interaction::InteractionId) -> bool {
    bridge()
        .shown
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .is_some_and(|shown| shown == id)
}

pub fn submit(answer: Option<crate::interaction::InteractionAnswer>) {
    *bridge()
        .shown
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = None;
    let question = bridge()
        .slot
        .try_lock()
        .ok()
        .and_then(|mut slot| slot.take());
    if let Some(question) = question {
        let _ = question.reply.send(answer);
    }
    bridge().idle.notify_one();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(fields: usize, title: &str, schema_bytes: usize) -> ElicitForm {
        ElicitForm {
            title: title.to_string(),
            fields: (0..fields)
                .map(|index| ElicitField {
                    name: format!("field-{index}"),
                    kind: FieldKind::String,
                    options: Vec::new(),
                    required: false,
                })
                .collect(),
            schema_bytes,
        }
    }

    #[test]
    fn a_normal_form_is_allowed() {
        let form = form(2, "Pick a value", 128);
        assert_eq!(decide("docs", &form, 0), Decision::Allow);
    }

    #[test]
    fn too_many_fields_is_refused() {
        let form = form(MAX_ELICIT_FIELDS + 1, "Too wide", 128);
        match decide("docs", &form, 0) {
            Decision::Refuse { note } => {
                assert!(note.contains("docs"), "{note}");
                assert!(note.contains(&MAX_ELICIT_FIELDS.to_string()), "{note}");
            }
            other => panic!("expected refuse, got {other:?}"),
        }
    }

    #[test]
    fn the_refusal_after_the_cap_is_cut_off() {
        let form = form(MAX_ELICIT_FIELDS + 4, "Again", 128);
        assert_eq!(decide("docs", &form, MAX_REFUSAL_NOTES), Decision::CutOff);
        assert_eq!(
            decide("docs", &form, MAX_REFUSAL_NOTES + 1),
            Decision::CutOff
        );
    }

    fn answer_text(text: &str) -> crate::interaction::InteractionAnswer {
        crate::interaction::InteractionAnswer {
            selected: Vec::new(),
            text: Some(text.to_string()),
        }
    }

    #[tokio::test]
    async fn mcp_elicitation_asks_validates_and_reasks_twice() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "count": {"type": "integer"},
                "color": {"type": "string", "enum": ["red", "blue"]}
            },
            "required": ["name", "count"]
        });
        let form = form_from_schema("Pick", &schema, 128).unwrap();
        let mut calls = 0_u32;
        let conducted = conduct(&form, |field, _attempt| {
            calls += 1;
            let reply = match field.name.as_str() {
                "name" if calls <= 2 => Some(answer_text("")),
                "name" => Some(answer_text("ada")),
                "count" if calls == 4 => Some(answer_text("nope")),
                "count" => Some(answer_text("3")),
                "color" => Some(crate::interaction::InteractionAnswer {
                    selected: vec!["blue".into()],
                    text: None,
                }),
                _ => None,
            };
            async move { reply }
        })
        .await;
        let Conducted::Accept(value) = conducted else {
            panic!("expected answers");
        };
        assert_eq!(value["name"], "ada");
        assert_eq!(value["count"], 3);
        assert_eq!(value["color"], "blue");
        assert!(calls >= 5);

        let mut refused = 0_u32;
        let declined = conduct(&form, |field, _| {
            refused += 1;
            let reply = (field.name == "name").then(|| answer_text(""));
            async move { reply }
        })
        .await;
        assert_eq!(declined, Conducted::Decline);
        assert_eq!(refused, MAX_REASKS + 1);

        let mut asked = false;
        let cancelled = conduct(&form, |_, _| {
            asked = true;
            async move { None }
        })
        .await;
        assert_eq!(cancelled, Conducted::Decline);
        assert!(asked);

        let wide = serde_json::json!({
            "type": "object",
            "properties": {
                "shade": {"type": "string", "enum": ["1","2","3","4","5","6","7","8","9","10"]}
            }
        });
        assert!(form_from_schema("Wide", &wide, 64).is_none());
    }
}
