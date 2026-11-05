//! Preserve the actual tool interface while withholding private argument data.

use super::Context;
use serde_json::Value;

const MAX_DEPTH: usize = 64;

#[derive(Clone, Copy)]
struct Schema<'a> {
    root: &'a Value,
    node: &'a Value,
    tool: &'static str,
    mode_control: bool,
}

impl<'a> Schema<'a> {
    fn public_value(self, value: &Value) -> bool {
        fn accepts(root: &Value, node: &Value, value: &Value, depth: usize) -> bool {
            if depth > MAX_DEPTH {
                return false;
            }
            node.get("const") == Some(value)
                || node
                    .get("enum")
                    .and_then(Value::as_array)
                    .is_some_and(|items| items.contains(value))
                || node
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|r| r.strip_prefix('#'))
                    .and_then(|reference| root.pointer(reference))
                    .is_some_and(|target| accepts(root, target, value, depth + 1))
                || ["anyOf", "oneOf", "allOf"].iter().any(|operator| {
                    node.get(*operator)
                        .and_then(Value::as_array)
                        .is_some_and(|items| {
                            items
                                .iter()
                                .any(|item| accepts(root, item, value, depth + 1))
                        })
                })
        }
        (self.mode_control
            && value
                .as_str()
                .is_some_and(|text| matches!(text, "agent" | "plan" | "ask")))
            || accepts(self.root, self.node, value, 0)
    }

    fn field(self, name: &str) -> Option<Self> {
        schema_member(self.root, self.node, "properties", Some(name), 0).map(|node| Self {
            root: self.root,
            node,
            tool: self.tool,
            mode_control: self.tool == "switch_mode"
                && std::ptr::eq(self.node, self.root)
                && name == "mode",
        })
    }

    fn additional(self) -> Option<Self> {
        schema_member(self.root, self.node, "additionalProperties", None, 0)
            .filter(|node| node.is_object())
            .map(|node| Self {
                root: self.root,
                node,
                tool: self.tool,
                mode_control: false,
            })
    }

    fn item(self) -> Option<Self> {
        schema_member(self.root, self.node, "items", None, 0).map(|node| Self {
            root: self.root,
            node,
            tool: self.tool,
            mode_control: false,
        })
    }
}

