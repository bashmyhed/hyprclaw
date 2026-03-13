use serde_json::Value;

/// Strip thinking blocks from Claude responses
pub fn strip_thinking_blocks(content: &mut Value) {
    if let Some(array) = content.as_array_mut() {
        array.retain(|item| {
            if let Some(obj) = item.as_object() {
                !obj.contains_key("thinking")
                    && obj.get("type").and_then(|v| v.as_str()) != Some("thinking")
            } else {
                true
            }
        });
    }
}

/// Clean JSON schema for Antigravity API
/// Removes unsupported keywords: $ref, $defs, const, default, examples, additionalProperties
pub fn clean_json_schema(schema: &mut Value) {
    match schema {
        Value::Object(map) => {
            // Remove unsupported keywords
            map.remove("$schema");
            map.remove("$defs");
            map.remove("definitions");
            map.remove("const");
            map.remove("$ref");
            map.remove("additionalProperties");
            map.remove("propertyNames");
            map.remove("title");
            map.remove("$id");
            map.remove("$comment");
            map.remove("default");
            map.remove("examples");
            map.remove("minLength");
            map.remove("maxLength");
            map.remove("exclusiveMinimum");
            map.remove("exclusiveMaximum");
            map.remove("pattern");
            map.remove("minItems");
            map.remove("maxItems");
            map.remove("format");

            // Recursively clean nested objects
            for value in map.values_mut() {
                clean_json_schema(value);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                clean_json_schema(item);
            }
        }
        _ => {}
    }
}

/// Add thinking configuration to request
pub fn add_thinking_config(
    body: &mut Value,
    thinking_budget: Option<u32>,
    thinking_level: Option<&str>,
) {
    if let Some(budget) = thinking_budget {
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "thinkingConfig".to_string(),
                serde_json::json!({
                    "thinkingBudget": budget
                }),
            );
        }
    } else if let Some(level) = thinking_level {
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "thinkingConfig".to_string(),
                serde_json::json!({
                    "thinkingLevel": level
                }),
            );
        }
    }
}

/// Transform tools in request body
pub fn transform_tools(body: &mut Value) {
    if let Some(tools) = body.get_mut("tools").and_then(|v| v.as_array_mut()) {
        for tool in tools {
            if let Some(function) = tool.get_mut("function") {
                if let Some(parameters) = function.get_mut("parameters") {
                    clean_json_schema(parameters);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_json_schema() {
        let mut schema = serde_json::json!({
            "type": "object",
            "$schema": "http://json-schema.org/draft-07/schema#",
            "$defs": {"Foo": {}},
            "const": "value",
            "default": "default_value",
            "properties": {
                "name": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 100
                }
            }
        });

        clean_json_schema(&mut schema);

        assert!(!schema.as_object().unwrap().contains_key("$schema"));
        assert!(!schema.as_object().unwrap().contains_key("$defs"));
        assert!(!schema.as_object().unwrap().contains_key("const"));
        assert!(!schema.as_object().unwrap().contains_key("default"));
    }

    #[test]
    fn test_strip_thinking_blocks() {
        let mut content = serde_json::json!([
            {"type": "thinking", "thinking": "internal thoughts"},
            {"type": "text", "text": "visible response"}
        ]);

        strip_thinking_blocks(&mut content);

        let array = content.as_array().unwrap();
        assert_eq!(array.len(), 1);
        assert_eq!(array[0]["type"], "text");
    }
}
