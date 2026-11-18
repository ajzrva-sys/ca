use crate::app::{PendingApproval, ResolvedToolResult};
use crossterm::event::{KeyCode, KeyModifiers};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

const MAX_MASKED_SECRET_BYTES: usize = 1024;
const MAX_FORM_QUESTIONS: usize = 5;
const MAX_FORM_OPTIONS: usize = 4;
const MAX_HEADER_CHARS: usize = 30;
const MAX_OPTION_CHARS: usize = 200;
const MAX_OPTION_DESCRIPTION_CHARS: usize = 400;
const RECOMMENDED_SUFFIX: &str = "(Recommended)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InteractionId(Uuid);

impl InteractionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for InteractionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct InteractionChoice {
    pub label: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

impl InteractionChoice {
    pub fn resolved_value(&self) -> String {
        self.value.clone().unwrap_or_else(|| self.label.clone())
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelInteractionKind {
    #[default]
    FreeText,
    SingleChoice,
    MultiSelect,
    Confirmation,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QuestionOptionArgs {
    /// Row label. Put the recommended option first.
    pub label: String,
    /// One line shown under the label.
    #[serde(default)]
    #[schemars(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QuestionItemArgs {
    /// Complete question. Do not list the options in this text.
    pub question: String,
    /// Short label, at most 30 characters.
    #[serde(default)]
    #[schemars(default)]
    pub header: Option<String>,
    /// Up to 4 choices. Omit this to ask for typed text.
    #[serde(default)]
    #[schemars(default)]
    pub options: Vec<QuestionOptionArgs>,
    /// Allow more than one listed choice. A typed answer is still available.
    #[serde(default)]
    #[schemars(default)]
    pub multiple: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RequestUserInputArgs {
    /// One question. Omit this when `questions` is set.
    #[serde(default)]
    #[schemars(default)]
    pub question: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub kind: ModelInteractionKind,
    #[serde(default)]
    pub choices: Vec<InteractionChoice>,
    #[serde(default)]
    pub allow_other: bool,
    /// One to five independent questions. Each one accepts a typed answer.
    #[serde(default)]
    #[schemars(default)]
    pub questions: Vec<QuestionItemArgs>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionMode {
    FreeText,
    SingleChoice,
    MultiSelect,
    Confirmation,
    Masked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionRequest {
    pub question: String,
    pub description: Option<String>,
    pub mode: InteractionMode,
    pub choices: Vec<InteractionChoice>,
    pub allow_other: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedUserInput {
    One(InteractionRequest),
    Form(QuestionForm),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormQuestion {
    pub header: Option<String>,
    pub request: InteractionRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionForm {
    pub questions: Vec<FormQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormLock {
    pub choices: Vec<String>,
    pub custom: Option<String>,
}

impl FormLock {
    pub fn from_answer(answer: &InteractionAnswer) -> Self {
        Self {
            choices: answer.selected.clone(),
            custom: answer
                .text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string),
        }
    }
}

impl QuestionForm {
    pub fn report(&self, locks: &[FormLock]) -> String {
        let mut lines = vec![
            "QUESTION".to_string(),
            "outcome=submitted".to_string(),
            format!("count={}", self.questions.len()),
        ];
        for (index, question) in self.questions.iter().enumerate() {
            let number = index + 1;
            let lock = locks.get(index);
            let status = if lock.is_some() {
                "answered"
            } else {
                "unanswered"
            };
            let choice = lock.map(|lock| lock.choices.join(", ")).unwrap_or_default();
            let custom = lock
                .and_then(|lock| lock.custom.clone())
                .unwrap_or_default();
            lines.push(format!(
                "{number}.header={}",
                question.header.clone().unwrap_or_default()
            ));
            lines.push(format!(
                "{number}.question={}",
                one_line(&question.request.question)
            ));
            lines.push(format!("{number}.status={status}"));
            lines.push(format!("{number}.choice={}", one_line(&choice)));
            lines.push(format!("{number}.custom={}", one_line(&custom)));
        }
        lines.join("\n")
    }
}

impl RequestUserInputArgs {
    pub fn prepare(self) -> Result<PreparedUserInput, String> {
        if !self.questions.is_empty() {
            return parse_form(self.questions).map(PreparedUserInput::Form);
        }
        InteractionRequest::from_model(self).map(PreparedUserInput::One)
    }
}

impl InteractionRequest {
    pub fn from_model(args: RequestUserInputArgs) -> Result<Self, String> {
        if !args.questions.is_empty() {
            return Err("questions must be prepared as one form".into());
        }
        let question = args.question.trim().to_string();
        if question.is_empty() {
            return Err("question must not be empty".into());
        }
        let mode = match args.kind {
            ModelInteractionKind::FreeText => InteractionMode::FreeText,
            ModelInteractionKind::SingleChoice => InteractionMode::SingleChoice,
            ModelInteractionKind::MultiSelect => InteractionMode::MultiSelect,
            ModelInteractionKind::Confirmation => InteractionMode::Confirmation,
        };
        let mut choices = args.choices;
        if mode == InteractionMode::Confirmation {
            if !choices.is_empty() {
                return Err("confirmation does not accept custom choices".into());
            }
            choices = vec![
                InteractionChoice {
                    label: "Yes".into(),
                    value: Some("yes".into()),
                    description: None,
                },
                InteractionChoice {
                    label: "No".into(),
                    value: Some("no".into()),
                    description: None,
                },
            ];
        }
        if matches!(
            mode,
            InteractionMode::SingleChoice | InteractionMode::MultiSelect
        ) && choices.is_empty()
        {
            return Err("choice interactions require at least one choice".into());
        }
        if mode == InteractionMode::FreeText && (!choices.is_empty() || args.allow_other) {
            return Err("free-text interactions do not accept choices or Other".into());
        }
        let allow_other = match mode {
            InteractionMode::SingleChoice | InteractionMode::MultiSelect => true,
            InteractionMode::Confirmation | InteractionMode::FreeText | InteractionMode::Masked => {
                false
            }
        };
        let visible = choices.len() + usize::from(allow_other);
        if visible > 9 {
            return Err("interaction choices are limited to nine visible items".into());
        }
        let mut values = std::collections::HashSet::new();
        for choice in &choices {
            if choice.label.trim().is_empty() {
                return Err("choice labels must not be empty".into());
            }
            if !values.insert(choice.resolved_value()) {
                return Err("choice values must be unique".into());
            }
        }
        Ok(Self {
            question,
            description: args.description,
            mode,
            choices,
            allow_other,
        })
    }

    pub fn from_legacy_question(question: String) -> Result<Self, String> {
        Self::from_model(RequestUserInputArgs {
            question,
            description: None,
            kind: ModelInteractionKind::FreeText,
            choices: Vec::new(),
            allow_other: false,
            questions: Vec::new(),
        })
    }

    pub fn host_masked(question: impl Into<String>, description: Option<String>) -> Self {
        Self {
            question: question.into(),
            description,
            mode: InteractionMode::Masked,
            choices: Vec::new(),
            allow_other: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingInteraction {
    pub id: InteractionId,
    pub tool_call_id: String,
    pub request: InteractionRequest,
    pub form: Option<QuestionForm>,
    pub write_access: Option<crate::write_access::WriteAccessRequest>,
}

impl PendingInteraction {
    pub fn model(tool_call_id: String, request: InteractionRequest) -> Self {
        Self {
            id: InteractionId::new(),
            tool_call_id,
            request,
            form: None,
            write_access: None,
        }
    }

    pub fn form(tool_call_id: String, form: QuestionForm) -> Self {
        let request = form.questions[0].request.clone();
        Self {
            id: InteractionId::new(),
            tool_call_id,
            request,
            form: Some(form),
            write_access: None,
        }
    }
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

fn strip_recommended(label: &str) -> String {
    let trimmed = label.trim();
    if trimmed.to_ascii_lowercase().ends_with("(recommended)")
        && trimmed.len() > RECOMMENDED_SUFFIX.len()
    {
        trimmed[..trimmed.len() - RECOMMENDED_SUFFIX.len()]
            .trim()
            .to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_form(items: Vec<QuestionItemArgs>) -> Result<QuestionForm, String> {
    if items.is_empty() || items.len() > MAX_FORM_QUESTIONS {
        return Err(format!(
            "questions must contain 1 to {MAX_FORM_QUESTIONS} items"
        ));
    }
    let mut questions = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let question = item.question.trim().to_string();
        if question.is_empty() {
            return Err(format!(
                "questions[{index}].question must be non-empty text"
            ));
        }
        let header = match item.header {
            Some(header) => {
                let header = header.trim().to_string();
                if header.is_empty() {
                    None
                } else if header.chars().count() > MAX_HEADER_CHARS {
                    return Err(format!(
                        "questions[{index}].header is longer than {MAX_HEADER_CHARS} characters"
                    ));
                } else {
                    Some(header)
                }
            }
            None => None,
        };
        if item.options.len() > MAX_FORM_OPTIONS {
            return Err(format!(
                "questions[{index}].options supports at most {MAX_FORM_OPTIONS} choices"
            ));
        }
        let mut seen = std::collections::HashSet::new();
        let mut labels = Vec::with_capacity(item.options.len());
        for (position, option) in item.options.into_iter().enumerate() {
            let label = option.label.trim().to_string();
            if label.is_empty() {
                return Err(format!(
                    "questions[{index}].options[{position}].label must be non-empty"
                ));
            }
            if label.chars().count() > MAX_OPTION_CHARS {
                return Err(format!(
                    "questions[{index}].options[{position}].label is longer than {MAX_OPTION_CHARS} characters"
                ));
            }
            let description = option
                .description
                .map(|description| description.trim().to_string())
                .filter(|description| !description.is_empty());
            if description.as_ref().is_some_and(|description| {
                description.chars().count() > MAX_OPTION_DESCRIPTION_CHARS
            }) {
                return Err(format!(
                    "questions[{index}].options[{position}].description is longer than {MAX_OPTION_DESCRIPTION_CHARS} characters"
                ));
            }
            if !seen.insert(label.clone()) {
                return Err(format!("questions[{index}].options must be unique"));
            }
            labels.push((label, description));
        }
        if labels.len() >= 2 {
            let first = &labels[0].0;
            if !first.to_ascii_lowercase().ends_with("(recommended)") {
                labels[0].0 = format!("{first} {RECOMMENDED_SUFFIX}");
            }
        }
        let allow_other = !labels.is_empty();
        let mode = if labels.is_empty() {
            InteractionMode::FreeText
        } else if item.multiple {
            InteractionMode::MultiSelect
        } else {
            InteractionMode::SingleChoice
        };
        let choices = labels
            .into_iter()
            .map(|(label, description)| InteractionChoice {
                value: Some(strip_recommended(&label)),
                label,
                description,
            })
            .collect();
        questions.push(FormQuestion {
            header,
            request: InteractionRequest {
                question,
                description: None,
                mode,
                choices,
                allow_other,
            },
        });
    }
    Ok(QuestionForm { questions })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingManualResolution {
    Approval(PendingApproval),
    Privilege(Box<crate::privilege::PendingPrivilegeApproval>),
    Interaction(PendingInteraction),
    Execution(PendingExecution),
}

impl PendingManualResolution {
    pub fn tool_call_id(&self) -> &str {
        match self {
            Self::Approval(approval) => &approval.tool_id,
            Self::Privilege(privilege) => &privilege.tool_id,
            Self::Interaction(interaction) => &interaction.tool_call_id,
            Self::Execution(execution) => &execution.tool_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingExecution {
    pub tool_name: String,
    pub tool_args: String,
    pub tool_id: String,
    pub network_allowed: bool,
    pub pre_resolved: Option<crate::agent::registry::ToolExecutionResult>,
}

#[derive(Debug, Clone, Default)]
pub struct ManualResolutionQueue {
    remaining: VecDeque<PendingManualResolution>,
    current: Option<PendingManualResolution>,
    total: usize,
    resolved: usize,
}

impl ManualResolutionQueue {
    pub fn begin(&mut self, pending: Vec<PendingManualResolution>) {
        self.clear();
        self.total = pending.len();
        self.remaining = pending.into();
        self.advance();
    }

    pub fn current(&self) -> Option<&PendingManualResolution> {
        self.current.as_ref()
    }

    pub fn current_privilege(&self) -> Option<&crate::privilege::PendingPrivilegeApproval> {
        match self.current.as_ref() {
            Some(PendingManualResolution::Privilege(privilege)) => Some(privilege),
            _ => None,
        }
    }

    pub fn replace_current(&mut self, replacement: PendingManualResolution) -> bool {
        if self.current.is_none() {
            return false;
        }
        self.current = Some(replacement);
        true
    }

    pub fn is_active(&self) -> bool {
        self.current.is_some() || !self.remaining.is_empty()
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.resolved, self.total)
    }

    pub fn resolve_tool(&mut self, tool_call_id: &str) -> bool {
        if self
            .current
            .as_ref()
            .map(PendingManualResolution::tool_call_id)
            != Some(tool_call_id)
        {
            return false;
        }
        self.resolved += 1;
        self.current = None;
        self.advance();
        true
    }

    pub fn resolve_interaction(&mut self, id: InteractionId) -> bool {
        let matches = matches!(
            self.current.as_ref(),
            Some(PendingManualResolution::Interaction(interaction)) if interaction.id == id
        );
        if !matches {
            return false;
        }
        self.resolved += 1;
        self.current = None;
        self.advance();
        true
    }

    pub fn clear(&mut self) {
        self.remaining.clear();
        self.current = None;
        self.total = 0;
        self.resolved = 0;
    }

    fn advance(&mut self) {
        self.current = self.remaining.pop_front();
    }
}

#[derive(Debug, Clone)]
pub struct ToolBatchOutcome {
    pub results: Vec<ResolvedToolResult>,
    pub pending: Vec<PendingManualResolution>,
    pub repository_revision_before: Option<String>,
    pub routed_capsule: Option<Box<RoutedCapsuleBatch>>,
}

#[derive(Debug, Clone)]
pub struct RoutedCapsuleBatch {
    pub capsule: codex_team_protocol::MechanisticEvidenceCapsuleV1,
    pub call_ids: Vec<String>,
    pub action_mapping: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InteractionAnswer {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

pub enum InteractionOutcome {
    Answer(InteractionAnswer),
    Cancelled,
    Secret(SecretHandle),
}

impl InteractionOutcome {
    pub fn model_result(&self) -> Result<String, &'static str> {
        match self {
            Self::Answer(answer) => {
                if let Some(report) = answer
                    .text
                    .as_deref()
                    .and_then(|text| text.strip_prefix('\u{0}'))
                {
                    return Ok(report.to_string());
                }
                serde_json::to_string(answer).map_err(|_| "serialization failed")
            }
            Self::Cancelled => Ok("Cancelled by user".into()),
            Self::Secret(_) => Err("secret interactions cannot produce model-visible results"),
        }
    }
}

pub struct InteractionResolution {
    pub id: InteractionId,
    pub tool_call_id: Option<String>,
    pub outcome: InteractionOutcome,
}

enum InteractionDraft {
    Text { value: String, cursor: usize },
    Secret(Vec<u8>),
}

impl std::fmt::Debug for InteractionDraft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text { value, cursor } => formatter
                .debug_struct("Text")
                .field("value", value)
                .field("cursor", cursor)
                .finish(),
            Self::Secret(_) => formatter.write_str("Secret([REDACTED])"),
        }
    }
}

impl Drop for InteractionDraft {
    fn drop(&mut self) {
        if let Self::Secret(chars) = self {
            chars.fill(0);
        }
    }
}

#[derive(Debug, Clone, Default)]
struct FormCursor {
    selected_index: usize,
    selected: Vec<usize>,
    draft: String,
    other_editing: bool,
}

#[derive(Debug)]
struct FormRuntime {
    questions: Vec<FormQuestion>,
    active: usize,
    locked: Vec<Option<FormLock>>,
    cursors: Vec<FormCursor>,
}

impl FormRuntime {
    fn new(form: QuestionForm) -> Self {
        let len = form.questions.len();
        Self {
            questions: form.questions,
            active: 0,
            locked: (0..len).map(|_| None).collect(),
            cursors: (0..len).map(|_| FormCursor::default()).collect(),
        }
    }

    fn report(&self) -> String {
        let locks = self
            .locked
            .iter()
            .map(|lock| {
                lock.clone().unwrap_or(FormLock {
                    choices: Vec::new(),
                    custom: None,
                })
            })
            .collect::<Vec<_>>();
        QuestionForm {
            questions: self.questions.clone(),
        }
        .report(&locks)
    }
}

#[derive(Debug)]
pub struct InteractionState {
    pub id: InteractionId,
    pub tool_call_id: Option<String>,
    pub request: InteractionRequest,
    pub header: Option<String>,
    pub custom_label: &'static str,
    pub progress: Option<(usize, usize)>,
    pub selected_index: usize,
    selected: Vec<usize>,
    draft: InteractionDraft,
    other_editing: bool,
    form: Option<FormRuntime>,
}

impl InteractionState {
    pub fn from_pending(pending: PendingInteraction) -> Self {
        let mut state = Self::new(pending.id, Some(pending.tool_call_id), pending.request);
        state.form = pending.form.map(FormRuntime::new);
        if state.form.is_some() {
            state.apply_active();
        }
        state
    }

    pub fn host(request: InteractionRequest) -> Self {
        Self::new(InteractionId::new(), None, request)
    }

    fn new(id: InteractionId, tool_call_id: Option<String>, request: InteractionRequest) -> Self {
        let draft = if request.mode == InteractionMode::Masked {
            InteractionDraft::Secret(Vec::new())
        } else {
            InteractionDraft::Text {
                value: String::new(),
                cursor: 0,
            }
        };
        Self {
            id,
            tool_call_id,
            request,
            header: None,
            custom_label: "Other",
            progress: None,
            selected_index: 0,
            selected: Vec::new(),
            draft,
            other_editing: false,
            form: None,
        }
    }

    fn apply_active(&mut self) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        let question = form.questions[form.active].clone();
        let saved = form.cursors[form.active].clone();
        self.request = question.request;
        self.header = question.header;
        self.custom_label = if self.request.allow_other {
            "Type your own answer"
        } else {
            "Other"
        };
        self.progress = Some((form.active, form.questions.len()));
        self.selected_index = saved.selected_index;
        self.selected = saved.selected;
        self.other_editing = saved.other_editing;
        if let InteractionDraft::Text { value, cursor } = &mut self.draft {
            *cursor = saved.draft.len();
            *value = saved.draft;
        }
    }

    fn save_active(&mut self) {
        let draft = match &self.draft {
            InteractionDraft::Text { value, .. } => value.clone(),
            InteractionDraft::Secret(_) => String::new(),
        };
        let selected_index = self.selected_index;
        let selected = self.selected.clone();
        let other_editing = self.other_editing;
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let cursor = &mut form.cursors[form.active];
        cursor.selected_index = selected_index;
        cursor.selected = selected;
        cursor.draft = draft;
        cursor.other_editing = other_editing;
    }

    fn shift_form(&mut self, delta: isize) {
        let len = self
            .form
            .as_ref()
            .map(|form| form.questions.len())
            .unwrap_or(0);
        if len < 2 {
            return;
        }
        self.save_active();
        let active = self.form.as_ref().expect("form length was checked").active as isize;
        let next = (active + delta).rem_euclid(len as isize) as usize;
        self.form.as_mut().expect("form length was checked").active = next;
        self.apply_active();
    }

    fn form_switch(&mut self, code: KeyCode, modifiers: KeyModifiers) -> bool {
        let Some(form) = self.form.as_ref() else {
            return false;
        };
        if form.questions.len() < 2 {
            return false;
        }
        if !(modifiers.is_empty() || modifiers == KeyModifiers::ALT) {
            return false;
        }
        let editing = self.other_editing
            || matches!(
                self.request.mode,
                InteractionMode::FreeText | InteractionMode::Masked
            );
        let switching = matches!(code, KeyCode::Left | KeyCode::Right)
            && (modifiers.contains(KeyModifiers::ALT) || !editing);
        if !switching {
            return false;
        }
        self.shift_form(if code == KeyCode::Right { 1 } else { -1 });
        true
    }

    fn fold_form(&mut self, resolution: InteractionResolution) -> Option<InteractionResolution> {
        if self.form.is_none() {
            return Some(resolution);
        }
        let InteractionOutcome::Answer(answer) = resolution.outcome else {
            return Some(resolution);
        };
        let lock = FormLock::from_answer(&answer);
        if lock.choices.is_empty() && lock.custom.is_none() {
            return None;
        }
        self.save_active();
        let report = {
            let form = self.form.as_mut().expect("form is present");
            form.locked[form.active] = Some(lock);
            if form.locked.iter().all(Option::is_some) {
                Some(form.report())
            } else {
                form.active = form
                    .locked
                    .iter()
                    .position(Option::is_none)
                    .expect("an unlocked question remains");
                None
            }
        };
        if let Some(report) = report {
            return Some(
                self.resolution(InteractionOutcome::Answer(InteractionAnswer {
                    selected: Vec::new(),
                    text: Some(format!("\u{0}{report}")),
                })),
            );
        }
        self.apply_active();
        None
    }

    pub fn visible_count(&self) -> usize {
        self.request.choices.len() + usize::from(self.request.allow_other)
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    pub fn other_editing(&self) -> bool {
        self.other_editing
    }

    /// Insert pasted text into the open answer. A choice list ignores the paste.
    pub fn insert_text(&mut self, text: &str) -> bool {
        if !self.is_editor_mode() {
            return false;
        }
        let text = crate::terminal_text::sanitize_terminal_text(text);
        match &mut self.draft {
            InteractionDraft::Text { value, cursor } => {
                let clean: String = text
                    .chars()
                    .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
                    .collect();
                if clean.is_empty() {
                    return false;
                }
                value.insert_str(*cursor, &clean);
                *cursor += clean.len();
                true
            }
            InteractionDraft::Secret(bytes) => {
                let mut inserted = false;
                for character in text.chars().filter(|character| !character.is_control()) {
                    let mut encoded_bytes = [0_u8; 4];
                    let encoded = character.encode_utf8(&mut encoded_bytes);
                    if bytes.len().saturating_add(encoded.len()) > MAX_MASKED_SECRET_BYTES {
                        break;
                    }
                    bytes.extend_from_slice(encoded.as_bytes());
                    inserted = true;
                    encoded_bytes.fill(0);
                }
                inserted
            }
        }
    }

    pub fn draft_display(&self) -> String {
        match &self.draft {
            InteractionDraft::Text { value, .. } => value.clone(),
            InteractionDraft::Secret(bytes) => "•".repeat(
                std::str::from_utf8(bytes)
                    .expect("masked interaction bytes originate as UTF-8")
                    .chars()
                    .count(),
            ),
        }
    }

    pub fn handle_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Option<InteractionResolution> {
        if self.form_switch(code, modifiers) {
            return None;
        }
        let resolution = self.handle_key_once(code, modifiers)?;
        self.fold_form(resolution)
    }

    fn handle_key_once(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Option<InteractionResolution> {
        if code == KeyCode::Esc {
            if self.other_editing {
                self.other_editing = false;
                self.clear_text_draft();
                return None;
            }
            return Some(self.resolution(InteractionOutcome::Cancelled));
        }
        if self.parks_on_escape()
            && ((code == KeyCode::Tab && modifiers.is_empty())
                || (code == KeyCode::BackTab && modifiers.contains(KeyModifiers::SHIFT)))
        {
            self.step_choice(code == KeyCode::Tab);
            return None;
        }

        if self.is_editor_mode() {
            return self.handle_editor_key(code, modifiers);
        }

        let count = self.visible_count();
        match code {
            KeyCode::Up if count > 0 => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            KeyCode::Down if count > 0 => {
                self.selected_index = (self.selected_index + 1).min(count - 1);
            }
            KeyCode::Char('y' | 'Y')
                if self.request.mode == InteractionMode::Confirmation
                    && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) =>
            {
                if let Some(index) = self.choice_index("yes") {
                    self.selected_index = index;
                    return self.activate_choice();
                }
            }
            KeyCode::Char('n' | 'N')
                if self.request.mode == InteractionMode::Confirmation
                    && (modifiers.is_empty() || modifiers == KeyModifiers::SHIFT) =>
            {
                if let Some(index) = self.choice_index("no") {
                    self.selected_index = index;
                    return self.activate_choice();
                }
            }
            KeyCode::Char(character @ '1'..='9') if modifiers.is_empty() => {
                let index = character.to_digit(10).unwrap() as usize - 1;
                if index < count {
                    self.selected_index = index;
                    if self.request.mode == InteractionMode::MultiSelect {
                        self.toggle_selected(index);
                        return None;
                    }
                    return self.activate_choice();
                }
            }
            KeyCode::Char(' ') if self.request.mode == InteractionMode::MultiSelect => {
                self.toggle_selected(self.selected_index);
            }
            KeyCode::Enter => return self.activate_choice(),
            _ => {}
        }
        None
    }

    fn choice_index(&self, value: &str) -> Option<usize> {
        self.request
            .choices
            .iter()
            .position(|choice| choice.resolved_value() == value)
    }

    pub fn parks_on_escape(&self) -> bool {
        !self.other_editing() && !self.is_editor_mode()
    }

    pub fn step_choice(&mut self, forward: bool) {
        let count = self.visible_count();
        if count == 0 {
            return;
        }
        if forward {
            self.selected_index = (self.selected_index + 1) % count;
        } else {
            self.selected_index = if self.selected_index == 0 {
                count - 1
            } else {
                self.selected_index - 1
            };
        }
    }

    fn is_editor_mode(&self) -> bool {
        matches!(
            self.request.mode,
            InteractionMode::FreeText | InteractionMode::Masked
        ) || self.other_editing
    }

    fn handle_editor_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Option<InteractionResolution> {
        match (&mut self.draft, code) {
            (InteractionDraft::Text { value, cursor }, KeyCode::Char(character))
                if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
            {
                value.insert(*cursor, character);
                *cursor += character.len_utf8();
            }
            (InteractionDraft::Secret(bytes), KeyCode::Char(character))
                if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT =>
            {
                let mut encoded_bytes = [0_u8; 4];
                let encoded = character.encode_utf8(&mut encoded_bytes);
                if bytes.len().saturating_add(encoded.len()) <= MAX_MASKED_SECRET_BYTES {
                    bytes.extend_from_slice(encoded.as_bytes());
                }
                encoded_bytes.fill(0);
            }
            (InteractionDraft::Text { value, cursor }, KeyCode::Backspace) => {
                if *cursor > 0 {
                    let previous = value[..*cursor]
                        .char_indices()
                        .next_back()
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                    value.replace_range(previous..*cursor, "");
                    *cursor = previous;
                }
            }
            (InteractionDraft::Secret(bytes), KeyCode::Backspace) => {
                if let Ok(text) = std::str::from_utf8(bytes) {
                    let next_len = text
                        .char_indices()
                        .next_back()
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                    bytes[next_len..].fill(0);
                    bytes.truncate(next_len);
                }
            }
            (InteractionDraft::Text { value, cursor }, KeyCode::Left) => {
                *cursor = value[..*cursor]
                    .char_indices()
                    .next_back()
                    .map(|(index, _)| index)
                    .unwrap_or(0);
            }
            (InteractionDraft::Text { value, cursor }, KeyCode::Right) => {
                if *cursor < value.len() {
                    *cursor += value[*cursor..]
                        .chars()
                        .next()
                        .map(char::len_utf8)
                        .unwrap_or(0);
                }
            }
            (_, KeyCode::Enter) => return self.finish_editor(),
            _ => {}
        }
        None
    }

    fn activate_choice(&mut self) -> Option<InteractionResolution> {
        if self.visible_count() == 0 {
            return None;
        }
        if self.request.allow_other && self.selected_index == self.request.choices.len() {
            self.other_editing = true;
            self.clear_text_draft();
            return None;
        }
        match self.request.mode {
            InteractionMode::SingleChoice | InteractionMode::Confirmation => {
                let value = self.request.choices[self.selected_index].resolved_value();
                Some(
                    self.resolution(InteractionOutcome::Answer(InteractionAnswer {
                        selected: vec![value],
                        text: None,
                    })),
                )
            }
            InteractionMode::MultiSelect => {
                if !self.selected.contains(&self.selected_index) {
                    self.selected.push(self.selected_index);
                }
                let selected = self
                    .selected
                    .iter()
                    .filter_map(|index| self.request.choices.get(*index))
                    .map(InteractionChoice::resolved_value)
                    .collect();
                Some(
                    self.resolution(InteractionOutcome::Answer(InteractionAnswer {
                        selected,
                        text: None,
                    })),
                )
            }
            _ => None,
        }
    }

    fn finish_editor(&mut self) -> Option<InteractionResolution> {
        match &mut self.draft {
            InteractionDraft::Text { value, .. } if !value.trim().is_empty() => {
                let text = std::mem::take(value);
                Some(
                    self.resolution(InteractionOutcome::Answer(InteractionAnswer {
                        selected: self
                            .selected
                            .iter()
                            .filter_map(|index| self.request.choices.get(*index))
                            .map(InteractionChoice::resolved_value)
                            .collect(),
                        text: Some(text),
                    })),
                )
            }
            InteractionDraft::Secret(bytes) if !bytes.is_empty() => {
                let bytes = std::mem::take(bytes);
                Some(self.resolution(InteractionOutcome::Secret(SecretHandle { bytes })))
            }
            _ => None,
        }
    }

    fn toggle_selected(&mut self, index: usize) {
        if index >= self.request.choices.len() {
            self.other_editing = true;
            return;
        }
        if let Some(position) = self.selected.iter().position(|selected| *selected == index) {
            self.selected.remove(position);
        } else {
            self.selected.push(index);
            self.selected.sort_unstable();
        }
    }

    fn clear_text_draft(&mut self) {
        if let InteractionDraft::Text { value, cursor } = &mut self.draft {
            value.clear();
            *cursor = 0;
        }
    }

    fn resolution(&self, outcome: InteractionOutcome) -> InteractionResolution {
        InteractionResolution {
            id: self.id,
            tool_call_id: self.tool_call_id.clone(),
            outcome,
        }
    }
}

pub struct SecretHandle {
    bytes: Vec<u8>,
}

impl SecretHandle {
    pub(crate) fn expose_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn zeroize(&mut self) {
        self.bytes.fill(0);
    }
}

impl std::fmt::Debug for SecretHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretHandle([REDACTED])")
    }
}

impl Drop for SecretHandle {
    fn drop(&mut self) {
        self.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(label: &str) -> InteractionChoice {
        InteractionChoice {
            label: label.into(),
            value: None,
            description: Some(format!("About {label}")),
        }
    }

    fn request(kind: ModelInteractionKind) -> InteractionRequest {
        InteractionRequest::from_model(RequestUserInputArgs {
            question: "Choose".into(),
            description: Some("Details".into()),
            kind,
            choices: vec![choice("One"), choice("Two")],
            allow_other: true,
            questions: Vec::new(),
        })
        .unwrap()
    }

    #[test]
    fn validates_modes_choice_caps_and_unique_values() {
        assert!(InteractionRequest::from_model(RequestUserInputArgs {
            question: "Text".into(),
            description: None,
            kind: ModelInteractionKind::FreeText,
            choices: Vec::new(),
            allow_other: false,
            questions: Vec::new(),
        })
        .is_ok());
        assert!(InteractionRequest::from_model(RequestUserInputArgs {
            question: "Too many".into(),
            description: None,
            kind: ModelInteractionKind::SingleChoice,
            choices: (0..9).map(|index| choice(&index.to_string())).collect(),
            allow_other: true,
            questions: Vec::new(),
        })
        .is_err());
        assert!(InteractionRequest::from_model(RequestUserInputArgs {
            question: "Duplicate".into(),
            description: None,
            kind: ModelInteractionKind::SingleChoice,
            choices: vec![choice("same"), choice("same")],
            allow_other: false,
            questions: Vec::new(),
        })
        .is_err());
    }

    #[test]
    fn free_text_uses_dedicated_draft_and_cancels() {
        let request = InteractionRequest::from_legacy_question("Explain".into()).unwrap();
        let pending = PendingInteraction::model("call-1".into(), request);
        let mut state = InteractionState::from_pending(pending);
        assert!(state
            .handle_key(KeyCode::Char('h'), KeyModifiers::NONE)
            .is_none());
        assert!(state
            .handle_key(KeyCode::Char('i'), KeyModifiers::NONE)
            .is_none());
        let resolution = state
            .handle_key(KeyCode::Enter, KeyModifiers::NONE)
            .unwrap();
        assert_eq!(resolution.tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(
            resolution.outcome.model_result().unwrap(),
            r#"{"text":"hi"}"#
        );

        let request = InteractionRequest::from_legacy_question("Explain".into()).unwrap();
        let mut state = InteractionState::host(request);
        assert!(matches!(
            state
                .handle_key(KeyCode::Esc, KeyModifiers::NONE)
                .unwrap()
                .outcome,
            InteractionOutcome::Cancelled
        ));
    }

    #[test]
    fn single_multi_confirmation_shortcuts_and_other_are_deterministic() {
        let mut single = InteractionState::host(request(ModelInteractionKind::SingleChoice));
        assert!(single
            .handle_key(KeyCode::Char('y'), KeyModifiers::NONE)
            .is_none());
        let answer = single
            .handle_key(KeyCode::Char('2'), KeyModifiers::NONE)
            .unwrap();
        assert_eq!(
            answer.outcome.model_result().unwrap(),
            r#"{"selected":["Two"]}"#
        );

        let mut multi = InteractionState::host(request(ModelInteractionKind::MultiSelect));
        multi.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        multi.handle_key(KeyCode::Down, KeyModifiers::NONE);
        multi.handle_key(KeyCode::Char(' '), KeyModifiers::NONE);
        let answer = multi
            .handle_key(KeyCode::Enter, KeyModifiers::NONE)
            .unwrap();
        assert!(answer.outcome.model_result().unwrap().contains("One"));
        assert!(answer.outcome.model_result().unwrap().contains("Two"));

        let confirmation = InteractionRequest::from_model(RequestUserInputArgs {
            question: "Continue?".into(),
            description: None,
            kind: ModelInteractionKind::Confirmation,
            choices: Vec::new(),
            allow_other: false,
            questions: Vec::new(),
        })
        .unwrap();
        let mut by_number = InteractionState::host(confirmation.clone());
        let answer = by_number
            .handle_key(KeyCode::Char('1'), KeyModifiers::NONE)
            .unwrap();
        assert!(answer.outcome.model_result().unwrap().contains("yes"));
        let mut by_yes = InteractionState::host(confirmation.clone());
        let answer = by_yes
            .handle_key(KeyCode::Char('y'), KeyModifiers::NONE)
            .unwrap();
        assert!(answer.outcome.model_result().unwrap().contains("yes"));
        let mut by_no = InteractionState::host(confirmation);
        let answer = by_no
            .handle_key(KeyCode::Char('N'), KeyModifiers::SHIFT)
            .unwrap();
        assert_eq!(
            answer.outcome.model_result().unwrap(),
            r#"{"selected":["no"]}"#
        );

        let mut other = InteractionState::host(request(ModelInteractionKind::SingleChoice));
        other.handle_key(KeyCode::Char('3'), KeyModifiers::NONE);
        assert!(other.other_editing());
        other.handle_key(KeyCode::Char('x'), KeyModifiers::NONE);
        let answer = other
            .handle_key(KeyCode::Enter, KeyModifiers::NONE)
            .unwrap();
        assert_eq!(answer.outcome.model_result().unwrap(), r#"{"text":"x"}"#);
    }

    #[test]
    fn paste_fills_an_open_answer_and_does_not_choose() {
        let mut free = InteractionState::host(
            InteractionRequest::from_legacy_question("Explain".into()).unwrap(),
        );
        assert!(free.insert_text("hello\nthere"));
        assert_eq!(free.draft_display(), "hello\nthere");

        let mut other = InteractionState::host(request(ModelInteractionKind::SingleChoice));
        other.handle_key(KeyCode::Char('3'), KeyModifiers::NONE);
        assert!(other.other_editing());
        assert!(other.insert_text("pasted"));
        assert_eq!(other.draft_display(), "pasted");

        let mut single = InteractionState::host(request(ModelInteractionKind::SingleChoice));
        let selected = single.selected_index;
        assert!(!single.insert_text("2"));
        assert_eq!(single.selected_index, selected);
        assert!(single.draft_display().is_empty());
    }

    #[test]
    fn manual_queue_preserves_order_and_rejects_stale_ids() {
        let interaction = PendingInteraction::model(
            "call-2".into(),
            InteractionRequest::from_legacy_question("Question".into()).unwrap(),
        );
        let interaction_id = interaction.id;
        let mut queue = ManualResolutionQueue::default();
        queue.begin(vec![
            PendingManualResolution::Approval(PendingApproval {
                tool_name: "write_file".into(),
                tool_args: "{}".into(),
                tool_id: "call-1".into(),
                network_requested: false,
                email_preview: None,
                email_draft_ref: None,
            }),
            PendingManualResolution::Interaction(interaction),
        ]);
        assert_eq!(queue.progress(), (0, 2));
        assert!(!queue.resolve_tool("stale"));
        assert!(queue.resolve_tool("call-1"));
        assert!(!queue.resolve_interaction(InteractionId::new()));
        assert!(queue.resolve_interaction(interaction_id));
        assert!(!queue.is_active());
        assert_eq!(queue.progress(), (2, 2));
    }

    #[test]
    fn masked_host_input_is_redacted_zeroized_and_not_model_visible() {
        let mut state = InteractionState::host(InteractionRequest::host_masked(
            "Token",
            Some("Host only".into()),
        ));
        for character in "sëcret".chars() {
            state.handle_key(KeyCode::Char(character), KeyModifiers::NONE);
        }
        assert_eq!(state.draft_display(), "••••••");
        assert!(!format!("{state:?}").contains("sëcret"));
        let resolution = state
            .handle_key(KeyCode::Enter, KeyModifiers::NONE)
            .unwrap();
        let InteractionOutcome::Secret(mut secret) = resolution.outcome else {
            panic!("expected secret handle")
        };
        assert_eq!(format!("{secret:?}"), "SecretHandle([REDACTED])");
        assert_eq!(secret.expose_bytes(), "sëcret".as_bytes());
        assert!(InteractionOutcome::Secret(SecretHandle { bytes: vec![] })
            .model_result()
            .is_err());
        secret.zeroize();
        assert!(secret.bytes.iter().all(|byte| *byte == 0));
    }

    #[test]
    fn masked_host_input_is_keyboard_only_and_bounded_to_one_kibibyte() {
        let mut state = InteractionState::host(InteractionRequest::host_masked("Token", None));
        for _ in 0..(MAX_MASKED_SECRET_BYTES + 32) {
            state.handle_key(KeyCode::Char('x'), KeyModifiers::NONE);
        }
        assert_eq!(
            state.draft_display().chars().count(),
            MAX_MASKED_SECRET_BYTES
        );
        let resolution = state
            .handle_key(KeyCode::Enter, KeyModifiers::NONE)
            .unwrap();
        let InteractionOutcome::Secret(secret) = resolution.outcome else {
            panic!("expected secret handle")
        };
        assert_eq!(secret.expose_bytes().len(), MAX_MASKED_SECRET_BYTES);
    }

    fn option(label: &str, description: &str) -> QuestionOptionArgs {
        QuestionOptionArgs {
            label: label.into(),
            description: Some(description.into()),
        }
    }

    fn form_args(questions: Vec<QuestionItemArgs>) -> RequestUserInputArgs {
        RequestUserInputArgs {
            question: String::new(),
            description: None,
            kind: ModelInteractionKind::FreeText,
            choices: Vec::new(),
            allow_other: false,
            questions,
        }
    }

    #[test]
    fn a_question_form_keeps_the_recommendation_and_the_custom_answer() {
        let forced = InteractionRequest::from_model(RequestUserInputArgs {
            question: "One".into(),
            description: None,
            kind: ModelInteractionKind::SingleChoice,
            choices: vec![choice("Stay")],
            allow_other: false,
            questions: Vec::new(),
        })
        .unwrap();
        assert!(forced.allow_other);
        let confirmation = InteractionRequest::from_model(RequestUserInputArgs {
            question: "Continue?".into(),
            description: None,
            kind: ModelInteractionKind::Confirmation,
            choices: Vec::new(),
            allow_other: true,
            questions: Vec::new(),
        })
        .unwrap();
        assert!(!confirmation.allow_other);

        let rejected = form_args(vec![QuestionItemArgs {
            question: "Too many".into(),
            header: None,
            options: (0..5)
                .map(|index| option(&index.to_string(), "detail"))
                .collect(),
            multiple: false,
        }]);
        assert!(rejected.prepare().is_err());
        let rejected = form_args(
            (0..6)
                .map(|index| QuestionItemArgs {
                    question: format!("Question {index}"),
                    header: None,
                    options: Vec::new(),
                    multiple: false,
                })
                .collect(),
        );
        assert!(rejected.prepare().is_err());

        let PreparedUserInput::Form(form) = form_args(vec![
            QuestionItemArgs {
                question: "Push this commit?".into(),
                header: Some("Publish".into()),
                options: vec![
                    option("Push main", "fast-forward"),
                    option("Leave it", "local"),
                ],
                multiple: false,
            },
            QuestionItemArgs {
                question: "Anything else?".into(),
                header: Some("Notes".into()),
                options: vec![option("No", "stop"), option("Yes", "add a note")],
                multiple: false,
            },
        ])
        .prepare()
        .unwrap() else {
            panic!("expected a question form");
        };
        assert_eq!(
            form.questions[0].request.choices[0].label,
            "Push main (Recommended)"
        );
        assert_eq!(
            form.questions[0].request.choices[0].resolved_value(),
            "Push main"
        );
        assert!(form.questions[0].request.allow_other);
        assert_eq!(
            form.questions[1].request.choices[1].description.as_deref(),
            Some("add a note")
        );

        let mut state =
            InteractionState::from_pending(PendingInteraction::form("call-1".into(), form));
        assert_eq!(state.header.as_deref(), Some("Publish"));
        assert_eq!(state.custom_label, "Type your own answer");
        assert_eq!(state.progress, Some((0, 2)));
        assert!(state
            .handle_key(KeyCode::Right, KeyModifiers::NONE)
            .is_none());
        assert_eq!(state.progress, Some((1, 2)));
        assert!(state
            .handle_key(KeyCode::Left, KeyModifiers::NONE)
            .is_none());
        let locked = state.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        assert!(locked.is_none());
        assert_eq!(state.header.as_deref(), Some("Notes"));
        state.handle_key(KeyCode::Char('3'), KeyModifiers::NONE);
        assert!(state.other_editing());
        state.handle_key(KeyCode::Char('h'), KeyModifiers::NONE);
        state.handle_key(KeyCode::Char('i'), KeyModifiers::NONE);
        let resolution = state
            .handle_key(KeyCode::Enter, KeyModifiers::NONE)
            .unwrap();
        let report = resolution.outcome.model_result().unwrap();
        assert!(report.contains("outcome=submitted"));
        assert!(report.contains("1.header=Publish"));
        assert!(report.contains("1.question=Push this commit?"));
        assert!(report.contains("1.choice=Leave it"));
        assert!(report.contains("1.custom=\n"));
        assert!(report.contains("2.header=Notes"));
        assert!(report.contains("2.choice=\n"));
        assert!(report.contains("2.custom=hi"));
        assert!(!report.contains("(Recommended)"));
        assert_eq!(resolution.tool_call_id.as_deref(), Some("call-1"));

        let PreparedUserInput::Form(form) = form_args(vec![
            QuestionItemArgs {
                question: "First?".into(),
                header: None,
                options: vec![option("A", "one"), option("B", "two")],
                multiple: false,
            },
            QuestionItemArgs {
                question: "Second?".into(),
                header: None,
                options: Vec::new(),
                multiple: false,
            },
        ])
        .prepare()
        .unwrap() else {
            panic!("expected a question form");
        };
        let mut state =
            InteractionState::from_pending(PendingInteraction::form("call-2".into(), form));
        state.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        let cancelled = state.handle_key(KeyCode::Esc, KeyModifiers::NONE).unwrap();
        assert!(matches!(cancelled.outcome, InteractionOutcome::Cancelled));
        assert_eq!(
            cancelled.outcome.model_result().unwrap(),
            "Cancelled by user"
        );
    }
}
