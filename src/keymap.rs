use crossterm::event::{KeyCode, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

const MAX_BINDINGS_PER_ACTION: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyAction {
    ModelPicker,
    OpenEditor,
    CycleModel,
    CycleRecentModel,
    CycleFavoriteModel,
    CycleReasoning,
    QueueInput,
    SaveOrSteer,
    ToggleExpand,
    HistorySearch,
    StashDraft,
    ComposerUndo,
    ComposerRedo,
    FocusAgentActivity,
    RemoveLastAttachment,
    EditLastQueued,
    TranscriptTop,
    TranscriptBottom,
    CopyTranscript,
    SplitHorizontal,
    SplitVertical,
    NextPane,
    PreviousPane,
    ClosePane,
}

impl KeyAction {
    pub const ALL: [Self; 24] = [
        Self::ModelPicker,
        Self::OpenEditor,
        Self::CycleModel,
        Self::CycleRecentModel,
        Self::CycleFavoriteModel,
        Self::CycleReasoning,
        Self::QueueInput,
        Self::SaveOrSteer,
        Self::ToggleExpand,
        Self::HistorySearch,
        Self::StashDraft,
        Self::ComposerUndo,
        Self::ComposerRedo,
        Self::FocusAgentActivity,
        Self::RemoveLastAttachment,
        Self::EditLastQueued,
        Self::TranscriptTop,
        Self::TranscriptBottom,
        Self::CopyTranscript,
        Self::SplitHorizontal,
        Self::SplitVertical,
        Self::NextPane,
        Self::PreviousPane,
        Self::ClosePane,
    ];

    pub const fn config_key(self) -> &'static str {
        match self {
            Self::ModelPicker => "model_picker",
            Self::OpenEditor => "open_editor",
            Self::CycleModel => "cycle_model",
            Self::CycleRecentModel => "cycle_recent_model",
            Self::CycleFavoriteModel => "cycle_favorite_model",
            Self::CycleReasoning => "cycle_reasoning",
            Self::QueueInput => "queue_input",
            Self::SaveOrSteer => "save_or_steer",
            Self::ToggleExpand => "toggle_expand",
            Self::HistorySearch => "history_search",
            Self::StashDraft => "stash_draft",
            Self::ComposerUndo => "composer_undo",
            Self::ComposerRedo => "composer_redo",
            Self::FocusAgentActivity => "focus_agent_activity",
            Self::RemoveLastAttachment => "remove_last_attachment",
            Self::EditLastQueued => "edit_last_queued",
            Self::TranscriptTop => "transcript_top",
            Self::TranscriptBottom => "transcript_bottom",
            Self::CopyTranscript => "copy_transcript",
            Self::SplitHorizontal => "split_horizontal",
            Self::SplitVertical => "split_vertical",
            Self::NextPane => "next_pane",
            Self::PreviousPane => "previous_pane",
            Self::ClosePane => "close_pane",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ModelPicker => "Switch model",
            Self::OpenEditor => "Open external editor",
            Self::CycleModel => "Cycle model",
            Self::CycleRecentModel => "Cycle recent model",
            Self::CycleFavoriteModel => "Cycle favorite model",
            Self::CycleReasoning => "Cycle reasoning",
            Self::QueueInput => "Queue message / save queued edit",
            Self::SaveOrSteer => "Interrupt and send / save session when idle",
            Self::ToggleExpand => "Expand latest response",
            Self::HistorySearch => "Search input history",
            Self::StashDraft => "Stash composer draft",
            Self::ComposerUndo => "Undo composer edit",
            Self::ComposerRedo => "Redo composer edit",
            Self::FocusAgentActivity => "Focus agent activity",
            Self::RemoveLastAttachment => "Remove last attachment",
            Self::EditLastQueued => "Edit last queued input",
            Self::TranscriptTop => "Jump to oldest transcript message",
            Self::TranscriptBottom => "Jump to latest transcript message",
            Self::CopyTranscript => "Copy complete visible transcript",
            Self::SplitHorizontal => "Split pane horizontally",
            Self::SplitVertical => "Split pane vertically",
            Self::NextPane => "Focus next pane",
            Self::PreviousPane => "Focus previous pane",
            Self::ClosePane => "Close focused pane",
        }
    }

    pub fn default_chords(self) -> Vec<KeyChord> {
        let values: &[&str] = match self {
            Self::ModelPicker => &["ctrl+k", "ctrl+l"],
            Self::OpenEditor => &["ctrl+g"],
            Self::CycleModel => &["ctrl+p"],
            Self::CycleRecentModel => &["ctrl+x r"],
            Self::CycleFavoriteModel => &["ctrl+x f"],
            Self::CycleReasoning => &["ctrl+t"],
            Self::QueueInput => &["alt+enter"],
            Self::SaveOrSteer => &["ctrl+s"],
            Self::ToggleExpand => &["ctrl+o"],
            Self::HistorySearch => &["ctrl+x ctrl+r"],
            Self::StashDraft => &["ctrl+x s"],
            Self::ComposerUndo => &["ctrl+x u"],
            Self::ComposerRedo => &["ctrl+x y"],
            // J is first because macOS takes Control+Down for App Exposé before
            // the terminal can deliver it. Down still works once Control is released,
            // and a held Control on J or Down is accepted in `resolve`.
            Self::FocusAgentActivity => &["ctrl+x j", "ctrl+x down"],
            Self::RemoveLastAttachment => &["ctrl+x a"],
            Self::EditLastQueued => &["alt+up"],
            Self::TranscriptTop => &["ctrl+home"],
            Self::TranscriptBottom => &["ctrl+end"],
            Self::CopyTranscript => &["ctrl+x c"],
            Self::SplitHorizontal => &["ctrl+x h"],
            Self::SplitVertical => &["ctrl+x v"],
            Self::NextPane => &["ctrl+x n"],
            Self::PreviousPane => &["ctrl+x p"],
            Self::ClosePane => &["ctrl+x q"],
        };
        values
            .iter()
            .map(|value| value.parse().expect("built-in key chord is valid"))
            .collect()
    }

    pub fn parse_config_key(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.config_key() == value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyBindingConfig {
    One(String),
    Many(Vec<String>),
}

impl KeyBindingConfig {
    fn values(&self) -> Vec<&str> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values.iter().map(String::as_str).collect(),
        }
    }
}

