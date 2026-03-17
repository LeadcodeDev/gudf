pub mod annotations;
pub mod engine;
pub mod error;
pub mod format;
pub mod formats;
pub mod merge;
pub mod mutation;
pub mod output;
pub mod patch;
pub mod pipeline;
pub mod result;
pub mod semantic;

pub use annotations::{
    annotate_changes, Annotation, AnnotationValue, Annotator, AstNodeAnnotator,
    PathDepthAnnotator, SensitiveFieldAnnotator, Severity,
};
pub use engine::DiffEngine;
pub use error::GudfError;
pub use format::{detect_format, Format, FormatKind};
pub use formats::cross::{diff_cross, CrossFormatKind};
pub use merge::{merge, merge_json, Conflict, MergeResult, MergeStrategy};
pub use mutation::{ContentSha, ExprDiffBuilder, LogEntry, MutationChain, MutationState};
pub use output::OutputFormatter;
pub use patch::Patchable;
pub use pipeline::DiffPipeline;
pub use result::{Change, ChangeKind, DiffResult, DiffStats, Location};
pub use semantic::{SemanticAnalyzer, SemanticOptions};

use formats::code::CodeFormat;
use formats::json::JsonFormat;
use formats::text::TextFormat;
use formats::toml::TomlFormat;
use formats::yaml::YamlFormat;

/// Diff two strings with auto-detected format.
pub fn diff(old: &str, new: &str) -> Result<DiffResult, GudfError> {
    DiffEngine::new().diff(old, new)
}

/// Diff two strings as plain text.
pub fn diff_text(old: &str, new: &str) -> Result<DiffResult, GudfError> {
    TextFormat.diff(old, new)
}

/// Diff two strings as JSON.
pub fn diff_json(old: &str, new: &str) -> Result<DiffResult, GudfError> {
    JsonFormat.diff(old, new)
}

/// Diff two strings as TOML.
pub fn diff_toml(old: &str, new: &str) -> Result<DiffResult, GudfError> {
    TomlFormat.diff(old, new)
}

/// Diff two strings as YAML.
pub fn diff_yaml(old: &str, new: &str) -> Result<DiffResult, GudfError> {
    YamlFormat.diff(old, new)
}

/// Diff two strings as code with a specified language.
pub fn diff_code(old: &str, new: &str, language: &str) -> Result<DiffResult, GudfError> {
    CodeFormat::new(language).diff(old, new)
}

/// Apply changes to reconstruct a document (auto-detect format).
pub fn patch(original: &str, changes: &[Change]) -> Result<String, GudfError> {
    patch::patch(original, changes)
}

/// Apply changes to reconstruct a document with a forced format.
pub fn patch_as(
    kind: FormatKind,
    original: &str,
    changes: &[Change],
) -> Result<String, GudfError> {
    patch::patch_as(kind, original, changes)
}
