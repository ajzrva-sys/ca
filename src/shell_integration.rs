use clap::{Parser, Subcommand};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const START_MARKER: &str = "# >>> ca shell integration v1 >>>";
const END_MARKER: &str = "# <<< ca shell integration v1 <<<";
const MAX_ZSHRC_BYTES: u64 = 1024 * 1024;

const ZSH_SCRIPT: &str = r#"# ca shell integration v1
autoload -Uz compinit
(( $+functions[compdef] )) || compinit
if ! (( $+functions[_ca] )); then
  _ca() {
    local -a commands
    commands=(
      'serve:run the local JSONL service'
      'attach:attach to a local CA service'
      'schedule:manage scheduled local runs'
      'skill:inspect and manage skills'
      'plugin:manage verified local plugins'
      'support-bundle:create a private metadata-only support archive'
      'config:explain effective setting origins without values'
      'repo:inspect local repository state'
      'acp:run the ACP stdio server'
      'shell-integration:print shell integration'
    )
    _describe 'command' commands
  }
fi
compdef _ca ca
bindkey '^[[1;5D' backward-word
bindkey '^[[1;5C' forward-word
"#;

fn managed_block() -> String {
    format!(
        "{START_MARKER}\nif command -v ca >/dev/null 2>&1; then\n  eval \"$(ca shell-integration zsh)\"\nfi\n{END_MARKER}\n"
    )
}

#[derive(Debug, Parser)]
#[command(name = "ca shell-integration", about = "Print CA shell integration")]
struct RawPrintArgs {
    #[command(subcommand)]
    shell: PrintShell,
}

#[derive(Debug, Subcommand)]
enum PrintShell {
    /// Print completion and terminal-key setup for zsh
    Zsh,
}

#[derive(Debug, Parser)]
#[command(
    name = "ca install-shell-integration",
    about = "Install CA's managed zsh integration block"
)]
struct RawInstallArgs {}

#[derive(Debug, Parser)]
#[command(
    name = "ca uninstall-shell-integration",
    about = "Remove CA's exact managed zsh integration block"
)]
struct RawUninstallArgs {}

#[derive(Debug, Clone, Copy)]
pub enum ShellIntegrationCli {
    PrintZsh,
    Install,
    Uninstall,
}

pub fn parse_print<I, T>(args: I) -> Result<ShellIntegrationCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let parsed = RawPrintArgs::try_parse_from(args)?;
    match parsed.shell {
        PrintShell::Zsh => Ok(ShellIntegrationCli::PrintZsh),
    }
}

pub fn parse_install<I, T>(args: I) -> Result<ShellIntegrationCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    RawInstallArgs::try_parse_from(args).map(|_| ShellIntegrationCli::Install)
}

pub fn parse_uninstall<I, T>(args: I) -> Result<ShellIntegrationCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    RawUninstallArgs::try_parse_from(args).map(|_| ShellIntegrationCli::Uninstall)
}

pub fn run_cli(cli: ShellIntegrationCli) -> i32 {
    match cli {
        ShellIntegrationCli::PrintZsh => {
            print!("{ZSH_SCRIPT}");
            0
        }
        ShellIntegrationCli::Install => {
            println!("CA managed block:\n{}", managed_block());
            match zshrc_path().and_then(|path| install(&path)) {
                Ok(result) => {
                    match result {
                        Mutation::Unchanged => println!("Shell integration is already installed."),
                        Mutation::Changed { backup } => {
                            println!("Installed shell integration in ~/.zshrc.");
                            if let Some(backup) = backup {
                                println!("Recovery copy: {}", backup.display());
                            }
                        }
                    }
                    0
                }
                Err(error) => {
                    eprintln!("ca install-shell-integration: {error}");
                    2
                }
            }
        }
        ShellIntegrationCli::Uninstall => match zshrc_path().and_then(|path| uninstall(&path)) {
            Ok(Mutation::Unchanged) => {
                println!("CA shell integration is not installed.");
                0
            }
            Ok(Mutation::Changed { backup }) => {
                println!("Removed CA's exact managed block from ~/.zshrc.");
                if let Some(backup) = backup {
                    println!("Recovery copy: {}", backup.display());
                }
                0
            }
            Err(error) => {
                eprintln!("ca uninstall-shell-integration: {error}");
                2
            }
        },
    }
}

#[derive(Debug, thiserror::Error)]
enum ShellIntegrationError {
    #[error("HOME is unavailable")]
    MissingHome,
    #[error("HOME is not a directory")]
    UnsafeHome,
    #[error(".zshrc must be a regular file and not a symlink")]
    UnsafeTarget,
    #[error(".zshrc exceeds the 1 MiB safety limit")]
    Oversized,
    #[error(".zshrc could not be read")]
    Read,
    #[error(".zshrc contains malformed or duplicate CA integration markers")]
    MarkerDrift,
    #[error("a recovery copy could not be created")]
    Backup,
    #[error("the updated .zshrc could not be written atomically")]
    Write,
}

#[derive(Debug, PartialEq, Eq)]
enum Mutation {
    Unchanged,
    Changed { backup: Option<PathBuf> },
}

fn zshrc_path() -> Result<PathBuf, ShellIntegrationError> {
    let home = crate::run_environment::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(ShellIntegrationError::MissingHome)?;
    let home = home
        .canonicalize()
        .map_err(|_| ShellIntegrationError::UnsafeHome)?;
    if !home.is_dir() {
        return Err(ShellIntegrationError::UnsafeHome);
    }
    Ok(home.join(".zshrc"))
}

