use serde_json::{Map, Value};

/// Return a recursively key-sorted JSON value.
///
/// CA historically used serde_json's sorted map representation. Some protocol
/// SDKs enable serde_json's process-wide `preserve_order` feature, so stable
/// user-facing and persisted bytes must sort explicitly at their boundary.
pub fn sorted_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(key, _)| *key);
            let mut sorted = Map::new();
            for (key, value) in entries {
                sorted.insert(key.clone(), sorted_value(value));
            }
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.iter().map(sorted_value).collect()),
        scalar => scalar.clone(),
    }
}

pub fn to_string(value: &Value) -> Result<String, serde_json::Error> {
    serde_json::to_string(&sorted_value(value))
}

pub fn to_string_pretty(value: &Value) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&sorted_value(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_serializer_sorts_nested_objects_without_reordering_arrays() {
        let value = serde_json::json!({
            "z": {"b": 2, "a": 1},
            "a": [{"y": 2, "x": 1}, 3]
        });
        assert_eq!(
            to_string(&value).unwrap(),
            r#"{"a":[{"x":1,"y":2},3],"z":{"a":1,"b":2}}"#
        );
    }
}
