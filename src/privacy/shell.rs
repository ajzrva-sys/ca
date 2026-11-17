//! Restore provider references only as literal shell arguments. Original
//! private bytes are never spliced into command syntax.
use super::store::{TOKEN_PREFIX, TOKEN_SUFFIX};
use std::cell::RefCell;
use std::ops::Range;
use tree_sitter::{Node, Parser};

const GUIDANCE: &str = "Private references in this shell expression cannot be restored safely; use native read_file, edit_file, apply_patch, or write_file tools.";
const MAX_COMMAND_BYTES: usize = 8 * 1024 * 1024;

thread_local! {
    static PARSER: RefCell<Option<Parser>> = const { RefCell::new(None) };
}

fn parse(command: &str) -> Result<tree_sitter::Tree, String> {
    PARSER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_bash::LANGUAGE.into())
                .map_err(|_| GUIDANCE.to_string())?;
            *slot = Some(parser);
        }
        slot.as_mut()
            .expect("parser initialized")
            .parse(command, None)
            .ok_or_else(|| GUIDANCE.to_string())
    })
}

fn tokens(text: &str) -> Result<Vec<Range<usize>>, String> {
    let mut found = Vec::new();
    let mut offset = 0;
    while let Some(start) = text[offset..].find(TOKEN_PREFIX) {
        let start = offset + start;
        let body = start + TOKEN_PREFIX.len();
        let end = body
            + text[body..]
                .find(TOKEN_SUFFIX)
                .ok_or_else(|| GUIDANCE.to_string())?;
        if end == body
            || !text[body..end]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err(GUIDANCE.into());
        }
        offset = end + TOKEN_SUFFIX.len();
        found.push(start..offset);
    }
    Ok(found)
}

/// Resolve only exact host-issued tokens. Commands without references retain
/// their existing behavior and do not instantiate a parser or call `resolve`.
pub fn restore(
    command: &str,
    mut resolve: impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String> {
    if !command.contains(TOKEN_PREFIX) {
        return Ok(command.to_string());
    }
    if command.len() > MAX_COMMAND_BYTES || command.contains('\0') {
        return Err(GUIDANCE.into());
    }
    let references = tokens(command)?;
    let tree = parse(command)?;
    if tree.root_node().has_error() {
        return Err(GUIDANCE.into());
    }
    let mut edits = Vec::<(Range<usize>, String)>::new();
    let mut stack = vec![(tree.root_node(), 0_usize)];
    let mut consumed = 0;
    while let Some((node, depth)) = stack.pop() {
        if depth > 128 {
            return Err(GUIDANCE.into());
        }
        let raw = &command[node.byte_range()];
        if !raw.contains(TOKEN_PREFIX) {
            continue;
        }
        if matches!(node.kind(), "word" | "raw_string" | "string") {
            if !is_literal_argument(node)
                || has_dynamic_ancestor(node)
                || is_evaluated_argument(node, command)
            {
                return Err(GUIDANCE.into());
            }
            let source_tokens = tokens(raw)?;
            let replacement = match node.kind() {
                "word" => {
                    if source_tokens.len() != 1 || source_tokens[0] != (0..raw.len()) {
                        return Err(GUIDANCE.into());
                    }
                    single_quote(&resolve_value(raw, &mut resolve)?)
                }
                "raw_string" => {
                    let inner = raw
                        .strip_prefix('\'')
                        .and_then(|value| value.strip_suffix('\''))
                        .ok_or_else(|| GUIDANCE.to_string())?;
                    single_quote(&restore_literal(inner, &mut resolve)?)
                }
                "string" => {
                    // Expansions and substitutions are named children rather
                    // than static string_content. Do not reinterpret them.
                    if node
                        .named_children(&mut node.walk())
                        .any(|child| child.kind() != "string_content")
                    {
                        return Err(GUIDANCE.into());
                    }
                    let inner = raw
                        .strip_prefix('"')
                        .and_then(|value| value.strip_suffix('"'))
                        .ok_or_else(|| GUIDANCE.to_string())?;
                    double_quote(&restore_literal(
                        &decode_double_quoted(inner),
                        &mut resolve,
                    )?)
                }
                _ => unreachable!(),
            };
            consumed += source_tokens.len();
            edits.push((node.byte_range(), replacement));
            continue;
        }
        for child in node.children(&mut node.walk()) {
            stack.push((child, depth + 1));
        }
    }
    if consumed != references.len() {
        return Err(GUIDANCE.into());
    }
    edits.sort_by_key(|(range, _)| range.start);
    let mut output = String::new();
    let mut offset = 0;
    for (range, replacement) in edits {
        if range.start < offset
            || output
                .len()
                .saturating_add(range.start - offset)
                .saturating_add(replacement.len())
                > MAX_COMMAND_BYTES
        {
            return Err(GUIDANCE.into());
        }
        output.push_str(&command[offset..range.start]);
        output.push_str(&replacement);
        offset = range.end;
    }
    if output.len().saturating_add(command.len() - offset) > MAX_COMMAND_BYTES {
        return Err(GUIDANCE.into());
    }
    output.push_str(&command[offset..]);
    // Quoting must preserve shell syntax even for embedded newlines/quotes.
    if parse(&output)?.root_node().has_error() {
        return Err(GUIDANCE.into());
    }
    Ok(output)
}

fn is_literal_argument(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    parent.kind() == "command"
        && parent
            .children_by_field_name("argument", &mut parent.walk())
            .any(|argument| argument.id() == node.id())
}

fn has_dynamic_ancestor(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "command_substitution"
                | "process_substitution"
                | "arithmetic_expansion"
                | "expansion"
                | "simple_expansion"
                | "concatenation"
        ) {
            return true;
        }
        node = parent;
    }
    false
}

