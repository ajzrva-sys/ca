//! Frozen machine-local VM identity. The executor owns commands and policy.
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub const HARNESS_VM_SCHEMA: &str = "ca.harness-vm.v1";
pub const HARNESS_VM_FILES: [&str; 4] = ["tart", "config.json", "disk.img", "nvram.bin"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HarnessVmProfile {
    pub tart: PathBuf,
    pub home: PathBuf,
    pub template: String,
    pub guest_agent_sha256: String,
}
impl HarnessVmProfile {
    pub fn validate(&self) -> Result<()> {
        if !normalized_absolute(&self.tart)
            || !normalized_absolute(&self.home)
            || self.tart == self.home
            || self.template.is_empty()
            || self.template.len() > 64
            || self.template.starts_with('-')
            || !self
                .template
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            bail!("invalid dedicated VM profile paths or template name");
        }
        digest(&self.guest_agent_sha256)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HarnessVmFile {
    pub sha256: String,
    pub bytes: u64,
    pub device: u64,
    pub inode: u64,
    pub uid: u32,
    pub mode: u32,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
    pub changed_seconds: i64,
    pub changed_nanoseconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenHarnessVm {
    pub schema: String,
    pub profile: HarnessVmProfile,
    /// Protected host authority and dependency inputs, stable across ordinary application edits.
    pub adapter_digest: String,
    pub files: BTreeMap<String, HarnessVmFile>,
}
impl FrozenHarnessVm {
    pub fn validate(&self) -> Result<()> {
        self.profile.validate()?;
        digest(&self.adapter_digest)?;
        if self.schema != HARNESS_VM_SCHEMA || self.files.len() != HARNESS_VM_FILES.len() {
            bail!("invalid frozen VM schema or inventory");
        }
        for name in HARNESS_VM_FILES {
            let Some(file) = self.files.get(name) else {
                bail!("frozen VM file missing");
            };
            let maximum = if name == "disk.img" {
                512 * 1024 * 1024 * 1024
            } else {
                256 * 1024 * 1024
            };
            if file.bytes == 0
                || file.bytes > maximum
                || file.mode & !0o777 != 0
                || file.mode & 0o022 != 0
                || !(0..1_000_000_000).contains(&file.modified_nanoseconds)
                || !(0..1_000_000_000).contains(&file.changed_nanoseconds)
                || (name == "tart" && file.mode & 0o100 == 0)
            {
                bail!("invalid frozen VM file metadata");
            }
            digest(&file.sha256)?;
        }
        Ok(())
    }
}
fn normalized_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.file_name().is_some()
        && path
            .components()
            .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
        && !path.as_os_str().as_encoded_bytes().contains(&0)
}
fn digest(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("invalid frozen VM digest");
    }
    Ok(())
}
