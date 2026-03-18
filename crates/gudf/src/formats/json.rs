use serde_json::Value;

use crate::error::GudfError;
use crate::format::{Format, FormatKind};
use crate::path;
use crate::result::{Change, ChangeKind, DiffResult, DiffStats};

pub struct JsonFormat;

impl Format for JsonFormat {
    fn kind(&self) -> FormatKind {
        FormatKind::Json
    }

    fn diff(&self, old: &str, new: &str) -> Result<DiffResult, GudfError> {
        let old_val: Value =
            serde_json::from_str(old).map_err(|e| GudfError::ParseError(e.to_string()))?;
        let new_val: Value =
            serde_json::from_str(new).map_err(|e| GudfError::ParseError(e.to_string()))?;

        let mut changes = Vec::new();
        diff_values(&old_val, &new_val, String::new(), &mut changes);

        let stats = DiffStats::from_changes(&changes);
        Ok(DiffResult {
            changes,
            format: FormatKind::Json,
            stats,
        })
    }
}

pub(crate) fn diff_values(old: &Value, new: &Value, path: String, changes: &mut Vec<Change>) {
    if old == new {
        changes.push(Change {
            kind: ChangeKind::Unchanged,
            path: Some(if path.is_empty() {
                "$".to_string()
            } else {
                path
            }),
            old_value: Some(old.to_string()),
            new_value: Some(new.to_string()),
            location: None,
            annotations: Vec::new(),
        });
        return;
    }

    match (old, new) {
        (Value::Object(old_map), Value::Object(new_map)) => {
            for (key, old_val) in old_map {
                let child_path = path::append_key(&path, key);
                match new_map.get(key) {
                    Some(new_val) => diff_values(old_val, new_val, child_path, changes),
                    None => changes.push(Change {
                        kind: ChangeKind::Removed,
                        path: Some(child_path),
                        old_value: Some(old_val.to_string()),
                        new_value: None,
                        location: None,
                        annotations: Vec::new(),
                    }),
                }
            }
            for (key, new_val) in new_map {
                if !old_map.contains_key(key) {
                    let child_path = path::append_key(&path, key);
                    changes.push(Change {
                        kind: ChangeKind::Added,
                        path: Some(child_path),
                        old_value: None,
                        new_value: Some(new_val.to_string()),
                        location: None,
                        annotations: Vec::new(),
                    });
                }
            }
        }
        (Value::Array(old_arr), Value::Array(new_arr)) => {
            let max_len = old_arr.len().max(new_arr.len());
            for i in 0..max_len {
                let child_path = path::append_index(&path, i);
                match (old_arr.get(i), new_arr.get(i)) {
                    (Some(old_val), Some(new_val)) => {
                        diff_values(old_val, new_val, child_path, changes);
                    }
                    (Some(old_val), None) => {
                        changes.push(Change {
                            kind: ChangeKind::Removed,
                            path: Some(child_path),
                            old_value: Some(old_val.to_string()),
                            new_value: None,
                            location: None,
                            annotations: Vec::new(),
                        });
                    }
                    (None, Some(new_val)) => {
                        changes.push(Change {
                            kind: ChangeKind::Added,
                            path: Some(child_path),
                            old_value: None,
                            new_value: Some(new_val.to_string()),
                            location: None,
                            annotations: Vec::new(),
                        });
                    }
                    (None, None) => unreachable!(),
                }
            }
        }
        _ => {
            changes.push(Change {
                kind: ChangeKind::Modified,
                path: Some(if path.is_empty() {
                    "$".to_string()
                } else {
                    path
                }),
                old_value: Some(old.to_string()),
                new_value: Some(new.to_string()),
                location: None,
                annotations: Vec::new(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_json() {
        let format = JsonFormat;
        let result = format.diff(r#"{"a": 1}"#, r#"{"a": 1}"#).unwrap();
        assert_eq!(result.stats.additions, 0);
        assert_eq!(result.stats.deletions, 0);
        assert_eq!(result.stats.modifications, 0);
    }

    #[test]
    fn test_added_key() {
        let format = JsonFormat;
        let result = format.diff(r#"{"a": 1}"#, r#"{"a": 1, "b": 2}"#).unwrap();
        assert_eq!(result.stats.additions, 1);
    }

    #[test]
    fn test_removed_key() {
        let format = JsonFormat;
        let result = format.diff(r#"{"a": 1, "b": 2}"#, r#"{"a": 1}"#).unwrap();
        assert_eq!(result.stats.deletions, 1);
    }

    #[test]
    fn test_modified_value() {
        let format = JsonFormat;
        let result = format.diff(r#"{"a": 1}"#, r#"{"a": 2}"#).unwrap();
        assert_eq!(result.stats.modifications, 1);
    }

    #[test]
    fn test_nested_diff() {
        let format = JsonFormat;
        let result = format
            .diff(
                r#"{"user": {"name": "Alice", "age": 30}}"#,
                r#"{"user": {"name": "Bob", "age": 30}}"#,
            )
            .unwrap();
        assert_eq!(result.stats.modifications, 1);
        let modified = result
            .changes
            .iter()
            .find(|c| c.kind == ChangeKind::Modified)
            .unwrap();
        assert_eq!(modified.path.as_deref(), Some("user.name"));
    }

    #[test]
    fn test_array_diff() {
        let format = JsonFormat;
        let result = format.diff(r#"[1, 2, 3]"#, r#"[1, 2, 4]"#).unwrap();
        assert_eq!(result.stats.modifications, 1);
    }
}
