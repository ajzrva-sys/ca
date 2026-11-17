//! Saved logins for the current page. The password never comes back to the model.

use crate::agent::registry::{ToolContext, ToolExecutionResult};
use crate::browser::BrowserManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Vault {
    items: Vec<Login>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Login {
    handle: String,
    origin: String,
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct PendingLogin {
    origin: String,
    username: String,
    password: String,
}

pub async fn execute(arguments: &Value, context: &ToolContext) -> ToolExecutionResult {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("");
    let paths = match vault_paths() {
        Ok(paths) => paths,
        Err(error) => return ToolExecutionResult::Failure(error),
    };
    match action {
        "list" => match list(&paths.vault) {
            Ok(text) => ToolExecutionResult::Success(text),
            Err(error) => ToolExecutionResult::Failure(error),
        },
        "fill" => {
            let handle = arguments
                .get("handle")
                .and_then(Value::as_str)
                .unwrap_or("");
            match fill(BrowserManager::process(), context, &paths.vault, handle).await {
                Ok(text) => ToolExecutionResult::Success(text),
                Err(error) => ToolExecutionResult::Failure(error),
            }
        }
        "save_login" => match save_login(&paths.vault, &paths.pending) {
            Ok(text) => ToolExecutionResult::Success(text),
            Err(error) => ToolExecutionResult::Failure(error),
        },
        _ => ToolExecutionResult::Failure(
            "browser_vault action must be list, fill, or save_login".into(),
        ),
    }
}

struct VaultPaths {
    vault: PathBuf,
    pending: PathBuf,
}

fn vault_paths() -> Result<VaultPaths, String> {
    let root = crate::runtime::RuntimePaths::discover()
        .map_err(|_| "browser vault directory is not available".to_string())?
        .config_root;
    Ok(VaultPaths {
        vault: root.join("browser-vault.json"),
        pending: root.join("browser-vault.pending.json"),
    })
}

fn list(path: &Path) -> Result<String, String> {
    let vault = read_vault(path)?;
    if vault.items.is_empty() {
        return Ok("No saved logins.".into());
    }
    let mut lines = vec![format!("Saved logins: {}", vault.items.len())];
    for item in vault.items {
        lines.push(format!("{} {} {}", item.handle, item.origin, item.username));
    }
    Ok(lines.join("\n"))
}

fn save_login(vault_path: &Path, pending_path: &Path) -> Result<String, String> {
    let pending = read_pending(pending_path)?;
    if pending.origin.is_empty()
        || pending.username.is_empty()
        || pending.password.is_empty()
        || !pending.origin.starts_with("https://")
        || pending.origin.chars().any(char::is_control)
        || pending.username.chars().any(char::is_control)
        || pending.password.chars().any(char::is_control)
    {
        return Err("the pending login file is incomplete".into());
    }
    let mut vault = read_vault(vault_path)?;
    let handle = format!("login-{}", vault.items.len() + 1);
    vault.items.push(Login {
        handle: handle.clone(),
        origin: pending.origin.clone(),
        username: pending.username.clone(),
        password: pending.password,
    });
    write_vault(vault_path, &vault)?;
    let _ = std::fs::remove_file(pending_path);
    Ok(format!(
        "Saved {} for {} as {handle}",
        pending.username, pending.origin
    ))
}

async fn fill(
    manager: &BrowserManager,
    context: &ToolContext,
    path: &Path,
    handle: &str,
) -> Result<String, String> {
    if handle.is_empty() {
        return Err("fill needs a handle from list".into());
    }
    let vault = read_vault(path)?;
    let login = vault
        .items
        .into_iter()
        .find(|item| item.handle == handle)
        .ok_or_else(|| format!("no saved login {handle}"))?;
    let origin = manager
        .page_origin(context)
        .await
        .map_err(|_| "start the browser and open the login page before filling".to_string())?;
    if origin != login.origin {
        return Err("the page origin does not match this saved login".into());
    }
    let filled = manager
        .fill_login(&login.username, &login.password, context)
        .await
        .map_err(|_| format!("could not fill {handle}"))?;
    Ok(format!("Filled {filled} fields for {handle}"))
}

fn read_vault(path: &Path) -> Result<Vault, String> {
    if !path.exists() {
        return Ok(Vault { items: Vec::new() });
    }
    let text = std::fs::read_to_string(path)
        .map_err(|_| "could not read the browser vault".to_string())?;
    serde_json::from_str(&text).map_err(|_| "the browser vault is unreadable".to_string())
}

fn read_pending(path: &Path) -> Result<PendingLogin, String> {
    let text = std::fs::read_to_string(path).map_err(|_| {
        "save_login needs ~/.config/ca/browser-vault.pending.json written by the user".to_string()
    })?;
    serde_json::from_str(&text).map_err(|_| "the pending login file is unreadable".to_string())
}

fn write_vault(path: &Path, vault: &Vault) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|_| "could not create the browser vault directory".to_string())?;
    }
    let text = serde_json::to_string_pretty(vault)
        .map_err(|_| "could not encode the browser vault".to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, text)
        .map_err(|_| "could not write the browser vault".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| "could not protect the browser vault".to_string())?;
    }
    std::fs::rename(&temporary, path)
        .map_err(|_| "could not replace the browser vault".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_login_stores_the_secret_and_list_does_not_return_it() {
        let root = tempfile::tempdir().unwrap();
        let vault = root.path().join("browser-vault.json");
        let pending = root.path().join("browser-vault.pending.json");
        std::fs::write(
            &pending,
            r#"{"origin":"https://courts.example","username":"ada","password":"s3cret-token"}"#,
        )
        .unwrap();
        let saved = save_login(&vault, &pending).unwrap();
        assert!(saved.contains("ada"));
        assert!(saved.contains("https://courts.example"));
        assert!(!saved.contains("s3cret-token"));
        assert!(!pending.exists());
        let listed = list(&vault).unwrap();
        assert!(listed.contains("login-1"));
        assert!(listed.contains("ada"));
        assert!(!listed.contains("s3cret-token"));
    }
}
