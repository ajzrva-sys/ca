use super::super::policy::AliasKind;
use super::Builder;
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use tree_sitter::{Language, Node, Parser, TreeCursor};

thread_local! {
    // Parsers are created only for formats actually inspected on this worker.
    static PARSERS: RefCell<HashMap<String, Parser>> = RefCell::new(HashMap::new());
    static KIND_METADATA: RefCell<HashMap<String, Arc<[KindMetadata]>>> = RefCell::new(HashMap::new());
}

pub(super) fn format_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" | "mdown" => "markdown",
        "json" | "jsonc" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "csv" => "csv",
        "tsv" => "tsv",
        "html" | "htm" | "xhtml" | "svg" => "html",
        "css" => "css",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "rs" | "rust" => "rust",
        "py" | "pyi" | "python" => "python",
        "sh" | "bash" | "zsh" | "ksh" | "shell" => "bash",
        "js" | "mjs" | "cjs" | "jsx" | "javascript" => "javascript",
        "ts" | "mts" | "cts" | "typescript" => "typescript",
        "tsx" => "tsx",
        "txt" | "text" | "log" | "plaintext" => "text",
        _ if !path.rsplit('/').next().unwrap_or("").contains('.') => "text",
        _ => "opaque",
    }
}

fn language(format: &str) -> Option<Language> {
    Some(match format {
        "json" => tree_sitter_json::LANGUAGE.into(),
        "yaml" => tree_sitter_yaml::LANGUAGE.into(),
        "toml" => tree_sitter_toml_ng::LANGUAGE.into(),
        "html" => tree_sitter_html::LANGUAGE.into(),
        "css" => tree_sitter_css::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        "cpp" => tree_sitter_cpp::LANGUAGE.into(),
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "bash" => tree_sitter_bash::LANGUAGE.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => return None,
    })
}

