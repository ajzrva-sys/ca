//! V1 JSON-value identities keep the original sorted object encoding even when
//! the host enables insertion order. Typed struct field order stays unchanged.
use anyhow::Result;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) fn digest_value(value: &Value) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(value_bytes(value)?)))
}

pub(crate) fn value_bytes(value: &Value) -> Result<Vec<u8>> {
    let mut value = value.clone();
    value.sort_all_objects();
    Ok(serde_json::to_vec(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_values_keep_legacy_sorted_bytes_with_insertion_order_enabled() -> Result<()> {
        let value = serde_json::json!({"z":{"second":2,"first":1},"a":[{"y":2,"x":1}]});
        let legacy = br#"{"a":[{"x":1,"y":2}],"z":{"first":1,"second":2}}"#;
        assert_eq!(
            digest_value(&value)?,
            format!("{:x}", Sha256::digest(legacy))
        );
        Ok(())
    }
}
