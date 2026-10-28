use anyhow::{Result, bail};
use serde_json::{Map, Value};

/// One JSON-compatible YAML/JSON document, preserving actual scalar types.
pub fn parse(source: &str) -> Result<Value> {
    fn convert(value: yaml_serde::Value) -> Result<Value> {
        Ok(match value {
            yaml_serde::Value::Null => Value::Null,
            yaml_serde::Value::Bool(v) => Value::Bool(v),
            yaml_serde::Value::String(v) => Value::String(v),
            yaml_serde::Value::Number(v) => serde_json::from_str(&v.to_string())?,
            yaml_serde::Value::Sequence(values) => {
                Value::Array(values.into_iter().map(convert).collect::<Result<_>>()?)
            }
            yaml_serde::Value::Mapping(values) => {
                let mut object = Map::new();
                for (key, value) in values {
                    let yaml_serde::Value::String(key) = key else {
                        bail!("Object keys must be strings")
                    };
                    object.insert(key, convert(value)?);
                }
                Value::Object(object)
            }
            yaml_serde::Value::Tagged(_) => {
                bail!("YAML tags are not supported; use JSON-compatible values")
            }
        })
    }
    convert(yaml_serde::from_str(source)?)
}
