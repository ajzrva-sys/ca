//! Soft hydration of local-model JSON before strict schema deserialize.
//!
//! Local OpenAI-compat servers often emit `null` for optional strings or
//! invent enum strings. Hydration coerces the common failures so Worker/Planner
//! parses can succeed without inventing authority fields. Worker `task_id` is
//! stamped from Rust-known context via [`stamp_known_task_id`], not invented.

use serde_json::{Map, Value};

/// Known string fields where JSON `null` should become `""` (or stay omitable).
const NULL_TO_EMPTY_STRING: &[&str] = &[
    "summary",
    "blocker",
    "title",
    "objective",
    "notes",
    "reason",
    "source",
    "id",
    "program",
    "kind",
    "status",
    "cwd",
    "commit_message",
    "request_id",
];

/// Map invented review/automation enum labels to Rust-owned values.
fn map_enum_string(key: &str, value: &str) -> Option<&'static str> {
    let v = value.trim().to_ascii_lowercase();
    match key {
        "review_tier" => match v.as_str() {
            "automated" | "auto" | "rust" => Some("rust_only"),
            "ai_review" | "review_on_failure" => Some("ai_review_on_failure"),
            "always_review" => Some("always_ai_review"),
            "human" | "human_only" => Some("human_review"),
            _ => None,
        },
        "automation_level" => match v.as_str() {
            "automated" | "auto" => Some("rust_only"),
            "ai_worker" | "worker" => Some("ai_worker_rust_review"),
            _ => None,
        },
        "failure_policy" => match v.as_str() {
            "continue" | "collect" => Some("collect"),
            "stop" | "fail" => Some("stop"),
            _ => None,
        },
        _ => None,
    }
}

/// Mutate a JSON value tree in place for local-provider tolerance.
pub fn hydrate_local_role_json(value: &mut Value) {
    match value {
        Value::Object(map) => hydrate_object(map),
        Value::Array(items) => {
            for item in items {
                hydrate_local_role_json(item);
            }
        }
        _ => {}
    }
}

fn hydrate_object(map: &mut Map<String, Value>) {
    let keys: Vec<String> = map.keys().cloned().collect();
    for key in keys {
        let Some(val) = map.get_mut(&key) else {
            continue;
        };
        if val.is_null() && NULL_TO_EMPTY_STRING.contains(&key.as_str()) {
            if key == "cwd" {
                // Optional cwd: drop null so Option<String> deserializes cleanly.
                map.remove(&key);
                continue;
            }
            *val = Value::String(String::new());
            continue;
        }
        if let Value::String(s) = val {
            if let Some(mapped) = map_enum_string(&key, s) {
                *s = mapped.to_string();
            }
        } else {
            hydrate_local_role_json(val);
        }
    }
}

/// Parse JSON text, hydrate, return Value (for further typed deserialize).
pub fn parse_and_hydrate_role_json(raw: &str) -> anyhow::Result<Value> {
    let trimmed = raw.trim();
    let mut value: Value = if let Ok(v) = serde_json::from_str(trimmed) {
        v
    } else if let Some(fenced) = extract_json_fence(trimmed) {
        serde_json::from_str(&fenced)?
    } else {
        let start = trimmed
            .find('{')
            .ok_or_else(|| anyhow::anyhow!("response did not contain JSON object"))?;
        let end = trimmed
            .rfind('}')
            .ok_or_else(|| anyhow::anyhow!("response did not contain complete JSON object"))?;
        serde_json::from_str(&trimmed[start..=end])?
    };
    hydrate_local_role_json(&mut value);
    Ok(value)
}

/// Stamp Rust-known `task_id` onto hydrated Worker JSON.
///
/// Local models frequently omit `task_id` even when they otherwise produce a
/// usable Worker object. Rust already knows the executing task id; stamping it
/// preserves authority instead of inventing a new one.
pub fn stamp_known_task_id(value: &mut Value, task_id: &str) {
    if task_id.trim().is_empty() {
        return;
    }
    if let Value::Object(map) = value {
        map.insert(
            "task_id".to_string(),
            Value::String(task_id.trim().to_string()),
        );
    }
}

fn extract_json_fence(s: &str) -> Option<String> {
    let marker = "```json";
    let start = s.find(marker)? + marker.len();
    let rest = &s[start..];
    let end = rest.find("```")?;
    Some(rest[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn null_blocker_and_cwd_hydrate() {
        let mut v = json!({
            "blocker": null,
            "summary": null,
            "handoff_to_rust": [{ "cwd": null, "program": "git", "args": [] }]
        });
        hydrate_local_role_json(&mut v);
        assert_eq!(v["blocker"], "");
        assert_eq!(v["summary"], "");
        assert!(v["handoff_to_rust"][0].get("cwd").is_none());
    }

    #[test]
    fn maps_automated_review_tier() {
        let mut v = json!({ "review_tier": "automated" });
        hydrate_local_role_json(&mut v);
        assert_eq!(v["review_tier"], "rust_only");
    }

    #[test]
    fn stamp_known_task_id_fills_and_overwrites() {
        let mut missing = json!({ "status": "done" });
        stamp_known_task_id(&mut missing, "MLX-DOG-001");
        assert_eq!(missing["task_id"], "MLX-DOG-001");

        let mut wrong = json!({ "task_id": "OTHER", "status": "done" });
        stamp_known_task_id(&mut wrong, "MLX-DOG-001");
        assert_eq!(wrong["task_id"], "MLX-DOG-001");
    }
}
