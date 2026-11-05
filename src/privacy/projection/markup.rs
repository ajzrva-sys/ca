use super::super::policy::AliasKind;
use super::{syntax, Builder};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

pub(super) fn plain(builder: &mut Builder<'_>, text: &str) {
    let mut offset = 0;
    for (line, raw) in text.split_inclusive('\n').enumerate() {
        let value = raw.trim_matches(char::is_whitespace);
        if !value.is_empty() {
            let start = offset + raw.find(value).expect("trimmed slice exists");
            builder.node(
                start..start + value.len(),
                format!("/text/line/{line}"),
                AliasKind::Text,
            );
        }
        offset += raw.len();
    }
}

pub(super) fn markdown(builder: &mut Builder<'_>, text: &str) -> Result<(), String> {
    let mut heading = 0_usize;
    let mut paragraph = 0_usize;
    let mut part = 0_usize;
    let mut code: Option<String> = None;
    for (event, range) in Parser::new_ext(text, Options::all()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                heading += 1;
                paragraph = 0;
                part = 0;
            }
            Event::Start(Tag::Paragraph) => {
                paragraph += 1;
                part = 0;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                let format = syntax::format_for(&format!("f.{language}"));
                // Only a supported grammar name is retained as fence metadata.
                if !matches!(format, "text" | "opaque") && !language.is_empty() {
                    if let Some(start) = text[range.clone()].find(&language) {
                        builder.safe(range.start + start..range.start + start + language.len());
                    }
                }
                code = Some(format.to_string());
            }
            Event::End(TagEnd::CodeBlock) => code = None,
            Event::Text(_) => {
                let selector =
                    format!("/markdown/section/{heading}/paragraph/{paragraph}/text/{part}");
                if let Some(format) = &code {
                    if format == "text" {
                        builder.node(range.clone(), selector, AliasKind::Text);
                    } else {
                        syntax::analyze(
                            builder,
                            format,
                            &text[range.clone()],
                            range.start,
                            &selector,
                        )?;
                    }
                } else {
                    builder.node(range, selector, AliasKind::Text);
                }
                part += 1;
            }
            Event::Code(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {
                let raw = &text[range.clone()];
                let marker = if raw.starts_with('`') { '`' } else { '$' };
                let left = raw.chars().take_while(|ch| *ch == marker).count();
                let right = raw.chars().rev().take_while(|ch| *ch == marker).count();
                if left + right <= raw.len() {
                    builder.safe(range.start..range.start + left);
                    builder.safe(range.end - right..range.end);
                    builder.node(
                        range.start + left..range.end - right,
                        format!("/markdown/section/{heading}/inline/{part}"),
                        AliasKind::String,
                    );
                } else {
                    builder.node(
                        range,
                        format!("/markdown/section/{heading}/inline/{part}"),
                        AliasKind::String,
                    );
                }
                part += 1;
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                syntax::analyze(
                    builder,
                    "html",
                    &text[range.clone()],
                    range.start,
                    &format!("/markdown/section/{heading}/html/{part}"),
                )?;
                part += 1;
            }
            Event::Start(
                Tag::Link {
                    dest_url, title, ..
                }
                | Tag::Image {
                    dest_url, title, ..
                },
            ) => {
                for (kind, value) in [
                    ("destination", dest_url.as_ref()),
                    ("title", title.as_ref()),
                ] {
                    if !value.is_empty() {
                        if let Some(start) = text[range.clone()].find(value) {
                            builder.node(
                                range.start + start..range.start + start + value.len(),
                                format!("/markdown/section/{heading}/link/{part}/{kind}"),
                                AliasKind::Path,
                            );
                        }
                    }
                }
                part += 1;
            }
            _ => {}
        }
    }
    // Markdown punctuation is retained only outside actual content nodes. This
    // covers fences/list/table/link delimiters; unparsed words remain opaque.
    for (offset, ch) in text.char_indices() {
        if ch.is_ascii() && "#*_~`>-+.![](){}|:$\\<>=".contains(ch) {
            builder.safe(offset..offset + ch.len_utf8());
        }
    }
    Ok(())
}

