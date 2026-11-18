//! Deterministic runtime spellings of selected quoted literals. Source aliases
//! still restore the original source bytes through their existing edit views.
#![allow(dead_code)]
use super::policy::AliasKind;
use super::projection::Change;

pub(super) fn decoded(path: &str, source: &str, change: &Change) -> Option<String> {
    if !matches!(change.kind, AliasKind::String | AliasKind::Key) || !change.original.contains('\\')
    {
        return None;
    }
    let format = super::projection::source_format(path);
    if !matches!(
        format,
        "python"
            | "rust"
            | "c"
            | "cpp"
            | "javascript"
            | "typescript"
            | "tsx"
            | "json"
            | "toml"
            | "yaml"
    ) {
        return None;
    }
    let before = source.get(..change.range.start)?;
    let quote = before.chars().next_back()?;
    if !matches!(quote, '\'' | '"' | '`') || !source.get(change.range.end..)?.starts_with(quote) {
        return None;
    }
    let quotes = if before.ends_with(&quote.to_string().repeat(3)) {
        3
    } else {
        1
    };
    let prefix = &before[..before.len() - quotes];
    let prefix: String = prefix
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphabetic() || *c == '#')
        .collect();
    if (format == "python"
        && matches!(
            prefix.to_ascii_lowercase().as_str(),
            "r" | "br" | "rb" | "fr" | "rf"
        ))
        || (format == "rust" && (prefix.contains('#') || matches!(prefix.as_str(), "r" | "rb")))
        || (matches!(format, "toml" | "yaml") && quote == '\'')
        || (quote == '`' && change.original.contains("${"))
    {
        return None;
    }
    if quote == '"' {
        if let Ok(decoded) = serde_json::from_str::<String>(&format!("\"{}\"", change.original)) {
            return (decoded != change.original).then_some(decoded);
        }
    }
    let mut chars = change.original.chars().peekable();
    let mut output = String::new();
    while let Some(character) = chars.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        let escaped = chars.next()?;
        let value = match escaped {
            '\\' | '\'' | '"' | '`' => escaped,
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'b' => '\u{8}',
            'f' => '\u{c}',
            'v' => '\u{b}',
            'a' => '\u{7}',
            '\n' => continue,
            'x' | 'u' | 'U' => {
                let mut digits = String::new();
                if escaped == 'u' && chars.peek() == Some(&'{') {
                    chars.next();
                    loop {
                        let c = chars.next()?;
                        if c == '}' {
                            break;
                        }
                        digits.push(c);
                    }
                } else {
                    for _ in 0..match escaped {
                        'x' => 2,
                        'u' => 4,
                        _ => 8,
                    } {
                        digits.push(chars.next()?);
                    }
                }
                char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?
            }
            '0'..='7' if matches!(format, "python" | "c" | "cpp") => {
                let mut digits = escaped.to_string();
                for _ in 0..2 {
                    if !chars.peek().is_some_and(|c| matches!(c, '0'..='7')) {
                        break;
                    }
                    digits.push(chars.next()?);
                }
                char::from_u32(u32::from_str_radix(&digits, 8).ok()?)?
            }
            '0' => '\0',
            _ => return None,
        };
        output.push(value);
    }
    (output != change.original).then_some(output)
}
