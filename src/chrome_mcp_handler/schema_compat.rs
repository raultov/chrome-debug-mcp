/// Schema compatibility layer for Gemini/strict tool-calling engines.
use rust_mcp_sdk::schema::Tool;
use serde_json::{Map, Value};

/// Returns `tool` with every property schema normalized.
pub(crate) fn normalize_tool(mut tool: Tool) -> Tool {
    if let Some(properties) = tool.input_schema.properties.as_mut() {
        for property in properties.values_mut() {
            normalize_object(property);
        }
    }
    tool
}

fn normalize_value(value: &mut Value) {
    match value {
        Value::Object(map) => normalize_object(map),
        Value::Array(entries) => entries.iter_mut().for_each(normalize_value),
        _ => {}
    }
}

/// Normalizes a schema object bottom-up.
fn normalize_object(map: &mut Map<String, Value>) {
    for child in map.values_mut() {
        normalize_value(child);
    }
    merge_variant_enums(map);
    collapse_nullable_type(map);
    fix_array_items_type(map);
}

/// Collapses a `oneOf`/`anyOf` list of single-value `enum` objects into one
/// string `enum`.
fn merge_variant_enums(map: &mut Map<String, Value>) {
    for keyword in ["oneOf", "anyOf"] {
        let Some(variants) = map.get(keyword).and_then(Value::as_array) else {
            continue;
        };
        if variants.is_empty() {
            continue;
        }

        let mut values = Vec::new();
        let all_plain_enums = variants.iter().all(|variant| {
            let Some(variant) = variant.as_object() else {
                return false;
            };
            let Some(entries) = variant.get("enum").and_then(Value::as_array) else {
                return false;
            };
            if variant.len() != 1 || !entries.iter().all(Value::is_string) {
                return false;
            }
            values.extend(entries.iter().cloned());
            true
        });
        if !all_plain_enums {
            continue;
        }

        map.remove(keyword);
        map.insert("type".to_string(), Value::String("string".to_string()));
        map.insert("enum".to_string(), Value::Array(values));
    }
}

/// Rewrites `"type": ["x", "null"]` as `"type": "x"`.
fn collapse_nullable_type(map: &mut Map<String, Value>) {
    let Some(types) = map.get("type").and_then(Value::as_array) else {
        return;
    };
    let Some(concrete) = types
        .iter()
        .filter_map(Value::as_str)
        .find(|name| *name != "null")
        .map(str::to_string)
    else {
        return;
    };
    map.insert("type".to_string(), Value::String(concrete));
}

/// If a map has an `"items"` field, ensures it has `"type": "array"`.
/// Also, if `items` is an object, ensures it has a `"type"` property (e.g. `"type": "string"` for enums).
fn fix_array_items_type(map: &mut Map<String, Value>) {
    if map.contains_key("items") {
        if let Some(Value::String(t)) = map.get("type") {
            if t != "array" {
                map.insert("type".to_string(), Value::String("array".to_string()));
            }
        } else if !map.contains_key("type") {
            map.insert("type".to_string(), Value::String("array".to_string()));
        }

        // Check if the items object itself is an enum without a type, and set it to string
        if let Some(Value::Object(items_map)) = map.get_mut("items") {
            if items_map.contains_key("enum") && !items_map.contains_key("type") {
                items_map.insert("type".to_string(), Value::String("string".to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn normalize(schema: Value) -> Value {
        let mut map = schema
            .as_object()
            .expect("schema must be an object")
            .clone();
        normalize_object(&mut map);
        Value::Object(map)
    }

    #[test]
    fn given_nullable_enum_array_when_normalizing_then_it_becomes_a_plain_typed_array() {
        let normalized = normalize(json!({
            "description": "presets",
            "items": {
                "oneOf": [{ "enum": ["WEB_MCP"] }, { "enum": ["WEBGL_SOFTWARE"] }]
            },
            "type": ["array", "null"]
        }));

        assert_eq!(
            normalized,
            json!({
                "description": "presets",
                "items": { "type": "string", "enum": ["WEB_MCP", "WEBGL_SOFTWARE"] },
                "type": "array"
            })
        );
    }

    #[test]
    fn given_nullable_scalar_when_normalizing_then_the_null_member_is_dropped() {
        let normalized = normalize(json!({ "type": ["string", "null"] }));
        assert_eq!(normalized, json!({ "type": "string" }));
    }

    #[test]
    fn given_a_scalar_type_when_normalizing_then_it_is_left_untouched() {
        let normalized = normalize(json!({ "type": "boolean" }));
        assert_eq!(normalized, json!({ "type": "boolean" }));
    }

    #[test]
    fn given_a_null_only_type_when_normalizing_then_it_is_left_untouched() {
        let normalized = normalize(json!({ "type": ["null"] }));
        assert_eq!(normalized, json!({ "type": ["null"] }));
    }

    #[test]
    fn given_a_one_of_with_richer_variants_when_normalizing_then_it_is_left_untouched() {
        let schema = json!({
            "oneOf": [
                { "type": "string" },
                { "enum": ["a"], "description": "not a bare variant" }
            ]
        });
        assert_eq!(normalize(schema.clone()), schema);
    }

    #[test]
    fn given_nested_object_properties_when_normalizing_then_they_are_rewritten_too() {
        let normalized = normalize(json!({
            "type": ["object", "null"],
            "properties": { "inner": { "type": ["integer", "null"] } }
        }));

        assert_eq!(
            normalized,
            json!({
                "type": "object",
                "properties": { "inner": { "type": "integer" } }
            })
        );
    }
}