/// RFC 4180-style quoted cells, including embedded delimiters and newlines.
/// Invalid quoting makes the whole input opaque; no guessed record boundaries.
pub(super) fn csv(builder: &mut Builder<'_>, text: &str, delimiter: u8) -> Result<(), String> {
    let bytes = text.as_bytes();
    let mut at = 0;
    let mut row = 0;
    let mut col = 0;
    let mut cells = Vec::new();
    let mut punctuation = Vec::new();
    while at < bytes.len() {
        let start = at;
        if bytes[at] == b'"' {
            at += 1;
            let inner = at;
            loop {
                if at >= bytes.len() {
                    builder.opaque(0..text.len(), "/csv");
                    return Ok(());
                }
                if bytes[at] == b'"' {
                    if bytes.get(at + 1) == Some(&b'"') {
                        at += 2;
                        continue;
                    }
                    cells.push((inner..at, row, col));
                    punctuation.push(start..inner);
                    punctuation.push(at..at + 1);
                    at += 1;
                    break;
                }
                at += 1;
            }
            if at < bytes.len() && bytes[at] != delimiter && !matches!(bytes[at], b'\r' | b'\n') {
                builder.opaque(0..text.len(), "/csv");
                return Ok(());
            }
        } else {
            while at < bytes.len() && bytes[at] != delimiter && !matches!(bytes[at], b'\r' | b'\n')
            {
                if bytes[at] == b'"' {
                    builder.opaque(0..text.len(), "/csv");
                    return Ok(());
                }
                at += 1;
            }
            cells.push((start..at, row, col));
        }
        if bytes.get(at) == Some(&delimiter) {
            punctuation.push(at..at + 1);
            at += 1;
            col += 1;
        } else if matches!(bytes.get(at), Some(b'\r' | b'\n')) {
            if bytes.get(at) == Some(&b'\r') && bytes.get(at + 1) == Some(&b'\n') {
                punctuation.push(at..at + 2);
                at += 2;
            } else {
                punctuation.push(at..at + 1);
                at += 1;
            }
            row += 1;
            col = 0;
        }
    }
    for range in punctuation {
        builder.safe(range);
    }
    for (range, row, col) in cells {
        builder.node(
            range,
            format!("/csv/row/{row}/column/{col}"),
            AliasKind::String,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::super::policy::{Action, Policy, Rule};
    use super::super::{analyze, project};
    fn private(path: &str, text: &str) -> String {
        project(
            path,
            text,
            &Policy {
                enabled: true,
                ..Policy::default()
            },
            &mut |kind, _| Ok(format!("__{}__", kind.label().to_uppercase())),
        )
        .unwrap()
        .text
    }
    #[test]
    fn csv_quotes_embedded_newlines_and_cells_do_not_leak() {
        let text = "name,notes\r\n\"Private Name\",\"secret,more\nnext\"\r\n";
        let out = private("a.csv", text);
        assert!(!out.contains("Private"));
        assert!(!out.contains("secret"));
        assert_eq!(out.matches('\n').count(), text.matches('\n').count());
        assert!(out.contains(","));
        assert!(!private("a.csv", "\"broken secret").contains("secret"));
    }
    #[test]
    fn markdown_retains_headings_lists_and_masks_destinations() {
        let text = "# Private heading\n\n- secret detail\n- [Private link](https://private.example/account)\n";
        let out = private("a.md", text);
        assert!(out.starts_with("# "));
        for secret in ["Private", "secret", "private.example", "account"] {
            assert!(!out.contains(secret), "{out}");
        }
        let document = analyze("a.md", text).unwrap();
        let node = document
            .nodes
            .iter()
            .find(|node| &text[node.range.clone()] == "Private heading")
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
        let out = project("a.md", text, &policy, &mut |_, _| Ok("__PRIVATE__".into()))
            .unwrap()
            .text;
        assert!(out.contains("Private heading"));
        assert!(!out.contains("secret detail"));
    }
    #[test]
    fn html_attrs_inline_code_css_and_custom_names_remain_private() {
        let text = "<div class=\"privateclass\" onclick=\"privateFunction('secret')\" style=\"color: red\"><private-tag data-secret=\"value\">Private text</private-tag><script>const privateName='hidden';</script></div>";
        let out = private("a.html", text);
        for secret in [
            "privateclass",
            "privateFunction",
            "secret",
            "Private text",
            "privateName",
            "hidden",
            "private-tag",
        ] {
            assert!(!out.contains(secret), "{secret}: {out}");
        }
        assert!(out.contains("<div"));
        assert!(out.contains("onclick="));
    }

    #[test]
    fn inline_css_fragment_maps_only_original_byte_ranges() {
        let source = "<div style=\"color: privateColor; --private-name: 91239px\">hidden</div>";
        let document = analyze("a.html", source).unwrap();
        let css: Vec<_> = document
            .nodes
            .iter()
            .filter(|node| node.selector.contains("/css/"))
            .collect();
        assert!(css.len() >= 4, "{css:?}");
        assert!(css
            .iter()
            .all(|node| source.is_char_boundary(node.range.start)
                && source.is_char_boundary(node.range.end)
                && !source[node.range.clone()].contains("<div")));
        let out = private("a.html", source);
        assert!(out.contains(": "));
        assert!(out.contains(';'));
        for value in [
            "privateColor",
            "private-name",
            "91239",
            "ca_private_fragment",
        ] {
            assert!(!out.contains(value), "{out}");
        }
    }

    #[test]
    fn shell_substitutions_heredocs_and_strings_never_publish_private_words() {
        let source = "secret_var=$(private_command --private-flag \"$private_input\")\nprintf '%s' \"${secret_var#private_prefix}\"\ncat <<PRIVATE_END\nprivate_body\nPRIVATE_END\n";
        let out = private("a.sh", source);
        for secret in [
            "secret_var",
            "private_command",
            "private-flag",
            "private_input",
            "private_prefix",
            "PRIVATE_END",
            "private_body",
        ] {
            assert!(!out.contains(secret), "{secret}: {out}");
        }
    }
}
