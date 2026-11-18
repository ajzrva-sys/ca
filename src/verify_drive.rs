use clap::{Parser, Subcommand};
use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MARKER_NAME: &str = ".ca-verify-drive";
const MARKER_BODY: &[u8] = b"in_process\n";
const MIN_COLUMNS: u16 = 80;
const MIN_ROWS: u16 = 24;

#[derive(Debug, Clone)]
struct ForbiddenTrees {
    config: PathBuf,
    data: PathBuf,
}

#[derive(Debug, Parser)]
#[command(name = "ca verify", about = "Drive an isolated ca and check the proof")]
pub struct VerifyCliArgs {
    #[command(subcommand)]
    command: VerifyCommand,
}

#[derive(Debug, Subcommand)]
enum VerifyCommand {
    Doctor {
        #[arg(long)]
        home: PathBuf,
    },
    Drive {
        #[arg(long)]
        home: PathBuf,
        #[arg(long)]
        script: PathBuf,
        #[arg(long)]
        proof: PathBuf,
        #[arg(long, default_value_t = 100)]
        columns: u16,
        #[arg(long, default_value_t = 32)]
        rows: u16,
    },
    ProofCheck {
        #[arg(long)]
        proof: PathBuf,
    },
    Cleanup {
        #[arg(long)]
        home: PathBuf,
        #[arg(long)]
        proof: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
}

struct DriveRequest {
    home: PathBuf,
    script: PathBuf,
    proof: PathBuf,
    columns: u16,
    rows: u16,
    forbidden: ForbiddenTrees,
}

enum ScriptStep {
    PressShiftTab,
    AssertContains(String),
}

#[derive(Serialize)]
struct CommandReport {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    driver: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    home: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    proof: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transcript: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dry_run: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    would_delete: Vec<String>,
}

pub fn parse_args<I, T>(args: I) -> Result<VerifyCliArgs, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    VerifyCliArgs::try_parse_from(args)
}

pub fn run_cli(cli: VerifyCliArgs) -> i32 {
    let paths = match crate::runtime::RuntimePaths::discover() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("verify: {error}. Set HOME, then retry ca verify.");
            return 2;
        }
    };
    let forbidden = ForbiddenTrees {
        config: paths.config_root,
        data: paths.data_dir,
    };
    match run(cli, &forbidden) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(error) => {
            eprintln!("verify: {error}");
            2
        }
    }
}

fn run(cli: VerifyCliArgs, forbidden: &ForbiddenTrees) -> Result<String, String> {
    match cli.command {
        VerifyCommand::Doctor { home } => {
            doctor(&home, forbidden)?;
            report(&CommandReport {
                ok: true,
                home: Some(display_path(&home)),
                ..CommandReport::cleared()
            })
        }
        VerifyCommand::Drive {
            home,
            script,
            proof,
            columns,
            rows,
        } => {
            let proof_value = drive(DriveRequest {
                home,
                script,
                proof,
                columns,
                rows,
                forbidden: forbidden.clone(),
            })?;
            report(&CommandReport {
                ok: true,
                driver: Some("in_process"),
                proof: Some(proof_value.proof_path),
                transcript: Some(proof_value.transcript),
                digest: Some(proof_value.digest),
                exit_code: Some(0),
                ..CommandReport::cleared()
            })
        }
        VerifyCommand::ProofCheck { proof } => {
            proof_check(&proof)?;
            report(&CommandReport {
                ok: true,
                driver: Some("in_process"),
                proof: Some(display_path(&proof)),
                ..CommandReport::cleared()
            })
        }
        VerifyCommand::Cleanup {
            home,
            proof,
            dry_run,
        } => cleanup(&home, &proof, dry_run, forbidden),
    }
}

