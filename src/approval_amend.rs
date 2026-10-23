//! An ephemeral edit of one pending command. Saving never grants execution.
use crossterm::event::{KeyCode, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;

const MAX_COMMAND_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct CommandEditor {
    pub id: uuid::Uuid,
    original: crate::app::PendingApproval,
    pub text: String,
    pub cursor: usize,
    pub error: Option<String>,
    pub saving: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EditAction {
    Continue,
    Cancel,
    Save,
}

impl CommandEditor {
    pub fn new(approval: &crate::app::PendingApproval) -> Option<Self> {
        if approval.tool_name != "run_command" {
            return None;
        }
        let arguments: serde_json::Value = serde_json::from_str(&approval.tool_args).ok()?;
        let text = arguments["command"].as_str()?.to_string();
        if text.len() > MAX_COMMAND_BYTES {
            return None;
        }
        Some(Self {
            id: uuid::Uuid::new_v4(),
            original: approval.clone(),
            cursor: text.len(),
            text,
            error: None,
            saving: false,
        })
    }

    pub fn matches(&self, approval: &crate::app::PendingApproval) -> bool {
        &self.original == approval
    }

    pub fn arguments(&self) -> Result<serde_json::Value, String> {
        if self.text.trim().is_empty() {
            return Err("Enter a command before saving".into());
        }
        let mut arguments: serde_json::Value = serde_json::from_str(&self.original.tool_args)
            .map_err(|_| "Original command arguments are invalid".to_string())?;
        arguments["command"] = self.text.clone().into();
        Ok(arguments)
    }

    pub fn insert(&mut self, text: &str) {
        if self.text.len().saturating_add(text.len()) > MAX_COMMAND_BYTES {
            self.error = Some("Command exceeds the 64 KiB editing limit".into());
            return;
        }
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.error = None;
    }

    pub fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> EditAction {
        if code == KeyCode::Esc {
            return EditAction::Cancel;
        }
        if self.saving {
            return EditAction::Continue;
        }
        if modifiers.contains(KeyModifiers::CONTROL) {
            match code {
                KeyCode::Char('s') => return EditAction::Save,
                KeyCode::Char('a') => self.cursor = self.line_start(),
                KeyCode::Char('e') => self.cursor = self.line_end(),
                _ => {}
            }
            return EditAction::Continue;
        }
        if modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER) {
            return EditAction::Continue;
        }
        match code {
            KeyCode::Char(character) => self.insert(&character.to_string()),
            KeyCode::Enter => self.insert("\n"),
            KeyCode::Tab => self.insert("\t"),
            KeyCode::Left => self.cursor = self.previous(),
            KeyCode::Right => self.cursor = self.next(),
            KeyCode::Home => self.cursor = self.line_start(),
            KeyCode::End => self.cursor = self.line_end(),
            KeyCode::Up => self.move_line(false),
            KeyCode::Down => self.move_line(true),
            KeyCode::Backspace => {
                let previous = self.previous();
                self.text.replace_range(previous..self.cursor, "");
                self.cursor = previous;
                self.error = None;
            }
            KeyCode::Delete => {
                self.text.replace_range(self.cursor..self.next(), "");
                self.error = None;
            }
            _ => {}
        }
        EditAction::Continue
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(offset, _)| offset)
    }

    fn next(&self) -> usize {
        self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(self.cursor, |grapheme| self.cursor + grapheme.len())
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map_or(0, |n| n + 1)
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |n| self.cursor + n)
    }

    fn move_line(&mut self, down: bool) {
        let column = self.text[self.line_start()..self.cursor]
            .graphemes(true)
            .count();
        let range = if down {
            let end = self.line_end();
            if end == self.text.len() {
                return;
            }
            let start = end + 1;
            start
                ..self.text[start..]
                    .find('\n')
                    .map_or(self.text.len(), |n| start + n)
        } else {
            let start = self.line_start();
            if start == 0 {
                return;
            }
            let end = start - 1;
            self.text[..end].rfind('\n').map_or(0, |n| n + 1)..end
        };
        self.cursor = self.text[range.clone()]
            .grapheme_indices(true)
            .nth(column)
            .map_or(range.end, |(offset, _)| range.start + offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(command: &str) -> crate::app::PendingApproval {
        crate::app::PendingApproval {
            tool_name: "run_command".into(),
            tool_args: serde_json::json!({"command": command, "cwd": "/tmp", "network": false})
                .to_string(),
            tool_id: "call-original".into(),
            network_requested: false,
            email_preview: None,
            email_draft_ref: None,
        }
    }

    #[test]
    fn agy295_inline_command_unicode_multiline_and_save_are_separate() {
        let original = approval("printf 界e\u{301}");
        let mut edit = CommandEditor::new(&original).unwrap();
        edit.key(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(edit.text, "printf 界");
        edit.key(KeyCode::Enter, KeyModifiers::NONE);
        edit.insert("printf 'second'");
        edit.key(KeyCode::Up, KeyModifiers::NONE);
        edit.key(KeyCode::Home, KeyModifiers::NONE);
        edit.insert("echo ");
        assert_eq!(edit.text, "echo printf 界\nprintf 'second'");
        assert_eq!(
            edit.key(KeyCode::Char('s'), KeyModifiers::CONTROL),
            EditAction::Save
        );
        let args = edit.arguments().unwrap();
        assert_eq!(args["cwd"], "/tmp");
        assert_eq!(args["network"], false);
        assert!(edit.matches(&original));
        assert_eq!(original.tool_args, approval("printf 界e\u{301}").tool_args);
    }

    #[test]
    fn agy295_inline_command_cancel_and_changed_identity_preserve_original() {
        let original = approval("echo original");
        let mut edit = CommandEditor::new(&original).unwrap();
        edit.insert(" changed");
        assert_eq!(
            edit.key(KeyCode::Esc, KeyModifiers::NONE),
            EditAction::Cancel
        );
        assert_eq!(original, approval("echo original"));
        let mut replaced = original.clone();
        replaced.tool_id = "replacement".into();
        assert!(!edit.matches(&replaced));
        replaced = original.clone();
        replaced.tool_args = approval("different").tool_args;
        assert!(!edit.matches(&replaced));
    }

    #[test]
    fn agy295_inline_command_budget_and_empty_save_do_not_replace_arguments() {
        let mut edit = CommandEditor::new(&approval(" ")).unwrap();
        assert!(edit.arguments().is_err());
        edit.insert(&"x".repeat(MAX_COMMAND_BYTES));
        assert_eq!(edit.text, " ");
        assert!(edit.error.as_deref().unwrap().contains("limit"));
    }
}
