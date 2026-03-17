use crate::error::GudfError;
use crate::format::FormatKind;
use crate::formats::json::diff_values;
use crate::formats::toml::toml_to_json;
use crate::result::{Change, DiffResult, DiffStats};

/// Supported format kinds for cross-format diffing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossFormatKind {
    Json,
    Toml,
    Yaml,
}

impl CrossFormatKind {
    /// Detect format kind from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }

    /// Parse content into a normalized `serde_json::Value`.
    pub fn parse_to_json(&self, content: &str) -> Result<serde_json::Value, GudfError> {
        match self {
            Self::Json => serde_json::from_str(content)
                .map_err(|e| GudfError::ParseError(format!("JSON: {e}"))),
            Self::Toml => {
                let toml_val: toml::Value = content
                    .parse()
                    .map_err(|e: toml::de::Error| GudfError::ParseError(format!("TOML: {e}")))?;
                Ok(toml_to_json(toml_val))
            }
            Self::Yaml => serde_yaml::from_str(content)
                .map_err(|e| GudfError::ParseError(format!("YAML: {e}"))),
        }
    }
}

/// Diff two documents of potentially different formats by normalizing both to JSON.
pub fn diff_cross(
    old: &str,
    old_kind: CrossFormatKind,
    new: &str,
    new_kind: CrossFormatKind,
) -> Result<DiffResult, GudfError> {
    let old_json = old_kind.parse_to_json(old)?;
    let new_json = new_kind.parse_to_json(new)?;

    let mut changes: Vec<Change> = Vec::new();
    diff_values(&old_json, &new_json, String::new(), &mut changes);

    let stats = DiffStats::from_changes(&changes);
    Ok(DiffResult {
        changes,
        format: FormatKind::Json, // normalized representation
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::ChangeKind;

    #[test]
    fn test_toml_to_yaml_identical_content() {
        let toml_content = r#"
name = "test"
version = "1.0"
"#;
        let yaml_content = "name: test\nversion: '1.0'\n";

        let result = diff_cross(
            toml_content,
            CrossFormatKind::Toml,
            yaml_content,
            CrossFormatKind::Yaml,
        )
        .unwrap();

        assert_eq!(result.stats.additions, 0);
        assert_eq!(result.stats.deletions, 0);
        assert_eq!(result.stats.modifications, 0);
    }

    #[test]
    fn test_json_to_yaml_with_changes() {
        let json_content = r#"{"name": "old", "port": 3000}"#;
        let yaml_content = "name: new\nport: 3000\n";

        let result = diff_cross(
            json_content,
            CrossFormatKind::Json,
            yaml_content,
            CrossFormatKind::Yaml,
        )
        .unwrap();

        assert_eq!(result.stats.modifications, 1);
        let modified = result
            .changes
            .iter()
            .find(|c| c.kind == ChangeKind::Modified)
            .unwrap();
        assert_eq!(modified.path.as_deref(), Some("name"));
    }

    #[test]
    fn test_toml_to_json_with_additions() {
        let toml_content = "name = \"test\"\n";
        let json_content = r#"{"name": "test", "version": "2.0"}"#;

        let result = diff_cross(
            toml_content,
            CrossFormatKind::Toml,
            json_content,
            CrossFormatKind::Json,
        )
        .unwrap();

        assert_eq!(result.stats.additions, 1);
    }

    #[test]
    fn test_from_extension() {
        assert_eq!(
            CrossFormatKind::from_extension("json"),
            Some(CrossFormatKind::Json)
        );
        assert_eq!(
            CrossFormatKind::from_extension("toml"),
            Some(CrossFormatKind::Toml)
        );
        assert_eq!(
            CrossFormatKind::from_extension("yaml"),
            Some(CrossFormatKind::Yaml)
        );
        assert_eq!(
            CrossFormatKind::from_extension("yml"),
            Some(CrossFormatKind::Yaml)
        );
        assert_eq!(CrossFormatKind::from_extension("txt"), None);
    }

    #[test]
    fn test_nested_cross_format() {
        let toml_content = r#"
[database]
host = "localhost"
port = 5432
"#;
        let yaml_content = "database:\n  host: localhost\n  port: 5433\n";

        let result = diff_cross(
            toml_content,
            CrossFormatKind::Toml,
            yaml_content,
            CrossFormatKind::Yaml,
        )
        .unwrap();

        assert_eq!(result.stats.modifications, 1);
        let modified = result
            .changes
            .iter()
            .find(|c| c.kind == ChangeKind::Modified)
            .unwrap();
        assert_eq!(modified.path.as_deref(), Some("database.port"));
    }
}
