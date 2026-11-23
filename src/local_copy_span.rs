use std::future::Future;

pub const EDGE_FLOOR: usize = 64;

tokio::task_local! {
    static LOCAL_ROUTE: bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyDecision {
    Copied {
        content: String,
        copied_bytes: usize,
    },
    Generate,
}

pub async fn scope<T>(enabled: bool, future: impl Future<Output = T>) -> T {
    LOCAL_ROUTE.scope(enabled, future).await
}

pub fn route_enabled() -> bool {
    LOCAL_ROUTE.try_with(|enabled| *enabled).unwrap_or(false)
}

pub fn decide(
    local: bool,
    read_in_full: bool,
    content: &str,
    old_string: &str,
    new_string: &str,
) -> CopyDecision {
    if !local || !read_in_full || old_string.is_empty() || old_string == new_string {
        return CopyDecision::Generate;
    }
    let Some((start, end)) = unique_span(content, old_string) else {
        return CopyDecision::Generate;
    };
    let old = &content[start..end];
    let prefix = shared_prefix(old, new_string);
    let suffix = shared_suffix(old, new_string, prefix);
    if prefix.saturating_add(suffix) < EDGE_FLOOR {
        return CopyDecision::Generate;
    }
    let middle_end = new_string.len().saturating_sub(suffix);
    if middle_end < prefix
        || !new_string.is_char_boundary(prefix)
        || !new_string.is_char_boundary(middle_end)
    {
        return CopyDecision::Generate;
    }
    let mut applied = String::with_capacity(
        content.len() - (end - start) + prefix + (middle_end - prefix) + suffix,
    );
    applied.push_str(&content[..start]);
    applied.push_str(&content[start..start + prefix]);
    applied.push_str(&new_string[prefix..middle_end]);
    if suffix > 0 {
        applied.push_str(&content[end - suffix..end]);
    }
    applied.push_str(&content[end..]);
    CopyDecision::Copied {
        content: applied,
        copied_bytes: prefix + suffix,
    }
}

fn unique_span(content: &str, needle: &str) -> Option<(usize, usize)> {
    let start = content.find(needle)?;
    let end = start + needle.len();
    if content[end..].contains(needle) {
        None
    } else {
        Some((start, end))
    }
}

fn shared_prefix(left: &str, right: &str) -> usize {
    let bytes = left
        .as_bytes()
        .iter()
        .zip(right.as_bytes())
        .take_while(|(left_byte, right_byte)| left_byte == right_byte)
        .count();
    char_boundary_at_or_before(left, bytes)
}

fn shared_suffix(left: &str, right: &str, prefix: usize) -> usize {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let max = left_bytes
        .len()
        .saturating_sub(prefix)
        .min(right_bytes.len().saturating_sub(prefix));
    let mut size = 0;
    while size < max
        && left_bytes[left_bytes.len() - 1 - size] == right_bytes[right_bytes.len() - 1 - size]
    {
        size += 1;
    }
    while size > 0
        && (!left.is_char_boundary(left.len() - size)
            || !right.is_char_boundary(right.len() - size))
    {
        size -= 1;
    }
    size
}

fn char_boundary_at_or_before(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (String, String, String) {
        let edge = "x".repeat(80);
        let old = format!("{edge}ONE{edge}");
        let new = format!("{edge}TWO{edge}");
        let mut lines = Vec::new();
        for index in 0..2_000 {
            if index == 100 {
                lines.push(old.clone());
            } else {
                lines.push(format!("line {index} stays"));
            }
        }
        (lines.join("\n"), old, new)
    }

    #[test]
    fn local_copy_span_keeps_the_read_edges_and_a_cloud_call_generates() {
        let (content, old, new) = sample();
        let decision = decide(true, true, &content, &old, &new);
        let CopyDecision::Copied {
            content: applied,
            copied_bytes,
        } = decision
        else {
            panic!("expected a copied span");
        };
        assert!(copied_bytes >= EDGE_FLOOR);
        assert!(applied.contains("TWO"));
        assert!(!applied.contains("ONE"));
        assert!(applied.contains("line 0 stays"));
        assert!(applied.contains("line 1999 stays"));
        assert_eq!(applied.matches("line ").count(), 1_999);
        assert!(matches!(
            decide(false, true, &content, &old, &new),
            CopyDecision::Generate
        ));
        let duplicated = format!("{content}\n{old}");
        assert!(matches!(
            decide(true, true, &duplicated, &old, &new),
            CopyDecision::Generate
        ));
        assert!(matches!(
            decide(true, false, &content, &old, &new),
            CopyDecision::Generate
        ));
    }
}
