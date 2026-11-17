//! Mid-turn steer envelope.
//!
//! User text is cut at [`STEER_CHAR_LIMIT`] on a char boundary. Cancel uses the
//! same envelope plus an interrupt note.

/// Maximum user text kept in a steer or cancel envelope, in Unicode scalars.
pub const STEER_CHAR_LIMIT: usize = 25_000;

pub const WORKING_NOTE: &str = "The user sent a message while you were working.";

pub const CLOSING_NOTE: &str = "Answer the user first, then finish the unfinished work.";

pub const INTERRUPT_NOTE: &str = "The user interrupted the current work.";

/// `text` cut to `max_chars` scalars. The cut is always a UTF-8 char boundary.
#[must_use]
pub fn truncate_chars(text: &str, max_chars: usize) -> &str {
    for (count, (index, _)) in text.char_indices().enumerate() {
        if count == max_chars {
            return &text[..index];
        }
    }
    text
}

/// Wrap `text` as a mid-turn user message. `cancel` adds [`INTERRUPT_NOTE`].
#[must_use]
pub fn steer_envelope(text: &str, cancel: bool) -> String {
    let body = truncate_chars(text, STEER_CHAR_LIMIT);
    let mut out = String::with_capacity(body.len() + 160);
    out.push_str(WORKING_NOTE);
    out.push_str("\n\n");
    if cancel {
        out.push_str(INTERRUPT_NOTE);
        out.push_str("\n\n");
    }
    out.push_str(body);
    out.push_str("\n\n");
    out.push_str(CLOSING_NOTE);
    out
}

/// Steer envelope for a user message that arrived during the turn.
#[must_use]
pub fn wrap_steer(text: &str) -> String {
    steer_envelope(text, false)
}

/// Same envelope as [`wrap_steer`], plus the interrupt note.
#[must_use]
pub fn wrap_cancel(text: &str) -> String {
    steer_envelope(text, true)
}

#[cfg(test)]
mod tests {
    use super::{
        truncate_chars, wrap_cancel, wrap_steer, CLOSING_NOTE, INTERRUPT_NOTE, STEER_CHAR_LIMIT,
        WORKING_NOTE,
    };

    #[test]
    fn short_note_is_wrapped() {
        let text = wrap_steer("check the lock order");
        assert!(text.contains(WORKING_NOTE));
        assert!(text.contains("check the lock order"));
        assert!(text.contains(CLOSING_NOTE));
        assert!(!text.contains(INTERRUPT_NOTE));
        assert!(text.find(WORKING_NOTE) < text.find("check the lock order"));
        assert!(text.find("check the lock order") < text.find(CLOSING_NOTE));
    }

    #[test]
    fn long_text_is_cut_on_a_char_boundary() {
        let body = "a".repeat(STEER_CHAR_LIMIT + 40);
        let text = wrap_steer(&body);
        let kept = "a".repeat(STEER_CHAR_LIMIT);
        assert!(text.contains(&kept));
        assert!(!text.contains(&"a".repeat(STEER_CHAR_LIMIT + 1)));
        assert_eq!(
            truncate_chars(&body, STEER_CHAR_LIMIT).chars().count(),
            STEER_CHAR_LIMIT
        );
        assert!(body.is_char_boundary(truncate_chars(&body, STEER_CHAR_LIMIT).len()));
    }

    #[test]
    fn multibyte_char_is_not_split() {
        let unit = "你";
        assert_eq!(unit.len(), 3);
        let body = unit.repeat(STEER_CHAR_LIMIT + 1);
        assert!(!body.is_char_boundary(STEER_CHAR_LIMIT));
        let kept = unit.repeat(STEER_CHAR_LIMIT);
        assert_eq!(truncate_chars(&body, STEER_CHAR_LIMIT), kept.as_str());
        assert!(kept.is_char_boundary(kept.len()));
        assert!(std::str::from_utf8(kept.as_bytes()).is_ok());
        let text = wrap_steer(&body);
        assert!(text.contains(&kept));
        assert!(!text.contains(&body));
    }

    #[test]
    fn cancel_includes_the_interrupt_note() {
        let text = wrap_cancel("stop and listen");
        assert!(text.contains(WORKING_NOTE));
        assert!(text.contains(INTERRUPT_NOTE));
        assert!(text.contains("stop and listen"));
        assert!(text.contains(CLOSING_NOTE));
        assert!(text.contains("interrupt"));
    }
}