pub type KeymapConfig = HashMap<String, KeyBindingConfig>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyStroke {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyStroke {
    pub fn from_event(code: KeyCode, modifiers: KeyModifiers) -> Self {
        let modifiers = modifiers
            & (KeyModifiers::SHIFT
                | KeyModifiers::CONTROL
                | KeyModifiers::ALT
                | KeyModifiers::SUPER
                | KeyModifiers::HYPER
                | KeyModifiers::META);
        let code = match code {
            KeyCode::Char(character) => KeyCode::Char(character.to_ascii_lowercase()),
            other => other,
        };
        Self { code, modifiers }
    }

    fn is_ctrl_char(self, expected: char) -> bool {
        self.modifiers == KeyModifiers::CONTROL && self.code == KeyCode::Char(expected)
    }

    fn validate(self) -> anyhow::Result<()> {
        if self.is_ctrl_char('c') || self.is_ctrl_char('d') {
            anyhow::bail!("{} is reserved for emergency control", self);
        }
        if self.modifiers == KeyModifiers::CONTROL
            && matches!(
                self.code,
                KeyCode::Char('a' | 'e' | 'j' | 'u' | 'v' | 'w' | 'y')
            )
        {
            anyhow::bail!("{} is reserved for composer editing", self);
        }
        if self.modifiers == KeyModifiers::ALT && self.code == KeyCode::Char('y') {
            anyhow::bail!("{} is reserved for the composer kill ring", self);
        }
        if matches!(
            (self.modifiers, self.code),
            (KeyModifiers::ALT, KeyCode::Up)
                | (KeyModifiers::CONTROL, KeyCode::Home | KeyCode::End)
        ) {
            return Ok(());
        }
        match self.code {
            KeyCode::Esc
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown => anyhow::bail!("{} is reserved for editing or navigation", self),
            KeyCode::Enter
                if self.modifiers.is_empty() || self.modifiers == KeyModifiers::SHIFT =>
            {
                anyhow::bail!("{} is reserved for submission or new lines", self)
            }
            KeyCode::Char(_)
                if !self.modifiers.contains(KeyModifiers::CONTROL)
                    && !self.modifiers.contains(KeyModifiers::ALT) =>
            {
                anyhow::bail!("character bindings require Ctrl or Alt")
            }
            _ => Ok(()),
        }
    }

    fn is_navigation(self) -> bool {
        self.modifiers.is_empty()
            && matches!(
                self.code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::PageUp
                    | KeyCode::PageDown
            )
    }

    fn is_unmodified_character(self) -> bool {
        self.modifiers.is_empty() && matches!(self.code, KeyCode::Char(_))
    }

    /// Drop a Control key that is still held from the chord prefix.
    /// Shift and Alt stay, so Ctrl+Shift+Down does not match plain Down.
    fn without_held_control(self) -> Option<Self> {
        if !self.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        Some(Self {
            code: self.code,
            modifiers: self.modifiers.difference(KeyModifiers::CONTROL),
        })
    }

    fn parse_unvalidated(raw: &str) -> Result<Self, anyhow::Error> {
        let raw = raw.trim();
        if raw.is_empty() {
            anyhow::bail!("key stroke is empty");
        }
        let mut modifiers = KeyModifiers::NONE;
        let mut code = None;
        for part in raw.split('+') {
            let part = part.trim().to_ascii_lowercase();
            match part.as_str() {
                "ctrl" | "control" => modifiers.insert(KeyModifiers::CONTROL),
                "alt" | "option" => modifiers.insert(KeyModifiers::ALT),
                "shift" => modifiers.insert(KeyModifiers::SHIFT),
                "enter" | "return" => set_code(&mut code, KeyCode::Enter)?,
                "tab" => set_code(&mut code, KeyCode::Tab)?,
                "backtab" => set_code(&mut code, KeyCode::BackTab)?,
                "backspace" => set_code(&mut code, KeyCode::Backspace)?,
                "delete" => set_code(&mut code, KeyCode::Delete)?,
                "up" => set_code(&mut code, KeyCode::Up)?,
                "down" => set_code(&mut code, KeyCode::Down)?,
                "left" => set_code(&mut code, KeyCode::Left)?,
                "right" => set_code(&mut code, KeyCode::Right)?,
                "home" => set_code(&mut code, KeyCode::Home)?,
                "end" => set_code(&mut code, KeyCode::End)?,
                "pageup" => set_code(&mut code, KeyCode::PageUp)?,
                "pagedown" => set_code(&mut code, KeyCode::PageDown)?,
                value if value.len() == 1 => {
                    set_code(&mut code, KeyCode::Char(value.chars().next().unwrap()))?
                }
                value if value.starts_with('f') => {
                    let number = value[1..]
                        .parse::<u8>()
                        .map_err(|_| anyhow::anyhow!("unsupported key {value}"))?;
                    if !(1..=24).contains(&number) {
                        anyhow::bail!("function key must be between F1 and F24");
                    }
                    set_code(&mut code, KeyCode::F(number))?;
                }
                value => anyhow::bail!("unsupported key component {value}"),
            }
        }
        Ok(Self::from_event(
            code.ok_or_else(|| anyhow::anyhow!("key stroke has no key"))?,
            modifiers,
        ))
    }
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            write!(formatter, "Ctrl+")?;
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            write!(formatter, "Alt+")?;
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            write!(formatter, "Shift+")?;
        }
        if self.modifiers.contains(KeyModifiers::SUPER) {
            write!(formatter, "Command+")?;
        }
        if self.modifiers.contains(KeyModifiers::HYPER) {
            write!(formatter, "Hyper+")?;
        }
        if self.modifiers.contains(KeyModifiers::META) {
            write!(formatter, "Meta+")?;
        }
        match self.code {
            KeyCode::Char(character) => write!(formatter, "{}", character.to_ascii_uppercase()),
            KeyCode::Enter => write!(formatter, "Enter"),
            KeyCode::Tab => write!(formatter, "Tab"),
            KeyCode::BackTab => write!(formatter, "BackTab"),
            KeyCode::Backspace => write!(formatter, "Backspace"),
            KeyCode::Delete => write!(formatter, "Delete"),
            KeyCode::Up => write!(formatter, "Up"),
            KeyCode::Down => write!(formatter, "Down"),
            KeyCode::Left => write!(formatter, "Left"),
            KeyCode::Right => write!(formatter, "Right"),
            KeyCode::Home => write!(formatter, "Home"),
            KeyCode::End => write!(formatter, "End"),
            KeyCode::PageUp => write!(formatter, "PageUp"),
            KeyCode::PageDown => write!(formatter, "PageDown"),
            KeyCode::F(number) => write!(formatter, "F{number}"),
            _ => write!(formatter, "{:?}", self.code),
        }
    }
}