fn schema_member<'a>(
    root: &'a Value,
    node: &'a Value,
    member: &str,
    name: Option<&str>,
    depth: usize,
) -> Option<&'a Value> {
    if depth > MAX_DEPTH {
        return None;
    }
    if let Some(value) = node.get(member) {
        let found = match name {
            Some(name) => value.get(name),
            None => Some(value),
        };
        if found.is_some() {
            return found;
        }
    }
    if let Some(reference) = node
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix('#'))
    {
        if let Some(target) = root.pointer(reference) {
            if let Some(found) = schema_member(root, target, member, name, depth + 1) {
                return Some(found);
            }
        }
    }
    for operator in ["anyOf", "oneOf", "allOf"] {
        if let Some(items) = node.get(operator).and_then(Value::as_array) {
            for item in items {
                if let Some(found) = schema_member(root, item, member, name, depth + 1) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn builtin_schema(name: Option<&str>) -> Option<Schema<'static>> {
    let spec = crate::agent::registry::ToolRegistry::builtins().get(name?)?;
    Some(Schema {
        root: &spec.schema,
        node: &spec.schema,
        tool: spec.name.as_str(),
        mode_control: false,
    })
}

// These are public controls only when declared at this exact location by a
// builtin schema. The same spelling in a data map grants no exemption.
fn scalar_control(key: &str) -> bool {
    matches!(
        key,
        "start_line"
            | "end_line"
            | "start_page"
            | "end_page"
            | "limit"
            | "offset"
            | "timeout"
            | "timeout_secs"
            | "timeout_ms"
            | "max_results"
            | "max_entries"
            | "max_depth"
            | "depth"
            | "recursive"
            | "include_hidden"
            | "case_sensitive"
            | "literal"
            | "line_numbers"
            | "include_line_hashes"
            | "head"
            | "tail"
            | "network"
            | "background"
            | "wait_ms"
            | "poll_ms"
            | "parallel"
            | "max_wait_seconds"
            | "max_wait_ms"
    )
}

fn check_key(context: &Context, key: &str, declared: bool) -> Result<(), String> {
    if !declared && context.mask_known(key)? != key {
        return Err("A private data-map key is required by tool arguments; use a fresh privacy-safe continuation".into());
    }
    Ok(())
}

fn check_scalar(context: &Context, value: &Value, public_control: bool) -> Result<(), String> {
    if public_control {
        return Ok(());
    }
    let spelling =
        serde_json::to_string(value).map_err(|_| "Private scalar cannot be inspected")?;
    if context.mask_known(&spelling)? != spelling {
        return Err("A private typed value remains in tool arguments; use a fresh privacy-safe continuation".into());
    }
    Ok(())
}

/// Rewrite only string values. Required keys and scalar types remain exact;
/// private typed scalars or data-map keys block rather than change the schema.
pub(super) fn project(context: &Context, name: &str, value: &mut Value) -> Result<(), String> {
    project_node(context, builtin_schema(Some(name)), value, false, 0)
}

fn project_node(
    context: &Context,
    schema: Option<Schema<'_>>,
    value: &mut Value,
    public_control: bool,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("Tool privacy arguments exceed their nesting bound".into());
    }
    match value {
        Value::String(_) if schema.is_some_and(|schema| schema.public_value(value)) => {}
        Value::String(text) => *text = context.mask_known(text)?,
        Value::Object(items) => {
            for (key, value) in items {
                let declared = schema.and_then(|schema| schema.field(key));
                check_key(context, key, declared.is_some())?;
                let next = declared.or_else(|| schema.and_then(Schema::additional));
                let control = declared.is_some() && scalar_control(key);
                project_node(context, next, value, control, depth + 1)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                project_node(
                    context,
                    schema.and_then(Schema::item),
                    item,
                    false,
                    depth + 1,
                )?;
            }
        }
        _ => check_scalar(context, value, public_control)?,
    }
    Ok(())
}

/// Inspect final/native arguments without mutating immutable provider items.
pub(super) fn validate(context: &Context, name: Option<&str>, value: &Value) -> Result<(), String> {
    validate_node(context, builtin_schema(name), value, false, 0)
}

fn validate_node(
    context: &Context,
    schema: Option<Schema<'_>>,
    value: &Value,
    public_control: bool,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("Tool privacy arguments exceed their nesting bound".into());
    }
    match value {
        Value::String(_) if schema.is_some_and(|schema| schema.public_value(value)) => {}
        Value::String(text) => {
            if context.mask_known(text)? != *text {
                return Err("A private string remains in native tool arguments; use a fresh privacy-safe continuation".into());
            }
        }
        Value::Object(items) => {
            for (key, value) in items {
                let declared = schema.and_then(|schema| schema.field(key));
                check_key(context, key, declared.is_some())?;
                let next = declared.or_else(|| schema.and_then(Schema::additional));
                let control = declared.is_some() && scalar_control(key);
                validate_node(context, next, value, control, depth + 1)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                validate_node(
                    context,
                    schema.and_then(Schema::item),
                    item,
                    false,
                    depth + 1,
                )?;
            }
        }
        _ => check_scalar(context, value, public_control)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (tempfile::TempDir, std::sync::Arc<Context>) {
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
        context.project_source("private.json", r#"{"private_project_field":"delicate-client","number":123456,"small":1,"flag":false,"missing":null,"path":"value"}"#).unwrap();
        (root, context)
    }

    #[test]
    fn privacy_argument_data_map_keys_and_typed_custom_values_block() {
        let (_root, context) = fixture();
        for original in [
            json!({"private_project_field":"public"}),
            json!({"id":123456}),
            json!({"data":false}),
            json!({"data":null}),
        ] {
            let mut projected = original.clone();
            assert!(project(&context, "external_tool", &mut projected).is_err());
            assert!(validate(&context, Some("external_tool"), &original).is_err());
        }
    }

    #[test]
    fn privacy_argument_declared_builtin_keys_and_read_controls_remain_public() {
        let (_root, context) = fixture();
        let original = json!({"path":"ordinary.txt", "start_line":1, "end_line":1});
        let mut projected = original.clone();
        project(&context, "read_file", &mut projected).unwrap();
        assert_eq!(projected, original);
        validate(&context, Some("read_file"), &projected).unwrap();
        assert!(validate(&context, None, &original).is_err());
    }

    #[test]
    fn privacy_argument_string_secrets_mask_without_changing_keys_or_types() {
        let (_root, context) = fixture();
        let original = json!({"path":"delicate-client"});
        let mut projected = original.clone();
        project(&context, "read_file", &mut projected).unwrap();
        assert_eq!(
            projected.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["path"]
        );
        assert!(projected["path"].is_string());
        assert!(!projected["path"]
            .as_str()
            .unwrap()
            .contains("delicate-client"));
        assert!(validate(&context, Some("read_file"), &original).is_err());
        validate(&context, Some("read_file"), &projected).unwrap();
    }

    #[test]
    fn privacy_argument_control_spelling_in_unknown_data_is_not_exempt() {
        let (_root, context) = fixture();
        let mut data = json!({"payload":{"start_line":1}});
        assert!(project(&context, "read_file", &mut data).is_err());
        assert!(validate(&context, Some("read_file"), &data).is_err());
    }
}