fn is_evaluated_argument(node: Node<'_>, command: &str) -> bool {
    let Some(parent) = node.parent() else {
        return true;
    };
    let Some(name) = parent.child_by_field_name("name") else {
        return true;
    };
    let name = &command[name.byte_range()];
    // Quoted/escaped/dynamic executable spellings could disguise an evaluator
    // (for example `'eval'`). Their argv contract is not established here.
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte))
    {
        return true;
    }
    // Shell builtins that explicitly evaluate argument strings cannot promise
    // literal semantics. Wrappers and interpreters are likewise conservative.
    let executable = name.rsplit('/').next().unwrap_or(name);
    if matches!(
        executable,
        "eval" | "source" | "." | "command" | "builtin" | "env"
    ) {
        return true;
    }
    if matches!(
        executable,
        "sh" | "bash"
            | "zsh"
            | "ksh"
            | "dash"
            | "csh"
            | "tcsh"
            | "python"
            | "python3"
            | "node"
            | "ruby"
            | "perl"
            | "php"
            | "osascript"
    ) {
        return parent
            .children_by_field_name("argument", &mut parent.walk())
            .any(|arg| {
                let raw = &command[arg.byte_range()];
                let raw = raw
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
                    .or_else(|| {
                        raw.strip_prefix('"')
                            .and_then(|value| value.strip_suffix('"'))
                    })
                    .unwrap_or(raw);
                matches!(raw, "-c" | "-e" | "--eval")
                    || (raw.starts_with('-') && !raw.starts_with("--") && raw[1..].contains('c'))
            });
    }
    false
}