impl CommandReport {
    fn cleared() -> Self {
        Self {
            ok: false,
            driver: None,
            home: None,
            proof: None,
            transcript: None,
            digest: None,
            exit_code: None,
            dry_run: None,
            would_delete: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct WrittenProof {
    proof_path: String,
    transcript: String,
    digest: String,
}

fn doctor(home: &Path, forbidden: &ForbiddenTrees) -> Result<(), String> {
    require_absolute(home, "home")?;
    refuse_user_tree(home, forbidden, "home")?;
    if !home.exists() {
        return Err(
            "This home does not exist. Run ca verify drive with a new --home so it can prepare the marker."
                .into(),
        );
    }
    if !marker_ok(home) {
        return Err(
            "This directory is not a verify-drive home. Pass a new directory that drive can create."
                .into(),
        );
    }
    Ok(())
}

fn drive(request: DriveRequest) -> Result<WrittenProof, String> {
    require_absolute(&request.home, "home")?;
    require_absolute(&request.proof, "proof")?;
    if request.columns < MIN_COLUMNS || request.rows < MIN_ROWS {
        return Err(format!(
            "Use at least {MIN_COLUMNS} columns and {MIN_ROWS} rows so the mode toast stays on one line."
        ));
    }
    refuse_user_tree(&request.home, &request.forbidden, "home")?;
    refuse_user_tree(&request.proof, &request.forbidden, "proof")?;
    if path_is_within(&request.proof, &request.home) {
        return Err(
            "Put the proof outside the home. Cleanup deletes the home and must leave the proof."
                .into(),
        );
    }
    let transcript_path = request.proof.with_extension("transcript");
    if path_is_within(&transcript_path, &request.home) {
        return Err(
            "Put the proof outside the home. Cleanup deletes the home and must leave the proof."
                .into(),
        );
    }
    if request.home.exists() && !marker_ok(&request.home) {
        return Err(
            "This directory is not a verify-drive home. Pass a new directory that drive can create."
                .into(),
        );
    }
    if !request.home.exists() {
        prepare_home(&request.home)?;
    }
    doctor(&request.home, &request.forbidden)?;
    let script_text = fs::read_to_string(&request.script).map_err(|error| {
        format!(
            "Could not read {}. {error}. Pass a script of press and assert lines.",
            request.script.display()
        )
    })?;
    let steps = parse_script(&script_text)?;
    let (transcript, asserted) =
        render_script(&request.home, request.columns, request.rows, &steps)?;
    let parent = request.proof.parent().ok_or_else(|| {
        "The proof path has no directory. Pass a file path such as /tmp/ca-verify/proof.json."
            .to_string()
    })?;
    if !parent.is_dir() {
        return Err(format!(
            "Create the proof directory {} before driving.",
            parent.display()
        ));
    }
    let digest = crate::workflow::proof_digest(transcript.as_bytes());
    let home = canonical_path(&request.home)?;
    let proof = crate::workflow::VerificationProof {
        id: script_id(&request.script),
        kind: crate::workflow::ProofKind::Screen,
        driver: crate::workflow::ProofDriver::InProcess,
        columns: Some(request.columns),
        rows: Some(request.rows),
        asserted,
        command_digest: None,
        output_digest: digest.clone(),
        exit_code: 0,
        artifact_path: display_path(&transcript_path),
        cleanup_may_delete: vec![display_path(&home)],
    };
    let json = serde_json::to_vec_pretty(&proof)
        .map_err(|_| "The proof could not be encoded. Retry the drive.".to_string())?;
    write_new(&transcript_path, transcript.as_bytes())?;
    write_new(&request.proof, &json)?;
    Ok(WrittenProof {
        proof_path: display_path(&request.proof),
        transcript: display_path(&transcript_path),
        digest,
    })
}

fn proof_check(proof_path: &Path) -> Result<(), String> {
    require_absolute(proof_path, "proof")?;
    let bytes = fs::read(proof_path).map_err(|_| {
        format!(
            "The proof {} is missing. Drive the script again.",
            proof_path.display()
        )
    })?;
    let proof: crate::workflow::VerificationProof =
        serde_json::from_slice(&bytes).map_err(|_| {
            format!(
                "The proof {} is not a verification proof. Drive the script again.",
                proof_path.display()
            )
        })?;
    if proof.driver != crate::workflow::ProofDriver::InProcess {
        return Err("The proof driver is not in_process. Drive the script again.".into());
    }
    if proof.exit_code != 0 {
        return Err(
            "The proof records a failed drive. Drive the script again before using it.".into(),
        );
    }
    let artifact = PathBuf::from(&proof.artifact_path);
    if !artifact.is_absolute() {
        return Err(
            "The proof names a relative transcript. Drive the script again with an absolute --proof."
                .into(),
        );
    }
    let transcript = fs::read(&artifact).map_err(|_| {
        format!(
            "The transcript {} is missing. Restore it or drive the script again.",
            artifact.display()
        )
    })?;
    if crate::workflow::proof_digest(&transcript) != proof.output_digest {
        return Err(
            "The transcript does not match the proof. It may have been truncated. Drive the script again."
                .into(),
        );
    }
    let text = String::from_utf8(transcript).map_err(|_| {
        "The transcript is not text. Drive the script again so the screen cells are recorded."
            .to_string()
    })?;
    for expected in &proof.asserted {
        if !text.contains(expected) {
            return Err(format!(
                "The transcript is missing {expected:?}. Drive the script again."
            ));
        }
    }
    Ok(())
}

fn cleanup(
    home: &Path,
    proof_path: &Path,
    dry_run: bool,
    forbidden: &ForbiddenTrees,
) -> Result<String, String> {
    require_absolute(home, "home")?;
    require_absolute(proof_path, "proof")?;
    refuse_user_tree(home, forbidden, "home")?;
    if path_is_within(proof_path, home)
        || path_is_within(&proof_path.with_extension("transcript"), home)
    {
        return Err(
            "Put the proof outside the home. Cleanup deletes the home and must leave the proof."
                .into(),
        );
    }
    if !marker_ok(home) {
        return Err(
            "This directory is not a verify-drive home. Cleanup only deletes a home this drive created."
                .into(),
        );
    }
    let bytes = fs::read(proof_path).map_err(|_| {
        format!(
            "The proof {} is missing. Drive again before cleanup.",
            proof_path.display()
        )
    })?;
    let proof: crate::workflow::VerificationProof =
        serde_json::from_slice(&bytes).map_err(|_| {
            "The proof is not a verification proof. Drive again before cleanup.".to_string()
        })?;
    let home_canon = canonical_path(home)?;
    let listed = proof.cleanup_may_delete.iter().any(|item| {
        Path::new(item)
            .canonicalize()
            .ok()
            .is_some_and(|path| path == home_canon)
    });
    if !listed {
        return Err(
            "The proof does not list this home as removable. Drive again so the proof names the home."
                .into(),
        );
    }
    if dry_run {
        return report(&CommandReport {
            ok: true,
            dry_run: Some(true),
            home: Some(display_path(&home_canon)),
            proof: Some(display_path(proof_path)),
            would_delete: vec![display_path(&home_canon)],
            ..CommandReport::cleared()
        });
    }
    fs::remove_dir_all(home).map_err(|error| {
        format!(
            "Could not delete {}. {error}. Retry cleanup after closing anything using that directory.",
            home.display()
        )
    })?;
    if !proof_path.is_file() {
        return Err(
            "Cleanup removed the proof. That is a driver bug. Put the proof outside the home and drive again."
                .into(),
        );
    }
    report(&CommandReport {
        ok: true,
        dry_run: Some(false),
        home: Some(display_path(&home_canon)),
        proof: Some(display_path(proof_path)),
        would_delete: vec![display_path(&home_canon)],
        ..CommandReport::cleared()
    })
}

fn prepare_home(home: &Path) -> Result<(), String> {
    fs::create_dir_all(home.join(".config/ca")).map_err(|error| {
        format!(
            "Could not prepare {}. {error}. Pass a new directory the process can create.",
            home.display()
        )
    })?;
    fs::create_dir_all(home.join(".local/share/ca")).map_err(|error| {
        format!(
            "Could not prepare {}. {error}. Pass a new directory the process can create.",
            home.display()
        )
    })?;
    fs::write(home.join(MARKER_NAME), MARKER_BODY).map_err(|error| {
        format!(
            "Could not write the verify marker in {}. {error}. Pass a new directory.",
            home.display()
        )
    })?;
    Ok(())
}

fn marker_ok(home: &Path) -> bool {
    fs::read(home.join(MARKER_NAME)).ok().as_deref() == Some(MARKER_BODY)
}

fn parse_script(text: &str) -> Result<Vec<ScriptStep>, String> {
    let mut steps = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let number = index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "press shift-tab" {
            steps.push(ScriptStep::PressShiftTab);
            continue;
        }
        if let Some(expected) = line.strip_prefix("assert:contains:") {
            if expected.is_empty() {
                return Err(format!(
                    "Line {number} has an empty assert. Write assert:contains:Plan mode."
                ));
            }
            steps.push(ScriptStep::AssertContains(expected.to_string()));
            continue;
        }
        return Err(format!(
            "Line {number} is an unknown script step {line:?}. Use press shift-tab or assert:contains:TEXT."
        ));
    }
    if steps.is_empty() {
        return Err(
            "The script has no steps. Add press shift-tab and assert:contains: lines.".into(),
        );
    }
    Ok(steps)
}

fn render_script(
    home: &Path,
    columns: u16,
    rows: u16,
    steps: &[ScriptStep],
) -> Result<(String, Vec<String>), String> {
    let mut state = crate::app::AppState::new(crate::context::ShellHistory::in_memory());
    state.cwd = display_path(home);
    state.viewport_width = columns;
    state.viewport_height = rows;
    if !state
        .statusline_fields
        .contains(&crate::ui_settings::StatusField::Mode)
    {
        state
            .statusline_fields
            .insert(0, crate::ui_settings::StatusField::Mode);
    }
    let backend = ratatui::backend::TestBackend::new(columns, rows);
    let mut terminal = ratatui::Terminal::new(backend).map_err(|error| {
        format!("Could not open the in-process terminal. {error}. Retry the drive.")
    })?;
    let mut transcript = String::new();
    let mut asserted = Vec::new();
    let mut latest = String::new();
    for step in steps {
        match step {
            ScriptStep::PressShiftTab => {
                let event = crate::tui::event::Event::Key(
                    crossterm::event::KeyCode::BackTab,
                    crossterm::event::KeyModifiers::SHIFT,
                );
                let cwd = state.cwd.clone();
                let handled = crate::tui::input::handle_event(&mut state, &event, &cwd);
                if !handled {
                    return Err(
                        "Shift+Tab was ignored. Close any open card, then drive again.".into(),
                    );
                }
                latest = draw_frame(&mut terminal, &state)?;
                transcript.push_str(&latest);
            }
            ScriptStep::AssertContains(expected) => {
                if !latest.contains(expected) {
                    return Err(format!(
                        "The screen is missing {expected:?}. The mode toast must show that text. Last frame:\n{latest}"
                    ));
                }
                asserted.push(expected.clone());
            }
        }
    }
    if transcript.is_empty() {
        return Err(
            "The script never pressed a key. Add press shift-tab before the asserts.".into(),
        );
    }
    Ok((transcript, asserted))
}

fn draw_frame(
    terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
    state: &crate::app::AppState,
) -> Result<String, String> {
    terminal
        .draw(|frame| crate::tui::render(frame, state))
        .map_err(|error| {
            format!("Could not render the in-process screen. {error}. Retry the drive.")
        })?;
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            text.push(cell.symbol().chars().next().unwrap_or(' '));
        }
        text.push('\n');
    }
    Ok(text)
}

