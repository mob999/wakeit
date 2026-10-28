//! Persist literal text separately from runtime result references.
use crate::config::validate_id;
use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(untagged)]
pub enum Prompt {
    Legacy(String),
    Structured(StructuredPrompt),
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StructuredPrompt {
    pub parts: Vec<PromptPart>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(untagged)]
pub enum PromptPart {
    Text(TextPart),
    Result(ResultPart),
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TextPart {
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResultPart {
    pub result: String,
}
impl PromptPart {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextPart { text: text.into() })
    }
    pub fn result(id: impl Into<String>) -> Self {
        Self::Result(ResultPart { result: id.into() })
    }
}
impl From<String> for Prompt {
    fn from(value: String) -> Self {
        Self::Legacy(value)
    }
}
impl From<&str> for Prompt {
    fn from(value: &str) -> Self {
        Self::Legacy(value.into())
    }
}
impl Prompt {
    pub fn structured(parts: Vec<PromptPart>) -> Self {
        Self::Structured(StructuredPrompt { parts })
    }
    pub fn is_blank(&self) -> bool {
        match self {
            Self::Legacy(text) => text.trim().is_empty(),
            Self::Structured(value) => value.parts.iter().all(|part| match part {
                PromptPart::Text(text) => text.text.trim().is_empty(),
                PromptPart::Result(_) => false,
            }),
        }
    }
    /// Human-readable source, used only by configuration/environment inspection.
    /// Execution must use `dag::render`, never parse this display string again.
    pub fn display_text(&self) -> String {
        match self {
            Self::Legacy(text) => text.clone(),
            Self::Structured(value) => value
                .parts
                .iter()
                .map(|part| match part {
                    PromptPart::Text(text) => text.text.clone(),
                    PromptPart::Result(reference) => {
                        format!("{{{{ nodes.{}.result }}}}", reference.result)
                    }
                })
                .collect(),
        }
    }
    pub fn source_len(&self) -> usize {
        match self {
            Self::Legacy(text) => text.len(),
            Self::Structured(value) => value
                .parts
                .iter()
                .map(|part| match part {
                    PromptPart::Text(text) => text.text.len(),
                    PromptPart::Result(reference) => reference.result.len() + 19,
                })
                .sum(),
        }
    }
    pub fn to_parts(&self) -> Result<Vec<PromptPart>> {
        Ok(self
            .fragments()?
            .into_iter()
            .flat_map(|(text, id)| {
                let mut parts = vec![];
                if !text.is_empty() {
                    parts.push(PromptPart::text(text));
                }
                if let Some(id) = id {
                    parts.push(PromptPart::result(id));
                }
                parts
            })
            .collect())
    }
}

/// Legacy strings retain their original reserved-namespace parser. Structured
/// text bypasses it in both dispatch and retry/provenance code.
pub trait PromptSource {
    fn fragments(&self) -> Result<Vec<(&str, Option<String>)>>;
}
impl PromptSource for Prompt {
    fn fragments(&self) -> Result<Vec<(&str, Option<String>)>> {
        match self {
            Self::Legacy(text) => text.fragments(),
            Self::Structured(value) => value
                .parts
                .iter()
                .map(|part| match part {
                    PromptPart::Text(text) => Ok((text.text.as_str(), None)),
                    PromptPart::Result(reference) => {
                        validate_id(&reference.result)?;
                        Ok(("", Some(reference.result.clone())))
                    }
                })
                .collect(),
        }
    }
}
impl PromptSource for String {
    fn fragments(&self) -> Result<Vec<(&str, Option<String>)>> {
        self.as_str().fragments()
    }
}
impl PromptSource for str {
    fn fragments(&self) -> Result<Vec<(&str, Option<String>)>> {
        let mut rest = self;
        let mut result = vec![];
        while let Some(start) = rest.find("{{") {
            let (before, tail) = rest.split_at(start);
            let Some(end) = tail.find("}}") else {
                ensure!(
                    !tail[2..].trim_start().starts_with("nodes."),
                    "Unclosed result reference"
                );
                break;
            };
            let key = tail[2..end].trim();
            if let Some(key) = key.strip_prefix("nodes.") {
                let id = key
                    .strip_suffix(".result")
                    .ok_or_else(|| anyhow::anyhow!("Invalid result reference: {key}"))?;
                validate_id(id)?;
                result.push((before, Some(id.to_owned())));
            } else {
                result.push((&rest[..start + end + 2], None));
            }
            rest = &tail[end + 2..];
        }
        result.push((rest, None));
        Ok(result)
    }
}
