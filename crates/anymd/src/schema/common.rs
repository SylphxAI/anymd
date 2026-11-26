use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_SOURCES_PER_REQUEST: usize = 32;

// schema_with replaces schemars' Option type detection. Each field using these
// nullable schemas also needs serde(default) so an omitted key stays optional
// in the generated input schema, just as it is in the decoder (#881).
pub(super) fn option_bool_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    // Keep Option semantics (nullable) while guaranteeing a boolean type keyword for tools/list.
    serde_json::from_value(serde_json::json!({
        "type": ["boolean", "null"]
    }))
    .expect("option bool schema")
}

pub(super) fn option_revisions_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::from_value(serde_json::json!({
        "type": ["string", "null"],
        "enum": ["markup", "accept", "reject", null]
    }))
    .expect("option revisions schema")
}

pub(super) fn option_images_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::from_value(serde_json::json!({
        "type": ["string", "null"],
        "enum": ["refs", "none", null]
    }))
    .expect("option images schema")
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct PageNumber(#[schemars(range(min = 1))] pub u32);

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PageSpecifier {
    Pages(Vec<PageNumber>),
    Range(#[schemars(length(min = 1))] String),
}

impl PageSpecifier {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Pages(pages) if pages.iter().any(|page| page.0 == 0) => {
                Err("sources[].pages entries must be integers greater than or equal to 1.".into())
            }
            Self::Range(range) if range.is_empty() => {
                Err("sources[].pages range string must not be empty.".into())
            }
            _ => Ok(()),
        }
    }
}

// Client-visible tools/list schema intentionally omits oneOf/not exclusive
// constructs: OpenCode + Fireworks (and similar strict tool-schema validators)
// reject `{"required":["path"],"not":{"required":["url"]}}` (issue #562).
// Exactly-one-of path|url remains a hard runtime contract via `validate()`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PdfSource {
    #[schemars(
        length(min = 1),
        description = "Path to the local PDF file (absolute or relative to cwd). Provide exactly one of path or url (not both)."
    )]
    pub path: Option<String>,
    #[schemars(
        length(min = 1),
        description = "Explicit source URL. Provide exactly one of path or url (not both)."
    )]
    pub url: Option<String>,
    pub pages: Option<PageSpecifier>,
}

impl PdfSource {
    pub fn validate(&self) -> Result<(), String> {
        let has_path = self.path.as_ref().is_some_and(|value| !value.is_empty());
        let has_url = self.url.as_ref().is_some_and(|value| !value.is_empty());
        match (has_path, has_url) {
            (true, false) | (false, true) => {}
            (false, false) | (true, true) => {
                return Err("Provide exactly one of path or url for each PDF source.".into());
            }
        }
        if let Some(pages) = &self.pages {
            pages.validate()?;
        }
        Ok(())
    }

    pub fn label(&self) -> String {
        self.path
            .clone()
            .or_else(|| self.url.clone())
            .unwrap_or_else(|| "unknown".into())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadPdfAutoDetail {
    Fast,
    Balanced,
    Full,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrustReportRedaction {
    Standard,
    Strict,
    Off,
}

pub(super) fn validate_source_count<T>(sources: &[T]) -> Result<(), String> {
    if sources.len() > MAX_SOURCES_PER_REQUEST {
        Err(format!(
            "Request accepts at most {MAX_SOURCES_PER_REQUEST} sources."
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_u32_min(name: &str, value: Option<u32>, min: u32) -> Result<(), String> {
    if value.is_some_and(|value| value < min) {
        Err(format!("{name} must be at least {min}."))
    } else {
        Ok(())
    }
}

pub(super) fn validate_u32_range(
    name: &str,
    value: Option<u32>,
    min: u32,
    max: u32,
) -> Result<(), String> {
    if value.is_some_and(|value| !(min..=max).contains(&value)) {
        Err(format!("{name} must be between {min} and {max}."))
    } else {
        Ok(())
    }
}

pub(super) fn validate_u64_range(
    name: &str,
    value: Option<u64>,
    min: u64,
    max: u64,
) -> Result<(), String> {
    if value.is_some_and(|value| !(min..=max).contains(&value)) {
        Err(format!("{name} must be between {min} and {max}."))
    } else {
        Ok(())
    }
}

pub(super) fn validate_f64_range(
    name: &str,
    value: Option<f64>,
    min: f64,
    max: f64,
) -> Result<(), String> {
    if value.is_some_and(|value| !value.is_finite() || !(min..=max).contains(&value)) {
        Err(format!("{name} must be between {min} and {max}."))
    } else {
        Ok(())
    }
}
