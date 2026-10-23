use crate::app::{PendingApproval, ResolvedToolResult};
use crossterm::event::{KeyCode, KeyModifiers};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use uuid::Uuid;

const MAX_MASKED_SECRET_BYTES: usize = 1024;

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

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RequestUserInputArgs {
    pub question: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub kind: ModelInteractionKind,
    #[serde(default)]
    pub choices: Vec<InteractionChoice>,
    #[serde(default)]
    pub allow_other: bool,
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

impl InteractionRequest {
    pub fn from_model(args: RequestUserInputArgs) -> Result<Self, String> {
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
        let visible = choices.len() + usize::from(args.allow_other);
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
            allow_other: args.allow_other,
        })
    }

    pub fn from_legacy_question(question: String) -> Result<Self, String> {
        Self::from_model(RequestUserInputArgs {
            question,
            description: None,
            kind: ModelInteractionKind::FreeText,
            choices: Vec::new(),
            allow_other: false,
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
    pub write_access: Option<crate::write_access::WriteAccessRequest>,
}

impl PendingInteraction {
    pub fn model(tool_call_id: String, request: InteractionRequest) -> Self {
        Self {
            id: InteractionId::new(),
            tool_call_id,
            request,
            write_access: None,
        }
    }
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

#[derive(Debug)]
pub struct InteractionState {
    pub id: InteractionId,
    pub tool_call_id: Option<String>,
    pub request: InteractionRequest,
    pub selected_index: usize,
    selected: Vec<usize>,
    draft: InteractionDraft,
    other_editing: bool,
}

impl InteractionState {
    pub fn from_pending(pending: PendingInteraction) -> Self {
        Self::new(pending.id, Some(pending.tool_call_id), pending.request)
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
            selected_index: 0,
            selected: Vec::new(),
            draft,
            other_editing: false,
        }
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
        if code == KeyCode::Esc {
            if self.other_editing {
                self.other_editing = false;
                self.clear_text_draft();
                return None;
            }
            return Some(self.resolution(InteractionOutcome::Cancelled));
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
        })
        .is_ok());
        assert!(InteractionRequest::from_model(RequestUserInputArgs {
            question: "Too many".into(),
            description: None,
            kind: ModelInteractionKind::SingleChoice,
            choices: (0..9).map(|index| choice(&index.to_string())).collect(),
            allow_other: true,
        })
        .is_err());
        assert!(InteractionRequest::from_model(RequestUserInputArgs {
            question: "Duplicate".into(),
            description: None,
            kind: ModelInteractionKind::SingleChoice,
            choices: vec![choice("same"), choice("same")],
            allow_other: false,
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
}
