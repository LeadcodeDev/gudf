use crate::error::GudfError;
use crate::result::DiffResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatKind {
    Text,
    Json,
    Toml,
    Yaml,
    Code(String),
}

pub trait Format {
    fn kind(&self) -> FormatKind;
    fn diff(&self, old: &str, new: &str) -> Result<DiffResult, GudfError>;
}

pub fn detect_format(content: &str) -> FormatKind {
    let trimmed = content.trim();

    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return FormatKind::Json;
    }

    if trimmed.parse::<toml::Value>().is_ok() && trimmed.contains('=') {
        return FormatKind::Toml;
    }

    if trimmed.contains(':')
        && !trimmed.is_empty()
        && serde_yaml::from_str::<serde_yaml::Value>(trimmed).is_ok()
    {
        if trimmed.contains(": ") || trimmed.contains(":\n") {
            return FormatKind::Yaml;
        }
    }

    FormatKind::Text
}
