//! Backend behaviors taken from Grok's agent loop and expressed in CA's own types.
//!
//! This module does not copy oracle source. Each function is the observable
//! contract: what is kept, what is refused, and what the model is told.

use std::path::{Component, Path, PathBuf};

pub const MAX_IN_FLIGHT_STEERS: usize = 4;
pub const DOOM_LOOP_RESAMPLES: u8 = 2;
pub const WORKFLOW_MAX_PHASES: usize = 64;
pub const WORKFLOW_DEFAULT_BUDGET: u64 = 128;
pub const WORKFLOW_MAX_BUDGET: u64 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteerDelivery {
    Steer,
    Queue,
    Interject,
}

impl SteerDelivery {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "steer" => Some(Self::Steer),
            "queue" => Some(Self::Queue),
            "interject" => Some(Self::Interject),
            _ => None,
        }
    }

    #[must_use]
    pub fn jumps_ahead(self) -> bool {
        matches!(self, Self::Interject)
    }
}

/// A denied path proposes the nearest specific directory.
/// The workspace, the home directory, `~/.local`, and `~/Library` are never the proposal.
#[must_use]
pub fn propose_directory_grant(
    blocked: &Path,
    workspace: &Path,
    home: Option<&Path>,
) -> Option<PathBuf> {
    let mut cursor = blocked.to_path_buf();
    let mut missing_top = None;
    if !cursor.exists() || cursor.is_file() {
        cursor.pop();
    }
    while !cursor.as_os_str().is_empty() && !cursor.exists() {
        missing_top = Some(cursor.clone());
        if !cursor.pop() {
            break;
        }
    }
    if cursor.as_os_str().is_empty() || too_broad(&cursor, workspace, home) {
        return missing_top.filter(|path| !too_broad(path, workspace, home));
    }
    Some(cursor)
}

fn too_broad(path: &Path, workspace: &Path, home: Option<&Path>) -> bool {
    if path == workspace || path == Path::new("/") || path == Path::new("/Users") {
        return true;
    }
    let Some(home) = home else {
        return false;
    };
    path == home
        || path == home.join(".local")
        || path == home.join("Library")
        || path == home.join(".local").join("share")
}

/// Split a shell command on `&&`, `||`, `;`, `|`, and a newline or CR the
/// shell would run as its own command.
///
/// A newline inside quotes stays in the segment. A newline inside `$(...)`
/// or backticks is still a command start, including when that substitution
/// sits inside double quotes. A backslash before `;` or `|` stays in the
/// segment, because the shell does not start a command there. A literal
/// heredoc body that `cat` or `tee` only copies is blanked first, so a line
/// of that body is not a command.
#[must_use]
pub fn split_command_segments(command: &str) -> Vec<String> {
    split_shell_segments(&blank_literal_heredoc_bodies(command))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellQuote {
    Single,
    Double,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellFrameKind {
    Root,
    Dollar,
    Tick,
}

struct ShellFrame {
    kind: ShellFrameKind,
    quote: Option<ShellQuote>,
    paren_depth: i32,
}

impl ShellFrame {
    fn root() -> Self {
        Self {
            kind: ShellFrameKind::Root,
            quote: None,
            paren_depth: 0,
        }
    }
}

fn next_char(text: &str, index: usize) -> Option<(usize, char)> {
    let ch = text[index..].chars().next()?;
    Some((index + ch.len_utf8(), ch))
}

fn push_substitution(command: &str, index: usize, frames: &mut Vec<ShellFrame>) -> Option<usize> {
    let (next, ch) = next_char(command, index)?;
    if ch == '`' {
        frames.push(ShellFrame {
            kind: ShellFrameKind::Tick,
            quote: None,
            paren_depth: 0,
        });
        return Some(next);
    }
    if ch == '$' {
        let (after, peek) = next_char(command, next)?;
        if peek == '(' {
            frames.push(ShellFrame {
                kind: ShellFrameKind::Dollar,
                quote: None,
                paren_depth: 1,
            });
            return Some(after);
        }
    }
    None
}

fn close_tick(frames: &mut Vec<ShellFrame>, ch: char) -> bool {
    if ch != '`' {
        return false;
    }
    let close = frames
        .last()
        .is_some_and(|frame| frame.kind == ShellFrameKind::Tick && frame.quote.is_none());
    if close {
        frames.pop();
    }
    close
}

fn note_paren(frames: &mut Vec<ShellFrame>, ch: char) -> bool {
    let Some(frame) = frames.last() else {
        return false;
    };
    if frame.kind != ShellFrameKind::Dollar || frame.quote.is_some() {
        return false;
    }
    if ch == '(' {
        frames.last_mut().unwrap().paren_depth += 1;
        return true;
    }
    if ch == ')' {
        let depth = frame.paren_depth - 1;
        if depth <= 0 && frames.len() > 1 {
            frames.pop();
        } else if let Some(frame) = frames.last_mut() {
            frame.paren_depth = depth;
        }
        return true;
    }
    false
}

fn at_word_start(command: &str, index: usize) -> bool {
    index == 0
        || command[..index]
            .ends_with(|ch: char| ch.is_whitespace() || matches!(ch, ';' | '|' | '&' | '(' | ')'))
}

fn skip_comment(command: &str, index: usize) -> usize {
    let mut cursor = index;
    while let Some((next, ch)) = next_char(command, cursor) {
        if ch == '\n' || ch == '\r' {
            break;
        }
        cursor = next;
    }
    cursor
}

fn push_segment(segments: &mut Vec<String>, current: &mut String) {
    let segment = current.trim().to_string();
    if !segment.is_empty() {
        segments.push(segment);
    }
    current.clear();
}

fn escaped_break_end(command: &str, after_slash: usize) -> Option<usize> {
    let (after, escaped) = next_char(command, after_slash)?;
    if escaped == '\n' {
        return Some(after);
    }
    if escaped != '\r' {
        return None;
    }
    let Some((after_line, next)) = next_char(command, after) else {
        return Some(after);
    };
    if next == '\n' {
        Some(after_line)
    } else {
        Some(after)
    }
}

fn split_boundary(command: &str, index: usize) -> Option<usize> {
    let (next, ch) = next_char(command, index)?;
    if matches!(ch, ';' | '|' | '\n' | '\r') {
        return Some(next);
    }
    if ch == '&' {
        let (after, peek) = next_char(command, next)?;
        if peek == '&' {
            return Some(after);
        }
    }
    None
}

fn split_shell_segments(command: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut frames = vec![ShellFrame::root()];
    let mut index = 0;
    while index < command.len() {
        let (next, ch) = next_char(command, index).expect("index stays on a char");
        let quote = frames.last().expect("root frame").quote;
        if quote == Some(ShellQuote::Single) {
            current.push(ch);
            if ch == '\'' {
                frames.last_mut().expect("root frame").quote = None;
            }
            index = next;
            continue;
        }
        if quote == Some(ShellQuote::Double) && ch == '\\' {
            current.push(ch);
            index = next;
            if let Some((after, escaped)) = next_char(command, index) {
                current.push(escaped);
                index = after;
            }
            continue;
        }
        if quote == Some(ShellQuote::Double) {
            if ch == '"' {
                current.push(ch);
                frames.last_mut().expect("root frame").quote = None;
                index = next;
                continue;
            }
            if let Some(end) = push_substitution(command, index, &mut frames) {
                current.push_str(&command[index..end]);
                index = end;
                continue;
            }
            current.push(ch);
            index = next;
            continue;
        }
        if ch == '\\' {
            if let Some(end) = escaped_break_end(command, next) {
                current.push_str(&command[index..end]);
                index = end;
                continue;
            }
            if let Some((after, escaped)) = next_char(command, next) {
                if matches!(escaped, ';' | '|') {
                    current.push_str(&command[index..after]);
                    index = after;
                    continue;
                }
            }
        }
        if ch == '\'' || ch == '"' {
            current.push(ch);
            frames.last_mut().expect("root frame").quote = if ch == '\'' {
                Some(ShellQuote::Single)
            } else {
                Some(ShellQuote::Double)
            };
            index = next;
            continue;
        }
        if ch == '#' && at_word_start(command, index) {
            let comment_end = skip_comment(command, next);
            current.push_str(&command[index..comment_end]);
            index = comment_end;
            continue;
        }
        if close_tick(&mut frames, ch) {
            current.push(ch);
            index = next;
            continue;
        }
        if let Some(end) = push_substitution(command, index, &mut frames) {
            current.push_str(&command[index..end]);
            index = end;
            continue;
        }
        if note_paren(&mut frames, ch) {
            current.push(ch);
            index = next;
            continue;
        }
        if let Some(end) = split_boundary(command, index) {
            push_segment(&mut segments, &mut current);
            index = end;
            continue;
        }
        current.push(ch);
        index = next;
    }
    push_segment(&mut segments, &mut current);
    segments
}

fn line_continues(header: &str) -> bool {
    header.chars().rev().take_while(|ch| *ch == '\\').count() % 2 == 1
}

fn header_runs_the_body(header: &str) -> bool {
    header.chars().any(|ch| matches!(ch, '|' | ';' | '&'))
}

fn is_literal_reader(word: &str) -> bool {
    let base = word.rsplit(['/', '\\']).next().unwrap_or(word);
    base.eq_ignore_ascii_case("cat") || base.eq_ignore_ascii_case("tee")
}

fn command_word_before(command: &str, index: usize) -> Option<&str> {
    let head = &command[..index];
    let line_start = head.rfind('\n').map_or(0, |found| found + 1);
    let piece_start = head[line_start..]
        .rfind(['&', ';', '|', '('])
        .map_or(line_start, |found| line_start + found + 1);
    command[piece_start..index].split_whitespace().next()
}

fn is_delim_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
}

fn quoted_heredoc_opener(command: &str, index: usize) -> Option<(String, usize)> {
    let bytes = command.as_bytes();
    if bytes.get(index) != Some(&b'<') || bytes.get(index + 1) != Some(&b'<') {
        return None;
    }
    let word = command_word_before(command, index)?;
    if !is_literal_reader(word) {
        return None;
    }
    let mut cursor = index + 2;
    if bytes.get(cursor) == Some(&b'-') {
        cursor += 1;
    }
    while matches!(bytes.get(cursor), Some(b' ' | b'\t')) {
        cursor += 1;
    }
    let quote = *bytes.get(cursor)?;
    if quote != b'\'' && quote != b'"' {
        return None;
    }
    cursor += 1;
    let delim_start = cursor;
    while cursor < bytes.len() && is_delim_byte(bytes[cursor]) {
        cursor += 1;
    }
    if cursor == delim_start || bytes.get(cursor) != Some(&quote) {
        return None;
    }
    let delim = command[delim_start..cursor].to_string();
    cursor += 1;
    let newline = command[cursor..].find('\n')?;
    let mut header = command[cursor..cursor + newline].to_string();
    let mut body_start = cursor + newline + 1;
    for _ in 0..8 {
        if !line_continues(&header) || header_runs_the_body(&header) {
            break;
        }
        let rest = command.get(body_start..)?;
        let newline = rest.find('\n')?;
        header.pop();
        header.push_str(&rest[..newline]);
        body_start += newline + 1;
    }
    if line_continues(&header) || header_runs_the_body(&header) {
        return None;
    }
    Some((delim, body_start))
}

fn heredoc_body_end(command: &str, body_start: usize, delim: &str) -> usize {
    let mut line_start = body_start;
    for line in command[body_start..].split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']).trim_start_matches('\t');
        if bare == delim {
            return line_start;
        }
        line_start += line.len();
    }
    command.len()
}

