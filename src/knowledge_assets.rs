//! Digest-bound local Markdown asset mutations used by rule and command managers.

use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

const MAX_ASSET_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetKind {
    Rule,
    Command,
}

impl AssetKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Command => "command",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetMutationKind {
    Create,
    Edit,
    Delete,
}

impl AssetMutationKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Edit => "edit",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AssetMutationPreview {
    pub asset: AssetKind,
    pub mutation: AssetMutationKind,
    pub display_id: String,
    pub root: PathBuf,
    pub target: PathBuf,
    pub expected_digest: Option<String>,
    pub replacement: Option<String>,
    pub confirmation_digest: String,
}

impl AssetMutationPreview {
    pub(crate) fn prepare_create(
        asset: AssetKind,
        display_id: String,
        root: PathBuf,
        target: PathBuf,
        replacement: String,
    ) -> Result<Self, String> {
        validate_replacement(&replacement)?;
        validate_target(&root, &target)?;
        if target.exists() {
            return Err(format!("{} already exists", asset.label()));
        }
        Ok(Self::new(
            asset,
            AssetMutationKind::Create,
            display_id,
            root,
            target,
            None,
            Some(replacement),
        ))
    }

    pub(crate) fn prepare_edit(
        asset: AssetKind,
        display_id: String,
        root: PathBuf,
        target: PathBuf,
        expected_digest: String,
        replacement: String,
    ) -> Result<Self, String> {
        validate_replacement(&replacement)?;
        validate_target(&root, &target)?;
        validate_existing(&target, &expected_digest)?;
        Ok(Self::new(
            asset,
            AssetMutationKind::Edit,
            display_id,
            root,
            target,
            Some(expected_digest),
            Some(replacement),
        ))
    }

    pub(crate) fn prepare_delete(
        asset: AssetKind,
        display_id: String,
        root: PathBuf,
        target: PathBuf,
        expected_digest: String,
    ) -> Result<Self, String> {
        validate_target(&root, &target)?;
        validate_existing(&target, &expected_digest)?;
        Ok(Self::new(
            asset,
            AssetMutationKind::Delete,
            display_id,
            root,
            target,
            Some(expected_digest),
            None,
        ))
    }

    fn new(
        asset: AssetKind,
        mutation: AssetMutationKind,
        display_id: String,
        root: PathBuf,
        target: PathBuf,
        expected_digest: Option<String>,
        replacement: Option<String>,
    ) -> Self {
        let confirmation_digest = preview_digest(
            asset,
            mutation,
            &display_id,
            &target,
            expected_digest.as_deref(),
            replacement.as_deref(),
        );
        Self {
            asset,
            mutation,
            display_id,
            root,
            target,
            expected_digest,
            replacement,
            confirmation_digest,
        }
    }

    pub(crate) fn render(&self) -> String {
        let mut lines = vec![
            format!("{} {} preview", self.asset.label(), self.mutation.label()),
            String::new(),
            format!("Asset: {}", self.display_id),
            format!("Confirmation digest: {}", self.confirmation_digest),
        ];
        if let Some(expected) = &self.expected_digest {
            lines.push(format!("Current digest: {expected}"));
        }
        if let Some(replacement) = &self.replacement {
            lines.push(String::new());
            lines.push("Replacement:".into());
            lines.push(replacement.clone());
        }
        lines.push(String::new());
        lines.push(format!(
            "Confirm with /{} confirm {}",
            self.asset.label(),
            self.confirmation_digest
        ));
        lines.join("\n")
    }

    pub(crate) fn apply(&self, confirmation_digest: &str) -> Result<(), String> {
        if confirmation_digest != self.confirmation_digest {
            return Err("confirmation digest does not match the preview".into());
        }
        validate_target(&self.root, &self.target)?;
        match self.mutation {
            AssetMutationKind::Create => {
                if self.target.exists() {
                    return Err(format!("{} appeared after preview", self.asset.label()));
                }
            }
            AssetMutationKind::Edit | AssetMutationKind::Delete => validate_existing(
                &self.target,
                self.expected_digest
                    .as_deref()
                    .ok_or("preview is missing its drift digest")?,
            )?,
        }
        match self.mutation {
            AssetMutationKind::Create | AssetMutationKind::Edit => write_atomic(
                &self.root,
                &self.target,
                self.replacement
                    .as_deref()
                    .ok_or("preview is missing replacement text")?,
            ),
            AssetMutationKind::Delete => fs::remove_file(&self.target)
                .map_err(|error| format!("could not delete {}: {error}", self.asset.label())),
        }
    }
}

fn validate_replacement(content: &str) -> Result<(), String> {
    if content.is_empty()
        || content.len() > MAX_ASSET_BYTES
        || content
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err("asset content is empty, unsafe, or exceeds 64 KiB".into());
    }
    Ok(())
}

