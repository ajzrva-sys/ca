//! Bounds for an MCP elicitation form. A server that keeps drawing refusals is cut off.

pub const MAX_ELICIT_FIELDS: usize = 16;
pub const MAX_TITLE_CHARS: usize = 120;
pub const MAX_SCHEMA_BYTES: usize = 8 * 1024;
pub const MAX_REFUSAL_NOTES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitField {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElicitForm {
    pub title: String,
    pub fields: Vec<ElicitField>,
    pub schema_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Refuse { note: String },
    CutOff,
}

/// Allow a form inside the caps. Refuse with a note, or cut the server off after
/// `MAX_REFUSAL_NOTES` earlier refusals.
pub fn decide(server_id: &str, form: &ElicitForm, prior_refusals: usize) -> Decision {
    if prior_refusals >= MAX_REFUSAL_NOTES {
        return Decision::CutOff;
    }
    if let Some(note) = bound_note(server_id, form) {
        return Decision::Refuse { note };
    }
    Decision::Allow
}

fn bound_note(server_id: &str, form: &ElicitForm) -> Option<String> {
    if form.fields.len() > MAX_ELICIT_FIELDS {
        return Some(format!(
            "server {server_id} sent {} fields; limit is {MAX_ELICIT_FIELDS}",
            form.fields.len()
        ));
    }
    let title_chars = form.title.chars().count();
    if title_chars > MAX_TITLE_CHARS {
        return Some(format!(
            "server {server_id} title is {title_chars} characters; limit is {MAX_TITLE_CHARS}"
        ));
    }
    if form.schema_bytes > MAX_SCHEMA_BYTES {
        return Some(format!(
            "server {server_id} schema is {} bytes; limit is {MAX_SCHEMA_BYTES}",
            form.schema_bytes
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(fields: usize, title: &str, schema_bytes: usize) -> ElicitForm {
        ElicitForm {
            title: title.to_string(),
            fields: (0..fields)
                .map(|index| ElicitField {
                    name: format!("field-{index}"),
                })
                .collect(),
            schema_bytes,
        }
    }

    #[test]
    fn a_normal_form_is_allowed() {
        let form = form(2, "Pick a value", 128);
        assert_eq!(decide("docs", &form, 0), Decision::Allow);
    }

    #[test]
    fn too_many_fields_is_refused() {
        let form = form(MAX_ELICIT_FIELDS + 1, "Too wide", 128);
        match decide("docs", &form, 0) {
            Decision::Refuse { note } => {
                assert!(note.contains("docs"), "{note}");
                assert!(note.contains(&MAX_ELICIT_FIELDS.to_string()), "{note}");
            }
            other => panic!("expected refuse, got {other:?}"),
        }
    }

    #[test]
    fn the_refusal_after_the_cap_is_cut_off() {
        let form = form(MAX_ELICIT_FIELDS + 4, "Again", 128);
        assert_eq!(decide("docs", &form, MAX_REFUSAL_NOTES), Decision::CutOff);
        assert_eq!(
            decide("docs", &form, MAX_REFUSAL_NOTES + 1),
            Decision::CutOff
        );
    }
}