fn literal_heredoc_ranges(command: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut frames = vec![ShellFrame::root()];
    let mut index = 0;
    while index < command.len() {
        let (next, ch) = next_char(command, index).expect("index stays on a char");
        let quote = frames.last().expect("root frame").quote;
        if quote == Some(ShellQuote::Single) {
            if ch == '\'' {
                frames.last_mut().expect("root frame").quote = None;
            }
            index = next;
            continue;
        }
        if ch == '\\' && quote != Some(ShellQuote::Single) {
            index = next_char(command, next).map_or(command.len(), |(after, _)| after);
            continue;
        }
        if quote == Some(ShellQuote::Double) {
            if ch == '"' {
                frames.last_mut().expect("root frame").quote = None;
            } else if let Some(end) = push_substitution(command, index, &mut frames) {
                index = end;
                continue;
            }
            index = next;
            continue;
        }
        if ch == '\'' || ch == '"' {
            frames.last_mut().expect("root frame").quote = if ch == '\'' {
                Some(ShellQuote::Single)
            } else {
                Some(ShellQuote::Double)
            };
            index = next;
            continue;
        }
        if ch == '#' && at_word_start(command, index) {
            index = skip_comment(command, next);
            continue;
        }
        if close_tick(&mut frames, ch) || note_paren(&mut frames, ch) {
            index = next;
            continue;
        }
        if let Some(end) = push_substitution(command, index, &mut frames) {
            index = end;
            continue;
        }
        if ch == '<' {
            if let Some((delim, body_start)) = quoted_heredoc_opener(command, index) {
                let body_end = heredoc_body_end(command, body_start, &delim);
                if body_end > body_start {
                    ranges.push((body_start, body_end));
                }
                index = body_end;
                continue;
            }
        }
        index = next;
    }
    ranges
}