fn validate_target(root: &Path, target: &Path) -> Result<(), String> {
    let root = fs::canonicalize(root).map_err(|_| "native asset root is unavailable")?;
    if !root.is_dir()
        || !target.starts_with(&root)
        || target.extension().is_none_or(|ext| ext != "md")
    {
        return Err("asset target is outside its native root".into());
    }
    let relative = target
        .strip_prefix(&root)
        .map_err(|_| "asset target is outside its native root")?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("asset target is invalid".into());
    }
    let mut current = root;
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err("asset target is invalid".into());
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("asset target cannot contain symlinks".into())
            }
            Ok(metadata) if current != target && !metadata.is_dir() => {
                return Err("asset parent is not a directory".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("asset target could not be inspected".into()),
        }
    }
    Ok(())
}

fn validate_existing(target: &Path, expected_digest: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(target).map_err(|_| "asset disappeared after preview")?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_ASSET_BYTES as u64
    {
        return Err("asset is not a bounded regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err("asset cannot be hard-linked".into());
        }
    }
    let bytes = fs::read(target).map_err(|_| "asset could not be read")?;
    if digest(&bytes) != expected_digest {
        return Err("asset changed after preview; reload before replacing it".into());
    }
    Ok(())
}

fn write_atomic(root: &Path, target: &Path, content: &str) -> Result<(), String> {
    let parent = target.parent().ok_or("asset target has no parent")?;
    create_private_directories(root, parent)?;
    let temporary = parent.join(format!(".ca-asset-{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("could not stage asset: {error}"))?;
        file.write_all(content.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("could not stage asset: {error}"))?;
        fs::rename(&temporary, target)
            .map_err(|error| format!("could not publish asset: {error}"))?;
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn create_private_directories(root: &Path, parent: &Path) -> Result<(), String> {
    let canonical_root = fs::canonicalize(root).map_err(|_| "native asset root is unavailable")?;
    let relative = parent
        .strip_prefix(&canonical_root)
        .map_err(|_| "asset parent escaped its native root")?;
    let mut current = canonical_root;
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err("asset parent is invalid".into());
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err("asset parent is unsafe".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current)
                    .map_err(|error| format!("could not create asset directory: {error}"))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&current, fs::Permissions::from_mode(0o700))
                        .map_err(|error| format!("could not secure asset directory: {error}"))?;
                }
            }
            Err(_) => return Err("asset parent could not be inspected".into()),
        }
    }
    Ok(())
}

fn preview_digest(
    asset: AssetKind,
    mutation: AssetMutationKind,
    display_id: &str,
    target: &Path,
    expected: Option<&str>,
    replacement: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{asset:?}\0{mutation:?}\0{display_id}\0").as_bytes());
    hasher.update(target.as_os_str().as_encoded_bytes());
    hasher.update(b"\0");
    hasher.update(expected.unwrap_or_default().as_bytes());
    hasher.update(b"\0");
    hasher.update(replacement.unwrap_or_default().as_bytes());
    hex(&hasher.finalize())
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_edit_delete_require_exact_preview_and_reject_drift() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let target = root.join(".ca/rules/demo.md");
        let create = AssetMutationPreview::prepare_create(
            AssetKind::Rule,
            "workspace-0/demo".into(),
            root.clone(),
            target.clone(),
            "first".into(),
        )
        .unwrap();
        assert!(create.apply("wrong").is_err());
        create.apply(&create.confirmation_digest).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "first");

        let first_digest = digest(b"first");
        let edit = AssetMutationPreview::prepare_edit(
            AssetKind::Rule,
            "workspace-0/demo".into(),
            root.clone(),
            target.clone(),
            first_digest,
            "second".into(),
        )
        .unwrap();
        fs::write(&target, "drift").unwrap();
        assert!(edit.apply(&edit.confirmation_digest).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "drift");

        let delete = AssetMutationPreview::prepare_delete(
            AssetKind::Rule,
            "workspace-0/demo".into(),
            root,
            target.clone(),
            digest(b"drift"),
        )
        .unwrap();
        delete.apply(&delete.confirmation_digest).unwrap();
        assert!(!target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_and_hardlink_targets_are_rejected() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let outside = root.join("outside");
        fs::write(&outside, "outside").unwrap();
        fs::create_dir(root.join(".ca")).unwrap();
        symlink(&outside, root.join(".ca/rules")).unwrap();
        assert!(AssetMutationPreview::prepare_create(
            AssetKind::Command,
            "demo".into(),
            root.clone(),
            root.join(".ca/rules/demo.md"),
            "body".into(),
        )
        .is_err());

        fs::remove_file(root.join(".ca/rules")).unwrap();
        fs::create_dir(root.join(".ca/rules")).unwrap();
        fs::hard_link(&outside, root.join(".ca/rules/demo.md")).unwrap();
        assert!(validate_existing(&root.join(".ca/rules/demo.md"), &digest(b"outside")).is_err());
    }
}