fn path_is_user_tree(path: &Path, forbidden: &ForbiddenTrees) -> bool {
    component_inside(path, &forbidden.config, 0) || component_inside(path, &forbidden.data, 0)
}

fn component_inside(path: &Path, root: &Path, depth: u8) -> bool {
    if depth > 8 {
        return true;
    }
    let path = logical_clean(path);
    let root = logical_clean(root);
    if rooted(&path, &root) {
        return true;
    }
    let mut prefix = path.clone();
    let mut missing = Vec::new();
    while !prefix.as_os_str().is_empty() && !is_symlink(&prefix) && !prefix.exists() {
        let Some((parent, name)) = pop_component(&prefix) else {
            break;
        };
        missing.push(name);
        prefix = parent;
    }
    if is_symlink(&prefix) {
        if let Ok(target) = fs::read_link(&prefix) {
            let base = prefix.parent().unwrap_or_else(|| Path::new("/"));
            let joined = if target.is_absolute() {
                target
            } else {
                base.join(target)
            };
            let mut restored = logical_clean(&joined);
            for name in missing.iter().rev() {
                restored.push(name);
            }
            return component_inside(&restored, &root, depth + 1);
        }
    }
    if let Ok(canonical) = prefix.canonicalize() {
        let mut restored = canonical;
        for name in missing.iter().rev() {
            restored.push(name);
        }
        let root_canon = root.canonicalize().unwrap_or_else(|_| root.clone());
        if rooted(&restored, &root_canon) || rooted(&logical_clean(&restored), &root) {
            return true;
        }
    }
    false
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = logical_clean(path);
    let root = logical_clean(root);
    if rooted(&path, &root) {
        return true;
    }
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => rooted(&path, &root),
        _ => false,
    }
}

