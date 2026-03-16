use crate::error::GudfError;
use crate::format::{Format, FormatKind};
use crate::formats::json::diff_values;
use crate::result::{Change, DiffResult, DiffStats};

pub struct YamlFormat;

impl Format for YamlFormat {
    fn kind(&self) -> FormatKind {
        FormatKind::Yaml
    }

    fn diff(&self, old: &str, new: &str) -> Result<DiffResult, GudfError> {
        let old_yaml: serde_json::Value =
            serde_yaml::from_str(old).map_err(|e| GudfError::ParseError(e.to_string()))?;
        let new_yaml: serde_json::Value =
            serde_yaml::from_str(new).map_err(|e| GudfError::ParseError(e.to_string()))?;

        let mut changes: Vec<Change> = Vec::new();
        diff_values(&old_yaml, &new_yaml, String::new(), &mut changes);

        let stats = DiffStats::from_changes(&changes);
        Ok(DiffResult {
            changes,
            format: FormatKind::Yaml,
            stats,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::ChangeKind;

    #[test]
    fn test_identical_yaml() {
        let format = YamlFormat;
        let result = format.diff("name: test\n", "name: test\n").unwrap();
        assert_eq!(result.stats.modifications, 0);
    }

    #[test]
    fn test_modified_value() {
        let format = YamlFormat;
        let result = format.diff("name: old\n", "name: new\n").unwrap();
        assert_eq!(result.stats.modifications, 1);
        let modified = result
            .changes
            .iter()
            .find(|c| c.kind == ChangeKind::Modified)
            .unwrap();
        assert_eq!(modified.path.as_deref(), Some("name"));
    }

    #[test]
    fn test_nested_yaml() {
        let format = YamlFormat;
        let old = "user:\n  name: Alice\n  age: 30\n";
        let new = "user:\n  name: Bob\n  age: 30\n";
        let result = format.diff(old, new).unwrap();
        assert_eq!(result.stats.modifications, 1);
    }
}