fn resolve_value(
    token: &str,
    resolve: &mut impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String> {
    let value = resolve(token)?;
    if value.len() > MAX_COMMAND_BYTES || value.contains('\0') {
        return Err(GUIDANCE.into());
    }
    Ok(value)
}

fn restore_literal(
    text: &str,
    resolve: &mut impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String> {
    let mut out = String::new();
    let mut offset = 0;
    for token in tokens(text)? {
        out.push_str(&text[offset..token.start]);
        out.push_str(&resolve_value(&text[token.clone()], resolve)?);
        offset = token.end;
        if out.len() > MAX_COMMAND_BYTES {
            return Err(GUIDANCE.into());
        }
    }
    out.push_str(&text[offset..]);
    Ok(out)
}

fn single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn double_quote(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        if matches!(ch, '\\' | '"' | '$' | '`') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

fn decode_double_quoted(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\n') => {}
            Some(next @ ('\\' | '"' | '$' | '`')) => out.push(next),
            Some(next) => {
                out.push('\\');
                out.push(next);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Filename references are complete path arguments, never executable syntax.
pub(crate) fn restore_filename_references(
    command: &str,
    mut resolve: impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String> {
    if !command.contains("__CA_PATH_") && !command.contains("__CA_NAME_") {
        return Ok(command.into());
    }
    transform_literals(command, true, |value| {
        if value.contains("__CA_PATH_") || value.contains("__CA_NAME_") {
            let (prefix, reference) = value
                .split_once('=')
                .filter(|(prefix, _)| prefix.starts_with('-'))
                .map_or(("", value), |(prefix, reference)| (prefix, reference));
            let resolved = resolve(reference)?;
            Ok(if prefix.is_empty() {
                resolved
            } else {
                format!("{prefix}={resolved}")
            })
        } else {
            Ok(value.into())
        }
    })
}

pub(crate) fn rebase_paths(
    command: &str,
    mut map: impl FnMut(&std::path::Path) -> Result<std::path::PathBuf, String>,
) -> Result<String, String> {
    transform_literals(command, false, |value| {
        let (prefix, path) = value
            .split_once('=')
            .filter(|(prefix, path)| prefix.starts_with('-') && path.starts_with('/'))
            .map_or(("", value), |(prefix, path)| (prefix, path));
        if !path.starts_with('/') {
            return Ok(value.into());
        }
        match map(std::path::Path::new(path)) {
            Ok(mapped) => Ok(if prefix.is_empty() {
                mapped.to_string_lossy().into_owned()
            } else {
                format!("{prefix}={}", mapped.display())
            }),
            Err(_) => Ok(value.into()), // system/read-only paths retain normal sandbox authority
        }
    })
}

fn literal(node: Node<'_>, command: &str) -> Option<String> {
    let raw = &command[node.byte_range()];
    match node.kind() {
        "raw_string" => Some(raw.strip_prefix('\'')?.strip_suffix('\'')?.into()),
        "string"
            if node
                .named_children(&mut node.walk())
                .all(|child| child.kind() == "string_content") =>
        {
            Some(decode_double_quoted(
                raw.strip_prefix('"')?.strip_suffix('"')?,
            ))
        }
        "word" if !raw.contains(['$', '`']) => {
            let mut result = String::new();
            let mut chars = raw.chars();
            while let Some(ch) = chars.next() {
                result.push(if ch == '\\' { chars.next()? } else { ch });
            }
            Some(result)
        }
        "concatenation" => {
            let mut result = String::new();
            for child in node.named_children(&mut node.walk()) {
                result.push_str(&literal(child, command)?);
            }
            Some(result)
        }
        _ => None,
    }
}

fn transform_literals(
    command: &str,
    require_all_references: bool,
    mut transform: impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String> {
    let tree = parse(command)?;
    if tree.root_node().has_error() {
        return Err(GUIDANCE.into());
    }
    let mut stack = vec![tree.root_node()];
    let mut edits = Vec::new();
    while let Some(node) = stack.pop() {
        let redirect = node.parent().is_some_and(|parent| {
            parent.kind() == "file_redirect"
                && parent
                    .child_by_field_name("destination")
                    .is_some_and(|target| target.id() == node.id())
        });
        if is_literal_argument(node) || redirect {
            let raw = &command[node.byte_range()];
            if let Some(value) = literal(node, command) {
                let contains = value.contains("__CA_PATH_") || value.contains("__CA_NAME_");
                if contains
                    && (has_dynamic_ancestor(node)
                        || (!redirect && is_evaluated_argument(node, command)))
                {
                    return Err(GUIDANCE.into());
                }
                let next = transform(&value)?;
                if next != value {
                    edits.push((node.byte_range(), single_quote(&next)));
                }
                continue;
            }
            if require_all_references && (raw.contains("__CA_PATH_") || raw.contains("__CA_NAME_"))
            {
                return Err(GUIDANCE.into());
            }
        }
        for child in node.children(&mut node.walk()) {
            stack.push(child);
        }
    }
    edits.sort_by_key(|(range, _)| range.start);
    let mut result = String::new();
    let mut end = 0;
    for (range, next) in edits {
        result.push_str(&command[end..range.start]);
        result.push_str(&next);
        end = range.end;
    }
    result.push_str(&command[end..]);
    if require_all_references && (result.contains("__CA_PATH_") || result.contains("__CA_NAME_")) {
        return Err(GUIDANCE.into());
    }
    if parse(&result)?.root_node().has_error() {
        return Err(GUIDANCE.into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    const TOKEN: &str = "__CA_PRIVATE_0123abcd_STRING_1__";
    fn value(token: &str) -> Result<String, String> {
        if token == TOKEN {
            Ok("private $HOME; `echo no` \"quoted\" 'single' \\ space\nnext".into())
        } else {
            Err("unknown private reference".into())
        }
    }
    #[test]
    fn no_reference_is_an_exact_fast_path() {
        let command = "intentionally (unfinished";
        assert_eq!(
            restore(command, |_| panic!("must not resolve")).unwrap(),
            command
        );
    }
    #[test]
    fn unquoted_alias_is_one_literal_argument() {
        let out = restore(&format!("printf '%s' {TOKEN}"), value).unwrap();
        assert_eq!(
            out,
            format!("printf '%s' {}", single_quote(&value(TOKEN).unwrap()))
        );
        assert!(out.contains("'\\''single'\\''"));
    }
    #[test]
    fn quoted_static_aliases_preserve_dollars_quotes_and_original_literal_escapes() {
        let out = restore(&format!("printf '%s' \"prefix \\\\{TOKEN} suffix\""), value).unwrap();
        let expected = format!("prefix \\{} suffix", value(TOKEN).unwrap());
        assert_eq!(out, format!("printf '%s' {}", double_quote(&expected)));
        assert!(out.contains("\\$HOME"));
        assert!(out.contains("\\`echo no\\`"));
        let out = restore(&format!("printf '%s' 'prefix {TOKEN} suffix'"), value).unwrap();
        assert_eq!(
            out,
            format!(
                "printf '%s' {}",
                single_quote(&format!("prefix {} suffix", value(TOKEN).unwrap()))
            )
        );
    }
    #[test]
    fn dynamic_mixed_command_and_evaluation_positions_are_rejected() {
        for command in [
            format!("{TOKEN} argument"),
            format!("printf prefix{TOKEN}"),
            format!("printf --path=\"{TOKEN}\""),
            format!("printf \"$HOME/{TOKEN}\""),
            format!("printf $(cat {TOKEN})"),
            format!("printf `cat {TOKEN}`"),
            format!("PRIVATE={TOKEN} printf hi"),
            format!("cat < {TOKEN}"),
            format!("eval {TOKEN}"),
            format!("'eval' {TOKEN}"),
            format!("sh -c {TOKEN}"),
            format!("sh '-c' {TOKEN}"),
            format!("env sh -c {TOKEN}"),
            format!("printf '\"{TOKEN}"),
        ] {
            assert!(restore(&command, value).is_err(), "{command}");
        }
    }
    #[test]
    fn unknown_incomplete_and_nul_references_are_rejected() {
        assert!(restore("cat __CA_PRIVATE_incomplete", value).is_err());
        assert!(restore("cat __CA_PRIVATE_0123abcd_STRING_999__", value).is_err());
        assert!(restore(&format!("cat {TOKEN}"), |_| Ok("bad\0value".into())).is_err());
    }
}
