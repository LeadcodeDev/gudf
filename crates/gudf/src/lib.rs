pub mod engine;
pub mod error;
pub mod format;
pub mod formats;
pub mod output;
pub mod patch;
pub mod result;

pub use engine::DiffEngine;
pub use error::GudfError;
pub use format::{detect_format, Format, FormatKind};
pub use output::OutputFormatter;
pub use patch::Patchable;
pub use result::{Change, ChangeKind, DiffResult, DiffStats, Location};

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
