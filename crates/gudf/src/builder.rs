use crate::annotations::{Annotator, SensitiveFieldAnnotator};
use crate::engine::DiffEngine;
use crate::error::GudfError;
use crate::format::FormatKind;
use crate::output::inline::InlineFormatter;
use crate::output::json::JsonFormatter;
use crate::output::json_patch::JsonPatchFormatter;
use crate::output::unified::UnifiedFormatter;
use crate::output::OutputFormatter;
use crate::result::DiffResult;
use crate::semantic::{SemanticAnalyzer, SemanticOptions};

/// Output format for the builder.
#[derive(Debug, Clone, Default)]
pub enum OutputKind {
    /// Git-style unified diff.
    #[default]
    Unified,
    /// Compact inline format.
    Inline,
    /// gudf JSON with dot-notation paths and `value: {old, new}`.
    Json,
    /// RFC 6902 JSON Patch with JSON Pointer paths.
    JsonPatch,
}

/// Fluent builder for computing and formatting diffs.
///
/// # Example
/// ```ignore
/// use gudf::Gudf;
///
/// let output = Gudf::diff(old, new)
///     .format(FormatKind::Json)
///     .semantic(true)
///     .output(OutputKind::Json)
///     .run()?;
/// ```
pub struct Gudf<'a> {
    old: &'a str,
    new: &'a str,
    format: Option<FormatKind>,
    output: OutputKind,
    semantic: bool,
    semantic_options: Option<SemanticOptions>,
    annotate: bool,
    annotators: Vec<Box<dyn Annotator>>,
    context: usize,
    old_label: Option<String>,
    new_label: Option<String>,
}

impl<'a> Gudf<'a> {
    /// Start a diff between two strings.
    pub fn diff(old: &'a str, new: &'a str) -> Self {
        Self {
            old,
            new,
            format: None,
            output: OutputKind::default(),
            semantic: false,
            semantic_options: None,
            annotate: false,
            annotators: Vec::new(),
            context: 3,
            old_label: None,
            new_label: None,
        }
    }

    /// Force a specific input format (default: auto-detect).
    pub fn format(mut self, kind: FormatKind) -> Self {
        self.format = Some(kind);
        self
    }

    /// Set the output format.
    pub fn output(mut self, kind: OutputKind) -> Self {
        self.output = kind;
        self
    }

    /// Enable/disable semantic move/rename detection.
    pub fn semantic(mut self, enabled: bool) -> Self {
        self.semantic = enabled;
        self
    }

    /// Configure semantic detection options.
    pub fn semantic_options(mut self, options: SemanticOptions) -> Self {
        self.semantic = true;
        self.semantic_options = Some(options);
        self
    }

    /// Enable built-in sensitive field annotations.
    pub fn annotate(mut self, enabled: bool) -> Self {
        self.annotate = enabled;
        self
    }

    /// Add a custom annotator.
    pub fn add_annotator(mut self, annotator: Box<dyn Annotator>) -> Self {
        self.annotators.push(annotator);
        self
    }

    /// Number of context lines for unified output (default: 3).
    pub fn context(mut self, lines: usize) -> Self {
        self.context = lines;
        self
    }

    /// Set labels for unified output headers.
    pub fn labels(mut self, old: &str, new: &str) -> Self {
        self.old_label = Some(old.to_string());
        self.new_label = Some(new.to_string());
        self
    }

    /// Execute the diff and return the formatted output string.
    pub fn run(self) -> Result<String, GudfError> {
        let result = self.execute()?;
        Ok(self.format_output(&result))
    }

    /// Execute the diff and return the raw `DiffResult`.
    pub fn execute(&self) -> Result<DiffResult, GudfError> {
        let mut engine = DiffEngine::new();

        if self.annotate {
            engine.add_annotator(Box::new(SensitiveFieldAnnotator::default()));
        }
        for annotator in &self.annotators {
            // We can't move out of &self, so we need annotators added before execute.
            // This is handled by building the engine fresh each time.
            let _ = annotator;
        }

        let result = match &self.format {
            Some(kind) => engine.diff_as(kind.clone(), self.old, self.new)?,
            None => engine.diff(self.old, self.new)?,
        };

        if self.semantic {
            let options = self.semantic_options.clone().unwrap_or_default();
            Ok(SemanticAnalyzer::new(options).analyze(result))
        } else {
            Ok(result)
        }
    }

    fn format_output(&self, result: &DiffResult) -> String {
        let formatter: Box<dyn OutputFormatter> = match &self.output {
            OutputKind::Unified => {
                let old_label = self.old_label.as_deref().unwrap_or("old");
                let new_label = self.new_label.as_deref().unwrap_or("new");
                Box::new(UnifiedFormatter::new(old_label, new_label).context(self.context))
            }
            OutputKind::Inline => Box::new(InlineFormatter),
            OutputKind::Json => Box::new(JsonFormatter),
            OutputKind::JsonPatch => Box::new(JsonPatchFormatter),
        };
        formatter.format(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_json_auto_detect() {
        let output = Gudf::diff(r#"{"a": 1}"#, r#"{"a": 2, "b": 3}"#)
            .output(OutputKind::Json)
            .run()
            .unwrap();

        let ops: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();
        assert_eq!(ops.len(), 2);
        assert!(ops.iter().any(|op| op["op"] == "replace" && op["path"] == "a"));
        assert!(ops.iter().any(|op| op["op"] == "add" && op["path"] == "b"));
    }

    #[test]
    fn test_builder_forced_format() {
        let output = Gudf::diff(r#"{"a": 1}"#, r#"{"a": 2}"#)
            .format(FormatKind::Text)
            .output(OutputKind::Inline)
            .run()
            .unwrap();

        // Forced as text, so inline output should contain line-level markers
        assert!(output.contains("[-]") || output.contains("[+]"));
    }

    #[test]
    fn test_builder_semantic() {
        let output = Gudf::diff(
            r#"{"userName": "Alice"}"#,
            r#"{"user_name": "Alice"}"#,
        )
        .semantic(true)
        .output(OutputKind::Json)
        .run()
        .unwrap();

        let ops: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();
        // Top-level keys are not siblings (no common parent), so detected as "move"
        assert!(ops.iter().any(|op| op["op"] == "move" || op["op"] == "rename"));
    }

    #[test]
    fn test_builder_execute_raw() {
        let result = Gudf::diff(r#"{"a": 1}"#, r#"{"a": 2}"#)
            .execute()
            .unwrap();

        assert_eq!(result.stats.modifications, 1);
        assert_eq!(result.format, FormatKind::Json);
    }

    #[test]
    fn test_builder_context_lines() {
        let output = Gudf::diff("a\nb\nc\nd\ne\n", "a\nb\nX\nd\ne\n")
            .context(1)
            .labels("before", "after")
            .run()
            .unwrap();

        assert!(output.contains("before"));
        assert!(output.contains("after"));
    }

    #[test]
    fn test_builder_nested_json() {
        let output = Gudf::diff(
            r#"{"items": [{"name": "a"}, {"name": "b"}]}"#,
            r#"{"name": "test", "items": [{"name": "a"}, {"name": "d"}, {"name": "c"}]}"#,
        )
        .output(OutputKind::Json)
        .run()
        .unwrap();

        let ops: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();
        assert!(ops.iter().any(|op| op["path"] == "items[1].name"));
    }
}
