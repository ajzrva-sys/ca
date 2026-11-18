//! Lossless, host-only source analysis. Unknown bytes never become public data.
//! Projected views intentionally use typed markers, including at scalar/code
//! positions; only locally restored bytes may be parsed or executed as edits.
#![allow(dead_code)]
use super::policy::{Action, AliasKind, Policy};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::ops::Range;

mod markup;
mod regions;
mod syntax;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub selector: String,
    pub label: String,
    pub range: Range<usize>,
    pub kind: AliasKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub format: String,
    pub nodes: Vec<Node>,
    pub digest: String,
    web_data: Vec<Range<usize>>,
}

fn automatic_action(document: &Document, node: &Node, default: Action) -> Action {
    let ranges = &document.web_data;
    let index = ranges.partition_point(|range| range.end <= node.range.start);
    if default == Action::Allow
        && ranges
            .get(index)
            .is_some_and(|range| range.start < node.range.end)
    {
        Action::Alias
    } else {
        default
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub selector: String,
    pub range: Range<usize>,
    pub original: String,
    pub replacement: String,
    pub kind: AliasKind,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projection {
    pub text: String,
    pub changes: Vec<Change>,
    pub nodes: Vec<Node>,
    pub digest: String,
}

pub fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn retained_selector(value: &str) -> String {
    // Shape binding inserts this fixed prefix after analysis. Reserve it in the
    // retained leaf's single allocation rather than allocating that path twice.
    let mut selector = String::with_capacity(value.len() + "/@shape/".len() + 64);
    selector.push_str(value);
    selector
}

/// Validate the actual locally restored mutation before native publication.
/// Existing incomplete code can be repaired without requiring unrelated syntax
/// cleanup, but a valid source/new file cannot gain parser errors.
pub fn validate_restored(path: &str, before: Option<&str>, after: &str) -> Result<(), String> {
    if after.len() > 8 * 1024 * 1024 {
        return Err("Restored source exceeds the 8 MiB parsing bound".into());
    }
    // Source-bound restoration rejects unresolved references before this syntax
    // boundary. Original literal prefix bytes are valid source, not new aliases.
    let format = syntax::format_for(path);
    if matches!(format, "text" | "markdown") {
        return Ok(());
    }
    if matches!(format, "csv" | "tsv") {
        return validate_csv(after, if format == "csv" { b',' } else { b'\t' });
    }
    let errors = syntax::error_count(format, after)?;
    if errors == 0 {
        return Ok(());
    }
    if matches!(format, "json" | "yaml" | "toml" | "html") {
        return Err("Restored structured data has syntax errors".into());
    }
    let previous = match before {
        Some(before) => syntax::error_count(format, before)?,
        None => 0,
    };
    if errors > previous {
        return Err("Restored code introduces additional syntax errors".into());
    }
    Ok(())
}

fn validate_csv(text: &str, delimiter: u8) -> Result<(), String> {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'"' {
            at += 1;
            loop {
                if at == bytes.len() {
                    return Err("Restored CSV has an unterminated quoted cell".into());
                }
                if bytes[at] == b'"' {
                    at += 1;
                    if bytes.get(at) == Some(&b'"') {
                        at += 1;
                        continue;
                    }
                    break;
                }
                at += 1;
            }
            if at < bytes.len() && bytes[at] != delimiter && !matches!(bytes[at], b'\r' | b'\n') {
                return Err("Restored CSV has data after a quoted cell".into());
            }
        } else {
            while at < bytes.len() && bytes[at] != delimiter && !matches!(bytes[at], b'\r' | b'\n')
            {
                if bytes[at] == b'"' {
                    return Err("Restored CSV has an unescaped quote".into());
                }
                at += 1;
            }
        }
        if bytes.get(at) == Some(&b'\r') && bytes.get(at + 1) == Some(&b'\n') {
            at += 2;
        } else if at < bytes.len() {
            at += 1;
        }
    }
    Ok(())
}

/// Selectors contain only grammar structure and ordinal indices, never values.
/// Exact selections use `/@digest/<digest>/<start>/<end>` and stop matching as
/// soon as any source byte changes. Ordinary selectors can be reused by shape.
pub fn exact_selector(document: &Document, node: &Node) -> String {
    format!(
        "/@digest/{}/{}/{}",
        document.digest, node.range.start, node.range.end
    )
}

pub fn analyze(path: &str, text: &str) -> Result<Document, String> {
    analyze_with_details(path, text, true)
}

fn analyze_with_details(path: &str, text: &str, details: bool) -> Result<Document, String> {
    let format = syntax::format_for(path);
    let mut builder = Builder::new(text);
    builder.details = details;
    match format {
        "markdown" => markup::markdown(&mut builder, text)?,
        "text" => markup::plain(&mut builder, text),
        "opaque" => builder.opaque(0..text.len(), "/unsupported"),
        "csv" | "tsv" => markup::csv(
            &mut builder,
            text,
            if format == "csv" { b',' } else { b'\t' },
        )?,
        format => syntax::analyze(&mut builder, format, text, 0, &format!("/{format}"))?,
    }
    builder.finish(format)
}

pub(super) fn source_format(path: &str) -> &'static str {
    syntax::format_for(path)
}

pub fn region_group_selector(node: &Node) -> Option<String> {
    regions::group_selector(node)
}

#[cfg(test)]
pub fn analyze_for_policy(path: &str, text: &str, policy: &Policy) -> Result<Document, String> {
    let mut document = analyze(path, text)?;
    let boundaries = policy
        .for_path(path)
        .exact_boundaries(&document.digest, text);
    if !boundaries.is_empty() {
        document.nodes = document
            .nodes
            .iter()
            .flat_map(|node| split_node(node, &boundaries))
            .collect();
    }
    Ok(document)
}

#[cfg(test)]
pub fn action_for_node(
    path: &str,
    text: &str,
    policy: &Policy,
    document: &Document,
    node: &Node,
) -> Action {
    if node.kind == AliasKind::Opaque && policy.default_action != Action::Allow {
        Action::Hide
    } else if document.web_data.is_empty() {
        policy.for_path(path).action_for_source(
            &node.selector,
            &exact_selector(document, node),
            &document.digest,
            text,
        )
    } else {
        policy
            .for_path(path)
            .selected_action_for_source(
                &node.selector,
                &exact_selector(document, node),
                &document.digest,
                text,
            )
            .unwrap_or_else(|| automatic_action(document, node, policy.default_action))
    }
}

pub(super) fn split_node(node: &Node, boundaries: &[usize]) -> Vec<Node> {
    let first = boundaries.partition_point(|offset| *offset <= node.range.start);
    let last = boundaries.partition_point(|offset| *offset < node.range.end);
    if first == last {
        return vec![node.clone()];
    }
    let mut cuts = Vec::with_capacity(last - first + 2);
    cuts.push(node.range.start);
    cuts.extend_from_slice(&boundaries[first..last]);
    cuts.push(node.range.end);
    cuts.windows(2)
        .enumerate()
        .map(|(index, range)| Node {
            selector: format!("{}/part/{index}", node.selector),
            label: format!("{} · part {}", node.label, index + 1),
            range: range[0]..range[1],
            kind: node.kind,
        })
        .collect()
}

