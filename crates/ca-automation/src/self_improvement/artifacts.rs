use super::{
    canonical_digest, ArtifactContentV1, ArtifactEncodingV1, ArtifactFileV1,
    ARTIFACT_CONTENT_SCHEMA,
};
use anyhow::{bail, Result};
use std::collections::BTreeSet;
use std::path::{Component, Path};

const MAX_FILES: usize = 64;
const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_PACKAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_INJECTED_CHARS: usize = 64 * 1024;

pub fn validate_artifact_content(content: &ArtifactContentV1) -> Result<String> {
    if content.schema != ARTIFACT_CONTENT_SCHEMA {
        bail!("unsupported artifact content schema");
    }
    if content.files.is_empty() || content.files.len() > MAX_FILES {
        bail!("artifact package must contain 1 to {MAX_FILES} files");
    }
    let mut seen = BTreeSet::new();
    let mut total = 0_usize;
    for file in &content.files {
        validate_package_path(&file.path)?;
        if !seen.insert(file.path.as_str()) {
            bail!("artifact package repeats a file path");
        }
        let bytes = decoded_len(file)?;
        if bytes > MAX_FILE_BYTES {
            bail!("artifact package file exceeds the 1 MiB bound");
        }
        total = total.saturating_add(bytes);
        if total > MAX_PACKAGE_BYTES {
            bail!("artifact package exceeds the 8 MiB bound");
        }
        if file.inject_into_prompt {
            if file.executable || file.encoding != ArtifactEncodingV1::Utf8 {
                bail!("only inert UTF-8 artifact files may be injected into prompts");
            }
            if super::trace_text_contains_secret_pattern(&file.content) {
                bail!("injectable artifact content resembles secret-bearing material");
            }
        }
    }
    let tree_digest = artifact_tree_digest(&content.files)?;
    if content.content_id != tree_digest {
        bail!("artifact content id does not match its canonical content tree");
    }
    Ok(tree_digest)
}

pub fn artifact_tree_digest(files: &[ArtifactFileV1]) -> Result<String> {
    let mut ordered = files.to_vec();
    ordered.sort_by(|left, right| left.path.cmp(&right.path));
    canonical_digest(&ordered)
}

pub fn render_injected_material(content: &ArtifactContentV1) -> Result<String> {
    validate_artifact_content(content)?;
    let mut files = content
        .files
        .iter()
        .filter(|file| file.inject_into_prompt)
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut rendered = String::new();
    for file in files {
        if !rendered.is_empty() {
            rendered.push_str("\n\n");
        }
        rendered.push_str("## ");
        rendered.push_str(&file.path);
        rendered.push('\n');
        rendered.push_str(&file.content);
        if rendered.chars().count() > MAX_INJECTED_CHARS {
            bail!("injectable artifact material exceeds the 64 KiB prompt bound");
        }
    }
    Ok(rendered)
}

fn validate_package_path(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 512
        || value.contains('\\')
        || value.starts_with('/')
        || Path::new(value)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("artifact package path must be safe, relative, and slash-separated");
    }
    Ok(())
}

fn decoded_len(file: &ArtifactFileV1) -> Result<usize> {
    match file.encoding {
        ArtifactEncodingV1::Utf8 => Ok(file.content.len()),
        ArtifactEncodingV1::Hex => {
            if !file.content.len().is_multiple_of(2)
                || !file.content.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                bail!("hex artifact content is malformed");
            }
            Ok(file.content.len() / 2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> ArtifactContentV1 {
        let files = vec![ArtifactFileV1 {
            path: "SKILL.md".to_string(),
            encoding: ArtifactEncodingV1::Utf8,
            content: "Use deterministic evidence before model judgment.".to_string(),
            inject_into_prompt: true,
            executable: false,
        }];
        ArtifactContentV1 {
            schema: ARTIFACT_CONTENT_SCHEMA.to_string(),
            content_id: artifact_tree_digest(&files).unwrap(),
            files,
            digest: String::new(),
        }
    }

    #[test]
    fn portable_multifile_content_is_digest_bound_and_renderable() -> Result<()> {
        let mut package = content();
        package.files.push(ArtifactFileV1 {
            path: "assets/example.hex".to_string(),
            encoding: ArtifactEncodingV1::Hex,
            content: "00ff".to_string(),
            inject_into_prompt: false,
            executable: false,
        });
        package.content_id = artifact_tree_digest(&package.files)?;
        assert_eq!(validate_artifact_content(&package)?, package.content_id);
        assert!(render_injected_material(&package)?.contains("deterministic evidence"));
        Ok(())
    }

    #[test]
    fn traversal_executable_injection_and_secret_material_are_rejected() {
        let mut package = content();
        package.files[0].path = "../SKILL.md".to_string();
        package.content_id = artifact_tree_digest(&package.files).unwrap();
        assert!(validate_artifact_content(&package).is_err());

        let mut package = content();
        package.files[0].executable = true;
        package.content_id = artifact_tree_digest(&package.files).unwrap();
        assert!(validate_artifact_content(&package).is_err());

        let mut package = content();
        package.files[0].content = "Authorization: Bearer canary".to_string();
        package.content_id = artifact_tree_digest(&package.files).unwrap();
        assert!(validate_artifact_content(&package).is_err());
    }
}
