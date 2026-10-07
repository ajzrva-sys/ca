use crate::workspace::WorkspaceContext;
use std::path::{Path, PathBuf};

const MAX_LINK_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalAppearance {
    Light,
    Dark,
}

#[cfg(test)]
pub fn parse_osc11_response(response: &[u8]) -> Option<TerminalAppearance> {
    let prefix = b"\x1b]11;";
    let start = response
        .windows(prefix.len())
        .position(|window| window == prefix)?;
    let body = &response[start + prefix.len()..];
    let bel = body.iter().position(|byte| *byte == 0x07);
    let st = body.windows(2).position(|window| window == b"\x1b\\");
    let end = match (bel, st) {
        (Some(left), Some(right)) => left.min(right),
        (Some(end), None) | (None, Some(end)) => end,
        (None, None) => return None,
    };
    let value = std::str::from_utf8(&body[..end]).ok()?.trim();
    let (red, green, blue) = parse_color(value)?;
    let luminance = 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    Some(if luminance < 0.5 {
        TerminalAppearance::Dark
    } else {
        TerminalAppearance::Light
    })
}

#[cfg(test)]
fn parse_color(value: &str) -> Option<(f64, f64, f64)> {
    if let Some(rgb) = value.strip_prefix("rgb:") {
        let parts = rgb.split('/').collect::<Vec<_>>();
        if parts.len() != 3 {
            return None;
        }
        let channel = |part: &str| {
            if part.is_empty()
                || part.len() > 4
                || !part.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return None;
            }
            let value = u32::from_str_radix(part, 16).ok()?;
            let maximum = (1_u32 << (part.len() * 4)).saturating_sub(1);
            Some(value as f64 / maximum as f64)
        };
        return Some((channel(parts[0])?, channel(parts[1])?, channel(parts[2])?));
    }
    let hex = value.strip_prefix('#')?;
    let expanded = match hex.len() {
        3 => hex
            .chars()
            .flat_map(|character| [character, character])
            .collect(),
        6 => hex.to_string(),
        _ => return None,
    };
    let channel = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&expanded[range], 16)
            .ok()
            .map(|value| value as f64 / 255.0)
    };
    Some((channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

pub fn probe_terminal_appearance() -> Option<TerminalAppearance> {
    appearance_from_colorfgbg(&std::env::var("COLORFGBG").ok()?)
}

fn appearance_from_colorfgbg(value: &str) -> Option<TerminalAppearance> {
    let parts = value.split(';').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let colors = parts
        .iter()
        .map(|part| part.parse::<u8>().ok())
        .collect::<Option<Vec<_>>>()?;
    match *colors.last()? {
        0..=6 | 8 => Some(TerminalAppearance::Dark),
        7 | 9..=15 => Some(TerminalAppearance::Light),
        _ => None,
    }
}

#[cfg(test)]
mod safe_appearance_tests {
    use super::*;

    #[test]
    fn environment_colors_are_validated_without_terminal_reads() {
        assert_eq!(
            appearance_from_colorfgbg("15;0"),
            Some(TerminalAppearance::Dark)
        );
        assert_eq!(
            appearance_from_colorfgbg("0;15"),
            Some(TerminalAppearance::Light)
        );
        assert_eq!(
            appearance_from_colorfgbg("15;0;7"),
            Some(TerminalAppearance::Light)
        );
        for value in ["", "15", "15;", "15;999", "15;16", "0;1;2;3", "0;7\x1b"] {
            assert_eq!(appearance_from_colorfgbg(value), None, "{value:?}");
        }
    }
}

pub fn safe_terminal_title(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(80)
        .collect()
}

pub fn title_sequence(value: &str) -> String {
    format!("\x1b]2;{}\x1b\\", safe_terminal_title(value))
}

pub const fn clear_title_sequence() -> &'static str {
    "\x1b]2;\x1b\\"
}

pub fn supports_osc8() -> bool {
    std::env::var("TERM").map_or(true, |term| term != "dumb")
}

pub fn safe_link_destination(raw: &str, workspace: &WorkspaceContext) -> Option<String> {
    if raw.is_empty() || raw.len() > MAX_LINK_BYTES || raw.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(raw).ok()?;
    match url.scheme() {
        "http" | "https" if url.host_str().is_some() => Some(url.to_string()),
        "mailto" if !url.path().is_empty() => Some(url.to_string()),
        "file" => {
            let relative = raw
                .strip_prefix("file:")
                .filter(|path| !path.starts_with('/') && !path.starts_with("//"));
            let path = relative
                .map(|path| workspace.cwd().join(path))
                .or_else(|| url.to_file_path().ok())?;
            let contained = contained_path(&path)?;
            workspace.root_relative(&contained)?;
            url::Url::from_file_path(contained)
                .ok()
                .map(|url| url.to_string())
        }
        _ => None,
    }
}

fn contained_path(path: &Path) -> Option<PathBuf> {
    if path.exists() {
        return path.canonicalize().ok();
    }
    let parent = path.parent()?.canonicalize().ok()?;
    Some(parent.join(path.file_name()?))
}

pub fn osc8_cell(symbol: &str, destination: &str) -> String {
    format!("\x1b]8;;{destination}\x1b\\{symbol}\x1b]8;;\x1b\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc11_parser_accepts_common_shapes_and_rejects_malformed_input() {
        assert_eq!(
            parse_osc11_response(b"\x1b]11;rgb:0000/0000/0000\x07"),
            Some(TerminalAppearance::Dark)
        );
        assert_eq!(
            parse_osc11_response(b"noise\x1b]11;#ffffff\x1b\\tail"),
            Some(TerminalAppearance::Light)
        );
        assert_eq!(parse_osc11_response(b"\x1b]11;rgb:zz/00/00\x07"), None);
        assert_eq!(parse_osc11_response(b"\x1b]11;#fff"), None);
    }

    #[test]
    fn titles_and_links_reject_terminal_control_and_workspace_escape() {
        assert_eq!(safe_terminal_title("ca\x1b]2;bad\x07"), "ca]2;bad");
        let root = tempfile::tempdir().unwrap();
        let workspace = WorkspaceContext::from_roots(root.path(), &[]).unwrap();
        let file = root.path().join("file.rs");
        std::fs::write(&file, "ok").unwrap();
        let file_url = url::Url::from_file_path(&file).unwrap();
        assert!(safe_link_destination("https://example.test/a", &workspace).is_some());
        assert!(safe_link_destination("javascript:alert(1)", &workspace).is_none());
        assert!(safe_link_destination(file_url.as_ref(), &workspace).is_some());
        assert!(safe_link_destination("file:file.rs", &workspace).is_some());
        assert!(safe_link_destination("file:///etc/passwd", &workspace).is_none());
        assert!(safe_link_destination("https://example.test/\x1b", &workspace).is_none());
    }
}