pub fn project(
    path: &str,
    text: &str,
    policy: &Policy,
    alias: &mut impl FnMut(AliasKind, &str) -> Result<String, String>,
) -> Result<Projection, String> {
    let document = analyze(path, text)?;
    project_document_inner(path, text, Cow::Owned(document), policy, alias)
}

/// Runtime projection can omit UI-only metadata when no disclosure rules can
/// select it. Parsing, source ranges, literal kinds and private bytes remain
/// identical to the detailed editor projection. A policy with any rule always
/// uses the detailed path, including exact ranges and named groups.
pub fn project_request(
    path: &str,
    text: &str,
    policy: &Policy,
    alias: &mut impl FnMut(AliasKind, &str) -> Result<String, String>,
) -> Result<Projection, String> {
    if !policy.enabled || !policy.rules.is_empty() {
        return project(path, text, policy, alias);
    }
    let document = analyze_with_details(path, text, false)?;
    project_document_inner(path, text, Cow::Owned(document), policy, alias)
}

/// Reuse analysis only for byte-identical canonical source, never the preview.
#[cfg(test)]
pub fn project_document(
    path: &str,
    text: &str,
    document: &Document,
    policy: &Policy,
    alias: &mut impl FnMut(AliasKind, &str) -> Result<String, String>,
) -> Result<Projection, String> {
    project_document_inner(path, text, Cow::Borrowed(document), policy, alias)
}

fn project_document_inner(
    path: &str,
    text: &str,
    document: Cow<'_, Document>,
    policy: &Policy,
    alias: &mut impl FnMut(AliasKind, &str) -> Result<String, String>,
) -> Result<Projection, String> {
    policy.validate()?;
    // Owned analysis was just built from this same immutable source slice.
    // Only reused analysis can belong to different bytes.
    if matches!(&document, Cow::Borrowed(_)) && digest(text) != document.digest {
        return Err("privacy source changed after analysis".into());
    }
    let path_policy = policy.for_path(path);
    let boundaries = path_policy.exact_boundaries(&document.digest, text);
    let selected_nodes = (!boundaries.is_empty()).then(|| {
        document
            .nodes
            .iter()
            .flat_map(|node| split_node(node, &boundaries))
            .collect::<Vec<_>>()
    });
    let nodes = selected_nodes.as_deref().unwrap_or(&document.nodes);
    let mut output = String::with_capacity(text.len());
    let mut offset = 0;
    let mut changes = Vec::with_capacity(if policy.enabled { nodes.len() } else { 0 });
    for node in nodes {
        if node.range.start < offset
            || node.range.end > text.len()
            || !text.is_char_boundary(node.range.start)
            || !text.is_char_boundary(node.range.end)
        {
            return Err("invalid privacy source range".into());
        }
        output.push_str(&text[offset..node.range.start]);
        let original = &text[node.range.clone()];
        let exact = exact_selector(&document, node);
        let selected =
            path_policy.selected_action_for_source(&node.selector, &exact, &document.digest, text);
        let action = if node.kind == AliasKind::Opaque && policy.default_action != Action::Allow {
            Action::Hide
        } else {
            selected.unwrap_or_else(|| automatic_action(&document, node, policy.default_action))
        };
        if !policy.enabled || action == Action::Allow {
            output.push_str(original);
            if policy.enabled && action == Action::Allow && selected.is_some() {
                changes.push(Change {
                    selector: node.selector.clone(),
                    range: node.range.clone(),
                    original: original.to_string(),
                    replacement: original.to_string(),
                    kind: node.kind,
                    action,
                });
            }
        } else {
            let replacement = alias(node.kind, original)?;
            if replacement.is_empty()
                || replacement.len() > 1024
                || (if replacement.is_ascii() {
                    replacement.bytes().any(|byte| byte.is_ascii_control())
                } else {
                    replacement.chars().any(char::is_control)
                })
            {
                return Err("privacy alias must be a nonempty bounded visible token".into());
            }
            output.push_str(&replacement);
            // Preserve line structure without sending private spelling. Exact
            // byte restoration uses the ticket, not a global alias replacement.
            for byte in original.bytes().filter(|byte| *byte == b'\n') {
                output.push(char::from(byte));
            }
            changes.push(Change {
                selector: node.selector.clone(),
                range: node.range.clone(),
                original: original.to_string(),
                replacement,
                kind: node.kind,
                action,
            });
        }
        offset = node.range.end;
    }
    output.push_str(&text[offset..]);
    let digest = document.digest.clone();
    let nodes = selected_nodes.unwrap_or_else(|| document.into_owned().nodes);
    Ok(Projection {
        text: output,
        changes,
        nodes,
        digest,
    })
}

pub(super) struct Builder<'a> {
    text: &'a str,
    pub(super) nodes: Vec<Node>,
    safe: Vec<Range<usize>>,
    details: bool,
}