impl FromStr for KeyStroke {
    type Err = anyhow::Error;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let stroke = Self::parse_unvalidated(raw)?;
        stroke.validate()?;
        Ok(stroke)
    }
}

fn set_code(target: &mut Option<KeyCode>, code: KeyCode) -> anyhow::Result<()> {
    if target.replace(code).is_some() {
        anyhow::bail!("key stroke has more than one key");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyChord(pub Vec<KeyStroke>);

impl KeyChord {
    pub fn new(strokes: Vec<KeyStroke>) -> anyhow::Result<Self> {
        if !(1..=2).contains(&strokes.len()) {
            anyhow::bail!("a binding must contain one or two key strokes");
        }
        for (index, stroke) in strokes.iter().enumerate() {
            if index != 1 || (!stroke.is_navigation() && !stroke.is_unmodified_character()) {
                stroke.validate()?;
            }
        }
        if strokes
            .first()
            .is_some_and(|stroke| stroke.is_ctrl_char('r'))
        {
            anyhow::bail!("Ctrl+R is permanently reserved for Diff");
        }
        Ok(Self(strokes))
    }

    pub fn first(&self) -> KeyStroke {
        self.0[0]
    }

    pub fn is_two_stroke(&self) -> bool {
        self.0.len() == 2
    }

    pub fn canonical(&self) -> String {
        self.0
            .iter()
            .map(|stroke| stroke.to_string().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}",
            self.0
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

impl FromStr for KeyChord {
    type Err = anyhow::Error;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let strokes = raw
            .split_whitespace()
            .map(KeyStroke::parse_unvalidated)
            .collect::<anyhow::Result<Vec<_>>>()?;
        Self::new(strokes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyResolution {
    Action(KeyAction),
    Prefix,
    NoMatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: BTreeMap<KeyAction, Vec<KeyChord>>,
}

impl Default for Keymap {
    fn default() -> Self {
        let bindings = KeyAction::ALL
            .into_iter()
            .map(|action| (action, action.default_chords()))
            .collect();
        Self { bindings }
    }
}

impl Keymap {
    pub fn from_config(config: &KeymapConfig) -> anyhow::Result<Self> {
        let mut keymap = Self::default();
        for (name, configured) in config {
            let action = KeyAction::parse_config_key(name)
                .ok_or_else(|| anyhow::anyhow!("unknown keymap action {name}"))?;
            let values = configured.values();
            if values.is_empty() || values.len() > MAX_BINDINGS_PER_ACTION {
                anyhow::bail!(
                    "keymap action {name} requires 1 to {MAX_BINDINGS_PER_ACTION} bindings"
                );
            }
            let chords = values
                .into_iter()
                .map(KeyChord::from_str)
                .collect::<anyhow::Result<Vec<_>>>()?;
            keymap.bindings.insert(action, chords);
        }
        keymap.validate()?;
        Ok(keymap)
    }

    pub fn bindings(&self, action: KeyAction) -> &[KeyChord] {
        self.bindings.get(&action).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn display_binding(&self, action: KeyAction) -> String {
        self.bindings(action)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" / ")
    }

    /// First configured chord. Compact hints use this so a narrow footer does
    /// not clip the binding that works while Control is still held.
    pub fn primary_binding(&self, action: KeyAction) -> String {
        self.bindings(action)
            .first()
            .map(ToString::to_string)
            .unwrap_or_default()
    }

    pub fn resolve(&self, first: KeyStroke, second: Option<KeyStroke>) -> KeyResolution {
        let mut prefix = false;
        let mut held_control = None;
        for (action, chords) in &self.bindings {
            for chord in chords {
                if chord.first() != first {
                    continue;
                }
                match (chord.is_two_stroke(), second) {
                    (false, None) => return KeyResolution::Action(*action),
                    (true, None) => prefix = true,
                    (true, Some(stroke)) if chord.0[1] == stroke => {
                        return KeyResolution::Action(*action);
                    }
                    // An unmatched Ctrl+R remains the fixed Diff shortcut;
                    // only an explicit Ctrl+R second stroke may consume it.
                    (true, Some(stroke))
                        if held_control.is_none()
                            && !stroke.is_ctrl_char('r')
                            && stroke
                                .without_held_control()
                                .is_some_and(|plain| chord.0[1] == plain) =>
                    {
                        held_control = Some(*action);
                    }
                    _ => {}
                }
            }
        }
        if let Some(action) = held_control {
            return KeyResolution::Action(action);
        }
        if prefix && second.is_none() {
            KeyResolution::Prefix
        } else {
            KeyResolution::NoMatch
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        let mut seen = HashSet::new();
        let mut prefixes = HashSet::new();
        for chords in self.bindings.values() {
            for chord in chords {
                if !seen.insert(chord.clone()) {
                    anyhow::bail!("duplicate key binding {}", chord);
                }
                if chord.is_two_stroke() {
                    prefixes.insert(chord.first());
                }
            }
        }
        for chords in self.bindings.values() {
            for chord in chords {
                if !chord.is_two_stroke() && prefixes.contains(&chord.first()) {
                    anyhow::bail!("key binding {} is also a two-stroke prefix", chord);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_existing_shortcuts_and_keep_ctrl_r_fixed() {
        let map = Keymap::default();
        assert_eq!(
            map.display_binding(KeyAction::ModelPicker),
            "Ctrl+K / Ctrl+L"
        );
        assert_eq!(map.display_binding(KeyAction::CycleReasoning), "Ctrl+T");
        assert_eq!(
            map.display_binding(KeyAction::HistorySearch),
            "Ctrl+X Ctrl+R"
        );
        assert_eq!(map.display_binding(KeyAction::StashDraft), "Ctrl+X S");
        assert_eq!(map.display_binding(KeyAction::CycleRecentModel), "Ctrl+X R");
        assert_eq!(
            map.display_binding(KeyAction::CycleFavoriteModel),
            "Ctrl+X F"
        );
        assert_eq!(
            map.display_binding(KeyAction::FocusAgentActivity),
            "Ctrl+X J / Ctrl+X Down"
        );
        assert_eq!(
            map.primary_binding(KeyAction::FocusAgentActivity),
            "Ctrl+X J"
        );
        assert_eq!(
            map.display_binding(KeyAction::RemoveLastAttachment),
            "Ctrl+X A"
        );
        assert_eq!(map.display_binding(KeyAction::EditLastQueued), "Alt+Up");
        assert_eq!(map.display_binding(KeyAction::TranscriptTop), "Ctrl+Home");
        assert_eq!(map.display_binding(KeyAction::TranscriptBottom), "Ctrl+End");
        assert_eq!(map.display_binding(KeyAction::CopyTranscript), "Ctrl+X C");
        assert_eq!(map.display_binding(KeyAction::SplitHorizontal), "Ctrl+X H");
        assert_eq!(map.display_binding(KeyAction::SplitVertical), "Ctrl+X V");
        assert_eq!(map.display_binding(KeyAction::NextPane), "Ctrl+X N");
        assert_eq!(map.display_binding(KeyAction::PreviousPane), "Ctrl+X P");
        assert_eq!(map.display_binding(KeyAction::ClosePane), "Ctrl+X Q");
        assert_eq!(
            map.resolve(
                KeyStroke::from_event(KeyCode::Char('r'), KeyModifiers::CONTROL),
                None
            ),
            KeyResolution::NoMatch
        );
    }

    #[test]
    fn native_mac_composer_keys_are_not_remappable_or_stripped() {
        assert!("alt+backspace".parse::<KeyChord>().is_err());
        let command_backspace = KeyStroke::from_event(KeyCode::Backspace, KeyModifiers::SUPER);
        assert_eq!(command_backspace.modifiers, KeyModifiers::SUPER);
    }

    #[test]
    fn navigation_is_allowed_only_as_a_second_chord_stroke() {
        let chord: KeyChord = "ctrl+x down".parse().unwrap();
        assert_eq!(chord.to_string(), "Ctrl+X Down");
        assert!("down".parse::<KeyChord>().is_err());
        assert!("down ctrl+x".parse::<KeyChord>().is_err());
        assert!("s".parse::<KeyChord>().is_err());
        assert_eq!(
            "ctrl+x s".parse::<KeyChord>().unwrap().to_string(),
            "Ctrl+X S"
        );

        let map = Keymap::default();
        let ctrl_x = KeyStroke::from_event(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(
            map.resolve(
                ctrl_x,
                Some(KeyStroke::from_event(KeyCode::Down, KeyModifiers::NONE)),
            ),
            KeyResolution::Action(KeyAction::FocusAgentActivity)
        );
        assert_eq!(
            map.resolve(
                ctrl_x,
                Some(KeyStroke::from_event(KeyCode::Down, KeyModifiers::CONTROL)),
            ),
            KeyResolution::Action(KeyAction::FocusAgentActivity)
        );
        assert_eq!(
            map.resolve(
                ctrl_x,
                Some(KeyStroke::from_event(
                    KeyCode::Char('j'),
                    KeyModifiers::NONE
                )),
            ),
            KeyResolution::Action(KeyAction::FocusAgentActivity)
        );
        assert_eq!(
            map.resolve(
                ctrl_x,
                Some(KeyStroke::from_event(
                    KeyCode::Char('j'),
                    KeyModifiers::CONTROL
                )),
            ),
            KeyResolution::Action(KeyAction::FocusAgentActivity)
        );
        assert_eq!(
            map.resolve(
                ctrl_x,
                Some(KeyStroke::from_event(
                    KeyCode::Down,
                    KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                )),
            ),
            KeyResolution::NoMatch
        );
        assert_eq!(
            map.resolve(
                ctrl_x,
                Some(KeyStroke::from_event(
                    KeyCode::Char('r'),
                    KeyModifiers::CONTROL
                )),
            ),
            KeyResolution::Action(KeyAction::HistorySearch)
        );
    }

    #[test]
    fn parsing_normalizes_and_resolves_one_and_two_stroke_bindings() {
        let chord: KeyChord = "Control+Shift+K".parse().unwrap();
        assert_eq!(chord.to_string(), "Ctrl+Shift+K");
        let map = Keymap::default();
        let first = KeyStroke::from_event(KeyCode::Char('x'), KeyModifiers::CONTROL);
        let second = KeyStroke::from_event(KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert_eq!(map.resolve(first, None), KeyResolution::Prefix);
        assert_eq!(
            map.resolve(first, Some(second)),
            KeyResolution::Action(KeyAction::HistorySearch)
        );
    }

    #[test]
    fn unmatched_ctrl_r_does_not_alias_a_plain_r_second_stroke() {
        let config = KeymapConfig::from([(
            "history_search".into(),
            KeyBindingConfig::One("ctrl+x ctrl+h".into()),
        )]);
        let map = Keymap::from_config(&config).unwrap();
        let first = KeyStroke::from_event(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(
            map.resolve(
                first,
                Some(KeyStroke::from_event(
                    KeyCode::Char('r'),
                    KeyModifiers::CONTROL,
                )),
            ),
            KeyResolution::NoMatch
        );
        assert_eq!(
            map.resolve(
                first,
                Some(KeyStroke::from_event(
                    KeyCode::Char('r'),
                    KeyModifiers::NONE
                )),
            ),
            KeyResolution::Action(KeyAction::CycleRecentModel)
        );
    }

    #[test]
    fn older_partial_keymap_configuration_inherits_new_model_shortcuts() {
        let config =
            KeymapConfig::from([("cycle_model".into(), KeyBindingConfig::One("ctrl+q".into()))]);
        let map = Keymap::from_config(&config).unwrap();
        assert_eq!(map.display_binding(KeyAction::CycleModel), "Ctrl+Q");
        assert_eq!(map.display_binding(KeyAction::CycleRecentModel), "Ctrl+X R");
        assert_eq!(
            map.display_binding(KeyAction::CycleFavoriteModel),
            "Ctrl+X F"
        );
        assert_eq!(map.display_binding(KeyAction::SplitVertical), "Ctrl+X V");
    }

    #[test]
    fn malformed_unknown_reserved_duplicate_and_prefix_conflicts_fail() {
        for invalid in [
            "",
            "x",
            "ctrl+c",
            "ctrl+d",
            "ctrl+r",
            "ctrl+w",
            "alt+y",
            "shift+enter",
            "up",
            "ctrl+x ctrl+y ctrl+z",
        ] {
            assert!(invalid.parse::<KeyChord>().is_err(), "accepted {invalid}");
        }

        let mut unknown = KeymapConfig::new();
        unknown.insert("mystery".into(), KeyBindingConfig::One("ctrl+q".into()));
        assert!(Keymap::from_config(&unknown)
            .unwrap_err()
            .to_string()
            .contains("unknown"));

        let mut duplicate = KeymapConfig::new();
        duplicate.insert(
            "model_picker".into(),
            KeyBindingConfig::One("ctrl+q".into()),
        );
        duplicate.insert("open_editor".into(), KeyBindingConfig::One("ctrl+q".into()));
        assert!(Keymap::from_config(&duplicate)
            .unwrap_err()
            .to_string()
            .contains("duplicate"));

        let mut prefix = KeymapConfig::new();
        prefix.insert(
            "model_picker".into(),
            KeyBindingConfig::One("ctrl+x".into()),
        );
        assert!(Keymap::from_config(&prefix)
            .unwrap_err()
            .to_string()
            .contains("prefix"));
    }
}