fn blank_literal_heredoc_bodies(command: &str) -> String {
    let ranges = literal_heredoc_ranges(command);
    if ranges.is_empty() {
        return command.to_string();
    }
    let mut out = String::with_capacity(command.len());
    let mut cursor = 0;
    for (start, end) in ranges {
        if start < cursor || end > command.len() {
            continue;
        }
        out.push_str(&command[cursor..start]);
        for ch in command[start..end].chars() {
            out.push(if ch == '\n' { '\n' } else { ' ' });
        }
        cursor = end;
    }
    out.push_str(&command[cursor..]);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRisk {
    Plain,
    EnvAssignment,
    Exec,
    Injection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandFloor {
    Allow,
    Ask,
    Deny,
}

const SAFE_ENV_KEYS: &[&str] = &[
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "FORCE_COLOR",
    "COLORTERM",
    "CARGO_TERM_COLOR",
    "CARGO_TERM_PROGRESS_WHEN",
    "RUST_LOG",
    "RUST_LOG_STYLE",
    "RUST_BACKTRACE",
    "RUST_TEST_THREADS",
    "RUST_MIN_STACK",
];

const INJECTION_ENV_KEYS: &[&str] = &[
    "LD_PRELOAD",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "BASH_ENV",
    "ENV",
    "IFS",
    "PATH",
    "GIT_EXTERNAL_DIFF",
    "GIT_PROXY_COMMAND",
    "GIT_SSH_COMMAND",
    "PROMPT_COMMAND",
    "NODE_OPTIONS",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "PERL5OPT",
    "RUBYOPT",
];

#[must_use]
pub fn command_risk(segment: &str) -> CommandRisk {
    match command_floor(segment) {
        CommandFloor::Deny => CommandRisk::Injection,
        CommandFloor::Allow => CommandRisk::Plain,
        CommandFloor::Ask => {
            if unvetted_assignment_only(segment) {
                CommandRisk::EnvAssignment
            } else {
                CommandRisk::Exec
            }
        }
    }
}

/// The strictest floor for one shell segment. Deny stays above Ask.
#[must_use]
pub fn command_floor(segment: &str) -> CommandFloor {
    if substitution_executes(segment) {
        return CommandFloor::Ask;
    }
    let words = shell_words(segment);
    match peeled_command(&words) {
        Peel::Deny => CommandFloor::Deny,
        Peel::Ask => CommandFloor::Ask,
        Peel::Words(words) => score_words(&words),
    }
}

enum Peel {
    Deny,
    Ask,
    Words(Vec<String>),
}

fn peeled_command(words: &[String]) -> Peel {
    let mut words = words.to_vec();
    let mut floor = CommandFloor::Allow;
    while let Some(head) = words.first().map(|word| word.to_ascii_lowercase()) {
        if matches!(head.as_str(), "command" | "builtin" | "nohup" | "time") {
            words.remove(0);
            continue;
        }
        if head == "nice" {
            words.remove(0);
            if words.first().is_some_and(|word| word == "-n") {
                words.remove(0);
                if !words.is_empty() {
                    words.remove(0);
                }
            }
            continue;
        }
        if head == "env" {
            words.remove(0);
            while let Some(token) = words.first() {
                if let Some(key) = assignment_key(token) {
                    floor = raise_floor(floor, env_key_floor(&key));
                    words.remove(0);
                    continue;
                }
                break;
            }
            continue;
        }
        if let Some(key) = assignment_key(&head) {
            floor = raise_floor(floor, env_key_floor(&key));
            words.remove(0);
            continue;
        }
        break;
    }
    match floor {
        CommandFloor::Deny => Peel::Deny,
        CommandFloor::Ask => Peel::Ask,
        CommandFloor::Allow => Peel::Words(words),
    }
}

/// A plan-mode shell write. Ask is not this check: `curl` and `git log` stay reads.
#[must_use]
pub fn command_writes(command: &str) -> bool {
    split_command_segments(command)
        .iter()
        .any(|segment| segment_writes(segment))
}

fn segment_writes(segment: &str) -> bool {
    if has_unquoted_redirect(segment) {
        return true;
    }
    let words = shell_words(segment);
    let Peel::Words(words) = peeled_command(&words) else {
        return false;
    };
    write_program(&words)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellWriteTarget {
    Literal(PathBuf),
    Unverified,
}

/// Literal `>` / `>>` / `tee` destinations after a leading `cd <literal> &&`.
/// A quoted `>` and an fd copy such as `2>&1` are not destinations. A target
/// that is not a literal path is [`ShellWriteTarget::Unverified`].
#[must_use]
pub fn shell_write_targets(command: &str, cwd: &Path) -> Vec<ShellWriteTarget> {
    let mut base = cwd.to_path_buf();
    let mut trusted = true;
    let mut targets = Vec::new();
    for segment in split_command_segments(command) {
        if let Some(dir) = literal_cd(&shell_words(&segment)) {
            if trusted && path_is_literal(&dir) {
                let path = PathBuf::from(&dir);
                base = if path.is_absolute() {
                    path
                } else {
                    base.join(path)
                };
            } else {
                trusted = false;
            }
            continue;
        }
        for target in segment_write_targets(&segment) {
            if !trusted {
                targets.push(ShellWriteTarget::Unverified);
                continue;
            }
            match target {
                Err(()) => targets.push(ShellWriteTarget::Unverified),
                Ok(path) => {
                    let path = PathBuf::from(path);
                    let full = if path.is_absolute() {
                        path
                    } else {
                        base.join(path)
                    };
                    targets.push(ShellWriteTarget::Literal(full));
                }
            }
        }
    }
    targets
}

fn path_is_literal(word: &str) -> bool {
    !word.is_empty()
        && !word.contains(['$', '`', '*', '?', '~'])
        && !word.contains("$(")
        && !word.contains("${")
}

fn literal_cd(words: &[String]) -> Option<String> {
    let program = words.first()?.rsplit(['/', '\\']).next()?;
    if program != "cd" {
        return None;
    }
    let rest: Vec<&String> = words
        .iter()
        .skip(1)
        .filter(|word| word.as_str() != "--")
        .collect();
    match rest.as_slice() {
        [dir] => Some((*dir).clone()),
        _ => Some(String::new()),
    }
}

fn segment_write_targets(segment: &str) -> Vec<Result<String, ()>> {
    let mut targets = redirect_targets(segment);
    targets.extend(tee_targets(segment));
    targets
}

fn redirect_targets(segment: &str) -> Vec<Result<String, ()>> {
    let chars: Vec<char> = segment.chars().collect();
    let mut targets = Vec::new();
    let mut index = 0;
    let mut quote = None;
    while index < chars.len() {
        let ch = chars[index];
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
            index += 1;
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            index += 1;
            continue;
        }
        if ch != '>' {
            index += 1;
            continue;
        }
        if index > 0 && chars[index - 1] == '<' {
            index += 1;
            continue;
        }
        let mut cursor = index + 1;
        if chars.get(cursor) == Some(&'>') || chars.get(cursor) == Some(&'|') {
            cursor += 1;
        }
        if chars.get(cursor) == Some(&'&') {
            let after = cursor + 1;
            if chars
                .get(after)
                .is_some_and(|ch| ch.is_ascii_digit() || *ch == '-')
            {
                index = after + 1;
                continue;
            }
        }
        while chars.get(cursor).is_some_and(|ch| ch.is_whitespace()) {
            cursor += 1;
        }
        let mut word = String::new();
        let mut word_quote = None;
        while cursor < chars.len() {
            let ch = chars[cursor];
            if let Some(open) = word_quote {
                if ch == open {
                    word_quote = None;
                    cursor += 1;
                    continue;
                }
                word.push(ch);
                cursor += 1;
                continue;
            }
            if ch == '\'' || ch == '"' {
                word_quote = Some(ch);
                cursor += 1;
                continue;
            }
            if ch.is_whitespace() || matches!(ch, '>' | '<' | '|' | '&' | ';') {
                break;
            }
            word.push(ch);
            cursor += 1;
        }
        if word.is_empty() || !path_is_literal(&word) {
            targets.push(Err(()));
        } else {
            targets.push(Ok(word));
        }
        index = cursor;
    }
    targets
}

fn tee_targets(segment: &str) -> Vec<Result<String, ()>> {
    let mut words = shell_words(segment);
    while words.first().is_some_and(|word| {
        word.split_once('=').is_some_and(|(key, _)| {
            !key.is_empty()
                && key
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
    }) {
        words.remove(0);
    }
    let Some(program) = words.first() else {
        return Vec::new();
    };
    let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
    if program != "tee" {
        return Vec::new();
    }
    let mut targets = Vec::new();
    let mut ended = false;
    for word in words.iter().skip(1) {
        if !ended && word == "--" {
            ended = true;
            continue;
        }
        if !ended && word.starts_with('-') {
            continue;
        }
        if path_is_literal(word) {
            targets.push(Ok(word.clone()));
        } else {
            targets.push(Err(()));
        }
    }
    targets
}

fn has_unquoted_redirect(segment: &str) -> bool {
    let mut quote = None;
    for ch in segment.chars() {
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if ch == '>' {
            return true;
        }
    }
    false
}

fn write_program(words: &[String]) -> bool {
    let Some(program) = words.first() else {
        return false;
    };
    let program = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    if matches!(
        program.as_str(),
        "rm" | "mv"
            | "cp"
            | "mkdir"
            | "rmdir"
            | "touch"
            | "install"
            | "tee"
            | "truncate"
            | "ln"
            | "chmod"
            | "chown"
            | "dd"
    ) {
        return true;
    }
    matches!(program.as_str(), "sed" | "perl")
        && words
            .iter()
            .any(|word| word == "-i" || word.starts_with("-i"))
}

fn score_words(words: &[String]) -> CommandFloor {
    let Some(program) = words.first().map(|word| word.to_ascii_lowercase()) else {
        return CommandFloor::Allow;
    };
    if matches!(
        program.as_str(),
        "sudo" | "curl" | "wget" | "rm" | "dd" | "chmod" | "chown"
    ) {
        return CommandFloor::Ask;
    }
    if matches!(program.as_str(), "bash" | "sh" | "zsh" | "dash")
        && words.iter().any(|word| word == "-c" || word == "-lc")
    {
        return CommandFloor::Ask;
    }
    if matches!(
        program.as_str(),
        "python" | "python3" | "perl" | "ruby" | "node"
    ) && words.iter().any(|word| word == "-c" || word == "-e")
    {
        return CommandFloor::Ask;
    }
    if program == "find" {
        return find_action_floor(words);
    }
    if program == "git" {
        if words.iter().any(|word| word == "-c") {
            return CommandFloor::Ask;
        }
        if git_history_command(&words.join(" ")) {
            return CommandFloor::Ask;
        }
    }
    CommandFloor::Allow
}

fn find_action_floor(words: &[String]) -> CommandFloor {
    if words.iter().any(|word| {
        matches!(
            word.as_str(),
            "-delete" | "-fls" | "-fprint" | "-fprint0" | "-fprintf" | "-ok" | "-okdir"
        )
    }) {
        return CommandFloor::Ask;
    }
    let mut floor = CommandFloor::Allow;
    let mut index = 0;
    while index < words.len() {
        let word = words[index].as_str();
        if word == "-exec" || word == "-execdir" {
            index += 1;
            let start = index;
            while index < words.len() && !find_exec_end(&words[index]) {
                index += 1;
            }
            if start == index {
                return CommandFloor::Ask;
            }
            floor = raise_floor(floor, exec_program_floor(&words[start..index]));
            if floor == CommandFloor::Deny {
                return floor;
            }
        }
        index += 1;
    }
    floor
}

fn find_exec_end(word: &str) -> bool {
    matches!(word, ";" | "\\;" | "+")
}

fn exec_program_floor(words: &[String]) -> CommandFloor {
    let Some(program) = words.first() else {
        return CommandFloor::Ask;
    };
    if program.contains('/') || program.contains('\\') {
        return CommandFloor::Ask;
    }
    match peeled_command(words) {
        Peel::Deny => CommandFloor::Deny,
        Peel::Ask => CommandFloor::Ask,
        Peel::Words(words) => {
            let Some(program) = words.first() else {
                return CommandFloor::Ask;
            };
            if program.contains('/') || program.contains('\\') || !find_exec_reader(program) {
                return CommandFloor::Ask;
            }
            score_words(&words)
        }
    }
}

fn find_exec_reader(program: &str) -> bool {
    matches!(
        program.to_ascii_lowercase().as_str(),
        "basename"
            | "cat"
            | "cmp"
            | "comm"
            | "cut"
            | "dirname"
            | "echo"
            | "egrep"
            | "false"
            | "fgrep"
            | "grep"
            | "head"
            | "id"
            | "ls"
            | "md5"
            | "od"
            | "printf"
            | "pwd"
            | "readlink"
            | "realpath"
            | "shasum"
            | "strings"
            | "tail"
            | "tr"
            | "true"
            | "uname"
            | "wc"
            | "whoami"
    )
}

fn unvetted_assignment_only(segment: &str) -> bool {
    let words = shell_words(segment);
    words
        .first()
        .is_some_and(|word| assignment_key(word).is_some())
        && !substitution_executes(segment)
        && peeled_command(&words).is_ask_without_exec()
}

impl Peel {
    fn is_ask_without_exec(&self) -> bool {
        matches!(self, Peel::Ask)
    }
}

fn raise_floor(current: CommandFloor, next: CommandFloor) -> CommandFloor {
    match (current, next) {
        (CommandFloor::Deny, _) | (_, CommandFloor::Deny) => CommandFloor::Deny,
        (CommandFloor::Ask, _) | (_, CommandFloor::Ask) => CommandFloor::Ask,
        _ => CommandFloor::Allow,
    }
}

fn env_key_floor(key: &str) -> CommandFloor {
    if INJECTION_ENV_KEYS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(key))
    {
        CommandFloor::Deny
    } else if SAFE_ENV_KEYS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(key))
    {
        CommandFloor::Allow
    } else {
        CommandFloor::Ask
    }
}

fn assignment_key(token: &str) -> Option<String> {
    let (key, _) = token.split_once('=')?;
    if key.is_empty()
        || key.starts_with('-')
        || !key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    Some(key.to_string())
}

fn substitution_executes(segment: &str) -> bool {
    let chars: Vec<char> = segment.chars().collect();
    let mut quote = None;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if quote == Some('\'') {
            if ch == '\'' {
                quote = None;
            }
            index += 1;
            continue;
        }
        if ch == '\'' && quote.is_none() {
            quote = Some('\'');
            index += 1;
            continue;
        }
        if ch == '"' && quote.is_none() {
            quote = Some('"');
            index += 1;
            continue;
        }
        if ch == '"' && quote == Some('"') {
            quote = None;
            index += 1;
            continue;
        }
        if ch == '`' {
            return true;
        }
        if ch == '$' && chars.get(index + 1) == Some(&'(') {
            return true;
        }
        if ch == '<' && chars.get(index + 1) == Some(&'(') {
            return true;
        }
        index += 1;
    }
    false
}

pub(crate) fn shell_words(segment: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for ch in segment.chars() {
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// A repo-local git clean, smudge, or process filter can run code during git.
#[must_use]
pub fn git_content_filters_present(cwd: &Path) -> bool {
    let git = cwd.join(".git");
    let config = if git.is_file() {
        let Ok(text) = std::fs::read_to_string(&git) else {
            return false;
        };
        let Some(dir) = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("gitdir:"))
        else {
            return false;
        };
        PathBuf::from(dir.trim()).join("config")
    } else {
        git.join("config")
    };
    let Ok(text) = std::fs::read_to_string(config) else {
        return false;
    };
    let mut section_filter = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            section_filter = line.to_ascii_lowercase().starts_with("[filter");
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if section_filter
            && (lower.starts_with("clean")
                || lower.starts_with("smudge")
                || lower.starts_with("process"))
        {
            return true;
        }
        if lower.starts_with("filter.")
            && (lower.contains(".clean") || lower.contains(".smudge") || lower.contains(".process"))
        {
            return true;
        }
    }
    false
}

#[must_use]
pub fn command_risk_note(command: &str) -> String {
    let parts: Vec<String> = split_command_segments(command)
        .into_iter()
        .map(|segment| {
            let risk = match command_risk(&segment) {
                CommandRisk::Plain => "plain",
                CommandRisk::EnvAssignment => "env",
                CommandRisk::Exec => "exec",
                CommandRisk::Injection => "injection",
            };
            format!("{risk}:{segment}")
        })
        .collect();
    parts.join("; ")
}

#[must_use]
pub fn denial_grant_note(arguments: &serde_json::Value, workspace: &Path) -> Option<String> {
    let path = arguments
        .get("path")
        .or_else(|| arguments.get("cwd"))
        .and_then(serde_json::Value::as_str)?;
    let blocked = PathBuf::from(path);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let grant = propose_directory_grant(&blocked, workspace, home.as_deref())?;
    Some(format!(
        "A narrower directory grant is {}.",
        grant.display()
    ))
}

/// Index of the first message a mid-turn compaction must keep.
/// The tail includes the latest tool result, its tool call, and the user turn before that call.
#[must_use]
pub fn continuation_keep_index(is_tool_result: &[bool], has_tool_call: &[bool]) -> usize {
    let len = is_tool_result.len().min(has_tool_call.len());
    if len <= 2 {
        return 0;
    }
    let Some(result_at) = is_tool_result[..len]
        .iter()
        .rposition(|is_result| *is_result)
    else {
        return len - 2;
    };
    let mut start = result_at;
    if start > 0 && has_tool_call[start - 1] {
        start -= 1;
    }
    if start > 0 && !is_tool_result[start - 1] && !has_tool_call[start - 1] {
        start -= 1;
    }
    start
}

#[must_use]
pub fn local_compaction_summary<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    let mut summary = String::from("Earlier turns were compacted locally. Topics:");
    for line in lines {
        let topic = line.lines().next().unwrap_or("").trim();
        if topic.is_empty() {
            continue;
        }
        if summary.len() > 2_000 {
            summary.push_str(" …");
            break;
        }
        summary.push('\n');
        summary.push_str(topic.chars().take(160).collect::<String>().as_str());
    }
    summary
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptStep {
    pub phase: String,
    pub prompt: String,
    pub parallel: bool,
    pub when_word: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptPlan {
    pub steps: Vec<ScriptStep>,
    pub budget: u64,
    pub paused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptJournal {
    pub ran: Vec<String>,
    pub paused_at: Option<String>,
}

/// A line-oriented workflow script: `budget N`, `phase Title`, `agent prompt`, `pause`.
pub fn parse_workflow_script(source: &str) -> Result<ScriptPlan, String> {
    let mut budget = WORKFLOW_DEFAULT_BUDGET;
    let mut steps = Vec::new();
    let mut phase = String::new();
    let mut paused = false;
    let mut parallel = false;
    let mut when_word = None;
    let mut repeats = 1u64;
    for raw in source.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("budget ") {
            let parsed = value
                .trim()
                .parse::<u64>()
                .map_err(|_| "workflow budget must be a number".to_string())?;
            if parsed == 0 || parsed > WORKFLOW_MAX_BUDGET {
                return Err(format!("workflow budget must be 1..={WORKFLOW_MAX_BUDGET}"));
            }
            budget = parsed;
            continue;
        }
        if line == "parallel" {
            parallel = true;
            continue;
        }
        if line == "join" {
            parallel = false;
            continue;
        }
        if let Some(word) = line.strip_prefix("when ") {
            let word = word.trim();
            if word.is_empty() || word.contains(char::is_whitespace) {
                return Err("workflow when needs one word".into());
            }
            when_word = Some(word.to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("loop ") {
            let parsed = value
                .trim()
                .parse::<u64>()
                .map_err(|_| "workflow loop must be a number".to_string())?;
            if parsed == 0 || parsed > 4 {
                return Err("workflow loop must be 1..=4".into());
            }
            repeats = parsed;
            continue;
        }
        if let Some(title) = line.strip_prefix("phase ") {
            if steps.len() >= WORKFLOW_MAX_PHASES {
                return Err(format!(
                    "workflow script allows {WORKFLOW_MAX_PHASES} phases"
                ));
            }
            phase = title.trim().to_string();
            if phase.is_empty() {
                return Err("workflow phase needs a title".into());
            }
            continue;
        }
        if let Some(prompt) = line.strip_prefix("agent ") {
            if phase.is_empty() {
                return Err("workflow agent needs a phase".into());
            }
            let copies = repeats;
            repeats = 1;
            for copy in 0..copies {
                if steps.len() >= WORKFLOW_MAX_PHASES {
                    return Err(format!(
                        "workflow script allows {WORKFLOW_MAX_PHASES} phases"
                    ));
                }
                let title = if copies == 1 {
                    phase.clone()
                } else {
                    format!("{phase} {copy}")
                };
                steps.push(ScriptStep {
                    phase: title,
                    prompt: prompt.trim().to_string(),
                    parallel,
                    when_word: when_word.clone(),
                });
            }
            when_word = None;
            continue;
        }
        if line == "pause" {
            paused = true;
            break;
        }
        return Err(format!("unknown workflow script line: {line}"));
    }
    if steps.is_empty() {
        return Err("workflow script needs a phase and an agent".into());
    }
    Ok(ScriptPlan {
        steps,
        budget,
        paused,
    })
}

#[must_use]
pub fn run_script_until_pause(plan: &ScriptPlan) -> ScriptJournal {
    let mut journal = ScriptJournal {
        ran: Vec::new(),
        paused_at: None,
    };
    for (index, step) in plan.steps.iter().enumerate() {
        if index as u64 >= plan.budget {
            journal.paused_at = Some(step.phase.clone());
            break;
        }
        journal.ran.push(step.phase.clone());
    }
    if plan.paused {
        journal.paused_at = journal
            .ran
            .last()
            .cloned()
            .or_else(|| plan.steps.first().map(|step| step.phase.clone()));
    }
    journal
}

#[must_use]
pub fn https_hook_url(command: &str) -> Option<&str> {
    let command = command.trim();
    let rest = command.strip_prefix("https://")?;
    if rest.contains('@') || rest.is_empty() {
        return None;
    }
    let host = rest.split(['/', ':', '?']).next().unwrap_or("");
    if host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.parse::<std::net::IpAddr>().is_ok()
    {
        return None;
    }
    Some(command)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentContract {
    pub permission_mode: String,
    pub prompt_mode: String,
    pub disallowed_tools: Vec<String>,
    pub tool_aliases: Vec<(String, String)>,
    pub bash_timeout_secs: Option<u64>,
    pub completion_tool: Option<String>,
}

impl Default for AgentContract {
    fn default() -> Self {
        Self {
            permission_mode: "default".into(),
            prompt_mode: "extend".into(),
            disallowed_tools: Vec::new(),
            tool_aliases: Vec::new(),
            bash_timeout_secs: None,
            completion_tool: None,
        }
    }
}

#[must_use]
pub fn parse_agent_contract_line(line: &str, contract: &mut AgentContract) -> bool {
    let line = line.trim();
    if let Some(value) = line
        .strip_prefix("permissionMode:")
        .or_else(|| line.strip_prefix("permission_mode:"))
    {
        let mode = value.trim();
        if matches!(mode, "default" | "accept_edits" | "dont_ask" | "plan") {
            contract.permission_mode = mode.to_string();
        }
        return true;
    }
    if let Some(value) = line
        .strip_prefix("disallowedTools:")
        .or_else(|| line.strip_prefix("disallowed_tools:"))
    {
        contract.disallowed_tools = value
            .split(',')
            .map(|tool| tool.trim().to_string())
            .filter(|tool| !tool.is_empty())
            .collect();
        return true;
    }
    if let Some(value) = line
        .strip_prefix("bashTimeoutSecs:")
        .or_else(|| line.strip_prefix("bash_timeout_secs:"))
    {
        contract.bash_timeout_secs = value.trim().parse().ok();
        return true;
    }
    if let Some(value) = line
        .strip_prefix("completionTool:")
        .or_else(|| line.strip_prefix("completion_tool:"))
    {
        let tool = value.trim();
        if !tool.is_empty() {
            contract.completion_tool = Some(tool.to_string());
        }
        return true;
    }
    if let Some(value) = line
        .strip_prefix("promptMode:")
        .or_else(|| line.strip_prefix("prompt_mode:"))
    {
        let mode = value.trim();
        if matches!(mode, "extend" | "full") {
            contract.prompt_mode = mode.to_string();
        }
        return true;
    }
    if let Some(value) = line
        .strip_prefix("toolAliases:")
        .or_else(|| line.strip_prefix("tool_aliases:"))
    {
        contract.tool_aliases = value
            .split(',')
            .filter_map(|pair| {
                let (from, to) = pair.trim().split_once('=')?;
                let from = from.trim();
                let to = to.trim();
                if from.is_empty() || to.is_empty() {
                    None
                } else {
                    Some((from.to_string(), to.to_string()))
                }
            })
            .collect();
        return true;
    }
    false
}

#[must_use]
pub fn canonical_tool_name<'a>(name: &'a str, aliases: &'a [(String, String)]) -> &'a str {
    aliases
        .iter()
        .find(|(from, _)| from == name)
        .map(|(_, to)| to.as_str())
        .unwrap_or(name)
}

#[must_use]
pub fn command_requires_ask(command: &str) -> bool {
    split_command_segments(command)
        .iter()
        .any(|segment| command_floor(segment) == CommandFloor::Ask)
}

#[must_use]
pub fn command_requires_deny(command: &str) -> bool {
    split_command_segments(command)
        .iter()
        .any(|segment| command_floor(segment) == CommandFloor::Deny)
}

#[must_use]
pub fn segment_requires_ask(segment: &str) -> bool {
    command_floor(segment) == CommandFloor::Ask
}

#[must_use]
pub fn compaction_state_note(goal: Option<&str>, todos: &[(&str, &str)]) -> String {
    let mut note = String::new();
    if let Some(goal) = goal.map(str::trim).filter(|goal| !goal.is_empty()) {
        note.push_str("\nGoal: ");
        note.push_str(&goal.chars().take(240).collect::<String>());
    }
    if !todos.is_empty() {
        note.push_str("\nTodos:");
        for (status, content) in todos.iter().take(8) {
            note.push_str("\n- ");
            note.push_str(status);
            note.push_str(": ");
            note.push_str(&content.chars().take(120).collect::<String>());
        }
    }
    note
}

/// `git commit`, `rebase`, `filter-branch`, `reset`, and a force push.
#[must_use]
pub fn git_history_command(segment: &str) -> bool {
    let mut tokens = segment.split_whitespace();
    if tokens.next() != Some("git") {
        return false;
    }
    let mut subcommand = None;
    let mut force = false;
    let mut skip_value = false;
    for token in tokens {
        if token == "--" {
            break;
        }
        if skip_value {
            skip_value = false;
            continue;
        }
        if matches!(token, "-C" | "-c" | "--git-dir" | "--work-tree") {
            skip_value = true;
            continue;
        }
        if token.starts_with('-') {
            if token == "--force"
                || token == "-f"
                || token == "--force-with-lease"
                || token.starts_with("--force=")
            {
                force = true;
            }
            continue;
        }
        if subcommand.is_none() {
            subcommand = Some(token);
        }
    }
    match subcommand {
        Some("commit" | "rebase" | "filter-branch" | "reset") => true,
        Some("push") => force,
        _ => false,
    }
}

/// What a tool continuation does once the transcript is over the context limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuationSummary {
    /// Wait for the compaction model, then sample with that summary.
    UseModel,
    /// The model checkpoint did not start. Seed a local summary and sample.
    SeedLocal,
    /// Under the limit, or the active checkpoint already covers this boundary.
    Keep,
}

#[must_use]
pub fn continuation_summary(
    over_limit: bool,
    covered: bool,
    model_started: bool,
) -> ContinuationSummary {
    if !over_limit || covered {
        ContinuationSummary::Keep
    } else if model_started {
        ContinuationSummary::UseModel
    } else {
        ContinuationSummary::SeedLocal
    }
}

#[must_use]
pub fn model_compaction_prompt(seed: &str) -> String {
    format!(
        "Summarize the earlier turns for the next request. Keep tool names and file paths. Do not invent results.\n\n{seed}"
    )
}

#[must_use]
pub fn read_topic_lines(body: &str, start_line: usize, limit: usize) -> String {
    let start = start_line.max(1);
    let limit = limit.clamp(1, 500);
    body.lines()
        .enumerate()
        .skip(start - 1)
        .take(limit)
        .map(|(index, line)| format!("{}→{line}", index + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

#[must_use]
pub fn topic_manifest(topics: &[(String, String)], budget_bytes: usize) -> String {
    let mut out = String::from("# Memory\n");
    for (title, body) in topics {
        let line = format!("- {title}: {}\n", body.lines().next().unwrap_or(""));
        if out.len() + line.len() > budget_bytes {
            out.push_str("- …\n");
            break;
        }
        out.push_str(&line);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkAuthor {
    Agent,
    External,
}

#[must_use]
pub fn hunk_author(agent_after: &str, disk_now: &str) -> HunkAuthor {
    if agent_after == disk_now {
        HunkAuthor::Agent
    } else {
        HunkAuthor::External
    }
}

/// Definitions and references for `symbol` under `root`, using tree-sitter.
/// Rust, Go, Python, JavaScript, and TypeScript are included.
#[must_use]
pub fn code_graph(root: &Path, symbol: &str) -> Vec<String> {
    if symbol.is_empty() || symbol.len() > 128 || symbol.chars().any(char::is_control) {
        return Vec::new();
    }
    let cache_key = format!("{}:{symbol}", root.display());
    if let Some(hits) = cached_code_hits(&cache_key) {
        return hits;
    }
    let mut hits = Vec::new();
    let mut files = 0usize;
    let mut stamps = Vec::new();
    walk_sources(root, &mut files, &mut |path| {
        if let Ok(modified) = std::fs::metadata(path).and_then(|meta| meta.modified()) {
            stamps.push((path.to_path_buf(), modified));
        }
        if hits.len() >= 32 {
            return;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        if text.len() > 256 * 1024 {
            return;
        }
        let Some(language) = language_for(path) else {
            return;
        };
        collect_symbol(&text, language, path, symbol, &mut hits);
    });
    remember_code_hits(cache_key, stamps, hits.clone());
    hits
}

/// The identifier at a 1-based line and byte column, then the same name scan.
#[must_use]
pub fn code_graph_at(root: &Path, file: &Path, line: usize, column: usize) -> Vec<String> {
    let Some(symbol) = symbol_at(file, line, column) else {
        return Vec::new();
    };
    refresh_definition_index(root);
    let hits = indexed_definitions(&symbol);
    if hits.is_empty() {
        code_graph(root, &symbol)
    } else {
        hits
    }
}

struct FileDefinitions {
    path: PathBuf,
    mtime: std::time::SystemTime,
    items: Vec<(String, u32, String)>,
}

fn definition_index() -> &'static std::sync::Mutex<Vec<FileDefinitions>> {
    static INDEX: std::sync::OnceLock<std::sync::Mutex<Vec<FileDefinitions>>> =
        std::sync::OnceLock::new();
    INDEX.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn refresh_definition_index(root: &Path) {
    let mut files = 0usize;
    let mut seen = Vec::new();
    let mut updates = Vec::new();
    walk_sources(root, &mut files, &mut |path| {
        let Ok(modified) = std::fs::metadata(path).and_then(|meta| meta.modified()) else {
            return;
        };
        seen.push(path.to_path_buf());
        let current = {
            let guard = definition_index()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            guard
                .iter()
                .find(|entry| entry.path == path)
                .map(|entry| entry.mtime)
        };
        if current == Some(modified) {
            return;
        }
        updates.push((path.to_path_buf(), modified, definitions_in_file(path)));
    });
    let mut guard = definition_index()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    guard.retain(|entry| seen.iter().any(|path| path == &entry.path));
    for (path, mtime, items) in updates {
        guard.retain(|entry| entry.path != path);
        guard.push(FileDefinitions { path, mtime, items });
    }
}

fn definitions_in_file(path: &Path) -> Vec<(String, u32, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    if text.len() > 256 * 1024 {
        return Vec::new();
    }
    let Some(language) = language_for(path) else {
        return Vec::new();
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(&text, None) else {
        return Vec::new();
    };
    let bytes = text.as_bytes();
    let mut items = Vec::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if items.len() >= 64 {
            break;
        }
        let kind = node.kind();
        if matches!(
            kind,
            "function_item"
                | "struct_item"
                | "enum_item"
                | "function_definition"
                | "class_definition"
                | "function_declaration"
                | "method_declaration"
                | "type_spec"
                | "class_declaration"
                | "method_definition"
        ) {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(bytes).ok())
            {
                items.push((
                    name.to_string(),
                    u32::try_from(node.start_position().row + 1).unwrap_or(u32::MAX),
                    kind.to_string(),
                ));
            }
        }
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    items
}

fn indexed_definitions(symbol: &str) -> Vec<String> {
    let guard = definition_index()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    guard
        .iter()
        .flat_map(|file| {
            file.items
                .iter()
                .filter(|(name, _, _)| name == symbol)
                .map(|(name, line, kind)| {
                    format!(
                        "{}:{}: definition {kind} {name}",
                        display_path(&file.path),
                        line
                    )
                })
        })
        .take(32)
        .collect()
}

fn symbol_at(file: &Path, line: usize, column: usize) -> Option<String> {
    if line == 0 || column == 0 {
        return None;
    }
    let text = std::fs::read_to_string(file).ok()?;
    if text.len() > 256 * 1024 {
        return None;
    }
    let language = language_for(file)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(&text, None)?;
    let point = tree_sitter::Point {
        row: line - 1,
        column: column - 1,
    };
    let mut node = tree
        .root_node()
        .named_descendant_for_point_range(point, point)?;
    let bytes = text.as_bytes();
    loop {
        if matches!(
            node.kind(),
            "identifier" | "type_identifier" | "field_identifier" | "package_identifier"
        ) {
            return node.utf8_text(bytes).ok().map(str::to_string);
        }
        if let Some(name) = node.child_by_field_name("name") {
            if name.start_position().row <= point.row
                && name.end_position().row >= point.row
                && name.start_position().column <= point.column
                && name.end_position().column >= point.column
            {
                return name.utf8_text(bytes).ok().map(str::to_string);
            }
        }
        node = node.parent()?;
    }
}

struct CodeCacheEntry {
    key: String,
    files: Vec<(PathBuf, std::time::SystemTime)>,
    hits: Vec<String>,
}

fn code_index() -> &'static std::sync::Mutex<Vec<CodeCacheEntry>> {
    static INDEX: std::sync::OnceLock<std::sync::Mutex<Vec<CodeCacheEntry>>> =
        std::sync::OnceLock::new();
    INDEX.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

fn cached_code_hits(key: &str) -> Option<Vec<String>> {
    let guard = code_index()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let entry = guard.iter().find(|entry| entry.key == key)?;
    let fresh = entry.files.iter().all(|(path, seen)| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .as_ref()
            == Some(seen)
    });
    fresh.then(|| entry.hits.clone())
}

fn remember_code_hits(
    key: String,
    files: Vec<(PathBuf, std::time::SystemTime)>,
    hits: Vec<String>,
) {
    let mut guard = code_index()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    guard.retain(|entry| entry.key != key);
    if guard.len() >= 64 {
        guard.remove(0);
    }
    guard.push(CodeCacheEntry { key, files, hits });
}

/// Drop cached hits that mention `path` so the next lookup scans again.
pub fn invalidate_code_index(path: &Path) {
    let needle = path.display().to_string();
    let mut guard = code_index()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    guard.retain(|entry| {
        !entry.hits.iter().any(|hit| hit.contains(&needle))
            && !entry.files.iter().any(|(cached, _)| cached == path)
    });
}

fn walk_sources(root: &Path, files: &mut usize, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if *files > 200 {
            return;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            walk_sources(&path, files, visit);
            continue;
        }
        if language_for(&path).is_none() {
            continue;
        }
        *files += 1;
        visit(&path);
    }
}

fn language_for(path: &Path) -> Option<tree_sitter::Language> {
    let ext = path.extension().and_then(|ext| ext.to_str())?;
    Some(match ext {
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "py" => tree_sitter_python::LANGUAGE.into(),
        "js" | "mjs" | "cjs" => tree_sitter_javascript::LANGUAGE.into(),
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        _ => return None,
    })
}

fn collect_symbol(
    text: &str,
    language: tree_sitter::Language,
    path: &Path,
    symbol: &str,
    hits: &mut Vec<String>,
) {
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {
        return;
    }
    let Some(tree) = parser.parse(text, None) else {
        return;
    };
    let bytes = text.as_bytes();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if hits.len() >= 32 {
            return;
        }
        let kind = node.kind();
        let definition = matches!(
            kind,
            "function_item"
                | "struct_item"
                | "enum_item"
                | "function_definition"
                | "class_definition"
                | "function_declaration"
                | "method_declaration"
                | "type_spec"
                | "class_declaration"
                | "method_definition"
        );
        if definition {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(bytes).ok())
            {
                if name == symbol {
                    hits.push(format!(
                        "{}:{}: definition {kind} {name}",
                        display_path(path),
                        node.start_position().row + 1
                    ));
                }
            }
        } else if matches!(
            kind,
            "identifier" | "type_identifier" | "field_identifier" | "package_identifier"
        ) {
            if let Ok(name) = node.utf8_text(bytes) {
                if name == symbol {
                    hits.push(format!(
                        "{}:{}: reference {name}",
                        display_path(path),
                        node.start_position().row + 1
                    ));
                }
            }
        }
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
}

fn display_path(path: &Path) -> String {
    path.components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect::<PathBuf>()
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grant_stays_specific_and_a_command_splits_outside_quotes() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("app");
        let home = temp.path().join("home");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::create_dir_all(home.join(".local")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let grant =
            propose_directory_grant(&workspace.join("src/new/file.rs"), &workspace, Some(&home));
        assert_eq!(grant, Some(workspace.join("src")));
        assert!(propose_directory_grant(&workspace, &workspace, Some(&home)).is_none());
        assert!(propose_directory_grant(&home, &workspace, Some(&home)).is_none());
        let local =
            propose_directory_grant(&home.join(".local/lib/python"), &workspace, Some(&home));
        assert!(local.is_some_and(|path| path != home.join(".local") && path != home));
        let segments = split_command_segments("FOO=1 ls && echo 'a && b' | wc");
        assert_eq!(segments, vec!["FOO=1 ls", "echo 'a && b'", "wc"]);
        assert_eq!(command_risk("FOO=1 ls"), CommandRisk::EnvAssignment);
        assert_eq!(command_risk("rm -rf build"), CommandRisk::Exec);
        assert!(command_risk_note("rm a && ls").contains("exec:rm a"));
    }

    #[test]
    fn an_unquoted_newline_is_scored_as_its_own_command() {
        assert_eq!(
            split_command_segments("cargo test\nrm -rf build"),
            vec!["cargo test", "rm -rf build"]
        );
        assert_eq!(
            split_command_segments("cargo test\r\nrm -rf build"),
            vec!["cargo test", "rm -rf build"]
        );
        assert_eq!(
            split_command_segments("cargo test\rrm -rf build"),
            vec!["cargo test", "rm -rf build"]
        );
        assert!(command_requires_ask("cargo test\nrm -rf build"));
        assert_eq!(
            split_command_segments("echo \"hello\nworld\""),
            vec!["echo \"hello\nworld\""]
        );
        assert!(!command_requires_ask("echo \"hello\nworld\""));
        assert_eq!(
            split_command_segments("echo '$(grep x f\nrm -rf /)'"),
            vec!["echo '$(grep x f\nrm -rf /)'"]
        );
        assert!(!command_requires_ask("echo '$(grep x f\nrm -rf /)'"));
        assert_eq!(
            split_command_segments("echo \"$(grep x f\nrm -rf /)\""),
            vec!["echo \"$(grep x f", "rm -rf /)\""]
        );
        assert!(command_requires_ask("echo \"$(grep x f\nrm -rf /)\""));
        assert_eq!(
            split_command_segments("echo \"`grep x f\nrm -rf /`\""),
            vec!["echo \"`grep x f", "rm -rf /`\""]
        );
        assert_eq!(
            split_command_segments("cargo test \\\nrm -rf build"),
            vec!["cargo test \\\nrm -rf build"]
        );
        assert!(!command_requires_ask("cargo test \\\nrm -rf build"));
        assert_eq!(
            split_command_segments("echo \\; rm -rf /"),
            vec!["echo \\; rm -rf /"]
        );
        assert!(!command_requires_ask("echo \\; rm -rf /"));
        assert_eq!(
            split_command_segments("echo foo; rm -rf /"),
            vec!["echo foo", "rm -rf /"]
        );
        assert!(command_requires_ask("echo foo; rm -rf /"));
        assert_eq!(
            split_command_segments("echo \\| rm -rf /"),
            vec!["echo \\| rm -rf /"]
        );
        assert_eq!(
            split_command_segments("echo foo | rm -rf /"),
            vec!["echo foo", "rm -rf /"]
        );
        assert!(command_requires_ask("echo foo | rm -rf /"));
        assert_eq!(
            split_command_segments("find . -exec grep needle {} \\; -delete"),
            vec!["find . -exec grep needle {} \\; -delete"]
        );
        assert!(command_writes("echo hi > out"));
        assert!(command_writes("echo hi >> out"));
        assert!(!command_writes("echo 'a > b'"));
        assert!(!command_writes("echo \"a > b\""));
        assert_eq!(
            shell_write_targets(
                "cd /tmp/x && cat > .gitignore",
                std::path::Path::new("/tmp")
            ),
            vec![ShellWriteTarget::Literal(std::path::PathBuf::from(
                "/tmp/x/.gitignore"
            ))]
        );
        assert!(shell_write_targets("echo 'a > b'", std::path::Path::new("/tmp")).is_empty());
        assert!(shell_write_targets("echo hi 2>&1", std::path::Path::new("/tmp")).is_empty());
        assert_eq!(
            shell_write_targets("cat > \"$FILE\"", std::path::Path::new("/tmp")),
            vec![ShellWriteTarget::Unverified]
        );
        assert_eq!(
            shell_write_targets("tee kept.txt", std::path::Path::new("/work")),
            vec![ShellWriteTarget::Literal(std::path::PathBuf::from(
                "/work/kept.txt"
            ))]
        );
        assert!(command_writes("rm file"));
        assert!(command_writes("/bin/rm file"));
        assert!(command_writes("sed -i s/a/b/ file"));
        assert!(!command_writes("sed s/a/b/ file"));
        assert!(!command_writes("ls"));
        assert!(!command_writes("git status"));
        assert!(!command_writes("git diff"));
        assert!(!command_writes("rg pattern"));
        assert!(!command_writes("cargo test"));
        assert!(!command_writes("curl https://example.com"));
        assert!(!command_requires_ask("cat <<'EOF'\nrm -rf /\nEOF"));
        assert!(!command_requires_ask("tee <<'EOF'\nrm -rf /\nEOF"));
        assert!(!command_requires_ask("/bin/cat <<'EOF'\nrm -rf /\nEOF"));
        assert!(command_requires_ask("bash <<'EOF'\nrm -rf /\nEOF"));
        assert!(command_requires_ask("cat <<EOF\nrm -rf /\nEOF"));
        assert!(command_requires_ask("cat <<'EOF' | sh\nrm -rf /\nEOF"));
        assert!(command_requires_ask("ls # '\nrm -rf /"));
        assert_eq!(
            split_command_segments("echo \"cat <<'EOF'\nrm -rf /\nEOF\""),
            vec!["echo \"cat <<'EOF'\nrm -rf /\nEOF\""]
        );
    }

    #[test]
    fn compaction_keeps_the_latest_tool_pair() {
        let results = [false, false, true, false, true];
        let calls = [false, true, false, true, false];
        assert_eq!(continuation_keep_index(&results, &calls), 3);
        assert_eq!(continuation_keep_index(&[false, false], &[false, false]), 0);
        let summary = local_compaction_summary(["first topic\nrest", "second"]);
        assert!(summary.contains("first topic"));
        assert!(summary.contains("second"));
    }

    #[test]
    fn a_script_stops_at_pause_and_rejects_a_large_budget() {
        let plan = parse_workflow_script(
            "budget 2\nphase Look\nagent read the tree\npause\nphase Later\nagent skip",
        )
        .unwrap();
        assert!(plan.paused);
        assert_eq!(plan.budget, 2);
        assert_eq!(plan.steps.len(), 1);
        let journal = run_script_until_pause(&plan);
        assert_eq!(journal.ran, vec!["Look".to_string()]);
        assert_eq!(journal.paused_at.as_deref(), Some("Look"));
        assert!(parse_workflow_script("budget 2000\nphase A\nagent go").is_err());
    }

    #[test]
    fn hook_urls_memory_topics_and_hunks_follow_the_contract() {
        assert_eq!(
            https_hook_url("https://hooks.example.com/ca"),
            Some("https://hooks.example.com/ca")
        );
        assert!(https_hook_url("https://127.0.0.1/hook").is_none());
        assert!(https_hook_url("http://hooks.example.com/ca").is_none());
        let body = "alpha\nbeta\ngamma";
        assert_eq!(read_topic_lines(body, 2, 1), "2→beta");
        let manifest = topic_manifest(&[("tools".into(), "use memory_get\nmore".into())], 80);
        assert!(manifest.contains("tools"));
        assert_eq!(hunk_author("same", "same"), HunkAuthor::Agent);
        assert_eq!(hunk_author("agent", "disk"), HunkAuthor::External);
        let mut contract = AgentContract::default();
        assert!(parse_agent_contract_line(
            "permissionMode: plan",
            &mut contract
        ));
        assert!(parse_agent_contract_line(
            "disallowedTools: write_file, bash",
            &mut contract
        ));
        assert!(parse_agent_contract_line(
            "completionTool: complete_task",
            &mut contract
        ));
        assert_eq!(contract.permission_mode, "plan");
        assert_eq!(contract.disallowed_tools, vec!["write_file", "bash"]);
        assert_eq!(contract.completion_tool.as_deref(), Some("complete_task"));
        assert!(parse_agent_contract_line("promptMode: full", &mut contract));
        assert!(parse_agent_contract_line(
            "toolAliases: Bash=run_command, Read=read_file",
            &mut contract
        ));
        assert_eq!(contract.prompt_mode, "full");
        assert_eq!(
            canonical_tool_name("Bash", &contract.tool_aliases),
            "run_command"
        );
        assert!(command_requires_ask("ls && rm build"));
        assert!(command_requires_ask("FOO=1 ls"));
        assert!(command_requires_ask("command rm build"));
        assert!(command_requires_ask("bash -c 'rm -rf /tmp/x'"));
        assert!(command_requires_ask("echo $(curl https://example.com)"));
        assert!(command_requires_deny("LD_PRELOAD=/tmp/x.so ls"));
        assert!(command_requires_deny("env PATH=/tmp ls"));
        assert!(!command_requires_ask("NO_COLOR=1 ls"));
        assert!(command_requires_ask("git commit -m update"));
        assert!(command_requires_ask("git -C repo push --force origin main"));
        assert!(!command_requires_ask("git status"));
        assert!(!command_requires_ask("ls src"));
        let search = "find /Volumes/projects -maxdepth 4 -name '.git' 2>/dev/null | head -40; echo '---remotes mentioning ajzrva/ca---'; find /Volumes/projects -maxdepth 4 -path '*/.git/config' 2>/dev/null -exec grep -H -E 'ajzrva|/ca' {} \\; | head -40";
        assert!(!command_requires_ask(search), "{search}");
        assert!(!command_requires_ask("find . -exec grep -H needle {} \\;"));
        assert!(!command_requires_ask("find . -execdir grep needle {} +"));
        assert!(command_requires_ask("find . -exec rm {} \\;"));
        assert!(command_requires_ask("find . -exec touch marker ';'"));
        assert!(command_requires_ask(
            "find . -exec bash -c 'rm -rf /tmp/x' {} \\;"
        ));
        assert!(command_requires_ask("find . -exec /bin/grep needle {} \\;"));
        assert!(command_requires_deny(
            "find . -exec env LD_PRELOAD=/tmp/x.so grep needle {} \\;"
        ));
        assert!(command_requires_ask("find . -ok grep needle {} ';'"));
        assert!(command_requires_ask("find . -delete"));
        assert!(command_requires_ask(
            "find . -exec grep needle {} \\; -delete"
        ));
        assert!(command_requires_ask("find . -exec"));
        assert!(command_requires_ask("find . -exec env \\;"));
        assert!(command_requires_ask("find . -exec command \\;"));
        assert_eq!(
            continuation_summary(true, false, true),
            ContinuationSummary::UseModel
        );
        assert_eq!(
            continuation_summary(true, false, false),
            ContinuationSummary::SeedLocal
        );
        assert_eq!(
            continuation_summary(true, true, false),
            ContinuationSummary::Keep
        );
        assert!(model_compaction_prompt("seed").contains("seed"));
    }

    #[test]
    fn the_code_graph_finds_a_rust_function_without_a_language_server() {
        let root = std::env::temp_dir().join(format!("ca-graph-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "fn kept_symbol() {}\nfn other() { kept_symbol(); }\n",
        )
        .unwrap();
        let hits = code_graph(&root, "kept_symbol");
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            hits.iter()
                .any(|hit| hit.contains("definition") && hit.contains("kept_symbol")),
            "{hits:?}"
        );
        assert!(hits.iter().any(|hit| hit.contains("reference")), "{hits:?}");
    }

    #[test]
    fn the_code_graph_finds_a_go_method_without_a_language_server() {
        let root = std::env::temp_dir().join(format!("ca-graph-go-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("sample.go"),
            "package demo\n\ntype Server struct{}\n\nfunc (s *Server) KeptSymbol() int { return 1 }\n\nfunc Other(s *Server) int {\n    return s.KeptSymbol()\n}\n",
        )
        .unwrap();
        let hits = code_graph(&root, "KeptSymbol");
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            hits.iter()
                .any(|hit| hit.contains("definition") && hit.contains("method_declaration")),
            "{hits:?}"
        );
        assert!(hits.iter().any(|hit| hit.contains("reference")), "{hits:?}");
    }

    #[test]
    fn a_position_lookup_follows_the_identifier_and_a_newer_mtime_rescans() {
        let root = std::env::temp_dir().join(format!("ca-graph-pos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/lib.rs");
        std::fs::write(&file, "fn kept_symbol() {}\n").unwrap();
        let hits = code_graph_at(&root, &file, 1, 4);
        assert!(
            hits.iter()
                .any(|hit| hit.contains("definition") && hit.contains("kept_symbol")),
            "{hits:?}"
        );
        std::fs::write(&file, "fn other_symbol() {}\n").unwrap();
        let refreshed = std::fs::File::options().write(true).open(&file).unwrap();
        refreshed
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        drop(refreshed);
        let again = code_graph(&root, "kept_symbol");
        let _ = std::fs::remove_dir_all(&root);
        assert!(again.is_empty(), "{again:?}");
    }

    #[test]
    fn git_content_filters_block_unattended_git() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".git")).unwrap();
        std::fs::write(
            temp.path().join(".git/config"),
            "[filter \"lfs\"]\n\tclean = git-lfs clean\n",
        )
        .unwrap();
        assert!(git_content_filters_present(temp.path()));
        let plain = tempfile::tempdir().unwrap();
        assert!(!git_content_filters_present(plain.path()));
    }

    #[test]
    fn a_parallel_script_keeps_both_branches_until_join() {
        let plan = parse_workflow_script(
            "budget 4\nphase Look\nagent read\nparallel\nphase Left\nagent left\nphase Right\nagent right\njoin\nwhen left\nphase After\nagent finish",
        )
        .unwrap();
        assert_eq!(plan.steps.len(), 4);
        assert!(plan.steps[1].parallel);
        assert!(plan.steps[2].parallel);
        assert_eq!(plan.steps[3].when_word.as_deref(), Some("left"));
    }
}