impl<'a> Builder<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            nodes: Vec::new(),
            safe: Vec::new(),
            details: true,
        }
    }
    pub(super) fn safe(&mut self, range: Range<usize>) {
        if !range.is_empty() {
            self.safe.push(range);
        }
    }
    pub(super) fn node(&mut self, range: Range<usize>, selector: String, kind: AliasKind) {
        if range.is_empty() {
            return;
        }
        self.nodes.push(Node {
            label: if self.details {
                format!("{} {}", kind.label(), self.nodes.len() + 1)
            } else {
                String::new()
            },
            selector: if self.details {
                selector
            } else {
                String::new()
            },
            range,
            kind,
        });
    }
    pub(super) fn opaque(&mut self, range: Range<usize>, selector: &str) {
        let selector = if self.details {
            format!("{selector}/opaque")
        } else {
            String::new()
        };
        self.node(range, selector, AliasKind::Opaque);
    }
    pub(super) fn retained_selector(&self, value: &str) -> String {
        if self.details {
            retained_selector(value)
        } else {
            String::new()
        }
    }
    fn finish(mut self, format: &str) -> Result<Document, String> {
        self.nodes
            .sort_by_key(|node| (node.range.start, node.range.end));
        // Autolinks can yield both a destination and a text event for the same
        // exact bytes. They represent one private source region.
        self.nodes.dedup_by(|right, left| right.range == left.range);
        // Named leaves and grammar delimiters cover all intentionally public
        // bytes. Any unclaimed non-whitespace interval is private by default.
        let mut safe = std::mem::take(&mut self.safe);
        safe.sort_by_key(|range| (range.start, range.end));
        let original_nodes = self.nodes.len();
        let (mut node_index, mut safe_index) = (0, 0);
        let mut end = 0;
        // Private leaves are already sorted. Merge coverage in place instead
        // of copying every leaf range into a second vector and sorting it too.
        // New gaps append after original_nodes, so they cannot enter this walk.
        while node_index < original_nodes || safe_index < safe.len() {
            let range = if node_index < original_nodes
                && (safe_index == safe.len()
                    || (
                        self.nodes[node_index].range.start,
                        self.nodes[node_index].range.end,
                    ) <= (safe[safe_index].start, safe[safe_index].end))
            {
                let range = self.nodes[node_index].range.clone();
                node_index += 1;
                range
            } else {
                let range = safe[safe_index].clone();
                safe_index += 1;
                range
            };
            if range.start > end {
                self.cover_gap(end..range.start);
            }
            end = end.max(range.end);
        }
        if end < self.text.len() {
            self.cover_gap(end..self.text.len());
        }
        self.nodes
            .sort_by_key(|node| (node.range.start, node.range.end));
        let mut end = 0;
        for node in &self.nodes {
            if node.range.start < end
                || node.range.end > self.text.len()
                || !self.text.is_char_boundary(node.range.start)
                || !self.text.is_char_boundary(node.range.end)
            {
                return Err("overlapping privacy parser ranges".into());
            }
            end = node.range.end;
        }
        regions::bind(&mut self.nodes, self.text, format);
        if self.details {
            let shape = structural_digest(format, self.text, &self.nodes);
            let shape_prefix = format!("/@shape/{shape}");
            for node in &mut self.nodes {
                regions::bind_shape(&mut node.selector, &shape_prefix);
            }
        } else {
            // Named directives still split the same private ranges/kinds, but
            // their editor-only labels/selectors are never consulted here.
            for node in &mut self.nodes {
                node.selector.clear();
                node.label.clear();
            }
        }
        Ok(Document {
            format: format.to_string(),
            nodes: self.nodes,
            digest: digest(self.text),
            web_data: if matches!(format, "html" | "css" | "javascript" | "typescript" | "tsx") {
                syntax::web_data_ranges(format, self.text)?
            } else {
                Vec::new()
            },
        })
    }
    fn cover_gap(&mut self, range: Range<usize>) {
        let value = &self.text[range.clone()];
        let Some(start) = value.find(|ch: char| !ch.is_whitespace()) else {
            return;
        };
        let end = value.trim_end_matches(char::is_whitespace).len();
        self.opaque(
            range.start + start..range.start + end,
            &format!("/unclassified/{}", self.nodes.len()),
        );
    }
}

/// Reusable disclosure is bound to document identity/shape, not an ordinal
/// alone. Scalars can change within an established field; a different key,
/// symbol, section, element identity or grammar layout invalidates that grant.
fn structural_digest(format: &str, text: &str, nodes: &[Node]) -> String {
    fn part(hash: &mut Sha256, value: &[u8]) {
        hash.update((value.len() as u64).to_le_bytes());
        hash.update(value);
    }
    let mut hash = Sha256::new();
    part(&mut hash, b"ca-privacy-structure.v1");
    part(&mut hash, format.as_bytes());
    let mut offset = 0;
    for node in nodes {
        part(&mut hash, &text.as_bytes()[offset..node.range.start]);
        part(&mut hash, node.selector.as_bytes());
        part(&mut hash, node.kind.label().as_bytes());
        let identity = matches!(
            node.kind,
            AliasKind::Key | AliasKind::Identifier | AliasKind::Opaque
        ) || (matches!(format, "csv" | "tsv")
            && node.selector.starts_with("/csv/row/0/"))
            || html_element_identity(text, node);
        if identity {
            part(&mut hash, &text.as_bytes()[node.range.clone()]);
        }
        offset = node.range.end;
    }
    part(&mut hash, &text.as_bytes()[offset..]);
    if format == "markdown" {
        use pulldown_cmark::{Event, Options, Parser, Tag};
        for (event, range) in Parser::new_ext(text, Options::all()).into_offset_iter() {
            if matches!(event, Event::Start(Tag::Heading { .. })) {
                part(&mut hash, b"heading");
                part(&mut hash, &text.as_bytes()[range]);
            }
        }
    }
    if format == "yaml" {
        // YAML keys may live below flow/plain-scalar wrappers. Ordered mapping
        // keys supply the same identity binding as JSON/TOML Key nodes.
        fn yaml_keys(value: &serde_yaml_ng::Value, hash: &mut Sha256, depth: usize) {
            if depth > 128 {
                return;
            }
            match value {
                serde_yaml_ng::Value::Mapping(mapping) => {
                    for (key, value) in mapping {
                        part(hash, b"yaml-key");
                        if let Ok(encoded) = serde_yaml_ng::to_string(key) {
                            part(hash, encoded.as_bytes());
                        }
                        yaml_keys(value, hash, depth + 1);
                    }
                }
                serde_yaml_ng::Value::Sequence(items) => {
                    for item in items {
                        yaml_keys(item, hash, depth + 1);
                    }
                }
                serde_yaml_ng::Value::Tagged(tagged) => yaml_keys(&tagged.value, hash, depth + 1),
                _ => {}
            }
        }
        if let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text) {
            yaml_keys(&value, &mut hash, 0);
        }
    }
    format!("{:x}", hash.finalize())
}

fn html_element_identity(text: &str, node: &Node) -> bool {
    if node.kind != AliasKind::String
        || !node.selector.contains("/attribute/")
        || !node.selector.ends_with("/value")
    {
        return false;
    }
    let before = text[..node.range.start].trim_end_matches(['\'', '"', ' ', '\t', '\r', '\n']);
    let Some(before) = before.strip_suffix('=') else {
        return false;
    };
    let before = before.trim_end_matches(char::is_whitespace);
    let start = before
        .rfind(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | '-' | ':'))
        .map_or(0, |offset| {
            offset
                + before[offset..]
                    .chars()
                    .next()
                    .expect("character exists")
                    .len_utf8()
        });
    matches!(before[start..].to_ascii_lowercase().as_str(), "id" | "name")
}

