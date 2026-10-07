//! Shared with focused tests. Hash only explicit build inputs, never runtime
//! configuration, credentials, repository history, or build output directories.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

pub fn input_roots(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = vec![
        PathBuf::from("build.rs"),
        PathBuf::from("build_support"),
        PathBuf::from("Cargo.toml"),
        PathBuf::from("Cargo.lock"),
        PathBuf::from("src"),
        PathBuf::from("prompts"),
        PathBuf::from("tests/fixtures/codex-model-catalog.json"),
        PathBuf::from("tests/reasoning_pilot_hidden/src"),
    ];
    // Workspace members currently live directly below crates/. Each package's
    // manifest, build script, source and embedded assets affect its behavior.
    for entry in std::fs::read_dir(root.join("crates"))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let member = PathBuf::from("crates").join(entry.file_name());
        if !root.join(&member).join("Cargo.toml").is_file() {
            continue;
        }
        for name in ["Cargo.toml", "build.rs", "src", "prompts", "schemas"] {
            let input = member.join(name);
            if root.join(&input).exists() {
                paths.push(input);
            }
        }
    }
    for name in [
        "rust-toolchain",
        "rust-toolchain.toml",
        ".cargo/config",
        ".cargo/config.toml",
    ] {
        let input = PathBuf::from(name);
        if root.join(&input).exists() {
            paths.push(input);
        }
    }
    paths.sort();
    Ok(paths)
}

/// Recovery authority is stable across candidate-editable application changes.
/// Dependency manifests, lockfile and build configuration remain part of it.
#[allow(dead_code)] // Build-only entry point; other consumers share the digest encoding.
pub fn authority_roots(root: &Path, protected: &[&str]) -> io::Result<Vec<PathBuf>> {
    let mut paths = input_roots(root)?
        .into_iter()
        .filter(|path| {
            matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some(
                    "Cargo.toml"
                        | "Cargo.lock"
                        | "build.rs"
                        | "rust-toolchain"
                        | "rust-toolchain.toml"
                )
            ) || path.starts_with(".cargo")
        })
        .collect::<Vec<_>>();
    paths.extend(
        protected
            .iter()
            .map(PathBuf::from)
            .filter(|path| root.join(path).exists()),
    );
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn collect(root: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let path = root.join(relative);
    let kind = std::fs::symlink_metadata(&path)?.file_type();
    if kind.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            collect(root, &relative.join(entry.file_name()), files)?;
        }
    } else if kind.is_file() {
        files.push(relative.to_owned());
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "build identity input is not a regular file or directory",
        ));
    }
    Ok(())
}

fn field(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}

pub fn digest(
    root: &Path,
    inputs: &[PathBuf],
    settings: &BTreeMap<String, String>,
    compiler: &[u8],
) -> io::Result<String> {
    let mut files = Vec::new();
    for input in inputs {
        collect(root, input, &mut files)?;
    }
    digest_with_reader(files, settings, compiler, |relative| {
        std::fs::read(root.join(relative))
    })
}

/// The runtime supplies bounded descriptor-relative reads while build.rs uses
/// ordinary source reads. Both paths share the exact inventory/hash encoding.
pub fn digest_with_reader(
    mut files: Vec<PathBuf>,
    settings: &BTreeMap<String, String>,
    compiler: &[u8],
    mut read: impl FnMut(&Path) -> io::Result<Vec<u8>>,
) -> io::Result<String> {
    files.sort();
    files.dedup();
    let mut hash = Sha256::new();
    field(&mut hash, b"ca.build-identity.v2");
    field(&mut hash, compiler);
    for (key, value) in settings {
        field(&mut hash, b"setting");
        field(&mut hash, key.as_bytes());
        field(&mut hash, value.as_bytes());
    }
    for relative in files {
        let name = relative.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 build input path")
        })?;
        field(&mut hash, b"file");
        field(&mut hash, name.as_bytes());
        field(&mut hash, &read(&relative)?);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[allow(dead_code)] // Used by build.rs; runtime source validation shares only the hashing functions.
pub fn relevant_setting(key: &str) -> bool {
    key.starts_with("CARGO_FEATURE_")
        || key.starts_with("CARGO_CFG_")
        || matches!(
            key,
            "TARGET" | "HOST" | "PROFILE" | "OPT_LEVEL" | "DEBUG" | "CARGO_ENCODED_RUSTFLAGS"
        )
}