fn logical_clean(path: &Path) -> PathBuf {
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    clean
}

fn rooted(path: &Path, root: &Path) -> bool {
    !root.as_os_str().is_empty() && (path == root || path.starts_with(root))
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

fn pop_component(path: &Path) -> Option<(PathBuf, std::ffi::OsString)> {
    let name = path.file_name()?.to_os_string();
    let parent = path.parent()?;
    if parent.as_os_str().is_empty() || parent == path {
        return None;
    }
    Some((parent.to_path_buf(), name))
}

fn refuse_user_tree(path: &Path, forbidden: &ForbiddenTrees, label: &str) -> Result<(), String> {
    if path_is_user_tree(path, forbidden) {
        return Err(format!(
            "Refusing to use this {label} because it is inside the user ca config or data directory. Pass a new temp directory."
        ));
    }
    Ok(())
}

fn require_absolute(path: &Path, label: &str) -> Result<(), String> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(format!(
            "Pass an absolute {label} path. Example: ca verify drive --home /tmp/ca-verify-home --proof /tmp/ca-verify-proof.json."
        ))
    }
}

fn canonical_path(path: &Path) -> Result<PathBuf, String> {
    path.canonicalize().map_err(|error| {
        format!(
            "Could not resolve {}. {error}. Pass a directory that exists.",
            path.display()
        )
    })
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn script_id(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "verify-drive".into())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "{} already exists. Pick a new proof path and drive again.",
            path.display()
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        format!(
            "The path {} has no directory. Pass a file path and drive again.",
            path.display()
        )
    })?;
    let name = path.file_name().ok_or_else(|| {
        format!(
            "The path {} has no file name. Pass a file path and drive again.",
            path.display()
        )
    })?;
    let temporary = parent.join(format!(".{}.tmp", name.to_string_lossy()));
    fs::write(&temporary, bytes).map_err(|error| {
        format!(
            "Could not write {}. {error}. Choose a writable proof directory and drive again.",
            temporary.display()
        )
    })?;
    fs::rename(&temporary, path).map_err(|error| {
        format!(
            "Could not move the proof into {}. {error}. Choose a writable proof directory and drive again.",
            path.display()
        )
    })?;
    Ok(())
}