#[cfg(test)]
mod tests {
    use super::super::policy::Rule;
    use super::*;
    #[test]
    fn automatic_html_data_preserves_structure_and_restores_exactly() {
        let source = r##"<html><head><style>.card { color: red; background: url("PRIVATE_ASSET"); } .card::after { content: "PRIVATE_CSS"; }</style></head><body><h1 id="heading" class="card">PRIVATE_TITLE</h1><input type="text" value="PRIVATE_FORM" aria-label="PRIVATE_LABEL"><a href="https://PRIVATE_HOST">PRIVATE_LINK</a><div data-customer="PRIVATE_CUSTOMER"></div><!--PRIVATE_COMMENT--><script type="application/json">{"customer":"PRIVATE_JSON","balance":987654}</script><script>const customer = "PRIVATE_JS"; const balance = 876543; document.querySelector(".card").hidden = false;</script></body></html>"##;
        let policy = Policy {
            enabled: true,
            default_action: Action::Allow,
            ..Policy::default()
        };
        for request in [false, true] {
            let mut index = 0;
            let mut alias = |_: AliasKind, _: &str| {
                index += 1;
                Ok(format!("PRIVATE_TOKEN_{index}"))
            };
            let projected = if request {
                project_request("page.HTML", source, &policy, &mut alias)
            } else {
                project("page.HTML", source, &policy, &mut alias)
            }
            .unwrap();
            for private in [
                "PRIVATE_ASSET",
                "PRIVATE_CSS",
                "PRIVATE_TITLE",
                "PRIVATE_FORM",
                "PRIVATE_LABEL",
                "PRIVATE_HOST",
                "PRIVATE_LINK",
                "PRIVATE_CUSTOMER",
                "PRIVATE_COMMENT",
                "PRIVATE_JSON",
                "987654",
                "PRIVATE_JS",
                "876543",
            ] {
                assert!(
                    !projected.text.contains(private),
                    "leaked {private}: {}",
                    projected.text
                );
            }
            for public in [
                "<h1 id=\"heading\" class=\"card\">",
                ".card { color: red; background: url(",
                "const customer =",
                "document.querySelector(\".card\").hidden =",
                "\"customer\":",
            ] {
                assert!(
                    projected.text.contains(public),
                    "lost {public}: {}",
                    projected.text
                );
            }
            let mut restored = projected.text.clone();
            for change in projected.changes.iter().rev() {
                restored = restored.replace(&change.replacement, &change.original);
            }
            assert_eq!(restored, source);
            let plain = project("page.html", source, &Policy::default(), &mut alias).unwrap();
            assert_eq!(plain.text, source);
        }
    }

