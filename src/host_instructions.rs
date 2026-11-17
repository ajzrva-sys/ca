//! Process-only provenance for closed first-party instructions. Imported text
//! cannot acquire this authority, even when it copies an instruction verbatim.
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Instruction {
    Child,
    FilenamesAliased,
    StructuredResult,
    Summary,
    Budget { maximum: u64, used: u64 },
    Repair,
    Workflow(crate::plan_workflow::WorkflowRole),
    AutomaticAuthor,
    AuthorScope,
    AutomaticMap,
}

impl Instruction {
    fn text(&self) -> String {
        match self {
            Self::Child => "You are a bounded child coding agent. Use only granted tools. Never widen workspace roots or network authority.".into(),
            Self::FilenamesAliased => "\nProject filenames are already aliased before you see them. Report directory listings with those complete references. The word filter in the task is not a request to glob for it.".into(),
            Self::StructuredResult => "\nA structured result is required. Finish by calling report_structured_output exactly once as the sole final tool call.".into(),
            Self::Summary => "\n\nState summary:\n".into(),
            Self::Budget { maximum, used } => format!("\nThis task shares a fixed {maximum} total-token ceiling with the entire graph; {used} tokens were observed before this worker started. Input history counts again on every request, in addition to output/reasoning. Search first and read only relevant line ranges. Keep concise findings with citations in your progress text before further reads. Earlier successful read/search results may be omitted from later requests; original call arguments remain available for narrow rereads.\n"),
            Self::Repair => "\nYou only supply a replacement or diagnosis. Do not choose tools, paths, edits or verification commands.\n".into(),
            Self::Workflow(role) => match role {
                crate::plan_workflow::WorkflowRole::Author => "Author a complete implementation plan. Return title, full Markdown body, and ordered task titles.",
                crate::plan_workflow::WorkflowRole::Review => "Review the exact current plan. Return verdict accepted or needs_changes and concrete feedback. Acceptance is a plan quality verdict, never user execution authorization.",
                crate::plan_workflow::WorkflowRole::Revise => "Revise the full plan using the feedback. Return title, complete replacement Markdown body, and ordered task titles.",
                crate::plan_workflow::WorkflowRole::Implement => "Implement the current plan using ordinary Agent tools and record validation evidence.",
            }.into(),
            Self::AutomaticAuthor => "Author a focused task map for the user's requested outcome, including inspection, listing, reading, or running a command. Include code changes only when the user requested them. Return title, full Markdown body, and ordered task titles.".into(),
            Self::AuthorScope => crate::task_scope::AUTHOR_GUIDANCE.into(),
            Self::AutomaticMap => crate::automatic_tasks::GUIDANCE.into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Text {
    text: String,
    spans: Vec<(usize, usize, Instruction)>,
}

impl Text {
    pub(crate) fn data(&mut self, text: &str) {
        self.text.push_str(text);
    }
    pub(crate) fn instruction(&mut self, instruction: Instruction) {
        let start = self.text.len();
        self.text.push_str(&instruction.text());
        self.spans.push((start, self.text.len(), instruction));
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }
    pub(crate) fn matches(&self, text: &str) -> bool {
        self.text == text
    }
    pub(crate) fn project(
        &self,
        mut project: impl FnMut(&str, usize) -> Result<String, String>,
    ) -> Result<Self, String> {
        let mut result = Self::default();
        let mut offset = 0;
        for (start, end, instruction) in &self.spans {
            if *start < offset || self.text.get(*start..*end) != Some(instruction.text().as_str()) {
                return Err("Host instruction provenance changed".into());
            }
            result.data(&project(&self.text[offset..*start], offset)?);
            result.instruction(instruction.clone());
            offset = *end;
        }
        result.data(&project(&self.text[offset..], offset)?);
        Ok(result)
    }
    pub(crate) fn shadow(&self, replacements: &mut Vec<(String, String)>) -> String {
        let mut result = String::new();
        let mut offset = 0;
        for (start, end, instruction) in &self.spans {
            result.push_str(&self.text[offset..*start]);
            let marker = format!("__ca_host_{}__", uuid::Uuid::new_v4().simple());
            result.push_str(&marker);
            replacements.push((marker, instruction.text()));
            offset = *end;
        }
        result.push_str(&self.text[offset..]);
        result
    }
}

/// Exact concrete wire locations from lowering an otherwise identical shadow
/// request. Covers closed host instructions and typed, route/policy-owned
/// provider continuation state; neither grants authority to matching data
/// elsewhere, nor to filenames or tool arguments inside opaque state.
#[derive(Clone, Default)]
pub(crate) struct WireReceipt {
    fields: Vec<(String, String, String)>,
    replacements: Vec<(String, String)>,
}
impl std::fmt::Debug for WireReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WireReceipt")
            .field("fields", &self.fields.len())
            .field("fragments", &self.replacements.len())
            .finish()
    }
}
impl WireReceipt {
    pub(crate) fn bind(
        body: &[u8],
        shadow: &[u8],
        replacements: Vec<(String, String)>,
    ) -> Result<Self, String> {
        fn walk(
            actual: &Value,
            shadow: &Value,
            path: &str,
            result: &mut WireReceipt,
        ) -> Result<(), String> {
            if actual == shadow {
                return Ok(());
            }
            match (actual, shadow) {
                (Value::String(actual), Value::String(shadow)) => {
                    let restored = result.expand(shadow);
                    if &restored != actual {
                        return Err("Host instruction lowering changed data".into());
                    }
                    result
                        .fields
                        .push((path.into(), actual.clone(), shadow.clone()));
                }
                (Value::Array(actual), Value::Array(shadow)) if actual.len() == shadow.len() => {
                    for (index, (actual, shadow)) in actual.iter().zip(shadow).enumerate() {
                        walk(actual, shadow, &format!("{path}/{index}"), result)?;
                    }
                }
                (Value::Object(actual), Value::Object(shadow))
                    if actual.keys().eq(shadow.keys()) =>
                {
                    for (key, actual) in actual {
                        let key_path = key.replace('~', "~0").replace('/', "~1");
                        walk(actual, &shadow[key], &format!("{path}/{key_path}"), result)?;
                    }
                }
                _ => return Err("Host instruction lowering changed protocol shape".into()),
            }
            Ok(())
        }
        let actual: Value = serde_json::from_slice(body).map_err(|_| "Host request is not JSON")?;
        let shadow: Value =
            serde_json::from_slice(shadow).map_err(|_| "Host shadow is not JSON")?;
        let mut result = Self {
            fields: Vec::new(),
            replacements,
        };
        walk(&actual, &shadow, "", &mut result)?;
        Ok(result)
    }
    fn expand(&self, text: &str) -> String {
        let mut text = text.to_string();
        for (marker, instruction) in &self.replacements {
            text = text.replace(marker, instruction);
        }
        text
    }
    pub(crate) fn protect(&self, value: &mut Value) -> Result<(), String> {
        for (path, actual, shadow) in &self.fields {
            let field = value
                .pointer_mut(path)
                .ok_or("Host instruction field disappeared")?;
            if field.as_str() != Some(actual) {
                return Err("Host instruction field changed".into());
            }
            *field = shadow.clone().into();
        }
        Ok(())
    }
    pub(crate) fn restore(&mut self, value: &mut Value) -> Result<(), String> {
        let replacements = &self.replacements;
        for (path, actual, shadow) in &mut self.fields {
            let field = value
                .pointer_mut(path)
                .ok_or("Host instruction field disappeared")?;
            let text = field.as_str().ok_or("Host instruction field is not text")?;
            for (marker, _) in replacements {
                if shadow.matches(marker).count() != text.matches(marker).count() {
                    return Err("Host instruction marker changed during filtering".into());
                }
            }
            *shadow = text.into();
            let mut restored = text.to_string();
            for (marker, instruction) in replacements {
                if !restored.contains(marker) {
                    continue;
                }
                restored = restored.replace(marker, instruction);
            }
            *actual = restored.clone();
            *field = restored.into();
        }
        Ok(())
    }
    pub(crate) fn validation_copy(&self, value: &Value) -> Result<Value, String> {
        let mut copy = value.clone();
        for (path, actual, shadow) in &self.fields {
            let field = copy
                .pointer_mut(path)
                .ok_or("Host instruction field disappeared")?;
            if field.as_str() != Some(actual) {
                return Err("Host instruction field changed after filtering".into());
            }
            // The marker is opaque and contains no private source vocabulary.
            *field = shadow.clone().into();
        }
        Ok(copy)
    }
}