fn report(body: &CommandReport) -> Result<String, String> {
    serde_json::to_string(body)
        .map_err(|_| "The result could not be encoded. Retry the command.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    const MODE_CYCLE: &str = "\
# The toast shows only the latest mode name.
press shift-tab
assert:contains:Plan mode
press shift-tab
assert:contains:Auto-review
press shift-tab
assert:contains:Yolo
press shift-tab
assert:contains:Normal
";

    fn trees_from_runtime() -> ForbiddenTrees {
        let paths = crate::runtime::RuntimePaths::discover().expect("HOME");
        ForbiddenTrees {
            config: paths.config_root,
            data: paths.data_dir,
        }
    }

    fn drive_mode_cycle(home: &Path, proof: &Path, script: &Path) -> WrittenProof {
        fs::write(script, MODE_CYCLE).unwrap();
        drive(DriveRequest {
            home: home.to_path_buf(),
            script: script.to_path_buf(),
            proof: proof.to_path_buf(),
            columns: 100,
            rows: 32,
            forbidden: trees_from_runtime(),
        })
        .unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn doctor_accepts_a_temp_home_and_refuses_the_user_tree() {
        let trees = trees_from_runtime();
        let config_existed = trees.config.exists();
        let data_existed = trees.data.exists();
        let marker = trees.config.join(MARKER_NAME);
        let marker_existed = marker.exists();
        let temp = tempfile::tempdir().unwrap();
        let link = temp.path().join("into-config");
        symlink(&trees.config, &link).unwrap();
        let broken = temp.path().join("broken-config");
        symlink(trees.config.join("missing-child"), &broken).unwrap();
        let error = doctor(&link, &trees).unwrap_err();
        assert!(error.contains("user ca"), "{error}");
        let error = doctor(&broken, &trees).unwrap_err();
        assert!(error.contains("user ca"), "{error}");
        assert!(path_is_user_tree(&trees.config.join("nested"), &trees));
        assert!(path_is_user_tree(&trees.data.join("nested"), &trees));
        assert_eq!(trees.config.exists(), config_existed);
        assert_eq!(trees.data.exists(), data_existed);
        assert_eq!(marker.exists(), marker_existed);

        let home = temp.path().join("home");
        prepare_home(&home).unwrap();
        doctor(&home, &trees).unwrap();
        let unmarked = temp.path().join("plain");
        fs::create_dir(&unmarked).unwrap();
        let error = doctor(&unmarked, &trees).unwrap_err();
        assert!(error.contains("not a verify-drive home"), "{error}");
    }

    #[test]
    fn drive_refuses_a_proof_inside_the_home_and_an_unmarked_directory() {
        let trees = trees_from_runtime();
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let script = temp.path().join("mode.script");
        fs::write(&script, MODE_CYCLE).unwrap();
        let error = drive(DriveRequest {
            home: home.clone(),
            script: script.clone(),
            proof: home.join("proof.json"),
            columns: 100,
            rows: 32,
            forbidden: trees.clone(),
        })
        .unwrap_err();
        assert!(error.contains("outside the home"), "{error}");
        assert!(!home.exists());

        fs::create_dir(&home).unwrap();
        let error = drive(DriveRequest {
            home: home.clone(),
            script,
            proof: temp.path().join("proof.json"),
            columns: 100,
            rows: 32,
            forbidden: trees,
        })
        .unwrap_err();
        assert!(error.contains("not a verify-drive home"), "{error}");
        assert!(home.is_dir());
    }

    #[test]
    fn mode_cycle_proof_survives_cleanup_and_rejects_a_truncated_transcript() {
        let temp = tempfile::tempdir().unwrap();
        let proof_dir = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let script = proof_dir.path().join("mode-cycle.script");
        let proof = proof_dir.path().join("proof.json");
        let written = drive_mode_cycle(&home, &proof, &script);
        proof_check(&proof).unwrap();
        let parsed: crate::workflow::VerificationProof =
            serde_json::from_slice(&fs::read(&proof).unwrap()).unwrap();
        assert_eq!(parsed.driver, crate::workflow::ProofDriver::InProcess);
        let transcript = fs::read_to_string(&parsed.artifact_path).unwrap();
        for label in ["Plan mode", "Auto-review", "Yolo", "Normal"] {
            assert!(transcript.contains(label), "transcript missing {label}");
        }
        let dry = cleanup(&home, &proof, true, &trees_from_runtime()).unwrap();
        assert!(dry.contains("\"dry_run\":true"), "{dry}");
        assert!(home.exists());
        cleanup(&home, &proof, false, &trees_from_runtime()).unwrap();
        assert!(!home.exists());
        assert!(proof.is_file());
        fs::write(&written.transcript, b"Plan").unwrap();
        let error = proof_check(&proof).unwrap_err();
        assert!(
            error.contains("truncated") || error.contains("does not match"),
            "{error}"
        );
    }

    #[test]
    fn verify_cli_json_names_the_in_process_driver() {
        let temp = tempfile::tempdir().unwrap();
        let proof_dir = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let script = proof_dir.path().join("mode-cycle.script");
        let proof = proof_dir.path().join("proof.json");
        fs::write(&script, MODE_CYCLE).unwrap();
        let args = parse_args([
            "ca",
            "drive",
            "--home",
            home.to_str().unwrap(),
            "--script",
            script.to_str().unwrap(),
            "--proof",
            proof.to_str().unwrap(),
            "--columns",
            "100",
            "--rows",
            "32",
        ])
        .unwrap();
        let json = run(args, &trees_from_runtime()).unwrap_or_else(|error| panic!("{error}"));
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["driver"], "in_process");
        assert_eq!(value["exit_code"], 0);
        cleanup(&home, &proof, false, &trees_from_runtime()).unwrap();
    }

    #[test]
    fn every_feature_script_drives() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".ca/skills/verify-ca");
        let features = root.join("references/features");
        let mut found = 0usize;
        for entry in fs::read_dir(&features).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            found += 1;
            let body = fs::read_to_string(&path).unwrap();
            for heading in [
                "## Sub-features",
                "## How to get to it",
                "## Driving it with ca",
                "## Gotchas",
            ] {
                assert!(
                    body.contains(heading),
                    "{heading} missing from {}",
                    path.display()
                );
            }
            let stem = path.file_stem().unwrap();
            let script = root.join("scripts").join(stem).with_extension("script");
            assert!(
                script.is_file(),
                "feature {} has no script at {}",
                path.display(),
                script.display()
            );
            let temp = tempfile::tempdir().unwrap();
            let proof_dir = tempfile::tempdir().unwrap();
            let home = temp.path().join("home");
            let proof = proof_dir.path().join("proof.json");
            drive(DriveRequest {
                home: home.clone(),
                script,
                proof: proof.clone(),
                columns: 100,
                rows: 32,
                forbidden: trees_from_runtime(),
            })
            .unwrap_or_else(|error| panic!("{error}"));
            proof_check(&proof).unwrap();
            cleanup(&home, &proof, false, &trees_from_runtime()).unwrap();
            assert!(!home.exists());
            assert!(proof.is_file());
        }
        assert!(found >= 1, "verify-ca has no feature files");
    }
}