    #[test]
    fn automatic_html_data_explicit_allow_and_parse_failure() {
        let policy = Policy {
            enabled: true,
            default_action: Action::Allow,
            rules: vec![Rule {
                file: "*".into(),
                selector: "**".into(),
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        let source = "<p>approved text</p>";
        assert_eq!(
            project("a.html", source, &policy, &mut |_, _| Ok("TOKEN".into()))
                .unwrap()
                .text,
            source
        );
        let policy = Policy {
            rules: vec![],
            ..policy
        };
        let malformed = "<div title=\"PRIVATE_UNCLOSED";
        assert!(
            !project_request("a.htm", malformed, &policy, &mut |_, _| Ok("TOKEN".into()))
                .unwrap()
                .text
                .contains("PRIVATE_UNCLOSED")
        );
    }

    #[test]
    fn automatic_html_data_encoded_text_imports_and_dynamic_selectors() {
        let source = r#"<p>&#83;&#69;&#67;&#82;&#69;&#84;</p><style>@import "https://PRIVATE_IMPORT.example/secret.css";</style><script>document.querySelector(buildSelector("PRIVATE_DYNAMIC")); document.querySelector(".public-card");</script>"#;
        let policy = Policy {
            enabled: true,
            default_action: Action::Allow,
            ..Policy::default()
        };
        let projected =
            project_request("page.html", source, &policy, &mut |_, _| Ok("TOKEN".into())).unwrap();
        for private in ["&#83;", "&#69;", "PRIVATE_IMPORT", "PRIVATE_DYNAMIC"] {
            assert!(
                !projected.text.contains(private),
                "{private}: {}",
                projected.text
            );
        }
        assert!(
            projected.text.contains("@import \"TOKEN\""),
            "{}",
            projected.text
        );
        assert!(projected
            .text
            .contains("document.querySelector(\".public-card\")"));
    }
    fn private() -> Policy {
        Policy {
            enabled: true,
            ..Policy::default()
        }
    }
    fn hide(path: &str, text: &str) -> Projection {
        let mut count = 0;
        project(path, text, &private(), &mut |kind, _| {
            count += 1;
            Ok(format!("__{}_{}__", kind.label().to_uppercase(), count))
        })
        .unwrap()
    }
    #[test]
    fn privacy_merged_coverage_matches_combined_sort_reference() {
        fn reference(mut builder: Builder<'_>) -> Result<Vec<(Range<usize>, AliasKind)>, String> {
            builder
                .nodes
                .sort_by_key(|node| (node.range.start, node.range.end));
            builder
                .nodes
                .dedup_by(|right, left| right.range == left.range);
            // Preserve the former combined-vector implementation as an
            // independent oracle for gap order and overlap rejection.
            let mut covered = builder.safe.clone();
            covered.extend(builder.nodes.iter().map(|node| node.range.clone()));
            covered.sort_by_key(|range| (range.start, range.end));
            let mut end = 0;
            for range in covered {
                if range.start > end {
                    builder.cover_gap(end..range.start);
                }
                end = end.max(range.end);
            }
            if end < builder.text.len() {
                builder.cover_gap(end..builder.text.len());
            }
            builder
                .nodes
                .sort_by_key(|node| (node.range.start, node.range.end));
            let mut end = 0;
            for node in &builder.nodes {
                if node.range.start < end
                    || node.range.end > builder.text.len()
                    || !builder.text.is_char_boundary(node.range.start)
                    || !builder.text.is_char_boundary(node.range.end)
                {
                    return Err("overlapping privacy parser ranges".into());
                }
                end = node.range.end;
            }
            Ok(builder
                .nodes
                .into_iter()
                .map(|node| (node.range, node.kind))
                .collect())
        }
        for (source, private_ranges, safe_ranges) in [
            (
                "aa bb cc dd ee",
                vec![9..11, 3..5],
                vec![12..14, 0..2, 0..2],
            ),
            (
                "aa bb cc dd ee",
                vec![3..5, 9..11],
                vec![1..2, 0..8, 12..14, 0..2],
            ),
            ("aa bb cc dd ee", vec![], vec![12..14, 0..2]),
            ("aa bb cc dd ee", vec![3..5, 3..5, 9..11], vec![]),
            (
                "aa bb cc dd ee",
                vec![0..5, 3..8],
                std::iter::once(12..14).collect(),
            ),
            (
                "αβ cc δε\n  zz",
                std::iter::once(8..12).collect(),
                vec![15..17, 0..4],
            ),
            (" \n\t ", vec![], vec![]),
        ] {
            let build = || {
                let mut builder = Builder::new(source);
                builder.details = false;
                for range in &private_ranges {
                    builder.node(range.clone(), String::new(), AliasKind::String);
                }
                for range in &safe_ranges {
                    builder.safe(range.clone());
                }
                builder
            };
            let expected = reference(build());
            let actual = build().finish("text").map(|document| {
                document
                    .nodes
                    .into_iter()
                    .map(|node| (node.range, node.kind))
                    .collect::<Vec<_>>()
            });
            assert_eq!(actual, expected, "{source:?}");
        }
    }

    #[test]
    fn privacy_alias_validation_retains_unicode_and_rejects_ascii_and_unicode_controls() {
        for alias in ["__ASCII__", "é界", "é\u{200b}界"] {
            let projected = project("a.txt", "private source", &private(), &mut |_, _| {
                Ok(alias.to_string())
            })
            .unwrap();
            assert_eq!(projected.text, alias);
        }
        let oversized = "x".repeat(1025);
        for alias in [
            "",
            "bad\nvalue",
            "bad\u{7f}value",
            "badé\u{85}value",
            "bad\u{9f}界",
            oversized.as_str(),
        ] {
            assert!(
                project("a.txt", "private source", &private(), &mut |_, _| Ok(
                    alias.to_string()
                ))
                .is_err(),
                "{alias:?}"
            );
        }
    }
    #[test]
    fn privacy_request_default_hide_matches_full_private_ranges_values_and_order() {
        for (path, source) in [
            ("a.rs", "fn private_fn() { let secret_name: i64 = 12345; let enabled = true; let text = \"private value\"; }"),
            ("a.c", "int private_fn(void) { return 12345; }"),
            ("a.cpp", "class PrivateName { int secret; };"),
            ("a.py", "def private_fn(secret):\n    return 'private value'\n"),
            ("a.sh", "secret_name='private value'\nprintf '%s' \"$secret_name\"\n"),
            ("a.js", "function private_fn(secret) { return [false, null, 'private value']; }"),
            ("a.ts", "const private_name: string = 'private value';"),
            ("a.jsx", "const x = <PrivateName title=\"private value\">Secret text</PrivateName>;"),
            ("a.tsx", "const x: unknown = <PrivateName title=\"private value\" />;"),
            ("a.json", r#"{"private_key":["private value",12345,true,null]}"#),
            ("a.yaml", "private_key: private value\nprivate_number: 12345\n"),
            ("a.toml", "private_key = 'private value'\nprivate_number = 12345\n"),
            ("a.csv", "private_key,other\n\"private,value\",12345\n"),
            ("a.tsv", "private_key\tother\nprivate value\t12345\n"),
            ("a.html", "<div id=\"private-id\" class=\"private-class\" style=\"color: red\" onclick=\"private_fn('value')\">private text</div><script>let private_name = 'private value';</script>"),
            ("a.css", ".private-class { color: red; width: 12345px; }"),
            ("a.md", "# Private heading\n\nPrivate text.\n\n```rust\nlet private_name = \"private value\";\n```\n"),
            ("a.txt", "before\nca-privacy:begin PUBLIC\nprivate value\nca-privacy:end PUBLIC\nafter\n"),
            ("a.rs", "// ca-privacy:begin PUBLIC\nfn private_fn() {}\n// ca-privacy:end PUBLIC\n"),
            ("a.unknown", "private unsupported source"),
            ("a.json", "{ invalid private source"),
        ] {
            let run = |request: bool, policy: &Policy| {
                let mut next = 0;
                let mut alias = |kind: AliasKind, _: &str| {
                    next += 1;
                    Ok(format!("__{}_{}__", kind.label(), next))
                };
                if request { project_request(path, source, policy, &mut alias) }
                else { project(path, source, policy, &mut alias) }.unwrap()
            };
            let policy = private();
            let full = run(false, &policy);
            let request = run(true, &policy);
            assert!(request.nodes.iter().all(|node| node.selector.is_empty() && node.label.is_empty()), "{path}");
            let mut privacy_fields = full;
            for node in &mut privacy_fields.nodes { node.selector.clear(); node.label.clear(); }
            for change in &mut privacy_fields.changes { change.selector.clear(); }
            assert_eq!(request, privacy_fields, "{path}: every private range/kind/original/replacement/action and output byte must match");

            // Even an unrelated/path-only rule must restore full metadata:
            // later edits and previews may consult selectors under that policy.
            let mut explicit = policy;
            explicit.rules.push(Rule { file:"**".into(),selector:"/@path".into(),action:Action::Hide });
            assert_eq!(run(true, &explicit), run(false, &explicit), "{path}: explicit rules require detailed selectors");
            explicit.rules.clear();
            explicit.enabled = false;
            assert_eq!(run(true, &explicit), run(false, &explicit), "{path}: disabled preview semantics remain unchanged");
        }
    }
    #[test]
    fn owned_and_borrowed_analysis_projection_are_byte_identical() {
        for (path, source) in [
            (
                "source.rs",
                "fn private_fn() { let private_name = \"private_value\"; }",
            ),
            (
                "source.json",
                r#"{"private_key":"private_value","number":73}"#,
            ),
            ("source.md", "# Private heading\n\nPrivate paragraph.\n"),
        ] {
            let document = analyze(path, source).unwrap();
            for range_rule in [false, true] {
                let mut policy = private();
                if range_rule {
                    let start = source.find("private").unwrap_or(0);
                    policy.rules.push(Rule {
                        file: path.into(),
                        selector: format!("/@digest/{}/{start}/{}", document.digest, start + 3),
                        action: Action::Allow,
                    });
                }
                let run = |borrowed| {
                    let mut next = 0;
                    let mut alias = |kind: AliasKind, _: &str| {
                        next += 1;
                        Ok(format!("__{}_{}__", kind.label(), next))
                    };
                    if borrowed {
                        project_document(path, source, &document, &policy, &mut alias)
                    } else {
                        project(path, source, &policy, &mut alias)
                    }
                    .unwrap()
                };
                assert_eq!(run(false), run(true), "{path} exact_range={range_rule}");
            }
        }
    }

    #[test]
    fn strict_json_keeps_layout_and_hides_keys_values_and_unknown_unicode() {
        let source = "{\n  \"private_key\": \"秘密\", \"amount\": 72819, \"yes\": true\n}";
        let result = hide("a.json", source);
        for secret in ["private_key", "秘密", "amount", "72819", "yes", "true"] {
            assert!(!result.text.contains(secret), "{secret}: {}", result.text);
        }
        assert!(result.text.starts_with("{\n  \""));
        assert_eq!(
            result.text.matches('\n').count(),
            source.matches('\n').count()
        );
        let mut restored = result.text.clone();
        for change in result.changes.iter().rev() {
            let view = format!(
                "{}{}",
                change.replacement,
                "\n".repeat(change.original.matches('\n').count())
            );
            restored = restored.replace(&view, &change.original);
        }
        assert_eq!(restored, source);
    }
    #[test]
    fn reusable_shape_allow_and_digest_bound_exact_selection() {
        let source = "{\"private\":\"secret\"}";
        let document = analyze("a.json", source).unwrap();
        let node = document
            .nodes
            .iter()
            .find(|node| &source[node.range.clone()] == "secret")
            .unwrap();
        let policy = Policy {
            enabled: true,
            rules: vec![Rule {
                file: "*.json".into(),
                selector: exact_selector(&document, node),
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        let mut alias = |_, _: &str| Ok("__PRIVATE__".into());
        assert!(project("a.json", source, &policy, &mut alias)
            .unwrap()
            .text
            .contains("secret"));
        assert!(
            !project("a.json", "{\"private\":\"changed\"}", &policy, &mut alias)
                .unwrap()
                .text
                .contains("changed")
        );
        assert!(project_document("a.json", "stale", &document, &policy, &mut alias).is_err());
    }

    #[test]
    fn reusable_allow_tracks_field_identity_not_a_reused_ordinal() {
        let source = r#"{"public_field":"published","secret_field":"protected"}"#;
        let document = analyze("a.json", source).unwrap();
        let node = document
            .nodes
            .iter()
            .find(|node| &source[node.range.clone()] == "published")
            .unwrap();
        let policy = Policy {
            enabled: true,
            rules: vec![Rule {
                file: "*.json".into(),
                selector: node.selector.clone(),
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        let mut alias = |_, _: &str| Ok("__PRIVATE__".into());
        let updated = r#"{"public_field":"new_public_value","secret_field":"new_private_value"}"#;
        let projected = project("a.json", updated, &policy, &mut alias).unwrap();
        assert!(projected.text.contains("new_public_value"));
        assert!(!projected.text.contains("new_private_value"));
        for changed in [
            r#"{"inserted_secret":"new_secret","public_field":"published","secret_field":"protected"}"#,
            r#"{"secret_field":"new_secret","public_field":"published"}"#,
            r#"{"renamed_secret":"new_secret","secret_field":"protected"}"#,
        ] {
            let out = project("a.json", changed, &policy, &mut alias).unwrap();
            assert!(!out.text.contains("new_secret"), "{changed}: {}", out.text);
            assert!(out
                .changes
                .iter()
                .all(|change| change.action != Action::Allow));
        }
    }

    #[test]
    fn reusable_markdown_allow_invalidates_when_section_identity_changes() {
        let source = "# Public heading\n\npublic_body\n\n# Secret heading\n\nprivate_body\n";
        let document = analyze("a.md", source).unwrap();
        let node = document
            .nodes
            .iter()
            .find(|node| &source[node.range.clone()] == "public_body")
            .unwrap();
        let policy = Policy {
            enabled: true,
            rules: vec![Rule {
                file: "*.md".into(),
                selector: node.selector.clone(),
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        let mut alias = |_, _: &str| Ok("__PRIVATE__".into());
        let same = source
            .replace("public_body", "updated_public_body")
            .replace("private_body", "updated_private_body");
        let out = project("a.md", &same, &policy, &mut alias).unwrap();
        assert!(out.text.contains("updated_public_body"));
        assert!(!out.text.contains("updated_private_body"));
        for changed in [
            source.replace("Public heading", "Different identity"),
            format!("# Inserted secret\n\nnew_secret\n\n{source}"),
            "# Secret heading\n\nnew_secret\n\n# Public heading\n\npublic_body\n".into(),
        ] {
            let out = project("a.md", &changed, &policy, &mut alias).unwrap();
            assert!(!out.text.contains("public_body"));
            assert!(!out.text.contains("new_secret"));
        }
    }

    #[test]
    fn reusable_html_allow_binds_element_id_and_name() {
        let source = "<input id=\"public-control\" name=\"public-field\" value=\"published\">";
        let document = analyze("a.html", source).unwrap();
        let node = document
            .nodes
            .iter()
            .find(|node| &source[node.range.clone()] == "published")
            .unwrap();
        let policy = Policy {
            enabled: true,
            rules: vec![Rule {
                file: "*.html".into(),
                selector: node.selector.clone(),
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        let mut alias = |_, _: &str| Ok("__PRIVATE__".into());
        let same = source.replace("published", "new_public_value");
        assert!(project("a.html", &same, &policy, &mut alias)
            .unwrap()
            .text
            .contains("new_public_value"));
        for changed in [
            source.replace("public-control", "secret-control"),
            source.replace("public-field", "secret-field"),
        ] {
            assert!(!project("a.html", &changed, &policy, &mut alias)
                .unwrap()
                .text
                .contains("published"));
        }
    }
    #[test]
    fn code_formats_keep_grammar_and_hide_identifiers_comments_literals() {
        for (path, source, secret) in [
            (
                "a.rs",
                "fn private_fn() { let secret_name = \"hidden_value\"; }",
                "hidden_value",
            ),
            (
                "a.c",
                "int private_fn(void) { return 72819; }",
                "private_fn",
            ),
            ("a.cpp", "class PrivateName { int secret; };", "PrivateName"),
            (
                "a.py",
                "def private_fn(secret):\n    return 'hidden_value'\n",
                "hidden_value",
            ),
            (
                "a.sh",
                "secret_name='hidden_value'\nprintf '%s' \"$secret_name\"\n",
                "hidden_value",
            ),
            (
                "a.js",
                "function private_fn(secret) { return 'hidden_value'; }",
                "hidden_value",
            ),
            (
                "a.ts",
                "const private_name: string = 'hidden_value';",
                "private_name",
            ),
            (
                "a.jsx",
                "const x = <PrivateName title=\"hidden_value\">Secret text</PrivateName>;",
                "hidden_value",
            ),
            (
                "a.tsx",
                "const x: unknown = <PrivateName title=\"hidden_value\" />;",
                "hidden_value",
            ),
        ] {
            let result = hide(path, source);
            assert!(!result.text.contains(secret), "{path}: {}", result.text);
            assert!(!result.nodes.is_empty());
        }
    }
    #[test]
    fn invalid_syntax_is_opaque_and_never_passed_through() {
        for path in ["a.json", "a.rs", "a.yaml", "a.toml"] {
            let result = hide(path, "\"unterminated private_value");
            assert!(!result.text.contains("private_value"));
        }
    }

    #[test]
    fn privacy_unparsed_and_unsupported_content_stays_opaque_under_broad_allow() {
        let policy = Policy {
            enabled: true,
            rules: vec![super::super::policy::Rule {
                file: "**".into(),
                selector: "/**".into(),
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        for (path, source) in [
            ("unknown.xyz", "private unsupported canary"),
            ("broken.json", "{\"private malformed canary\":"),
            ("broken.rs", "fn broken( private_malformed_canary"),
            (
                "template.html",
                "<script type=\"text/template\">private template canary</script>",
            ),
        ] {
            let projection =
                project(path, source, &policy, &mut |_, _| Ok("OPAQUE_1".into())).unwrap();
            assert!(
                !projection.text.contains("private"),
                "{path}: {}",
                projection.text
            );
            assert!(projection
                .changes
                .iter()
                .any(|change| change.kind == AliasKind::Opaque && change.action == Action::Hide));
        }
        assert_eq!(
            project(
                "ordinary.txt",
                "allowed plain prose",
                &policy,
                &mut |_, _| Ok("OPAQUE_1".into())
            )
            .unwrap()
            .text,
            "allowed plain prose"
        );
    }

    #[test]
    fn privacy_markdown_named_and_unknown_fences_preserve_delimiters_and_hide_contents() {
        for (language, body) in [
            ("rust", "fn private_name() { let value = 73; }"),
            ("python", "private_name = 73"),
            ("javascript", "const private_name = 73;"),
            ("typescript", "const private_name: number = 73;"),
            ("sql", "select private_name from private_table;"),
            ("", "private_name"),
        ] {
            let source = format!("```{language}\n{body}\n```\n");
            let projection = hide("notes.md", &source);
            assert!(
                !projection.text.contains("private_name"),
                "{language}: {}",
                projection.text
            );
            assert!(
                projection.text.contains("```"),
                "{language}: {}",
                projection.text
            );
        }
    }

    #[test]
    fn restored_mutations_reject_new_syntax_errors() {
        assert!(validate_restored("a.rs", Some("fn main() {}"), "fn main( {").is_err());
        assert!(validate_restored("a.py", None, "def broken(:\n").is_err());
        assert!(validate_restored("a.json", Some("{}"), "{\"key\":}").is_err());
        assert!(validate_restored("a.csv", None, "\"unterminated").is_err());
        assert!(
            validate_restored("a.html", None, "<div onclick=\"function (\">text</div>").is_err()
        );
    }

    #[test]
    fn restored_mutations_allow_repairs_and_unchanged_incomplete_code() {
        let broken = "fn main( {";
        assert!(validate_restored("a.rs", Some(broken), "fn main() {}").is_ok());
        assert!(validate_restored("a.rs", Some(broken), broken).is_ok());
        for (path, value) in [
            ("a.rs", "fn main() {}"),
            ("a.json", "{\"key\":123}"),
            ("a.yaml", "key: value\n"),
            ("a.toml", "key = 123\n"),
            ("a.csv", "\"one,two\",three\n"),
            ("a.html", "<div style=\"color: red\">text</div>"),
            ("a.md", "unclosed *prose"),
        ] {
            assert!(
                validate_restored(path, Some(value), value).is_ok(),
                "{path}"
            );
        }
    }

    #[test]
    fn html_json_data_scripts_use_json_and_templates_stay_opaque() {
        for mime in ["application/json", "application/ld+json"] {
            let source = format!(
                r#"<script type="{mime}">{{"private_key":"PrivateValue","private_count":91239}}</script>"#
            );
            assert!(validate_restored("a.html", None, &source).is_ok());
            assert!(validate_restored(
                "a.html",
                Some(&source),
                &source.replace("PrivateValue", "ChangedValue")
            )
            .is_ok());
            let document = analyze("a.html", &source).unwrap();
            assert!(document.nodes.iter().any(
                |node| node.kind == AliasKind::Number && &source[node.range.clone()] == "91239"
            ));
            let out = hide("a.html", &source);
            assert!(!out.text.contains("PrivateValue"));
            assert!(!out.text.contains("private_key"));
            let malformed = format!(r#"<script type="{mime}">{{"private_key":}}</script>"#);
            assert!(validate_restored("a.html", Some(&source), &malformed).is_err());
        }
        let template = r#"<script type="text/x-template">{{ private_value }} <custom-part>secret text</custom-part></script>"#;
        assert!(validate_restored("a.html", None, template).is_ok());
        let document = analyze("a.html", template).unwrap();
        assert!(document
            .nodes
            .iter()
            .any(|node| node.kind == AliasKind::Opaque
                && template[node.range.clone()].contains("private_value")));
        let out = hide("a.html", template);
        assert!(!out.text.contains("private_value"));
        assert!(!out.text.contains("secret text"));
    }

    #[test]
    fn html_class_id_css_and_static_js_selectors_share_exact_aliases() {
        let policy = private();
        let mut registry = super::super::store::AliasRegistry::new(uuid::Uuid::new_v4());
        let mut alias = |kind: AliasKind, value: &str| registry.alias(kind.label(), value);
        let html =
            r#"<div id="private-panel" class="private-card  private-active">private body</div>"#;
        let css = "#private-panel .private-card.private-active > span { color: red; }";
        let js = r##"document.getElementById('private-panel'); document.querySelector('#private-panel .private-card.private-active > span'); element.querySelectorAll('.private-card'); element.matches('.private-active'); element.closest('.private-card');"##;
        let html_out = project("a.html", html, &policy, &mut alias).unwrap();
        let css_out = project("a.css", css, &policy, &mut alias).unwrap();
        let js_out = project("a.js", js, &policy, &mut alias).unwrap();
        for original in ["private-panel", "private-card", "private-active"] {
            let h = html_out
                .changes
                .iter()
                .find(|change| change.original == original)
                .unwrap();
            let c = css_out
                .changes
                .iter()
                .find(|change| change.original == original)
                .unwrap();
            let j = js_out
                .changes
                .iter()
                .find(|change| change.original == original)
                .unwrap();
            assert_eq!(
                h.replacement, c.replacement,
                "CSS relationship for {original}"
            );
            assert_eq!(
                h.replacement, j.replacement,
                "JS relationship for {original}"
            );
            assert!(!html_out.text.contains(original));
            assert!(!css_out.text.contains(original));
            assert!(!js_out.text.contains(original));
        }
        assert!(html_out.text.contains("  "));
        let card = html_out
            .changes
            .iter()
            .find(|change| change.original == "private-card")
            .unwrap();
        let active = html_out
            .changes
            .iter()
            .find(|change| change.original == "private-active")
            .unwrap();
        assert!(js_out.text.contains(&format!(
            ".{}.{} > span",
            card.replacement, active.replacement
        )));
        let dynamic = r#"document.querySelector('.private-card' + private_suffix); document.querySelector('[data-private="value"]');"#;
        let dynamic_out = project("a.js", dynamic, &policy, &mut alias).unwrap();
        assert!(!dynamic_out.text.contains("private_suffix"));
        assert!(!dynamic_out.text.contains("private-card"));
        assert!(dynamic_out
            .changes
            .iter()
            .any(|change| change.kind == AliasKind::Opaque
                && change.original.contains("private_suffix")));
    }

    #[test]
    fn arbitrary_exact_ranges_split_unicode_identifiers_strings_and_multiple_nodes() {
        for (path, source, selected) in [
            ("a.txt", "αβγδε", "βγ"),
            (
                "a.rs",
                "fn private_function() { let very_sensitive_name = \"private-string-value\"; }",
                "sensitive",
            ),
            ("a.json", r#"{"field":"private-string-value"}"#, "string"),
        ] {
            let start = source.find(selected).unwrap();
            let end = start + selected.len();
            let policy = Policy {
                enabled: true,
                rules: vec![Rule {
                    file: "**".into(),
                    selector: format!("/@digest/{}/{start}/{end}", digest(source)),
                    action: Action::Allow,
                }],
                ..Policy::default()
            };
            let projection =
                project(path, source, &policy, &mut |_, _| Ok("__MASK__".into())).unwrap();
            assert!(
                projection.text.contains(selected),
                "{path}: {}",
                projection.text
            );
            let allowed: Vec<_> = projection
                .changes
                .iter()
                .filter(|change| change.action == Action::Allow)
                .collect();
            assert_eq!(
                allowed
                    .iter()
                    .map(|change| change.original.as_str())
                    .collect::<String>(),
                selected
            );
            assert_eq!(allowed[0].range, start..end);
            assert!(
                !project(path, &format!("{source} "), &policy, &mut |_, _| Ok(
                    "__MASK__".into()
                ))
                .unwrap()
                .text
                .contains(selected)
            );
        }
        let source = r#"{"a":"one","b":"two"}"#;
        let start = source.find("one").unwrap() + 1;
        let end = source.find("two").unwrap() + 2;
        let policy = Policy {
            enabled: true,
            rules: vec![Rule {
                file: "**".into(),
                selector: format!("/@digest/{}/{start}/{end}", digest(source)),
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        let projection =
            project("a.json", source, &policy, &mut |_, _| Ok("__MASK__".into())).unwrap();
        assert!(projection.text.contains(r#"ne","b":"tw"#));
        assert!(!projection.text.contains("one"));
        assert!(!projection.text.contains("two"));
        let analyzed = analyze_for_policy("a.json", source, &policy).unwrap();
        assert!(
            analyzed
                .nodes
                .iter()
                .filter(
                    |node| action_for_node("a.json", source, &policy, &analyzed, node)
                        == Action::Allow
                )
                .count()
                >= 3
        );
    }

    #[test]
    fn invalid_utf8_or_opaque_exact_ranges_grant_nothing() {
        let source = "αβγ";
        for (start, end) in [(1, 4), (0, 99), (4, 2), (2, 2)] {
            let policy = Policy {
                enabled: true,
                rules: vec![Rule {
                    file: "**".into(),
                    selector: format!("/@digest/{}/{start}/{end}", digest(source)),
                    action: Action::Allow,
                }],
                ..Policy::default()
            };
            assert!(
                !project("a.txt", source, &policy, &mut |_, _| Ok("__MASK__".into()))
                    .unwrap()
                    .text
                    .contains('β')
            );
        }
        let source = "private unknown value";
        let policy = Policy {
            enabled: true,
            rules: vec![Rule {
                file: "**".into(),
                selector: format!("/@digest/{}/0/{}", digest(source), source.len()),
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        assert!(!project("a.unknown", source, &policy, &mut |_, _| Ok(
            "__MASK__".into()
        ))
        .unwrap()
        .text
        .contains("private"));
    }

    #[test]
    fn named_regions_reuse_marker_identity_keep_directives_local_and_allow_hide() {
        let source = "before-secret\nca-privacy:begin PUBLIC\npublished value\nca-privacy:end PUBLIC\nafter-secret\n";
        let document = analyze("a.txt", source).unwrap();
        let node = document
            .nodes
            .iter()
            .find(|node| &source[node.range.clone()] == "published value")
            .unwrap();
        let group = region_group_selector(node).unwrap();
        let policy = Policy {
            enabled: true,
            rules: vec![Rule {
                file: "**".into(),
                selector: group.clone(),
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        let updated = source.replace("published value", "changed public value");
        let out = project(
            "a.txt",
            &updated,
            &policy,
            &mut |_, _| Ok("__MASK__".into()),
        )
        .unwrap();
        assert!(out.text.contains("changed public value"));
        assert!(!out.text.contains("before-secret"));
        assert!(!out.text.contains("after-secret"));
        assert!(!out.text.contains("ca-privacy:"));
        assert!(!out.text.contains("PUBLIC"));
        assert_eq!(out.text.matches('\n').count(), source.matches('\n').count());
        let mut hide = policy.clone();
        hide.rules.insert(
            0,
            Rule {
                file: "**".into(),
                selector: "/**".into(),
                action: Action::Allow,
            },
        );
        hide.rules[1].action = Action::Hide;
        let out = project("a.txt", source, &hide, &mut |_, _| Ok("__MASK__".into())).unwrap();
        assert!(!out.text.contains("published value"));
        assert!(out.text.contains("before-secret"));
        let shape = node
            .selector
            .split("/@shape/")
            .nth(1)
            .unwrap()
            .split('/')
            .next()
            .unwrap();
        let whole = Policy {
            enabled: true,
            rules: vec![Rule {
                file: "**".into(),
                selector: format!("/**/@shape/{shape}/**"),
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        let out = project("a.txt", source, &whole, &mut |_, _| Ok("__MASK__".into())).unwrap();
        assert!(out.text.contains("published value"));
        assert!(out.text.contains("before-secret"));
    }

    #[test]
    fn unmatched_duplicate_crossing_or_string_literal_region_markers_grant_nothing() {
        let selector = format!("/@region/{}/**", digest("PUBLIC"));
        let policy = Policy {
            enabled: true,
            rules: vec![Rule {
                file: "**".into(),
                selector,
                action: Action::Allow,
            }],
            ..Policy::default()
        };
        for source in [
            "ca-privacy:begin PUBLIC\nprivate_value\n",
            "private_value\nca-privacy:end PUBLIC\n",
            "ca-privacy:begin PUBLIC\nprivate_value\nca-privacy:end PUBLIC\nca-privacy:begin PUBLIC\nother_value\nca-privacy:end PUBLIC\n",
            "ca-privacy:begin PUBLIC\nca-privacy:begin INNER\nprivate_value\nca-privacy:end PUBLIC\nca-privacy:end INNER\n",
        ] {
            let document = analyze("a.txt", source).unwrap();
            assert!(document.nodes.iter().all(|node| region_group_selector(node).is_none()));
            assert!(!project("a.txt", source, &policy, &mut |_, _| Ok("__MASK__".into())).unwrap().text.contains("private_value"));
        }
        let code = "const a = 'ca-privacy:begin PUBLIC'; const private_value = 'secret'; const b = 'ca-privacy:end PUBLIC';";
        assert!(analyze("a.js", code)
            .unwrap()
            .nodes
            .iter()
            .all(|node| region_group_selector(node).is_none()));
        let comment = "/* ca-privacy:begin PUBLIC\nprivate_value\nca-privacy:end PUBLIC */";
        assert!(
            project("a.c", comment, &policy, &mut |_, _| Ok("__MASK__".into()))
                .unwrap()
                .text
                .contains("private_value")
        );
    }
}
