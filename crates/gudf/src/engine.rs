use crate::annotations::{annotate_changes, Annotator};
use crate::error::GudfError;
use crate::format::{detect_format, Format, FormatKind};
use crate::formats::cross::{diff_cross, CrossFormatKind};
use crate::formats::json::JsonFormat;
use crate::formats::text::TextFormat;
use crate::formats::toml::TomlFormat;
use crate::formats::yaml::YamlFormat;
use crate::result::DiffResult;
use crate::semantic::SemanticAnalyzer;

pub struct DiffEngine {
    formats: Vec<Box<dyn Format>>,
    annotators: Vec<Box<dyn Annotator>>,
    semantic: Option<SemanticAnalyzer>,
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
            annotators: Vec::new(),
            semantic: None,
        }
    }

    pub fn with_formats(formats: Vec<Box<dyn Format>>) -> Self {
        Self {
            formats,
            annotators: Vec::new(),
            semantic: None,
        }
    }

    pub fn register(&mut self, format: Box<dyn Format>) {
        self.formats.push(format);
    }

    /// Add annotators that will be applied to diff results.
    pub fn with_annotators(mut self, annotators: Vec<Box<dyn Annotator>>) -> Self {
        self.annotators = annotators;
        self
    }

    /// Add a single annotator.
    pub fn add_annotator(&mut self, annotator: Box<dyn Annotator>) {
        self.annotators.push(annotator);
    }

    /// Enable semantic move/rename detection.
    pub fn with_semantic(mut self, analyzer: SemanticAnalyzer) -> Self {
        self.semantic = Some(analyzer);
        self
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
                let mut result = format.diff(old, new)?;
                self.post_process(&mut result);
                return Ok(result);
            }
        }
        Err(GudfError::UnsupportedFormat(format!("{kind:?}")))
    }

    /// Diff two documents of different formats by normalizing to JSON.
    pub fn diff_cross(
        &self,
        old: &str,
        old_kind: CrossFormatKind,
        new: &str,
        new_kind: CrossFormatKind,
    ) -> Result<DiffResult, GudfError> {
        let mut result = diff_cross(old, old_kind, new, new_kind)?;
        self.post_process(&mut result);
        Ok(result)
    }

    fn post_process(&self, result: &mut DiffResult) {
        // Apply annotations
        if !self.annotators.is_empty() {
            annotate_changes(&mut result.changes, &self.annotators);
        }
    }

    /// Run semantic analysis (move/rename detection) on a result.
    /// This consumes and returns a new result because it may restructure changes.
    pub fn analyze_semantic(&self, result: DiffResult) -> DiffResult {
        if let Some(ref analyzer) = self.semantic {
            analyzer.analyze(result)
        } else {
            result
        }
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
    use crate::annotations::SensitiveFieldAnnotator;
    use crate::result::ChangeKind;
    use crate::semantic::SemanticOptions;

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

    #[test]
    fn test_cross_format_diff() {
        let engine = DiffEngine::new();
        let result = engine
            .diff_cross(
                r#"{"name": "test"}"#,
                CrossFormatKind::Json,
                "name: test\n",
                CrossFormatKind::Yaml,
            )
            .unwrap();
        assert_eq!(result.stats.modifications, 0);
    }

    #[test]
    fn test_with_annotators() {
        let engine = DiffEngine::new()
            .with_annotators(vec![Box::new(SensitiveFieldAnnotator)]);
        let result = engine
            .diff(
                r#"{"password": "old"}"#,
                r#"{"password": "new"}"#,
            )
            .unwrap();
        let modified = result
            .changes
            .iter()
            .find(|c| c.kind == ChangeKind::Modified)
            .unwrap();
        assert!(!modified.annotations.is_empty());
    }

    #[test]
    fn test_with_semantic() {
        let engine = DiffEngine::new()
            .with_semantic(SemanticAnalyzer::new(SemanticOptions::default()));
        let result = engine
            .diff(
                r#"{"userName": "Alice"}"#,
                r#"{"user_name": "Alice"}"#,
            )
            .unwrap();
        let result = engine.analyze_semantic(result);
        assert!(result.stats.renames > 0 || result.stats.moves > 0);
    }
}
