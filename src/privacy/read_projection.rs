//! Keep projected native reads within their original result budget.

use super::Context;
use serde::{Deserialize, Serialize};

pub(super) const MAX_READ_BYTES: usize = 64 * 1024;
// The validated opaque token grammar is at most 77 ASCII bytes.
const TOKEN_WINDOW: usize = 96;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Footer {
    pub text: String,
    pub path: String,
    pub max_bytes: usize,
}

pub(super) fn project(
    context: &Context,
    selected: &str,
    footer: Option<&Footer>,
) -> Result<String, String> {
    let (footer, maximum) = if let Some(footer) = footer {
        if footer.max_bytes == 0 || footer.max_bytes > MAX_READ_BYTES {
            return Err("Protected read has an invalid result bound".into());
        }
        // This footer was emitted by the native reader. Its counts and range
        // controls are public structure; only its source pathname is private.
        let path = context.mask_known(&footer.path)?;
        let raw = serde_json::to_string(&footer.path).map_err(|_| "Read path cannot be encoded")?;
        let replacement =
            serde_json::to_string(&path).map_err(|_| "Read alias cannot be encoded")?;
        (footer.text.replace(&raw, &replacement), footer.max_bytes)
    } else {
        (String::new(), MAX_READ_BYTES)
    };
    let budget = maximum
        .checked_sub(footer.len())
        .ok_or("Protected read footer exceeds its result bound")?;
    let mut output = truncate(selected, budget)?;
    output.push_str(&footer);
    Ok(output)
}

fn truncate(text: &str, maximum: usize) -> Result<String, String> {
    if text.len() <= maximum {
        return Ok(text.to_string());
    }
    let mut retained = maximum.saturating_sub(64);
    loop {
        let head = safe_cut(text, retained.div_ceil(2), false)?;
        let tail = safe_cut(text, text.len().saturating_sub(retained / 2), true)?;
        let omitted = tail.saturating_sub(head);
        let marker = format!("\n[… {omitted} bytes omitted …]\n");
        let length = head + marker.len() + text.len().saturating_sub(tail);
        if length <= maximum {
            return Ok(format!("{}{}{}", &text[..head], marker, &text[tail..]));
        }
        if retained == 0 {
            return Err("Protected read has no room for its omission marker".into());
        }
        retained = retained.saturating_sub(length.saturating_sub(maximum));
    }
}

fn safe_cut(text: &str, requested: usize, tail: bool) -> Result<usize, String> {
    let mut cut = requested.min(text.len());
    if tail {
        while cut < text.len() && !text.is_char_boundary(cut) {
            cut += 1;
        }
    } else {
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
    }
    let mut start = cut.saturating_sub(TOKEN_WINDOW);
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = cut
        .saturating_add(super::store::TOKEN_PREFIX.len())
        .min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    for (offset, _) in text[start..end].match_indices(super::store::TOKEN_PREFIX) {
        let open = start + offset;
        if open >= cut {
            break;
        }
        let body = open + super::store::TOKEN_PREFIX.len();
        let mut bound = open.saturating_add(TOKEN_WINDOW).min(text.len());
        while !text.is_char_boundary(bound) {
            bound -= 1;
        }
        let close = text[body..bound]
            .find(super::store::TOKEN_SUFFIX)
            .map(|offset| body + offset + super::store::TOKEN_SUFFIX.len())
            .ok_or("Protected read contains an incomplete private reference")?;
        if cut < close {
            return Ok(if tail { close } else { open });
        }
    }
    Ok(cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn privacy_native_minified_reads_remain_capped_with_exact_receipts_and_footer() {
        for (extension, line) in [
            ("js", "const WidgetCanary123='SecretCanary456';"),
            ("html", "<div class='WidgetCanary123'>SecretCanary456</div>"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let context = Context::open(
                root.path().into(),
                root.path().into(),
                uuid::Uuid::new_v4(),
                None,
            )
            .unwrap();
            context.allow_ephemeral();
            let mut policy = context.policy();
            policy.enabled = true;
            context.save_policy(policy).unwrap();
            let path = root
                .path()
                .join(format!("PrivateFilenameCanary.{extension}"));
            let source = format!("{}\n{}\n", line.repeat(4000), line);
            std::fs::write(&path, source).unwrap();
            let workspace = crate::workspace::WorkspaceContext::discover(root.path()).unwrap();
            let args = serde_json::json!({"path":path,"start_line":1,"end_line":1});
            let execution = crate::privacy::scope(
                Some(context.clone()),
                crate::agent::tools::execute_prepared_tool_in_workspace_policy_owned(
                    "read_file",
                    &args,
                    &workspace,
                    5,
                    crate::sandbox::SandboxMode::PolicyOnly,
                    false,
                    crate::jobs::JobOwner::new(Some(context.session_id)),
                ),
            )
            .await;
            assert!(execution.is_success(), "{}", execution.model_text());
            let raw = execution.model_text();
            assert!(raw.len() <= MAX_READ_BYTES);
            let origin = execution.source_for(raw).unwrap();
            let sent = context.project_retrieved(raw, Some(&origin)).unwrap();
            assert!(sent.len() <= MAX_READ_BYTES);
            assert!(sent.contains("bytes omitted"));
            assert!(sent.contains("Showing lines 1-1 of 2"));
            assert!(sent.contains("Omitted lines: 0 before, 1 after"));
            assert!(sent.contains("Next range:"));
            for private in [
                "WidgetCanary123",
                "SecretCanary456",
                "PrivateFilenameCanary",
            ] {
                assert!(!sent.contains(private));
            }
            context
                .state
                .lock()
                .unwrap()
                .registry
                .tokens_in(&sent)
                .unwrap();
        }
    }

    #[test]
    fn privacy_read_cuts_never_split_alias_prefix_body_suffix_or_unicode() {
        let token = "__CA_PRIVATE_1234abcd_IDENTIFIER_123456__";
        let text = format!("α{token}β");
        let start = "α".len();
        let end = start + token.len();
        for cut in start + 1..end {
            assert_eq!(safe_cut(&text, cut, false).unwrap(), start);
            assert_eq!(safe_cut(&text, cut, true).unwrap(), end);
        }
        assert_eq!(safe_cut(&text, 1, false).unwrap(), 0);
        assert_eq!(safe_cut(&text, 1, true).unwrap(), start);
    }
}