pub(super) fn analyze(
    builder: &mut Builder<'_>,
    format: &str,
    text: &str,
    base: usize,
    prefix: &str,
) -> Result<(), String> {
    if format == "opaque" {
        builder.opaque(base..base + text.len(), prefix);
        return Ok(());
    }
    // Existing validators reject parser recovery being mistaken for valid data.
    let valid = data_valid(format, text);
    if !valid || text.len() > 8 * 1024 * 1024 {
        builder.opaque(base..base + text.len(), prefix);
        return Ok(());
    }
    let tree = parse(format, text)?;
    if tree.root_node().has_error() {
        builder.opaque(base..base + text.len(), prefix);
    } else {
        let mut cursor = tree.walk();
        let kinds = kind_metadata(format)?;
        let root_kind = kinds
            .get(cursor.node().kind_id() as usize)
            .ok_or("Unknown privacy grammar node kind")?;
        let mut selector = Selector::new(prefix, builder.details);
        visit(
            builder,
            &mut cursor,
            text,
            base,
            &mut selector,
            format,
            (0, false, root_kind, &kinds),
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpecialKind {
    Other,
    Attribute,
    RawText,
    BlockScalar,
    Primitive,
    TagName,
}

struct KindMetadata {
    name: &'static str,
    path: Box<str>,
    ordinal_group: u16,
    named: bool,
    comment: bool,
    string: bool,
    literal: Option<AliasKind>,
    leaf: AliasKind,
    key_parent: bool,
    special: SpecialKind,
}

fn kind_metadata(format: &str) -> Result<Arc<[KindMetadata]>, String> {
    KIND_METADATA.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(kinds) = cache.get(format) {
            return Ok(kinds.clone());
        }
        let language = language(format).ok_or("unsupported privacy syntax")?;
        let mut groups = HashMap::<&'static str, u16>::new();
        let mut kinds = Vec::with_capacity(language.node_kind_count());
        for index in 0..language.node_kind_count() {
            let id = u16::try_from(index).map_err(|_| "Privacy grammar has too many kinds")?;
            let name = language
                .node_kind_for_id(id)
                .ok_or("Privacy grammar kind is unavailable")?;
            let ordinal_group = *groups.entry(name).or_insert(id);
            kinds.push(KindMetadata {
                name,
                path: name
                    .chars()
                    .map(|character| {
                        if character.is_ascii_alphanumeric() || character == '_' {
                            character
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>()
                    .into_boxed_str(),
                ordinal_group,
                named: language.node_kind_is_named(id),
                comment: name.contains("comment"),
                string: is_string(name),
                literal: literal_kind(name),
                leaf: if name.contains("key") {
                    AliasKind::Key
                } else if name.contains("identifier") || name.contains("name") || name == "word" {
                    AliasKind::Identifier
                } else if name == "text" || name.contains("content") || name.contains("scalar") {
                    AliasKind::Text
                } else {
                    AliasKind::Opaque
                },
                key_parent: matches!(name, "pair" | "block_mapping_pair" | "flow_pair"),
                special: match name {
                    "attribute" => SpecialKind::Attribute,
                    "raw_text" => SpecialKind::RawText,
                    "block_scalar" => SpecialKind::BlockScalar,
                    "primitive_type" => SpecialKind::Primitive,
                    "tag_name" => SpecialKind::TagName,
                    _ => SpecialKind::Other,
                },
            });
        }
        let kinds: Arc<[KindMetadata]> = kinds.into();
        cache.insert(format.to_string(), kinds.clone());
        Ok(kinds)
    })
}

fn data_valid(format: &str, text: &str) -> bool {
    match format {
        "json" => serde_json::from_str::<serde_json::Value>(text).is_ok(),
        "yaml" => serde_yaml_ng::from_str::<serde_yaml_ng::Value>(text).is_ok(),
        "toml" => text.parse::<toml_edit::DocumentMut>().is_ok(),
        _ => true,
    }
}

fn parse(format: &str, text: &str) -> Result<tree_sitter::Tree, String> {
    PARSERS
        .with(|parsers| {
            let mut parsers = parsers.borrow_mut();
            if !parsers.contains_key(format) {
                let mut parser = Parser::new();
                parser
                    .set_language(&language(format).ok_or("unsupported privacy syntax")?)
                    .map_err(|_| "privacy grammar is incompatible")?;
                parsers.insert(format.to_string(), parser);
            }
            parsers
                .get_mut(format)
                .expect("parser inserted")
                .parse(text, None)
                .ok_or("privacy parser did not complete")
        })
        .map_err(str::to_string)
}

/// Count real ERROR/MISSING nodes instead of inferring validity from which
/// projection ranges happen to be opaque.
pub(super) fn error_count(format: &str, text: &str) -> Result<usize, String> {
    if text.len() > 8 * 1024 * 1024 {
        return Err("Restored source exceeds the 8 MiB parsing bound".into());
    }
    if !data_valid(format, text) {
        return Err("Restored structured data is invalid".into());
    }
    let tree = parse(format, text)?;
    let mut count = 0_usize;
    let mut nodes = vec![tree.root_node()];
    while let Some(node) = nodes.pop() {
        if node.is_error() || node.is_missing() {
            count = count.saturating_add(1);
        }
        if format == "html" && node.kind() == "raw_text" {
            let inner = &text[node.byte_range()];
            let nested = embedded_format(node, text);
            if let Some(nested) = nested {
                count = count.saturating_add(error_count(nested, inner)?);
            }
        }
        if format == "html" && node.kind() == "attribute" {
            let mut name = None;
            let mut value = None;
            for child in node.children(&mut node.walk()) {
                if child.kind() == "attribute_name" {
                    name = Some(&text[child.byte_range()]);
                }
                if matches!(child.kind(), "attribute_value" | "quoted_attribute_value") {
                    value = Some(&text[child.byte_range()]);
                }
            }
            if let (Some(name), Some(value)) = (name, value) {
                let name = name.to_ascii_lowercase();
                let inner = quote_inner(value).unwrap_or(0..value.len());
                let value = &value[inner];
                if name == "style" {
                    count = count.saturating_add(error_count(
                        "css",
                        &format!(".ca_private_fragment{{{value}}}"),
                    )?);
                } else if name.starts_with("on") {
                    count = count.saturating_add(error_count("javascript", value)?);
                }
            }
        }
        for child in node.children(&mut node.walk()) {
            nodes.push(child);
        }
    }
    Ok(count)
}

/// This private walk starts only after analyze rejects root.has_error(). In
/// pinned Tree-sitter, descendant ERROR costs accumulate into the root and
/// every MISSING leaf has a positive cost (node.c/subtree.c/subtree.h). The
/// immutable tree therefore needs no repeated descendant error/missing FFI.
fn visit(
    builder: &mut Builder<'_>,
    cursor: &mut TreeCursor<'_>,
    text: &str,
    base: usize,
    selector: &mut Selector,
    format: &str,
    ancestry: (usize, bool, &KindMetadata, &[KindMetadata]),
) -> Result<(), String> {
    let (depth, is_key, kind, kinds) = ancestry;
    let node = cursor.node();
    let range = node.byte_range();
    if range.is_empty() {
        return Ok(());
    }
    if depth > 128 || selector.len() > 16384 {
        builder.opaque(base + range.start..base + range.end, selector);
        return Ok(());
    }
    let raw = &text[range.clone()];
    if format == "html" && kind.special == SpecialKind::Attribute {
        let prefix = selector.parser_prefix();
        return attribute(builder, node, text, base, &prefix, depth);
    }
    if format == "html" && kind.special == SpecialKind::RawText {
        let language = embedded_format(node, text);
        if let Some(language) = language {
            let prefix = selector.parser_prefix();
            return analyze(builder, language, raw, base + range.start, &prefix);
        }
    }
    if matches!(format, "javascript" | "typescript" | "tsx") {
        if let Some(method) = selector_method(node, text) {
            return selector_argument(builder, node, raw, base, selector, method);
        }
    }
    if kind.comment {
        let (start, end) = comment_inner(raw);
        private_inner(
            builder,
            range,
            start..end,
            base,
            selector,
            AliasKind::Comment,
        );
        return Ok(());
    }
    if kind.special == SpecialKind::BlockScalar {
        // YAML's block-style header is syntax, while its indented body is data.
        let start = raw.find('\n').map_or(raw.len(), |offset| offset + 1);
        private_inner(
            builder,
            range,
            start..raw.len(),
            base,
            selector,
            AliasKind::String,
        );
        return Ok(());
    }
    if kind.string {
        let alias_kind = if is_key {
            AliasKind::Key
        } else {
            AliasKind::String
        };
        let inner = quote_inner(raw).unwrap_or(0..raw.len());
        private_inner(builder, range, inner, base, selector, alias_kind);
        return Ok(());
    }
    if let Some(kind) = kind.literal {
        builder.node(
            base + range.start..base + range.end,
            builder.retained_selector(selector),
            kind,
        );
        return Ok(());
    }
    if node.child_count() == 0 {
        if !kind.named || (kind.special == SpecialKind::Primitive && public_primitive(raw)) {
            // Anonymous grammar leaves are fixed delimiters or language words,
            // not arbitrary lexical identifiers. Data literals above override.
            builder.safe(base + range.start..base + range.end);
        } else if format == "html" && kind.special == SpecialKind::TagName && public_html_tag(raw) {
            builder.safe(base + range.start..base + range.end);
        } else {
            let alias_kind = if is_key { AliasKind::Key } else { kind.leaf };
            builder.node(
                base + range.start..base + range.end,
                builder.retained_selector(selector),
                alias_kind,
            );
        }
        return Ok(());
    }
    // Reuse one cursor and path for the whole depth-first walk. Allocating a
    // fresh cursor, ancestor path and map at every grammar node dominated cold
    // analysis on large sources. Only retained private leaves own selector text.
    let key = kind
        .key_parent
        .then(|| node.child_by_field_name("key"))
        .flatten()
        .map(|key| key.id());
    let mut counts = ChildOrdinals::default();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            let child_kind = kinds
                .get(child.kind_id() as usize)
                .ok_or("Unknown privacy grammar child kind")?;
            let index = counts.next(child_kind.ordinal_group);
            // A fixed grammar leaf contributes no private selector. Keep its
            // ordinal/coverage, without constructing a discarded path or
            // dispatching through the complete private-node classifier.
            if depth < 128
                && selector
                    .len()
                    .saturating_add(child_kind.name.len())
                    .saturating_add(22)
                    <= 16384
                && (!child_kind.named
                    || child_kind.special == SpecialKind::Primitive
                    || (format == "html" && child_kind.special == SpecialKind::TagName))
                && child.child_count() == 0
                && child_kind.literal.is_none()
                && !child_kind.string
                && !child_kind.comment
                && (!child_kind.named
                    || (child_kind.special == SpecialKind::Primitive
                        && public_primitive(&text[child.byte_range()]))
                    || (format == "html"
                        && child_kind.special == SpecialKind::TagName
                        && public_html_tag(&text[child.byte_range()])))
            {
                let range = child.byte_range();
                builder.safe(base + range.start..base + range.end);
                if !cursor.goto_next_sibling() {
                    break;
                }
                continue;
            }
            let parent_length = selector.push_child(child_kind, index);
            let result = visit(
                builder,
                cursor,
                text,
                base,
                selector,
                format,
                (depth + 1, key == Some(child.id()), child_kind, kinds),
            );
            selector.truncate(parent_length);
            result?;
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
    Ok(())
}

/// A rule-free runtime request needs only the exact path length for privacy
/// limits. The editor still materializes the identical grammar selector text.
struct Selector {
    text: Option<String>,
    length: usize,
}

impl Selector {
    fn new(prefix: &str, details: bool) -> Self {
        Self {
            text: details.then(|| prefix.to_string()),
            length: prefix.len(),
        }
    }

    fn len(&self) -> usize {
        self.length
    }

    fn push_child(&mut self, kind: &KindMetadata, index: usize) -> usize {
        let before = self.length;
        let kind_length = kind.path.len();
        let digits = if index == 0 {
            1
        } else {
            index.ilog10() as usize + 1
        };
        self.length = self
            .length
            .saturating_add(kind_length)
            .saturating_add(digits + 2);
        if let Some(text) = &mut self.text {
            text.push('/');
            text.push_str(&kind.path);
            text.push('/');
            append_ordinal(text, index);
        }
        before
    }

    fn truncate(&mut self, length: usize) {
        self.length = length;
        if let Some(text) = &mut self.text {
            text.truncate(length);
        }
    }

    fn parser_prefix(&self) -> Cow<'_, str> {
        match &self.text {
            Some(text) => Cow::Borrowed(text),
            // Embedded HTML/CSS/JS entrypoints retain their existing exact
            // prefix length and reset behavior. These bytes are never stored
            // or consulted as a selector in compact mode.
            None => Cow::Owned(" ".repeat(self.length)),
        }
    }
}

impl std::ops::Deref for Selector {
    type Target = str;
    fn deref(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }
}

fn literal_kind(kind: &str) -> Option<AliasKind> {
    match kind {
        "true" | "false" | "True" | "False" | "boolean" | "boolean_scalar" => {
            Some(AliasKind::Boolean)
        }
        "null" | "None" | "null_scalar" => Some(AliasKind::Null),
        "number" | "integer" | "float" | "integer_scalar" | "float_scalar" | "integer_literal"
        | "float_literal" | "number_literal" => Some(AliasKind::Number),
        _ => None,
    }
}

fn append_ordinal(output: &mut String, mut value: usize) {
    let mut digits = [b'0'; 20];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    output.push_str(std::str::from_utf8(&digits[start..]).expect("decimal digits are UTF-8"));
}

/// Grammar siblings normally use a handful of distinct kind names. Cached
/// numeric groups unify alias IDs with the same name, preserving the original
/// per-name ordinal without string comparison or a map allocation per parent.
#[derive(Default)]
struct ChildOrdinals {
    kinds: [(u16, usize); 8],
    len: usize,
    overflow: Vec<(u16, usize)>,
}
impl ChildOrdinals {
    fn next(&mut self, kind: u16) -> usize {
        for (seen, next) in &mut self.kinds[..self.len] {
            if *seen == kind {
                let index = *next;
                *next += 1;
                return index;
            }
        }
        if self.len < self.kinds.len() {
            self.kinds[self.len] = (kind, 1);
            self.len += 1;
            return 0;
        }
        for (seen, next) in &mut self.overflow {
            if *seen == kind {
                let index = *next;
                *next += 1;
                return index;
            }
        }
        self.overflow.push((kind, 1));
        0
    }
}

/// HTML raw-text content is interpreted only when its static MIME declaration
/// establishes a grammar. Templates and other data scripts remain opaque.
fn embedded_format(node: Node<'_>, text: &str) -> Option<&'static str> {
    let parent = node.parent()?;
    if parent.kind() == "style_element" {
        return Some("css");
    }
    if parent.kind() != "script_element" {
        return None;
    }
    let mut script_type = None;
    for tag in parent
        .children(&mut parent.walk())
        .filter(|child| child.kind() == "start_tag")
    {
        for attribute in tag
            .children(&mut tag.walk())
            .filter(|child| child.kind() == "attribute")
        {
            let mut name = None;
            let mut value = None;
            for child in attribute.children(&mut attribute.walk()) {
                if child.kind() == "attribute_name" {
                    name = Some(&text[child.byte_range()]);
                }
                if matches!(child.kind(), "attribute_value" | "quoted_attribute_value") {
                    let raw = &text[child.byte_range()];
                    value = Some(&raw[quote_inner(raw).unwrap_or(0..raw.len())]);
                }
            }
            if name.is_some_and(|name| name.eq_ignore_ascii_case("type")) {
                // Duplicate declarations do not establish one unambiguous type.
                if script_type.is_some() {
                    return None;
                }
                script_type = Some(value.unwrap_or("").trim().to_ascii_lowercase());
            }
        }
    }
    match script_type.as_deref().unwrap_or("") {
        "" | "module" | "text/javascript" | "application/javascript" => Some("javascript"),
        "application/json" | "application/ld+json" => Some("json"),
        _ => None,
    }
}

/// Automatic page-data policy, independent of editor selector allocation.
/// Ranges select existing reversible source nodes; syntax is never rewritten.
pub(super) fn web_data_ranges(format: &str, text: &str) -> Result<Vec<Range<usize>>, String> {
    fn collect(
        format: &str,
        text: &str,
        base: usize,
        ranges: &mut Vec<Range<usize>>,
    ) -> Result<(), String> {
        if text.len() > 8 * 1024 * 1024 {
            ranges.push(base..base + text.len());
            return Ok(());
        }
        let tree = parse(format, text)?;
        if tree.root_node().has_error() {
            ranges.push(base..base + text.len());
        } else {
            visit(tree.root_node(), format, text, base, 0, ranges)?;
        }
        Ok(())
    }
    fn visit(
        node: Node<'_>,
        format: &str,
        text: &str,
        base: usize,
        depth: usize,
        ranges: &mut Vec<Range<usize>>,
    ) -> Result<(), String> {
        let kind = node.kind();
        let range = node.byte_range();
        if depth > 128
            || kind.contains("comment")
            || (format == "html" && matches!(kind, "text" | "entity"))
            || matches!(kind, "jsx_text" | "html_character_reference")
        {
            ranges.push(base + range.start..base + range.end);
            return Ok(());
        }
        if kind == "jsx_attribute" {
            let name = node.named_children(&mut node.walk()).next();
            if name.is_some_and(|name| {
                matches!(
                    &text[name.byte_range()],
                    "className" | "class" | "id" | "role" | "type"
                )
            }) && node
                .named_children(&mut node.walk())
                .any(|child| child.kind() == "string")
            {
                return Ok(());
            }
        }
        if format == "html" && kind == "raw_text" {
            if let Some(language) = embedded_format(node, text) {
                collect(language, &text[range.clone()], base + range.start, ranges)?;
            } else {
                ranges.push(base + range.start..base + range.end);
            }
            return Ok(());
        }
        if format == "html" && kind == "attribute" {
            let mut name = "";
            let mut value = None;
            for child in node.children(&mut node.walk()) {
                if child.kind() == "attribute_name" {
                    name = &text[child.byte_range()];
                } else if matches!(child.kind(), "attribute_value" | "quoted_attribute_value") {
                    value = Some(child);
                }
            }
            if let Some(value) = value {
                let raw = &text[value.byte_range()];
                let inner = quote_inner(raw).unwrap_or(0..raw.len());
                let start = base + value.start_byte() + inner.start;
                let name = name.to_ascii_lowercase();
                if name == "style" {
                    // Parse declarations in their existing synthetic rule context.
                    let wrapped = format!("a{{{}}}", &raw[inner.clone()]);
                    let mut fragments = Vec::new();
                    collect("css", &wrapped, 0, &mut fragments)?;
                    for range in fragments {
                        if range.start >= 2 && range.end < wrapped.len() {
                            ranges.push(start + range.start - 2..start + range.end - 2);
                        } else {
                            ranges.push(start..start + inner.len());
                        }
                    }
                } else if name.starts_with("on") {
                    collect("javascript", &raw[inner], start, ranges)?;
                } else if !matches!(
                    name.as_str(),
                    "id" | "class"
                        | "type"
                        | "name"
                        | "role"
                        | "rel"
                        | "target"
                        | "tabindex"
                        | "disabled"
                        | "checked"
                        | "selected"
                        | "width"
                        | "height"
                        | "for"
                        | "method"
                        | "charset"
                        | "aria-hidden"
                        | "aria-expanded"
                        | "aria-controls"
                        | "aria-labelledby"
                        | "aria-describedby"
                        | "colspan"
                        | "rowspan"
                        | "media"
                        | "integrity"
                        | "crossorigin"
                        | "loading"
                        | "decoding"
                ) {
                    ranges.push(start..start + inner.len());
                }
            }
            return Ok(());
        }
        if format == "css" && kind == "declaration" {
            let raw = &text[range.clone()];
            let property = raw.split(':').next().unwrap_or("").trim();
            if property.eq_ignore_ascii_case("content") {
                // Keep property names/delimiters; select only existing data nodes.
                if let Some(colon) = raw.find(':') {
                    ranges.push(base + range.start + colon + 1..base + range.end);
                }
                return Ok(());
            }
        }
        if format == "css" && kind == "call_expression" {
            let raw = &text[range.clone()];
            if raw
                .split('(')
                .next()
                .is_some_and(|name| name.trim().eq_ignore_ascii_case("url"))
            {
                if let Some(open) = raw.find('(') {
                    ranges.push(base + range.start + open + 1..base + range.end - 1);
                }
                return Ok(());
            }
        }
        if format == "css"
            && kind == "string_value"
            && node
                .parent()
                .is_some_and(|parent| parent.kind() == "import_statement")
        {
            ranges.push(base + range.start..base + range.end);
            return Ok(());
        }
        if matches!(format, "javascript" | "typescript" | "tsx" | "json") {
            // Preserve object keys and DOM selectors, but withhold scalar data.
            let key = node
                .parent()
                .and_then(|parent| parent.child_by_field_name("key"));
            let is_key = key.is_some_and(|key| key.id() == node.id());
            let static_selector = selector_method(node, text).is_some()
                && matches!(kind, "string" | "template_string")
                && !text[range.clone()].contains('\\')
                && !node
                    .named_children(&mut node.walk())
                    .any(|child| child.kind() == "template_substitution");
            if static_selector || is_key {
                return Ok(());
            }
            if selector_method(node, text).is_some() {
                // The structural analyzer represents a dynamic selector as one
                // opaque node. Select that complete argument, never just its
                // nested literal ranges.
                ranges.push(base + range.start..base + range.end);
                return Ok(());
            }
            if matches!(
                kind,
                "string" | "template_string" | "number" | "true" | "false" | "null"
            ) {
                ranges.push(base + range.start..base + range.end);
                return Ok(());
            }
        }
        for child in node.named_children(&mut node.walk()) {
            visit(child, format, text, base, depth + 1, ranges)?;
        }
        Ok(())
    }
    let mut ranges = Vec::new();
    collect(format, text, 0, &mut ranges)?;
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut().filter(|last| range.start <= last.end) {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    Ok(merged)
}

fn public_primitive(raw: &str) -> bool {
    matches!(
        raw,
        "void"
            | "char"
            | "short"
            | "int"
            | "long"
            | "float"
            | "double"
            | "signed"
            | "unsigned"
            | "bool"
            | "boolean"
            | "str"
            | "string"
            | "number"
            | "unknown"
            | "never"
            | "any"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
    )
}

fn selector_method<'a>(node: Node<'_>, text: &'a str) -> Option<&'a str> {
    let arguments = node.parent()?;
    if arguments.kind() != "arguments" || arguments.named_child(0)?.id() != node.id() {
        return None;
    }
    let call = arguments.parent()?;
    if call.kind() != "call_expression" {
        return None;
    }
    let function = call.child_by_field_name("function")?;
    let name = if function.kind() == "member_expression" {
        function.child_by_field_name("property")?
    } else {
        function
    };
    let name = &text[name.byte_range()];
    matches!(
        name,
        "getElementById" | "querySelector" | "querySelectorAll" | "matches" | "closest"
    )
    .then_some(name)
}

fn selector_argument(
    builder: &mut Builder<'_>,
    node: Node<'_>,
    raw: &str,
    base: usize,
    selector: &str,
    method: &str,
) -> Result<(), String> {
    let range = node.byte_range();
    // Escapes require decoded-to-source mapping. Unknown/dynamic forms remain
    // one exact opaque source range rather than guessing an identifier.
    let static_literal = matches!(node.kind(), "string" | "template_string")
        && !raw.contains('\\')
        && !node
            .named_children(&mut node.walk())
            .any(|child| child.kind() == "template_substitution");
    if !static_literal {
        builder.opaque(base + range.start..base + range.end, selector);
        return Ok(());
    }
    let Some(inner) = quote_inner(raw) else {
        builder.opaque(base + range.start..base + range.end, selector);
        return Ok(());
    };
    let value = &raw[inner.clone()];
    if method == "getElementById" {
        private_inner(
            builder,
            range,
            inner,
            base,
            &format!("{selector}/id"),
            AliasKind::Identifier,
        );
        return Ok(());
    }
    let Some(parts) = selector_components(value)? else {
        private_inner(builder, range, inner, base, selector, AliasKind::Opaque);
        return Ok(());
    };
    builder.safe(base + range.start..base + range.start + inner.start);
    builder.safe(base + range.start + inner.end..base + range.end);
    let mut offset = 0;
    for (index, part) in parts.iter().enumerate() {
        let start = base + range.start + inner.start;
        builder.safe(start + offset..start + part.start);
        builder.node(
            start + part.start..start + part.end,
            format!("{selector}/component/{index}"),
            AliasKind::Identifier,
        );
        offset = part.end;
    }
    builder.safe(base + range.start + inner.start + offset..base + range.start + inner.end);
    Ok(())
}

fn selector_components(value: &str) -> Result<Option<Vec<Range<usize>>>, String> {
    if value.is_empty() {
        return Ok(Some(Vec::new()));
    }
    if parse("css", &format!("{value}{{}}"))?
        .root_node()
        .has_error()
    {
        return Ok(None);
    }
    let mut parts = Vec::new();
    let mut at = 0;
    while at < value.len() {
        let ch = value[at..].chars().next().expect("character exists");
        if ch.is_ascii_whitespace() || matches!(ch, '>' | '+' | '~' | ',' | '*') {
            at += ch.len_utf8();
            continue;
        }
        if matches!(ch, '.' | '#') {
            at += 1;
            let start = at;
            while at < value.len() {
                let ch = value[at..].chars().next().expect("character exists");
                if !(ch.is_alphanumeric() || matches!(ch, '-' | '_')) {
                    break;
                }
                at += ch.len_utf8();
            }
            if at == start {
                return Ok(None);
            }
            parts.push(start..at);
            continue;
        }
        if ch.is_ascii_alphabetic() {
            let start = at;
            while at < value.len() && value.as_bytes()[at].is_ascii_alphanumeric() {
                at += 1;
            }
            if public_html_tag(&value[start..at]) {
                continue;
            }
        }
        return Ok(None);
    }
    Ok(Some(parts))
}

fn is_string(kind: &str) -> bool {
    matches!(
        kind,
        "string"
            | "string_literal"
            | "raw_string_literal"
            | "char_literal"
            | "character_literal"
            | "raw_string"
            | "system_lib_string"
            | "string_content"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "quoted_key"
            | "block_scalar"
            | "timestamp_scalar"
    )
}

fn quote_inner(raw: &str) -> Option<Range<usize>> {
    let start = raw.find(['\'', '"', '`'])?;
    // A quote in an unquoted YAML scalar is data, not a delimiter.
    if start > 8
        || !raw[..start]
            .chars()
            .all(|ch| ch.is_ascii_alphabetic() || ch == '#')
    {
        return None;
    }
    let quote = raw.as_bytes()[start];
    let triple = raw
        .as_bytes()
        .get(start..start + 3)
        .is_some_and(|bytes| bytes.iter().all(|byte| *byte == quote));
    let width = if triple { 3 } else { 1 };
    let end = raw.rfind(char::from(quote))? + 1 - width;
    (end >= start + width).then_some(start + width..end)
}

fn comment_inner(raw: &str) -> (usize, usize) {
    if raw.starts_with("<!--") && raw.ends_with("-->") {
        (4, raw.len() - 3)
    } else if raw.starts_with("/*") && raw.ends_with("*/") {
        (2, raw.len() - 2)
    } else if raw.starts_with("//") {
        (2, raw.trim_end_matches(['\n', '\r']).len())
    } else if raw.starts_with('#') {
        (1, raw.trim_end_matches(['\n', '\r']).len())
    } else {
        (0, raw.len())
    }
}

fn private_inner(
    builder: &mut Builder<'_>,
    outer: Range<usize>,
    inner: Range<usize>,
    base: usize,
    selector: &str,
    kind: AliasKind,
) {
    builder.safe(base + outer.start..base + outer.start + inner.start);
    builder.node(
        base + outer.start + inner.start..base + outer.start + inner.end,
        builder.retained_selector(selector),
        kind,
    );
    builder.safe(base + outer.start + inner.end..base + outer.end);
}

fn public_html_tag(raw: &str) -> bool {
    matches!(
        raw.to_ascii_lowercase().as_str(),
        "html"
            | "head"
            | "body"
            | "title"
            | "meta"
            | "link"
            | "script"
            | "style"
            | "main"
            | "header"
            | "footer"
            | "nav"
            | "section"
            | "article"
            | "aside"
            | "div"
            | "span"
            | "p"
            | "a"
            | "img"
            | "picture"
            | "source"
            | "svg"
            | "path"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "th"
            | "td"
            | "form"
            | "label"
            | "input"
            | "button"
            | "select"
            | "option"
            | "textarea"
            | "fieldset"
            | "legend"
            | "br"
            | "hr"
            | "strong"
            | "em"
            | "b"
            | "i"
            | "code"
            | "pre"
            | "blockquote"
            | "details"
            | "summary"
            | "video"
            | "audio"
            | "canvas"
            | "noscript"
            | "template"
            | "slot"
    )
}

fn attribute(
    builder: &mut Builder<'_>,
    node: Node<'_>,
    text: &str,
    base: usize,
    selector: &str,
    depth: usize,
) -> Result<(), String> {
    let mut name = None;
    let mut value = None;
    for child in node.children(&mut node.walk()) {
        if child.kind() == "attribute_name" {
            name = Some(child);
        }
        if matches!(child.kind(), "attribute_value" | "quoted_attribute_value") {
            value = Some(child);
        }
    }
    let name_text = name.map(|name| &text[name.byte_range()]).unwrap_or("");
    // Standard names are structural syntax; custom/data attribute names remain private.
    if let Some(name) = name {
        if matches!(
            name_text.to_ascii_lowercase().as_str(),
            "id" | "class"
                | "href"
                | "src"
                | "alt"
                | "title"
                | "type"
                | "name"
                | "value"
                | "style"
                | "onclick"
                | "onload"
                | "onchange"
                | "oninput"
                | "onsubmit"
                | "onerror"
                | "width"
                | "height"
                | "role"
                | "tabindex"
                | "disabled"
                | "checked"
                | "selected"
                | "placeholder"
                | "rel"
                | "target"
                | "content"
                | "charset"
                | "for"
                | "method"
                | "action"
        ) {
            builder.safe(base + name.start_byte()..base + name.end_byte());
        } else {
            builder.node(
                base + name.start_byte()..base + name.end_byte(),
                format!("{selector}/name"),
                AliasKind::Key,
            );
        }
    }
    if let Some(value) = value {
        let raw = &text[value.byte_range()];
        let inner = quote_inner(raw).unwrap_or(0..raw.len());
        let inner_base = base + value.start_byte() + inner.start;
        builder.safe(base + value.start_byte()..inner_base);
        builder.safe(base + value.start_byte() + inner.end..base + value.end_byte());
        let lower = name_text.to_ascii_lowercase();
        if lower == "class" {
            let value = &raw[inner.clone()];
            let mut at = 0;
            let mut index = 0;
            while at < value.len() {
                if value.as_bytes()[at].is_ascii_whitespace() {
                    at += 1;
                    continue;
                }
                let start = at;
                while at < value.len() && !value.as_bytes()[at].is_ascii_whitespace() {
                    at += 1;
                }
                builder.node(
                    inner_base + start..inner_base + at,
                    format!("{selector}/class/{index}"),
                    AliasKind::Identifier,
                );
                index += 1;
            }
        } else if lower.starts_with("on") || lower == "style" {
            if lower == "style" {
                css_declarations(builder, &raw[inner], inner_base, &format!("{selector}/css"))?;
            } else {
                analyze(
                    builder,
                    "javascript",
                    &raw[inner],
                    inner_base,
                    &format!("{selector}/javascript"),
                )?;
            }
        } else {
            builder.node(
                inner_base..base + value.start_byte() + inner.end,
                format!("{selector}/value"),
                AliasKind::String,
            );
        }
    }
    // Equals and quotes are grammar-owned; everything else is checked by finish.
    for child in node.children(&mut node.walk()) {
        if !child.is_named() {
            builder.safe(base + child.start_byte()..base + child.end_byte());
        }
    }
    let _ = depth;
    Ok(())
}

/// CSS declarations need a rule context. Synthetic bytes never enter source
/// ranges, selectors, aliases or the public view.
fn css_declarations(
    builder: &mut Builder<'_>,
    text: &str,
    base: usize,
    selector: &str,
) -> Result<(), String> {
    const PREFIX: &str = ".ca_private_fragment{";
    let wrapped = format!("{PREFIX}{text}}}");
    let source = PREFIX.len()..PREFIX.len() + text.len();
    let mut fragment = Builder::new(&wrapped);
    fragment.details = builder.details;
    analyze(&mut fragment, "css", &wrapped, 0, selector)?;
    // Parser recovery or a cross-boundary token must hide the whole real
    // fragment. Publish no partial analysis before this check completes.
    if fragment.nodes.iter().any(|node| {
        node.range.start < source.end
            && node.range.end > source.start
            && (node.range.start < source.start || node.range.end > source.end)
    }) {
        builder.opaque(base..base + text.len(), selector);
        return Ok(());
    }
    for node in fragment.nodes {
        if node.range.start >= source.start && node.range.end <= source.end {
            builder.node(
                base + node.range.start - source.start..base + node.range.end - source.start,
                node.selector,
                node.kind,
            );
        }
    }
    for range in fragment.safe {
        let start = range.start.max(source.start);
        let end = range.end.min(source.end);
        if start < end {
            builder.safe(base + start - source.start..base + end - source.start);
        }
    }
    Ok(())
}

#[cfg(test)]
mod traversal_tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn privacy_stack_ordinals_match_exact_kind_names_including_spill() {
        let kinds = [
            "identifier",
            "string",
            "number",
            "{",
            "}",
            "(",
            ")",
            ";",
            "comment",
            "quoted_key",
            "type_identifier",
            "+",
            "identifier",
            "comment",
            "{",
            "quoted_key",
        ];
        let mut compact = ChildOrdinals::default();
        let mut legacy = BTreeMap::<&str, usize>::new();
        let mut groups = BTreeMap::<&str, u16>::new();
        for kind in kinds.into_iter().cycle().take(100) {
            let next_group = groups.len() as u16;
            let group = *groups.entry(kind).or_insert(next_group);
            let expected = legacy.entry(kind).or_default();
            assert_eq!(compact.next(group), *expected, "{kind}");
            *expected += 1;
        }
    }

    #[test]
    fn privacy_cached_grammar_kind_ids_preserve_alias_name_grouping() {
        for format in [
            "rust",
            "c",
            "cpp",
            "python",
            "bash",
            "javascript",
            "typescript",
            "tsx",
            "json",
            "yaml",
            "toml",
            "html",
            "css",
        ] {
            let language = language(format).unwrap();
            let kinds = kind_metadata(format).unwrap();
            let mut groups = BTreeMap::<&str, u16>::new();
            for (id, kind) in kinds.iter().enumerate() {
                assert_eq!(Some(kind.name), language.node_kind_for_id(id as u16));
                assert_eq!(kind.named, language.node_kind_is_named(id as u16));
                assert_eq!(
                    *groups.entry(kind.name).or_insert(kind.ordinal_group),
                    kind.ordinal_group,
                    "{format}: alias IDs with the same name must share one ordinal group"
                );
            }
            let mut compact = ChildOrdinals::default();
            let mut legacy = BTreeMap::<&str, usize>::new();
            for kind in kinds.iter().cycle().take(kinds.len() * 3) {
                let expected = legacy.entry(kind.name).or_default();
                assert_eq!(
                    compact.next(kind.ordinal_group),
                    *expected,
                    "{format}: {}",
                    kind.name
                );
                *expected += 1;
            }
        }
    }

    #[test]
    fn privacy_root_error_guard_covers_missing_and_error_descendants() {
        let mut saw_missing = false;
        let mut saw_error = false;
        for (format, source) in [
            ("rust", "fn private_fn() { let private_value = 1;"),
            ("json", "{\"private_key\": 1"),
            (
                "javascript",
                "function private_fn() { return 'private_value';",
            ),
            ("c", "int private_fn(void) { return 1;"),
            ("css", ".private_class { color: red;"),
            ("rust", "\"unterminated private_value"),
            ("json", "\"unterminated private_value"),
            ("javascript", "\"unterminated private_value"),
        ] {
            let tree = parse(format, source).unwrap();
            let mut pending = vec![tree.root_node()];
            let mut recovered = false;
            while let Some(node) = pending.pop() {
                if node.is_missing() {
                    saw_missing = true;
                    recovered = true;
                }
                if node.is_error() {
                    saw_error = true;
                    recovered = true;
                }
                pending.extend(node.children(&mut node.walk()));
            }
            assert!(
                recovered,
                "{format}: fixture must exercise actual parser recovery"
            );
            assert!(
                tree.root_node().has_error(),
                "{format}: every recovered tree must be rejected before traversal"
            );
            for details in [false, true] {
                let mut builder = Builder::new(source);
                builder.details = details;
                analyze(&mut builder, format, source, 0, "/fixture").unwrap();
                assert_eq!(builder.nodes.len(), 1, "{format}");
                assert_eq!(builder.nodes[0].range, 0..source.len(), "{format}");
                assert_eq!(builder.nodes[0].kind, AliasKind::Opaque, "{format}");
            }
        }
        assert!(saw_missing, "fixtures must include MISSING recovery");
        assert!(saw_error, "fixtures must include ERROR recovery");
    }

    #[test]
    fn privacy_shared_cursor_keeps_legacy_selectors_ranges_and_key_identity() {
        fn original_paths(
            node: Node<'_>,
            selector: String,
            paths: &mut BTreeMap<String, (Range<usize>, bool, String)>,
        ) {
            let key = node.parent().is_some_and(|parent| {
                matches!(parent.kind(), "pair" | "block_mapping_pair" | "flow_pair")
                    && parent
                        .child_by_field_name("key")
                        .is_some_and(|key| key.id() == node.id())
            });
            paths.insert(
                selector.clone(),
                (node.byte_range(), key, node.kind().into()),
            );
            let mut ordinals = BTreeMap::<&str, usize>::new();
            for child in node.children(&mut node.walk()) {
                let index = ordinals.entry(child.kind()).or_default();
                let kind = child
                    .kind()
                    .chars()
                    .map(|ch| {
                        if ch.is_ascii_alphanumeric() || ch == '_' {
                            ch
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>();
                let child_selector = format!("{selector}/{kind}/{index}");
                *index += 1;
                original_paths(child, child_selector, paths);
            }
        }
        for (format, source, private_values) in [
            (
                "rust",
                "fn private_fn(private_arg: &str) { let private_local = \"private_value\"; let private_number:i64 = 12345; let private_boolean = true; }",
                vec![
                    "private_fn",
                    "private_arg",
                    "private_local",
                    "private_value",
                    "private_number",
                    "12345",
                    "private_boolean",
                    "true",
                ],
            ),
            (
                "json",
                r#"{"private_key":"private_value","other_key":[13,"other_value"]}"#,
                vec![
                    "private_key",
                    "private_value",
                    "other_key",
                    "13",
                    "other_value",
                ],
            ),
            (
                "python",
                "def private_fn(private_arg):\n    return 'private_value'\n",
                vec!["private_fn", "private_arg", "private_value"],
            ),
            (
                "javascript",
                "function private_fn(private_arg) { let private_number = 12345; let private_boolean = true; let private_null = null; return 'private_value'; }",
                vec!["private_fn", "private_arg", "private_value", "private_number", "12345", "private_boolean", "true", "private_null", "null"],
            ),
            (
                "bash",
                "private_name='private_value'\nprintf '%s' \"$private_name\"\n",
                vec!["private_name", "private_value"],
            ),
        ] {
            let tree = parse(format, source).unwrap();
            assert!(!tree.root_node().has_error(), "{format}");
            let kinds = kind_metadata(format).unwrap();
            let mut pending = vec![tree.root_node()];
            while let Some(node) = pending.pop() {
                assert!(!node.is_error() && !node.is_missing(), "{format}: the root guard must cover every descendant");
                let cached = &kinds[node.kind_id() as usize];
                assert_eq!(cached.name, node.kind(), "{format}: cached alias kind name");
                assert_eq!(cached.named, node.is_named(), "{format}: cached alias named flag");
                pending.extend(node.children(&mut node.walk()));
            }
            let prefix = format!("/{format}");
            let mut expected = BTreeMap::new();
            original_paths(tree.root_node(), prefix.clone(), &mut expected);
            let mut builder = Builder::new(source);
            analyze(&mut builder, format, source, 0, &prefix).unwrap();
            for node in &builder.nodes {
                let (range, old_key, old_kind) = expected
                    .get(&node.selector)
                    .unwrap_or_else(|| panic!("{format}: changed selector {}", node.selector));
                assert!(range.start <= node.range.start && node.range.end <= range.end);
                assert_eq!(
                    node.kind == AliasKind::Key,
                    *old_key || old_kind.contains("key")
                );
            }
            for value in private_values {
                assert!(
                    builder
                        .nodes
                        .iter()
                        .any(|node| source[node.range.clone()].contains(value)),
                    "{format}: lost private value {value}"
                );
            }
        }
    }

    #[test]
    fn privacy_public_grammar_fast_path_preserves_depth_fallback_and_literal_hiding() {
        let source = format!(
            "fn private_fn() {{ {} true; {} }}",
            "{".repeat(130),
            "}".repeat(130)
        );
        let document = super::super::analyze("nested.rs", &source).unwrap();
        assert!(document
            .nodes
            .iter()
            .any(|node| node.kind == AliasKind::Opaque));
        let policy = super::super::super::policy::Policy {
            enabled: true,
            ..Default::default()
        };
        for (path, source) in [
            ("a.json", r#"{"private_key":[true,false,null,12345]}"#),
            ("a.js", "const private_value = [true, false, null, 12345];"),
            ("a.rs", "fn private_fn() { let private_boolean:bool = true; let private_number:i64 = 12345; }"),
        ] {
            let projected = super::super::project(path, source, &policy, &mut |_, _| Ok("__MASK__".into())).unwrap();
            for private in ["private_", "true", "false", "null", "12345"] {
                assert!(!projected.text.contains(private), "{path}: {}", projected.text);
            }
            assert!(projected.text.contains(';') || projected.text.contains('['));
        }
    }

    #[test]
    fn privacy_compact_selector_lengths_keep_exact_caps_and_embedded_fallbacks() {
        let repeated = "fn private_fn() { let private_value = true; }\n".repeat(32);
        let nested = format!(
            "fn private_fn() {{ {} true; {} }}",
            "{".repeat(130),
            "}".repeat(130)
        );
        for (format, source) in [
            ("rust", repeated.as_str()),
            ("rust", nested.as_str()),
            ("html", "<div style=\"color: red\" onclick=\"private_fn('value')\">private text</div><script>let private_name = 'value';</script>"),
            ("javascript", "document.querySelector('#private-id .private-class');"),
        ] {
            for prefix_length in [0, 16, 16300, 16364, 16380, 16384, 16385] {
                let prefix = "x".repeat(prefix_length);
                let build = |details| {
                    let mut builder = Builder::new(source);
                    builder.details = details;
                    analyze(&mut builder, format, source, 0, &prefix).unwrap();
                    let mut document = builder.finish(format).unwrap();
                    for node in &mut document.nodes { node.selector.clear(); node.label.clear(); }
                    document
                };
                assert_eq!(build(false), build(true), "{format} prefix={prefix_length}: compact paths must preserve every literal range/kind and exact limit fallback");
            }
        }
    }
}
