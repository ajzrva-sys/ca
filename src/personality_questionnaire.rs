//! Provider-free orchestration for the same questionnaire used in ordinary CA.
use crate::personality_preferences::{Preferences, Snapshot};
use crate::runtime::{ActiveProfile, RuntimePaths};
use crate::tui::personality::{Action, Editor};
use anyhow::{bail, Result};
use std::io::IsTerminal;

#[derive(Debug, Clone)]
pub(crate) enum Operation {
    Load,
    Save {
        revision: String,
        preferences: Preferences,
    },
}

pub(crate) fn perform(
    paths: &RuntimePaths,
    profile: &ActiveProfile,
    operation: Operation,
) -> Result<Snapshot> {
    match operation {
        Operation::Load => crate::personality_preferences::load(paths, profile),
        Operation::Save {
            revision,
            preferences,
        } => crate::personality_preferences::save(paths, profile, &revision, &preferences),
    }
}

pub(crate) fn interactive_terminal() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// The standalone editor has no provider configuration or network dependency.
pub(crate) async fn run(paths: RuntimePaths, profile: ActiveProfile) -> Result<()> {
    if matches!(profile, ActiveProfile::Bare) {
        bail!("personality preferences need a persistent profile");
    }
    if !interactive_terminal() {
        bail!("the interactive questionnaire requires a terminal");
    }
    let mut terminal = crate::tui::event::Tui::new(crate::ui_settings::MouseMode::Scroll)?;
    let mut editor = Editor::loading(profile.label().to_string());
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let load_paths = paths.clone();
    let load_profile = profile.clone();
    let load_sender = sender.clone();
    tokio::task::spawn_blocking(move || {
        let result =
            perform(&load_paths, &load_profile, Operation::Load).map_err(|error| error.to_string());
        let _ = load_sender.send((false, result));
    });
    loop {
        terminal.terminal.draw(|frame| {
            crate::tui::personality::render(frame, frame.area(), &editor);
        })?;
        tokio::select! {
            Some((saving, result)) = receiver.recv() => {
                if saving {
                    editor.save_completed(result);
                } else {
                    editor.load_completed(result);
                }
            }
            event = terminal.next_event_async() => {
                match editor.handle(&event?) {
                    Action::None => {}
                    Action::Close => break,
                    Action::Save { revision, preferences } => {
                        let save_paths = paths.clone();
                        let save_profile = profile.clone();
                        let save_sender = sender.clone();
                        tokio::task::spawn_blocking(move || {
                            let result = perform(&save_paths, &save_profile,
                                Operation::Save { revision, preferences })
                                .map_err(|error| error.to_string());
                            let _ = save_sender.send((true, result));
                        });
                    }
                }
            }
        }
    }
    Ok(())
}