fn install(path: &Path) -> Result<Mutation, ShellIntegrationError> {
    let current = read_target(path)?;
    match marker_range(&current)? {
        Some(range) => {
            if current[range] == managed_block() {
                return Ok(Mutation::Unchanged);
            }
            Err(ShellIntegrationError::MarkerDrift)
        }
        None => {
            let mut updated = current.clone();
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
            }
            if !updated.is_empty() {
                updated.push('\n');
            }
            updated.push_str(&managed_block());
            mutate(path, &current, &updated)
        }
    }
}

fn uninstall(path: &Path) -> Result<Mutation, ShellIntegrationError> {
    let current = read_target(path)?;
    let Some(range) = marker_range(&current)? else {
        return Ok(Mutation::Unchanged);
    };
    if current[range.clone()] != managed_block() {
        return Err(ShellIntegrationError::MarkerDrift);
    }
    let mut updated = current.clone();
    updated.replace_range(range, "");
    while updated.ends_with("\n\n\n") {
        updated.pop();
    }
    mutate(path, &current, &updated)
}

fn read_target(path: &Path) -> Result<String, ShellIntegrationError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                return Err(ShellIntegrationError::UnsafeTarget);
            }
            if metadata.len() > MAX_ZSHRC_BYTES {
                return Err(ShellIntegrationError::Oversized);
            }
            std::fs::read_to_string(path).map_err(|_| ShellIntegrationError::Read)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => Err(ShellIntegrationError::Read),
    }
}

fn marker_range(content: &str) -> Result<Option<std::ops::Range<usize>>, ShellIntegrationError> {
    let starts = content.match_indices(START_MARKER).collect::<Vec<_>>();
    let ends = content.match_indices(END_MARKER).collect::<Vec<_>>();
    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([(start, _)], [(end, _)]) if start < end => {
            let mut finish = end + END_MARKER.len();
            if content.as_bytes().get(finish) == Some(&b'\n') {
                finish += 1;
            }
            Ok(Some(*start..finish))
        }
        _ => Err(ShellIntegrationError::MarkerDrift),
    }
}

fn mutate(path: &Path, original: &str, updated: &str) -> Result<Mutation, ShellIntegrationError> {
    let originally_existed = path.exists();
    let backup = if originally_existed {
        Some(create_backup(path, original.as_bytes())?)
    } else {
        None
    };
    atomic_write(path, originally_existed, original, updated.as_bytes())?;
    Ok(Mutation::Changed { backup })
}

fn create_backup(path: &Path, bytes: &[u8]) -> Result<PathBuf, ShellIntegrationError> {
    let digest = format!("{:x}", Sha256::digest(bytes));
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let backup = path.with_file_name(format!(
        ".zshrc.ca-backup-{timestamp}-{}-{}",
        &digest[..12],
        uuid::Uuid::new_v4()
    ));
    let mut file = create_private_new(&backup).map_err(|_| ShellIntegrationError::Backup)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| ShellIntegrationError::Backup)?;
    Ok(backup)
}

fn atomic_write(
    path: &Path,
    originally_existed: bool,
    original: &str,
    bytes: &[u8],
) -> Result<(), ShellIntegrationError> {
    let parent = path.parent().ok_or(ShellIntegrationError::Write)?;
    let temporary = parent.join(format!(".zshrc.ca-tmp-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = create_private_new(&temporary).map_err(|_| ShellIntegrationError::Write)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| ShellIntegrationError::Write)?;
        if let Ok(metadata) = std::fs::metadata(path) {
            std::fs::set_permissions(&temporary, metadata.permissions())
                .map_err(|_| ShellIntegrationError::Write)?;
        }
        if path.exists() != originally_existed || read_target(path)? != original {
            return Err(ShellIntegrationError::MarkerDrift);
        }
        std::fs::rename(&temporary, path).map_err(|_| ShellIntegrationError::Write)?;
        let _ = File::open(parent).and_then(|directory| directory.sync_all());
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn create_private_new(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_atomic_idempotent_and_uninstall_removes_only_exact_block() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".zshrc");
        std::fs::write(&path, "export KEEP=yes\n").unwrap();
        let first = install(&path).unwrap();
        let Mutation::Changed {
            backup: Some(backup),
        } = first
        else {
            panic!("existing file must receive a recovery copy")
        };
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "export KEEP=yes\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&backup).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(install(&path).unwrap(), Mutation::Unchanged);
        let installed = std::fs::read_to_string(&path).unwrap();
        assert!(installed.contains(START_MARKER));
        assert!(installed.contains("export KEEP=yes"));
        assert!(matches!(
            uninstall(&path).unwrap(),
            Mutation::Changed { backup: Some(_) }
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "export KEEP=yes\n\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_target_is_refused() {
        use std::os::unix::fs::symlink;
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("actual");
        let path = home.path().join(".zshrc");
        std::fs::write(&target, "keep").unwrap();
        symlink(&target, &path).unwrap();
        assert!(matches!(
            install(&path),
            Err(ShellIntegrationError::UnsafeTarget)
        ));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "keep");
    }

    #[test]
    fn malformed_markers_fail_without_mutation() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".zshrc");
        std::fs::write(&path, format!("{START_MARKER}\nunclosed\n")).unwrap();
        assert!(matches!(
            install(&path),
            Err(ShellIntegrationError::MarkerDrift)
        ));
        assert!(std::fs::read_to_string(path).unwrap().contains("unclosed"));
    }

    #[test]
    fn generator_contains_only_completion_and_key_setup() {
        assert!(ZSH_SCRIPT.contains("compdef _ca ca"));
        assert!(ZSH_SCRIPT.contains("bindkey"));
        assert!(!ZSH_SCRIPT.contains("curl"));
        assert!(!ZSH_SCRIPT.contains("source "));
    }
}
