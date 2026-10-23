//! Bounded provider schema lowering; execution always uses the original registry.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    #[default]
    Compatible,
    RejectLossy,
}
impl Policy {
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Compatible)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Identity,
    Google,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Loss {
    pub path: String,
    pub category: &'static str,
    pub count: usize,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Translation<'a> {
    pub tool_name: String,
    pub schema: Cow<'a, Value>,
    pub losses: Vec<Loss>,
    pub omitted_losses: usize,
}
struct Lower<'a> {
    root: &'a Value,
    nodes: usize,
    active: BTreeSet<String>,
    losses: Vec<Loss>,
    omitted: usize,
}
fn google_type(kind: &str) -> Option<&'static str> {
    match kind {
        "string" => Some("STRING"),
        "number" => Some("NUMBER"),
        "integer" => Some("INTEGER"),
        "boolean" => Some("BOOLEAN"),
        "array" => Some("ARRAY"),
        "object" => Some("OBJECT"),
        "null" => Some("NULL"),
        _ => None,
    }
}
impl Lower<'_> {
    fn loss(&mut self, path: &str, category: &'static str) {
        let path = path.chars().take(192).collect::<String>();
        if let Some(loss) = self
            .losses
            .iter_mut()
            .find(|loss| loss.path == path && loss.category == category)
        {
            loss.count = loss.count.saturating_add(1);
        } else if self.losses.len() < 32 {
            self.losses.push(Loss {
                path,
                category,
                count: 1,
            });
        } else {
            self.omitted = self.omitted.saturating_add(1);
        }
    }
    fn node(&mut self, node: &Value, path: &str, depth: usize) -> Value {
        if depth >= 24 || self.nodes == 0 {
            self.loss(path, "translation_budget_exhausted");
            self.loss(path, "schema_narrowed");
            return json!({"type":"STRING"});
        }
        self.nodes -= 1;
        let Some(obj) = node.as_object() else {
            self.loss(
                path,
                if node == &Value::Bool(true) {
                    "schema_narrowed"
                } else {
                    "schema_widened"
                },
            );
            return json!({"type":"STRING"});
        };
        let mut out = Map::new();
        if let Some(reference) = obj.get("$ref").and_then(Value::as_str) {
            if reference.starts_with("#/") && self.active.insert(reference.into()) {
                if let Some(target) = self.root.pointer(&reference[1..]).cloned() {
                    if let Value::Object(value) = self.node(&target, path, depth + 1) {
                        out.extend(value);
                    }
                } else {
                    self.loss(path, "unresolved_reference");
                }
                self.active.remove(reference);
            } else {
                self.loss(path, "unresolved_reference");
            }
        }
        for (key, value) in obj {
            let child = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
            if !matches!(
                key.as_str(),
                "$ref" | "title" | "description" | "example" | "default"
            ) && out.get(key).is_some_and(|inherited| {
                if key == "type" {
                    value
                        .as_str()
                        .map(|kind| json!(kind.to_ascii_uppercase()))
                        .as_ref()
                        != Some(inherited)
                } else if matches!(
                    key.as_str(),
                    "minItems"
                        | "maxItems"
                        | "minProperties"
                        | "maxProperties"
                        | "minLength"
                        | "maxLength"
                ) {
                    value
                        .as_u64()
                        .map(|number| json!(number.to_string()))
                        .as_ref()
                        != Some(inherited)
                } else {
                    inherited != value
                }
            }) {
                self.loss(&child, "reference_sibling_constraint_changed");
            }
            match key.as_str() {
                "$ref" | "$defs" | "definitions" | "$schema" | "$id" | "examples" | "default"
                | "deprecated" | "readOnly" | "writeOnly" => {}
                "minItems" | "maxItems" | "minProperties" | "maxProperties" | "minLength"
                | "maxLength" => {
                    if let Some(number) = value.as_u64().filter(|number| *number <= i64::MAX as u64)
                    {
                        out.insert(key.clone(), json!(number.to_string()));
                    } else {
                        self.loss(&child, "constraint_dropped");
                    }
                }
                "title" | "description" | "format" | "nullable" | "required" | "minimum"
                | "maximum" | "pattern" => {
                    out.insert(key.clone(), value.clone());
                }
                "enum" => {
                    if value
                        .as_array()
                        .is_some_and(|items| items.iter().all(Value::is_string))
                    {
                        out.insert(key.clone(), value.clone());
                    } else {
                        self.loss(&child, "constraint_dropped");
                    }
                }
                "type" => {
                    if let Some(kind) = value.as_str() {
                        if let Some(kind) = google_type(kind) {
                            out.insert("type".into(), json!(kind));
                        } else {
                            self.loss(&child, "schema_widened");
                        }
                    } else if let Some(kinds) = value.as_array() {
                        let mut selected = None;
                        let mut nonnull = BTreeSet::new();
                        let mut nullable = false;
                        for kind in kinds {
                            match kind.as_str().and_then(google_type) {
                                Some("NULL") => nullable = true,
                                Some(kind) => {
                                    selected.get_or_insert(kind);
                                    nonnull.insert(kind);
                                }
                                None => self.loss(&child, "union_changed"),
                            }
                        }
                        if let Some(kind) = selected {
                            out.insert("type".into(), json!(kind));
                        } else if nullable {
                            out.insert("type".into(), json!("NULL"));
                        } else {
                            self.loss(&child, "union_changed");
                        }
                        if nullable && !nonnull.is_empty() {
                            out.insert("nullable".into(), json!(true));
                        }
                        if nonnull.len() > 1 {
                            self.loss(&child, "union_changed");
                        }
                    } else {
                        self.loss(&child, "schema_widened");
                    }
                }
                "properties" => {
                    if let Some(props) = value.as_object() {
                        let mut lowered = Map::new();
                        for (name, schema) in props.iter().take(1024) {
                            lowered.insert(
                                name.clone(),
                                self.node(
                                    schema,
                                    &format!(
                                        "{child}/{}",
                                        name.replace('~', "~0").replace('/', "~1")
                                    ),
                                    depth + 1,
                                ),
                            );
                        }
                        if props.len() > 1024 {
                            self.loss(&child, "translation_budget_exhausted");
                        }
                        out.insert(key.clone(), Value::Object(lowered));
                    } else {
                        self.loss(&child, "schema_widened");
                    }
                }
                "items" => {
                    out.insert(key.clone(), self.node(value, &child, depth + 1));
                }
                "anyOf" => {
                    if let Some(branches) = value.as_array() {
                        let mut lowered = Vec::new();
                        for (i, schema) in branches.iter().take(64).enumerate() {
                            lowered.push(self.node(schema, &format!("{child}/{i}"), depth + 1));
                        }
                        if branches.len() > 64 {
                            self.loss(&child, "translation_budget_exhausted");
                        }
                        if lowered.is_empty() {
                            self.loss(&child, "schema_widened");
                        } else {
                            out.insert(key.clone(), Value::Array(lowered));
                        }
                    } else {
                        self.loss(&child, "schema_widened");
                    }
                }
                "additionalProperties" if value == &Value::Bool(true) => {}
                "oneOf" | "allOf" => {
                    self.loss(&child, "union_changed");
                    // Retain one usable branch and its supported constraints.
                    // Its changed union/intersection semantics remain explicit.
                    if let Some(branch) = value.as_array().and_then(|branches| branches.first()) {
                        if let Value::Object(selected) =
                            self.node(branch, &format!("{child}/0"), depth + 1)
                        {
                            for (key, value) in selected {
                                out.entry(key).or_insert(value);
                            }
                        }
                    }
                }
                // Apply after all inherited/local enum constraints have been lowered.
                "const" => {}
                _ => self.loss(&child, "constraint_dropped"),
            }
        }
        if let Some(value) = obj.get("const") {
            let child = format!("{path}/const");
            if !value.is_string()
                || out
                    .get("enum")
                    .and_then(Value::as_array)
                    .is_some_and(|values| !values.contains(value))
            {
                self.loss(&child, "constraint_dropped");
            } else {
                out.insert("enum".into(), json!([value]));
            }
        }
        if !out.contains_key("type") {
            // Schema.type is required even on a union container and on every
            // nested schema. Same-type anyOf branches determine it exactly;
            // selecting among distinct types is a reported narrowing.
            if let Some(branches) = out.get("anyOf").and_then(Value::as_array) {
                let mut selected = None;
                let mut kinds = BTreeSet::new();
                let mut nullable = false;
                for branch in branches {
                    nullable |= branch.get("nullable").and_then(Value::as_bool) == Some(true);
                    if let Some(kind) = branch.get("type").and_then(Value::as_str) {
                        if kind == "NULL" {
                            nullable = true;
                        } else {
                            selected.get_or_insert(kind.to_owned());
                            kinds.insert(kind.to_owned());
                        }
                    }
                }
                if kinds.len() > 1 {
                    self.loss(&format!("{path}/anyOf"), "union_changed");
                }
                out.insert(
                    "type".into(),
                    json!(selected.unwrap_or_else(|| "NULL".into())),
                );
                if nullable && !kinds.is_empty() {
                    out.insert("nullable".into(), json!(true));
                }
            } else if out.contains_key("enum") {
                // Google enums and the supported const lowering contain only
                // strings, so this required type does not change their values.
                out.insert("type".into(), json!("STRING"));
            } else {
                let kind = if out.contains_key("properties")
                    || out.contains_key("required")
                    || out.contains_key("minProperties")
                    || out.contains_key("maxProperties")
                {
                    "OBJECT"
                } else if out.contains_key("items")
                    || out.contains_key("minItems")
                    || out.contains_key("maxItems")
                {
                    "ARRAY"
                } else if out.contains_key("minimum") || out.contains_key("maximum") {
                    "NUMBER"
                } else {
                    "STRING"
                };
                out.insert("type".into(), json!(kind));
                self.loss(path, "schema_narrowed");
            }
        }
        if out.get("type") == Some(&json!("ARRAY")) && !out.contains_key("items") {
            out.insert("items".into(), json!({"type":"STRING"}));
            self.loss(path, "schema_narrowed");
        }
        Value::Object(out)
    }
}
pub fn translate<'a>(
    tool_name: &str,
    schema: &'a Value,
    profile: Profile,
    policy: Policy,
) -> Result<Translation<'a>, String> {
    let tool_name = tool_name.chars().take(128).collect::<String>();
    if profile == Profile::Identity {
        return Ok(Translation {
            tool_name,
            schema: Cow::Borrowed(schema),
            losses: vec![],
            omitted_losses: 0,
        });
    }
    let mut lower = Lower {
        root: schema,
        nodes: 1024,
        active: BTreeSet::new(),
        losses: vec![],
        omitted: 0,
    };
    let mut result = lower.node(schema, "", 0);
    if result.get("type") != Some(&json!("OBJECT")) {
        lower.loss("", "root_object_required");
        result["type"] = json!("OBJECT");
    }
    if !result.get("properties").is_some_and(Value::is_object) {
        result["properties"] = json!({});
    }
    if policy == Policy::RejectLossy && (!lower.losses.is_empty() || lower.omitted > 0) {
        return Err("tool schema requires lossy translation under reject_lossy policy".into());
    }
    Ok(Translation {
        tool_name,
        schema: Cow::Owned(result),
        losses: lower.losses,
        omitted_losses: lower.omitted,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_identity_keeps_every_constraint() {
        let input = json!({"type":"object","additionalProperties":false,"properties":{"x":{"type":"integer","multipleOf":3}}});
        assert_eq!(
            translate("test", &input, Profile::Identity, Policy::RejectLossy)
                .unwrap()
                .schema
                .as_ref(),
            &input
        );
    }
    #[test]
    fn schema_google_references_nullable_and_loss_are_visible() {
        let input = json!({"type":"object","$defs":{"name":{"type":["string","null"]}},"properties":{"name":{"$ref":"#/$defs/name","pattern":"abc"}},"additionalProperties":false});
        let t = translate("test", &input, Profile::Google, Policy::Compatible).unwrap();
        assert_eq!(t.schema["properties"]["name"]["type"], "STRING");
        assert_eq!(t.schema["properties"]["name"]["nullable"], true);
        assert!(t.losses.iter().any(|v| v.path == "/additionalProperties"));
        assert!(translate("test", &input, Profile::Google, Policy::RejectLossy).is_err());
    }
    #[test]
    fn schema_google_recursion_and_report_are_bounded() {
        let input = json!({"type":"object","properties":{"x":{"$ref":"#/"}},"$defs":{"loop":{"$ref":"#/$defs/loop"}}});
        let t = translate("test", &input, Profile::Google, Policy::Compatible).unwrap();
        assert!(!t.losses.is_empty());
        assert!(t.losses.len() <= 32);
    }
    #[test]
    fn schema_annotations_do_not_fail_strict() {
        let input = json!({"type":"object","properties":{},"examples":[{}],"default":{}});
        assert!(translate("test", &input, Profile::Google, Policy::RejectLossy).is_ok());
    }
    #[test]
    fn schema_nested_constraints_and_union_changes_are_distinct() {
        let input = json!({"type":"object","properties":{"xs":{"type":"array","minItems":2,"items":{"type":"string","pattern":"^[A-Z]+$","minLength":3}},"choice":{"oneOf":[{"type":"string"},{"type":"integer"}]}}});
        let result = translate("inspect", &input, Profile::Google, Policy::Compatible).unwrap();
        assert_eq!(result.tool_name, "inspect");
        assert_eq!(
            result.schema["properties"]["xs"]["items"]["pattern"],
            "^[A-Z]+$"
        );
        assert_eq!(result.schema["properties"]["xs"]["minItems"], "2");
        assert!(result.losses.iter().any(
            |loss| loss.path == "/properties/choice/oneOf" && loss.category == "union_changed"
        ));
        assert!(translate("inspect", &input, Profile::Google, Policy::RejectLossy).is_err());
    }
    #[test]
    fn schema_depth_and_loss_reports_are_bounded_without_values() {
        let mut deep = json!({"type":"string"});
        for _ in 0..32 {
            deep = json!({"type":"array","items":deep});
        }
        let input = json!({"type":"object","properties":{"deep":deep}});
        assert!(
            translate("inspect", &input, Profile::Google, Policy::Compatible)
                .unwrap()
                .losses
                .iter()
                .any(|loss| loss.category == "translation_budget_exhausted")
        );
        let mut properties = Map::new();
        for index in 0..80 {
            properties.insert(
                format!("p{index}"),
                json!({"type":"string","unsupported":"private-schema-value"}),
            );
        }
        let input = json!({"type":"object","properties":properties});
        let result = translate("inspect", &input, Profile::Google, Policy::Compatible).unwrap();
        assert_eq!(result.losses.len(), 32);
        assert!(result.omitted_losses > 0);
        assert!(!serde_json::to_string(&result.losses)
            .unwrap()
            .contains("private-schema-value"));
    }
    #[test]
    fn schema_reference_annotations_and_identical_type_are_not_meaningful_loss() {
        let input = json!({"type":"object","$defs":{"name":{"type":"string","description":"old","minLength":2}},"properties":{"name":{"$ref":"#/$defs/name","type":"string","description":"new"}}});
        let result = translate("inspect", &input, Profile::Google, Policy::RejectLossy).unwrap();
        assert!(result.losses.is_empty());
        assert_eq!(result.schema["properties"]["name"]["minLength"], "2");
    }
    #[test]
    fn schema_policy_is_additive_and_validates_its_configuration_values() {
        let legacy = "base_url=\"http://127.0.0.1:1/v1\"\napi_key=\"\"\nmodels=[\"fixture\"]\n";
        let provider: crate::config::ProviderConfig = toml::from_str(legacy).unwrap();
        assert_eq!(provider.tool_schema_policy, Policy::Compatible);
        assert!(!toml::to_string(&provider)
            .unwrap()
            .contains("tool_schema_policy"));
        let strict: crate::config::ProviderConfig =
            toml::from_str(&format!("{legacy}tool_schema_policy=\"reject_lossy\"\n")).unwrap();
        assert_eq!(strict.tool_schema_policy, Policy::RejectLossy);
        assert!(toml::from_str::<crate::config::ProviderConfig>(&format!(
            "{legacy}tool_schema_policy=\"invalid\"\n"
        ))
        .is_err());
    }
    #[test]
    fn schema_lowering_never_changes_original_argument_validation() {
        use crate::agent::registry::{ToolRegistry, ToolSpec};
        let original = json!({"type":"object","additionalProperties":false,"properties":{"count":{"type":"integer","multipleOf":2}},"required":["count"]});
        let registry = ToolRegistry::new(vec![ToolSpec::mcp(
            "fixture",
            "local-fixture",
            String::new(),
            original.clone(),
        )
        .unwrap()])
        .unwrap();
        let compatible =
            translate("fixture", &original, Profile::Google, Policy::Compatible).unwrap();
        assert!(!compatible.losses.is_empty());
        assert!(compatible.schema.get("additionalProperties").is_none());
        for policy in [Policy::Compatible, Policy::RejectLossy] {
            let _ = translate("fixture", &original, Profile::Google, policy);
            assert!(registry.validate("fixture", &json!({"count":3})).is_err());
            assert!(registry
                .validate("fixture", &json!({"count":2,"extra":true}))
                .is_err());
            assert!(registry.validate("fixture", &json!({"count":2})).is_ok());
        }
        assert_eq!(registry.get("fixture").unwrap().schema, original);
    }
}

#[cfg(test)]
mod google_wire_tests {
    use super::*;
    #[test]
    fn google_int64_constraints_use_protobuf_json_strings() {
        let schema = json!({"type":"object","minProperties":1,"properties":{"names":{"type":"array","minItems":2,"items":{"type":"string","minLength":3}}}});
        let output = translate("example", &schema, Profile::Google, Policy::RejectLossy).unwrap();
        assert_eq!(output.schema["minProperties"], "1");
        assert_eq!(output.schema["properties"]["names"]["minItems"], "2");
        assert_eq!(
            output.schema["properties"]["names"]["items"]["minLength"],
            "3"
        );
        assert!(output.losses.is_empty());
    }
}

#[cfg(test)]
mod reference_const_tests {
    use super::*;
    #[test]
    fn schema_reference_const_never_silently_replaces_an_inherited_enum() {
        for inherited in [true, false] {
            let input = if inherited {
                json!({"type":"object","$defs":{"base":{"type":"string","enum":["a"]}},"properties":{"x":{"$ref":"#/$defs/base","const":"b"}}})
            } else {
                json!({"type":"object","properties":{"x":{"type":"string","enum":["a"],"const":"b"}}})
            };
            let compatible =
                translate("example", &input, Profile::Google, Policy::Compatible).unwrap();
            assert!(compatible
                .losses
                .iter()
                .any(|loss| loss.path == "/properties/x/const"));
            assert!(translate("example", &input, Profile::Google, Policy::RejectLossy).is_err());
        }
        let input = json!({"type":"object","$defs":{"base":{"type":"string","enum":["a","b"]}},"properties":{"x":{"$ref":"#/$defs/base","const":"a"}}});
        let exact = translate("example", &input, Profile::Google, Policy::RejectLossy).unwrap();
        assert_eq!(exact.schema["properties"]["x"]["enum"], json!(["a"]));
        assert!(exact.losses.is_empty());
    }
}

#[cfg(test)]
mod required_type_tests {
    use super::*;

    fn assert_required_types(schema: &Value) {
        assert!(
            matches!(
                schema.get("type").and_then(Value::as_str),
                Some("STRING" | "NUMBER" | "INTEGER" | "BOOLEAN" | "ARRAY" | "OBJECT" | "NULL")
            ),
            "schema has no supported required type: {schema}"
        );
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for child in properties.values() {
                assert_required_types(child);
            }
        }
        if let Some(items) = schema.get("items") {
            assert_required_types(items);
        }
        if schema["type"] == "ARRAY" {
            assert!(schema.get("items").is_some());
        }
        if let Some(branches) = schema.get("anyOf").and_then(Value::as_array) {
            assert!(!branches.is_empty());
            for branch in branches {
                assert_required_types(branch);
            }
        }
        assert!(schema.get("oneOf").is_none());
        assert!(schema.get("allOf").is_none());
        assert!(schema.get("$ref").is_none());
    }

    #[test]
    fn schema_google_nested_unions_always_emit_required_types() {
        let input = json!({"type":"object","properties":{
            "choice":{"oneOf":[{"type":"string","pattern":"^chosen"},{"type":"integer","minimum":2}]},
            "intersection":{"allOf":[{"type":"integer","minimum":2},{"type":"integer","maximum":5}]},
            "types":{"type":["number","string","null"]},
            "any":{"anyOf":[{"type":"string","minLength":2},{"type":"integer","minimum":3}]},
            "invalid_type":{"type":["unsupported",{}]}
        }});
        let result = translate("test", &input, Profile::Google, Policy::Compatible).unwrap();
        assert_required_types(&result.schema);
        assert_eq!(result.schema["properties"]["choice"]["pattern"], "^chosen");
        assert_eq!(result.schema["properties"]["intersection"]["minimum"], 2);
        assert_eq!(result.schema["properties"]["types"]["type"], "NUMBER");
        assert_eq!(result.schema["properties"]["types"]["nullable"], true);
        assert!(result
            .losses
            .iter()
            .any(|loss| loss.path == "/properties/any/anyOf" && loss.category == "union_changed"));
        assert!(translate("test", &input, Profile::Google, Policy::RejectLossy).is_err());
    }

    #[test]
    fn schema_google_same_type_unions_and_documented_null_keep_constraints() {
        let input = json!({"type":"object","properties":{
            "same":{"anyOf":[{"type":"string","minLength":2},{"type":"string","pattern":"^kept"}]},
            "nullable":{"anyOf":[{"type":"null"},{"type":"string","minLength":3}]},
            "null_only":{"type":"null"},
            "null_array":{"type":["null"]},
            "constant":{"const":"kept"}
        }});
        let result = translate("test", &input, Profile::Google, Policy::RejectLossy).unwrap();
        assert_required_types(&result.schema);
        assert!(result.losses.is_empty());
        assert_eq!(result.schema["properties"]["same"]["type"], "STRING");
        assert_eq!(
            result.schema["properties"]["same"]["anyOf"][0]["minLength"],
            "2"
        );
        assert_eq!(
            result.schema["properties"]["same"]["anyOf"][1]["pattern"],
            "^kept"
        );
        assert_eq!(result.schema["properties"]["nullable"]["type"], "STRING");
        assert_eq!(result.schema["properties"]["nullable"]["nullable"], true);
        assert_eq!(result.schema["properties"]["null_only"]["type"], "NULL");
        assert_eq!(result.schema["properties"]["null_array"]["type"], "NULL");
        assert_eq!(
            result.schema["properties"]["constant"]["enum"],
            json!(["kept"])
        );
    }

    #[test]
    fn schema_google_nullable_branch_keeps_container_nullable() {
        let input = json!({"type":"object","properties":{"x":{"anyOf":[
            {"type":["string","null"]},{"type":"string","pattern":"^x"}
        ]}}});
        let result = translate("test", &input, Profile::Google, Policy::RejectLossy).unwrap();
        assert_required_types(&result.schema);
        assert!(result.losses.is_empty());
        assert_eq!(result.schema["properties"]["x"]["nullable"], true);
    }

    #[test]
    fn schema_google_unresolved_references_and_boolean_schemas_have_required_types() {
        let input = json!({"type":"object","$defs":{"loop":{"$ref":"#/$defs/loop"}},"properties":{
            "anything":true,"nothing":false,"empty":{},
            "external":{"$ref":"https://example.test/schema"},
            "missing":{"$ref":"#/$defs/missing"},
            "recursive":{"$ref":"#/$defs/loop"}
        }});
        let original = input.clone();
        let result = translate("test", &input, Profile::Google, Policy::Compatible).unwrap();
        assert_required_types(&result.schema);
        assert!(result
            .losses
            .iter()
            .any(|loss| loss.path == "/properties/anything" && loss.category == "schema_narrowed"));
        assert!(result
            .losses
            .iter()
            .any(|loss| loss.path == "/properties/nothing" && loss.category == "schema_widened"));
        for name in ["external", "missing", "recursive"] {
            assert!(result
                .losses
                .iter()
                .any(|loss| loss.path == format!("/properties/{name}")
                    && loss.category == "unresolved_reference"));
        }
        assert_eq!(input, original);
        assert!(translate("test", &input, Profile::Google, Policy::RejectLossy).is_err());
    }

    #[test]
    fn schema_google_depth_and_node_budget_fallbacks_are_typed() {
        let mut deep = json!({"type":"integer","minimum":1});
        for _ in 0..40 {
            deep = json!({"type":"array","items":deep});
        }
        let mut properties = Map::new();
        for index in 0..1100 {
            properties.insert(
                format!("p{index:04}"),
                json!({"type":"string","minLength":1}),
            );
        }
        let inputs = [
            json!({"type":"object","properties":{"deep":deep}}),
            json!({"type":"object","properties":properties}),
            json!({"type":"object","properties":{"choice":{"anyOf":vec![json!({"type":"string"});70]}}}),
        ];
        for input in inputs {
            let result = translate("test", &input, Profile::Google, Policy::Compatible).unwrap();
            assert_required_types(&result.schema);
            assert!(result
                .losses
                .iter()
                .any(|loss| loss.category == "translation_budget_exhausted"));
            assert!(result.losses.len() <= 32);
            assert!(translate("test", &input, Profile::Google, Policy::RejectLossy).is_err());
        }
    }
}
