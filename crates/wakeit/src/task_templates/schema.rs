use super::Issue;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn present_value<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputSchema {
    #[serde(rename = "type")]
    pub kind: Value,
    pub title: Option<String>,
    pub description: Option<String>,
    pub format: Option<String>,
    #[serde(default)]
    pub properties: BTreeMap<String, InputSchema>,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(rename = "additionalProperties")]
    pub additional_properties: Option<bool>,
    pub items: Option<Box<InputSchema>>,
    #[serde(rename = "enum")]
    pub choices: Option<Vec<Value>>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub default: Option<Value>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    #[serde(rename = "minLength")]
    pub min_length: Option<usize>,
    #[serde(rename = "maxLength")]
    pub max_length: Option<usize>,
    #[serde(rename = "minItems")]
    pub min_items: Option<usize>,
    #[serde(rename = "maxItems")]
    pub max_items: Option<usize>,
}
impl InputSchema {
    pub fn object() -> Self {
        serde_json::from_value(serde_json::json!({"type":"object","additionalProperties":false}))
            .expect("static input schema")
    }
    pub fn types(&self) -> Vec<&str> {
        match &self.kind {
            Value::String(value) => vec![value],
            Value::Array(values) => values.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        }
    }
    pub fn validate_schema(&self, path: &str, depth: usize, issues: &mut Vec<Issue>) {
        let types = self.types();
        if types.is_empty()
            || types.iter().any(|t| {
                ![
                    "string", "boolean", "integer", "number", "array", "object", "null",
                ]
                .contains(t)
            })
            || (types.len() > 1 && (types.len() != 2 || !types.contains(&"null")))
            || (self.kind.is_array()
                && self.kind.as_array().is_some_and(|v| v.len() != types.len()))
            || types.iter().collect::<BTreeSet<_>>().len() != types.len()
        {
            issues.push(Issue::new("schema", path, "Unsupported input type"));
        }
        if depth == 0 && types != ["object"] {
            issues.push(Issue::new("schema", path, "Inputs must be an object"));
        }
        if self.default.is_some() && depth != 1 {
            issues.push(Issue::new(
                "schema",
                format!("{path}.default"),
                "Defaults are supported only on top-level inputs",
            ));
        }
        if self.additional_properties == Some(true) {
            issues.push(Issue::new(
                "schema",
                path,
                "Input objects must declare their properties",
            ));
        }
        if self.items.is_some() != types.contains(&"array") {
            issues.push(Issue::new(
                "schema",
                format!("{path}.items"),
                "Array inputs require items",
            ));
        }
        if (!self.properties.is_empty()
            || !self.required.is_empty()
            || self.additional_properties.is_some())
            && !types.contains(&"object")
        {
            issues.push(Issue::new(
                "schema",
                path,
                "Object properties require object type",
            ));
        }
        if (self.min_length.is_some() || self.max_length.is_some() || self.format.is_some())
            && !types.contains(&"string")
        {
            issues.push(Issue::new(
                "schema",
                path,
                "Text constraints require string type",
            ));
        }
        if (self.min_items.is_some() || self.max_items.is_some()) && !types.contains(&"array") {
            issues.push(Issue::new(
                "schema",
                path,
                "List constraints require array type",
            ));
        }
        if (self.minimum.is_some() || self.maximum.is_some())
            && !types.iter().any(|v| ["integer", "number"].contains(v))
        {
            issues.push(Issue::new(
                "schema",
                path,
                "Numeric constraints require number type",
            ));
        }
        if self.minimum.zip(self.maximum).is_some_and(|(a, b)| a > b)
            || self
                .min_length
                .zip(self.max_length)
                .is_some_and(|(a, b)| a > b)
            || self
                .min_items
                .zip(self.max_items)
                .is_some_and(|(a, b)| a > b)
        {
            issues.push(Issue::new("schema", path, "Minimum exceeds maximum"));
        }
        if self
            .format
            .as_deref()
            .is_some_and(|v| !["text", "multiline", "directory", "repository-url"].contains(&v))
        {
            issues.push(Issue::new(
                "schema",
                format!("{path}.format"),
                "Unsupported input format",
            ));
        }
        let mut required = BTreeSet::new();
        for key in &self.required {
            if !self.properties.contains_key(key) || !required.insert(key) {
                issues.push(
                    Issue::new(
                        "schema",
                        format!("{path}.required"),
                        "Unknown or duplicate required property",
                    )
                    .param("name", key.clone()),
                );
            }
        }
        for (name, child) in &self.properties {
            child.validate_schema(&format!("{path}.properties.{name}"), depth + 1, issues);
        }
        if let Some(items) = &self.items {
            items.validate_schema(&format!("{path}.items"), depth + 1, issues);
        }
        if let Some(default) = &self.default {
            self.validate_value(default, &format!("{path}.default"), issues);
        }
        if let Some(choices) = &self.choices {
            if choices.is_empty() {
                issues.push(Issue::new(
                    "schema",
                    format!("{path}.enum"),
                    "Enum must contain a value",
                ));
            }
            for (index, value) in choices.iter().enumerate() {
                if choices[..index].contains(value) {
                    issues.push(Issue::new(
                        "schema",
                        format!("{path}.enum"),
                        "Duplicate enum value",
                    ));
                }
                let mut spec = self.clone();
                spec.choices = None;
                spec.default = None;
                spec.validate_value(value, &format!("{path}.enum[{index}]"), issues);
            }
        }
    }
    pub fn validate_value(&self, value: &Value, path: &str, issues: &mut Vec<Issue>) {
        let valid = self.types().iter().any(|kind| match *kind {
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "number" => value.is_number(),
            "integer" => value.as_number().is_some_and(|n| {
                n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|n| n.fract() == 0.0)
            }),
            "array" => value.is_array(),
            "object" => value.is_object(),
            "null" => value.is_null(),
            _ => false,
        });
        if !valid {
            issues.push(
                Issue::new("inputType", path, "Input has the wrong type")
                    .param("expected", self.types().join(" / ")),
            );
            return;
        }
        if self
            .choices
            .as_ref()
            .is_some_and(|choices| !choices.contains(value))
        {
            issues.push(Issue::new(
                "inputEnum",
                path,
                "Value is not an allowed choice",
            ));
        }
        if let Some(text) = value.as_str() {
            let count = text.chars().count();
            if self.min_length.is_some_and(|n| count < n)
                || self.max_length.is_some_and(|n| count > n)
            {
                issues.push(Issue::new(
                    "inputLength",
                    path,
                    "Text length is outside the declared bounds",
                ));
            }
            if self.format.as_deref() == Some("repository-url") && !repository_url(text) {
                issues.push(Issue::new("repositoryUrl", path, "Invalid repository URL"));
            }
            if self.format.as_deref() == Some("directory")
                && (text.trim().is_empty() || text.contains('\0'))
            {
                issues.push(Issue::new("directory", path, "Invalid working directory"));
            }
        }
        if let Some(number) = value.as_f64()
            && (self.minimum.is_some_and(|n| number < n)
                || self.maximum.is_some_and(|n| number > n))
        {
            issues.push(Issue::new(
                "inputRange",
                path,
                "Number is outside the declared bounds",
            ));
        }
        if let Some(values) = value.as_array() {
            if self.min_items.is_some_and(|n| values.len() < n)
                || self.max_items.is_some_and(|n| values.len() > n)
            {
                issues.push(Issue::new(
                    "inputCount",
                    path,
                    "List length is outside the declared bounds",
                ));
            }
            if let Some(items) = &self.items {
                for (i, item) in values.iter().enumerate() {
                    items.validate_value(item, &format!("{path}[{i}]"), issues);
                }
            }
        }
        if let Some(values) = value.as_object() {
            for key in &self.required {
                if !values.contains_key(key) {
                    issues.push(Issue::new(
                        "required",
                        format!("{path}.{key}"),
                        "Required input is missing",
                    ));
                }
            }
            for (key, item) in values {
                if let Some(child) = self.properties.get(key) {
                    child.validate_value(item, &format!("{path}.{key}"), issues);
                } else {
                    issues.push(
                        Issue::new("unknownInput", format!("{path}.{key}"), "Unknown input")
                            .param("name", key.clone()),
                    );
                }
            }
        }
    }
}
fn repository_url(value: &str) -> bool {
    if let Ok(url) = reqwest::Url::parse(value) {
        return ["http", "https", "ssh"].contains(&url.scheme())
            && url.host_str().is_some()
            && url.path() != "/"
            && !url.path().is_empty();
    }
    value.split_once(':').is_some_and(|(host, path)| {
        !host.is_empty()
            && !path.is_empty()
            && !host.contains(['/', '\\', ' '])
            && host.contains('@')
    })
}
pub fn resolve_inputs(
    schema: &InputSchema,
    values: &Map<String, Value>,
    unset: &[String],
) -> (Map<String, Value>, Vec<Issue>) {
    let mut issues = vec![];
    let mut omitted = BTreeSet::new();
    for name in unset {
        if !schema.properties.contains_key(name) {
            issues.push(
                Issue::new("unknownInput", format!("inputs.{name}"), "Unknown input")
                    .param("name", name.clone()),
            );
        }
        if !omitted.insert(name) || values.contains_key(name) {
            issues.push(Issue::new(
                "overrideConflict",
                format!("inputs.{name}"),
                "Input cannot be both provided and unset",
            ));
        }
    }
    let mut resolved = Map::new();
    for (name, spec) in &schema.properties {
        if !omitted.contains(name)
            && let Some(value) = values.get(name).or(spec.default.as_ref())
        {
            resolved.insert(name.clone(), value.clone());
        }
    }
    for (name, value) in values {
        if !schema.properties.contains_key(name) {
            resolved.insert(name.clone(), value.clone());
        }
    }
    schema.validate_value(&Value::Object(resolved.clone()), "inputs", &mut issues);
    (resolved, issues)
}
