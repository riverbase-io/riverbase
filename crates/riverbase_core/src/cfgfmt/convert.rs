//! YAML → JSON conversion and legacy list-shaped YAML → singular block maps.

use serde_json::{Map, Value};

use crate::base::RiverbaseResult;

/// Parse a single YAML document into JSON.
pub fn parse_yaml_to_json(text: &str) -> RiverbaseResult<Value> {
    #[cfg(feature = "yaml")]
    {
        parse_yaml_to_json_inner(text)
    }
    #[cfg(not(feature = "yaml"))]
    {
        let _ = text;
        Err(crate::errors::CFG_207.with_data("Rebuild with riverbase_core feature `yaml`."))
    }
}

/// Parse a YAML stream (`---` separated documents) into JSON values.
pub fn parse_yaml_documents_to_json(text: &str) -> RiverbaseResult<Vec<Value>> {
    #[cfg(feature = "yaml")]
    {
        parse_yaml_documents_to_json_inner(text)
    }
    #[cfg(not(feature = "yaml"))]
    {
        let _ = text;
        Err(crate::errors::CFG_208.with_data("Rebuild with riverbase_core feature `yaml`."))
    }
}

#[cfg(feature = "yaml")]
fn parse_yaml_to_json_inner(text: &str) -> RiverbaseResult<Value> {
    use serde_yaml::Value as YamlValue;
    let yaml: YamlValue =
        serde_yaml::from_str(text).map_err(|e| crate::errors::CFG_020.with_data(e.to_string()))?;
    Ok(yaml_value_to_json(yaml))
}

#[cfg(feature = "yaml")]
fn parse_yaml_documents_to_json_inner(text: &str) -> RiverbaseResult<Vec<Value>> {
    use serde::Deserialize;
    use serde_yaml::Value as YamlValue;
    let mut docs = Vec::new();
    for document in serde_yaml::Deserializer::from_str(text) {
        let yaml = YamlValue::deserialize(document)
            .map_err(|e| crate::errors::CFG_021.with_data(e.to_string()))?;
        docs.push(yaml_value_to_json(yaml));
    }
    if docs.is_empty() {
        docs.push(Value::Null);
    }
    Ok(docs)
}

/// Yaml value to json.
#[cfg(feature = "yaml")]
pub fn yaml_value_to_json(value: serde_yaml::Value) -> Value {
    use serde_yaml::Value as YamlValue;
    match value {
        YamlValue::Null => Value::Null,
        YamlValue::Bool(b) => Value::Bool(b),
        YamlValue::Number(n) => serde_json::Number::from_f64(n.as_f64().unwrap_or(0.0))
            .map(Value::Number)
            .unwrap_or(Value::Null),
        YamlValue::String(s) => Value::String(s),
        YamlValue::Sequence(seq) => Value::Array(seq.into_iter().map(yaml_value_to_json).collect()),
        YamlValue::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                let key = match k {
                    YamlValue::String(s) => s,
                    other => serde_yaml::to_string(&other)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                };
                obj.insert(key, yaml_value_to_json(v));
            }
            Value::Object(obj)
        }
        YamlValue::Tagged(tagged) => yaml_value_to_json(tagged.value),
    }
}

/// Convert legacy YAML list keys (`stages`, `steps`, `datapipe` arrays) into singular
/// block maps (`stage`, `step`, `datapipe`) for schema validation. HCL is unchanged.
pub fn legacy_yaml_lists_to_blocks(mut root: Value) -> Value {
    let Some(obj) = root.as_object_mut() else {
        return root;
    };
    if let Some(stages) = obj.remove("stages") {
        obj.insert("stage".to_string(), list_to_label_map(stages, "name"));
    }
    if let Some(steps) = obj.remove("steps") {
        let mut step_map = list_to_label_map(steps, "name");
        if let Some(map) = step_map.as_object_mut() {
            for step in map.values_mut() {
                if let Some(step_obj) = step.as_object_mut() {
                    if let Some(on) = step_obj.remove("on") {
                        step_obj.insert("on".to_string(), handlers_list_to_map(on));
                    }
                }
            }
        }
        obj.insert("step".to_string(), step_map);
    }
    if let Some(on) = obj.get_mut("on") {
        if on.is_array() {
            *on = handlers_list_to_map(on.take());
        }
    }
    if let Some(datapipe) = obj.remove("datapipe") {
        if datapipe.is_array() {
            obj.insert("datapipe".to_string(), list_to_label_map(datapipe, "name"));
        } else {
            obj.insert("datapipe".to_string(), datapipe);
        }
    }
    root
}

/// Convert legacy YAML process documents to singular block maps.
pub fn legacy_yaml_process_document(root: Value) -> Value {
    legacy_yaml_lists_to_blocks(root)
}

fn list_to_label_map(value: Value, name_key: &str) -> Value {
    let Some(list) = value.as_array() else {
        return value;
    };
    let mut map = Map::new();
    for item in list {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let name = obj
            .get(name_key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let mut body = item.clone();
        if let Some(body_obj) = body.as_object_mut() {
            body_obj.remove(name_key);
        }
        map.insert(name, body);
    }
    Value::Object(map)
}

fn handlers_list_to_map(value: Value) -> Value {
    let Some(list) = value.as_array() else {
        return value;
    };
    let mut map = Map::new();
    for item in list {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let Some(event) = obj.get("event").and_then(Value::as_str) else {
            continue;
        };
        let mut body = obj.clone();
        body.remove("event");
        map.insert(event.to_string(), Value::Object(body));
    }
    Value::Object(map)
}

/// Json value to yaml.
#[cfg(feature = "yaml")]
pub fn json_value_to_yaml(value: Value) -> serde_yaml::Value {
    use serde_yaml::Value as YamlValue;
    match value {
        Value::Null => YamlValue::Null,
        Value::Bool(b) => YamlValue::Bool(b),
        Value::Number(n) => YamlValue::Number(serde_yaml::Number::from(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => YamlValue::String(s),
        Value::Array(arr) => YamlValue::Sequence(arr.into_iter().map(json_value_to_yaml).collect()),
        Value::Object(obj) => {
            let mut map = serde_yaml::Mapping::new();
            for (k, v) in obj {
                map.insert(YamlValue::String(k), json_value_to_yaml(v));
            }
            YamlValue::Mapping(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfgfmt::hcl::parse_hcl_to_json;

    #[test]
    fn hcl_datapipe_stays_label_map() {
        let v = parse_hcl_to_json(r#"datapipe "p1" { mapping = { a = "b" } }"#).unwrap();
        assert!(v["datapipe"].is_object());
        assert_eq!(v["datapipe"]["p1"]["mapping"]["a"], "b");
    }

    #[cfg(feature = "yaml")]
    #[test]
    fn yaml_list_becomes_singular_map() {
        let yaml = r#"
stages:
  - name: s1
    title: S
steps:
  - name: x
    stage: s1
    on:
      - event: ev
        action: []
"#;
        let v = parse_yaml_to_json(yaml).unwrap();
        let canon = legacy_yaml_process_document(v);
        assert!(canon["stage"]["s1"].is_object());
        assert_eq!(
            canon["step"]["x"]["on"]["ev"]["action"],
            Value::Array(vec![])
        );
    }
}
