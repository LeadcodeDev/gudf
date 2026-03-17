use crate::error::GudfError;
use crate::format::{Format, FormatKind};
use crate::formats::json::diff_values;
use crate::result::{Change, DiffResult, DiffStats};

pub struct TomlFormat;

impl Format for TomlFormat {
    fn kind(&self) -> FormatKind {
        FormatKind::Toml
    }

    fn diff(&self, old: &str, new: &str) -> Result<DiffResult, GudfError> {
        let old_toml: toml::Value =
            old.parse().map_err(|e: toml::de::Error| GudfError::ParseError(e.to_string()))?;
        let new_toml: toml::Value =
            new.parse().map_err(|e: toml::de::Error| GudfError::ParseError(e.to_string()))?;

        let old_json = toml_to_json(old_toml);
        let new_json = toml_to_json(new_toml);

        let mut changes: Vec<Change> = Vec::new();
        diff_values(&old_json, &new_json, String::new(), &mut changes);

        let stats = DiffStats::from_changes(&changes);
        Ok(DiffResult {
            changes,
            format: FormatKind::Toml,
            stats,
        })
    }
}

pub(crate) fn toml_to_json(value: toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(table) => {
            let map = table
                .into_iter()
                .map(|(k, v)| (k, toml_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::ChangeKind;

    #[test]
    fn test_identical_toml() {
        let format = TomlFormat;
        let result = format.diff("name = \"test\"\n", "name = \"test\"\n").unwrap();
        assert_eq!(result.stats.modifications, 0);
    }

    #[test]
    fn test_modified_value() {
        let format = TomlFormat;
        let result = format
            .diff("name = \"old\"\n", "name = \"new\"\n")
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
    fn test_added_key() {
        let format = TomlFormat;
        let result = format
            .diff("name = \"test\"\n", "name = \"test\"\nversion = \"1.0\"\n")
            .unwrap();
        assert_eq!(result.stats.additions, 1);
    }
}
