use crate::error::GudfError;
use crate::format::{detect_format, Format, FormatKind};
use crate::formats::json::JsonFormat;
use crate::formats::text::TextFormat;
use crate::formats::toml::TomlFormat;
use crate::formats::yaml::YamlFormat;
use crate::result::DiffResult;

pub struct DiffEngine {
    formats: Vec<Box<dyn Format>>,
}

impl DiffEngine {
    pub fn new() -> Self {
        Self {
            formats: vec![
                Box::new(TextFormat),
                Box::new(JsonFormat),
                Box::new(TomlFormat),
                Box::new(YamlFormat),
            ],
        }
    }

    pub fn with_formats(formats: Vec<Box<dyn Format>>) -> Self {
        Self { formats }
    }

    pub fn register(&mut self, format: Box<dyn Format>) {
        self.formats.push(format);
    }

    pub fn diff(&self, old: &str, new: &str) -> Result<DiffResult, GudfError> {
        let kind = detect_format(old);
        self.diff_as(kind, old, new)
    }

    pub fn diff_as(
        &self,
        kind: FormatKind,
        old: &str,
        new: &str,
    ) -> Result<DiffResult, GudfError> {
        for format in &self.formats {
            if format.kind() == kind {
                return format.diff(old, new);
            }
        }
        Err(GudfError::UnsupportedFormat(format!("{kind:?}")))
    }
}

impl Default for DiffEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_detect_json() {
        let engine = DiffEngine::new();
        let result = engine.diff(r#"{"a": 1}"#, r#"{"a": 2}"#).unwrap();
        assert_eq!(result.format, FormatKind::Json);
    }

    #[test]
    fn test_auto_detect_text() {
        let engine = DiffEngine::new();
        let result = engine.diff("hello world\n", "goodbye world\n").unwrap();
        assert_eq!(result.format, FormatKind::Text);
    }

    #[test]
    fn test_force_format() {
        let engine = DiffEngine::new();
        let result = engine
            .diff_as(FormatKind::Text, r#"{"a": 1}"#, r#"{"a": 2}"#)
            .unwrap();
        assert_eq!(result.format, FormatKind::Text);
    }
}
