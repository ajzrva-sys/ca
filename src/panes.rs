use crate::app::AppState;
use ratatui::layout::Rect;
use std::collections::BTreeMap;

pub const MAX_PANES: usize = 4;
pub const MIN_PANE_WIDTH: u16 = 36;
pub const MIN_PANE_HEIGHT: u16 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaneId(u8);

impl PaneId {
    pub const PRIMARY: Self = Self(1);

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl From<u8> for PaneId {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for PaneId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

impl SplitAxis {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneRequest {
    Split {
        axis: SplitAxis,
        session: Option<String>,
    },
    FocusNext,
    FocusPrevious,
    Focus(PaneId),
    Close,
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneLayout {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        first: Box<Self>,
        second: Box<Self>,
    },
}

impl PaneLayout {
    fn leaves(&self, output: &mut Vec<PaneId>) {
        match self {
            Self::Leaf(id) => output.push(*id),
            Self::Split { first, second, .. } => {
                first.leaves(output);
                second.leaves(output);
            }
        }
    }

    fn split_leaf(&mut self, target: PaneId, axis: SplitAxis, added: PaneId) -> bool {
        match self {
            Self::Leaf(id) if *id == target => {
                *self = Self::Split {
                    axis,
                    first: Box::new(Self::Leaf(target)),
                    second: Box::new(Self::Leaf(added)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_leaf(target, axis, added) || second.split_leaf(target, axis, added)
            }
        }
    }

    fn remove_leaf(self, target: PaneId) -> Option<Self> {
        match self {
            Self::Leaf(id) => (id != target).then_some(Self::Leaf(id)),
            Self::Split {
                axis,
                first,
                second,
            } => match (first.remove_leaf(target), second.remove_leaf(target)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    axis,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(remaining), None) | (None, Some(remaining)) => Some(remaining),
                (None, None) => None,
            },
        }
    }

    pub fn regions(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        let mut output = Vec::new();
        self.collect_regions(area, &mut output);
        output
    }

    fn collect_regions(&self, area: Rect, output: &mut Vec<(PaneId, Rect)>) {
        match self {
            Self::Leaf(id) => output.push((*id, area)),
            Self::Split {
                axis,
                first,
                second,
            } => {
                let (first_area, second_area) = split_rect(area, *axis);
                first.collect_regions(first_area, output);
                second.collect_regions(second_area, output);
            }
        }
    }

    fn region_for(&self, area: Rect, target: PaneId) -> Option<Rect> {
        self.regions(area)
            .into_iter()
            .find_map(|(id, region)| (id == target).then_some(region))
    }
}

fn split_rect(area: Rect, axis: SplitAxis) -> (Rect, Rect) {
    match axis {
        SplitAxis::Horizontal => {
            let first_height = area.height / 2;
            (
                Rect::new(area.x, area.y, area.width, first_height),
                Rect::new(
                    area.x,
                    area.y.saturating_add(first_height),
                    area.width,
                    area.height.saturating_sub(first_height),
                ),
            )
        }
        SplitAxis::Vertical => {
            let first_width = area.width / 2;
            (
                Rect::new(area.x, area.y, first_width, area.height),
                Rect::new(
                    area.x.saturating_add(first_width),
                    area.y,
                    area.width.saturating_sub(first_width),
                    area.height,
                ),
            )
        }
    }
}

#[derive(Debug)]
pub struct PaneDeck {
    focused: PaneId,
    next_id: u8,
    layout: PaneLayout,
    inactive: BTreeMap<PaneId, Box<AppState>>,
}

impl PaneDeck {
    pub fn single() -> Self {
        Self {
            focused: PaneId::PRIMARY,
            next_id: PaneId::PRIMARY.get().saturating_add(1),
            layout: PaneLayout::Leaf(PaneId::PRIMARY),
            inactive: BTreeMap::new(),
        }
    }

    pub const fn focused(&self) -> PaneId {
        self.focused
    }

    pub fn len(&self) -> usize {
        self.inactive.len().saturating_add(1)
    }

    pub fn is_single(&self) -> bool {
        self.inactive.is_empty()
    }

    pub const fn layout(&self) -> &PaneLayout {
        &self.layout
    }

    pub fn pane<'a>(&'a self, materialized: &'a AppState, id: PaneId) -> Option<&'a AppState> {
        if materialized.pane_id == id {
            Some(materialized)
        } else {
            self.inactive.get(&id).map(Box::as_ref)
        }
    }

    pub fn ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::with_capacity(self.len());
        self.layout.leaves(&mut ids);
        ids
    }

    pub fn next_after(&self, current: PaneId) -> PaneId {
        let ids = self.ids();
        let index = ids.iter().position(|id| *id == current).unwrap_or(0);
        ids[(index + 1) % ids.len()]
    }

    pub fn previous_before(&self, current: PaneId) -> PaneId {
        let ids = self.ids();
        let index = ids.iter().position(|id| *id == current).unwrap_or(0);
        ids[(index + ids.len() - 1) % ids.len()]
    }

    pub fn pane_at(&self, area: Rect, column: u16, row: u16) -> Option<PaneId> {
        self.layout
            .regions(area)
            .into_iter()
            .find_map(|(id, rect)| {
                let contained = column >= rect.x
                    && column < rect.right()
                    && row >= rect.y
                    && row < rect.bottom();
                contained.then_some(id)
            })
    }

    pub fn can_split(&self, area: Rect, target: PaneId, axis: SplitAxis) -> bool {
        if self.len() >= MAX_PANES {
            return false;
        }
        let Some(region) = self.layout.region_for(area, target) else {
            return false;
        };
        match axis {
            SplitAxis::Horizontal => region.height >= MIN_PANE_HEIGHT.saturating_mul(2),
            SplitAxis::Vertical => region.width >= MIN_PANE_WIDTH.saturating_mul(2),
        }
    }

    pub fn split(
        &mut self,
        materialized: &AppState,
        axis: SplitAxis,
        mut added: AppState,
        area: Rect,
    ) -> Result<PaneId, String> {
        if !self.can_split(area, self.focused, axis) {
            return Err(if self.len() >= MAX_PANES {
                format!("At most {MAX_PANES} panes may be open")
            } else {
                format!(
                    "The focused pane is too small for a {} split (minimum {}×{})",
                    axis.label(),
                    MIN_PANE_WIDTH,
                    MIN_PANE_HEIGHT
                )
            });
        }
        debug_assert_eq!(materialized.pane_id, self.focused);
        let id = PaneId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        added.pane_id = id;
        added.pane_deck = None;
        if !self.layout.split_leaf(self.focused, axis, id) {
            return Err("The focused pane is no longer present".into());
        }
        self.inactive.insert(id, Box::new(added));
        self.focused = id;
        Ok(id)
    }

    fn take_inactive(&mut self, id: PaneId) -> Option<AppState> {
        self.inactive.remove(&id).map(|state| *state)
    }

    fn store_inactive(&mut self, state: AppState) {
        self.inactive.insert(state.pane_id, Box::new(state));
    }

    fn remove_layout_leaf(&mut self, id: PaneId) -> Result<(), String> {
        self.layout = self
            .layout
            .clone()
            .remove_leaf(id)
            .ok_or_else(|| "Cannot close the only pane".to_string())?;
        Ok(())
    }
}

pub fn materialize(state: &mut AppState, target: PaneId) -> Result<(), String> {
    if state.pane_id == target {
        return Ok(());
    }
    let mut deck = state
        .pane_deck
        .take()
        .ok_or_else(|| "Pane ownership is unavailable".to_string())?;
    let Some(mut replacement) = deck.take_inactive(target) else {
        state.pane_deck = Some(deck);
        return Err(format!("Pane {target} is unavailable"));
    };
    replacement.pane_deck = None;
    let mut previous = std::mem::replace(state, replacement);
    previous.pane_deck = None;
    sync_process_services(&previous, state);
    deck.store_inactive(previous);
    state.pane_deck = Some(deck);
    Ok(())
}

fn sync_process_services(source: &AppState, target: &mut AppState) {
    target.session_store = source.session_store.clone();
    target.hook_runner = source.hook_runner.clone();
    target.runtime_diagnostics = source.runtime_diagnostics.clone();
    target.active_profile = source.active_profile.clone();
    target.tool_policy = source.tool_policy.clone();
    target.config_path = source.config_path.clone();
    target.run_capabilities = source.run_capabilities;
    target.keymap = source.keymap.clone();
    target.keymap_customized = source.keymap_customized.clone();
    target.model_list = source.model_list.clone();
    target.model_preferences = source.model_preferences.clone();
    target.model_preferences_store = source.model_preferences_store.clone();
    target.discovered_models = source.discovered_models.clone();
    target.model_catalog_loaded = source.model_catalog_loaded;
    target.model_catalog_status = source.model_catalog_status.clone();
    target.model_catalog_provider_status = source.model_catalog_provider_status.clone();
    target.provider_checks = source.provider_checks.clone();
    target.prompt_stash_store = source.prompt_stash_store.clone();
    target.workspace_catalog = source.workspace_catalog.clone();
    target.learning_ledger = source.learning_ledger.clone();
    target.skill_curator = source.skill_curator.clone();
    target.provider_trace = source.provider_trace.clone();
    target.ui_theme = source.ui_theme;
    target.terminal_theme_auto = source.terminal_theme_auto;
    target.terminal_appearance = source.terminal_appearance;
    target.terminal_focused = source.terminal_focused;
    target.terminal_hyperlinks_supported = source.terminal_hyperlinks_supported;
    target.status_indicators_enabled = source.status_indicators_enabled;
    target.notification_style = source.notification_style;
    target.mouse_mode = source.mouse_mode;
    target.command_output_policy = source.command_output_policy;
    target.statusline_fields = source.statusline_fields.clone();
    target.statusline_custom_configured = source.statusline_custom_configured;
    target.statusline_custom_enabled = source.statusline_custom_enabled;
    target.statusline_refresh_secs = source.statusline_refresh_secs;
    target.keep_awake_process_override = source.keep_awake_process_override;
    target.usage_ledger = source.usage_ledger.clone();
    target.official_pricing = source.official_pricing.clone();
    target.request_store = source.request_store.clone();
}

pub fn focus(state: &mut AppState, target: PaneId) -> Result<(), String> {
    let switching = state.pane_id != target;
    {
        let deck = state
            .pane_deck
            .as_mut()
            .ok_or_else(|| "Pane ownership is unavailable".to_string())?;
        if !deck.ids().contains(&target) {
            return Err(format!("Pane {target} is unavailable"));
        }
        deck.focused = target;
    }
    if switching {
        state.workspace.clear_session_writes();
    }
    materialize(state, target)?;
    if switching {
        state.workspace.clear_session_writes();
    }
    Ok(())
}

pub fn close_focused(state: &mut AppState) -> Result<(PaneId, AppState), String> {
    let current = state.pane_id;
    let mut deck = state
        .pane_deck
        .take()
        .ok_or_else(|| "Pane ownership is unavailable".to_string())?;
    if deck.len() == 1 {
        state.pane_deck = Some(deck);
        return Err("Cannot close the only pane".into());
    }
    let next = deck.next_after(current);
    deck.remove_layout_leaf(current)?;
    deck.focused = next;
    let Some(mut replacement) = deck.take_inactive(next) else {
        state.pane_deck = Some(deck);
        return Err(format!("Pane {next} is unavailable"));
    };
    replacement.pane_deck = None;
    let mut closed = std::mem::replace(state, replacement);
    closed.pane_deck = None;
    state.pane_deck = Some(deck);
    Ok((next, closed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        AppState::default()
    }

    #[test]
    fn layout_is_bounded_and_preserves_nested_regions() {
        let mut active = state();
        let area = Rect::new(0, 0, 160, 48);
        let mut deck = active.pane_deck.take().unwrap();
        let second = deck
            .split(&active, SplitAxis::Vertical, state(), area)
            .unwrap();
        active.pane_deck = Some(deck);
        focus(&mut active, second).unwrap();

        let mut deck = active.pane_deck.take().unwrap();
        let third = deck
            .split(&active, SplitAxis::Horizontal, state(), area)
            .unwrap();
        active.pane_deck = Some(deck);
        focus(&mut active, third).unwrap();

        let deck = active.pane_deck.as_ref().unwrap();
        let regions = deck.layout().regions(area);
        assert_eq!(regions.len(), 3);
        assert!(regions.iter().all(|(_, region)| {
            region.width >= MIN_PANE_WIDTH && region.height >= MIN_PANE_HEIGHT
        }));
    }

    #[test]
    fn fifth_pane_and_undersized_splits_are_rejected() {
        let mut active = state();
        let area = Rect::new(0, 0, 200, 80);
        for axis in [
            SplitAxis::Vertical,
            SplitAxis::Horizontal,
            SplitAxis::Vertical,
        ] {
            let mut deck = active.pane_deck.take().unwrap();
            let id = deck.split(&active, axis, state(), area).unwrap();
            active.pane_deck = Some(deck);
            focus(&mut active, id).unwrap();
        }
        let mut deck = active.pane_deck.take().unwrap();
        assert!(deck
            .split(&active, SplitAxis::Horizontal, state(), area)
            .unwrap_err()
            .contains("At most 4"));
        active.pane_deck = Some(deck);

        let mut small = state();
        let mut deck = small.pane_deck.take().unwrap();
        assert!(deck
            .split(
                &small,
                SplitAxis::Vertical,
                state(),
                Rect::new(0, 0, 60, 30),
            )
            .is_err());
        small.pane_deck = Some(deck);
    }

    #[test]
    fn focus_materializes_complete_isolated_states_and_close_keeps_sessions() {
        let mut active = state();
        active.input = "first draft".into();
        let area = Rect::new(0, 0, 120, 40);
        let mut other = state();
        other.input = "second draft".into();
        let mut deck = active.pane_deck.take().unwrap();
        let second = deck
            .split(&active, SplitAxis::Vertical, other, area)
            .unwrap();
        active.pane_deck = Some(deck);

        focus(&mut active, second).unwrap();
        assert_eq!(active.input, "second draft");
        focus(&mut active, PaneId::PRIMARY).unwrap();
        assert_eq!(active.input, "first draft");
        focus(&mut active, second).unwrap();
        assert_eq!(close_focused(&mut active).unwrap().0, PaneId::PRIMARY);
        assert_eq!(active.input, "first draft");
        assert!(active.pane_deck.as_ref().unwrap().is_single());
    }

    #[test]
    fn hit_testing_follows_layout_geometry() {
        let mut active = state();
        let area = Rect::new(0, 0, 120, 30);
        let mut deck = active.pane_deck.take().unwrap();
        let second = deck
            .split(&active, SplitAxis::Vertical, state(), area)
            .unwrap();
        assert_eq!(deck.pane_at(area, 5, 5), Some(PaneId::PRIMARY));
        assert_eq!(deck.pane_at(area, 100, 5), Some(second));
        active.pane_deck = Some(deck);
    }

    #[test]
    fn fresh_panes_share_process_configuration_but_not_session_authority() {
        let mut source = state();
        source.cwd = "/workspace".into();
        source.input = "private draft".into();
        source.messages.push(crate::app::Message {
            role: crate::app::Role::User,
            content: "private transcript".into(),
            tool_calls: None,
            thinking: None,
            timestamp: chrono::Utc::now(),
            response_items: Vec::new(),
        });
        source.manual_rule_ids.insert("manual-rule".into());
        let pane = source.new_pane_from();

        assert_eq!(pane.cwd, source.cwd);
        assert_eq!(pane.current_model, source.current_model);
        assert!(pane.input.is_empty());
        assert!(pane.messages.is_empty());
        assert!(pane.manual_rule_ids.is_empty());
        assert_ne!(pane.job_owner, source.job_owner);
        assert!(!std::sync::Arc::ptr_eq(
            &pane.session_grants,
            &source.session_grants
        ));
        assert!(!std::sync::Arc::ptr_eq(
            &pane.work_state,
            &source.work_state
        ));
    }
}
