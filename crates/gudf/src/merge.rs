use std::collections::{BTreeMap, BTreeSet};

use crate::error::GudfError;
use crate::format::FormatKind;
use crate::formats::json::diff_values;
use crate::result::{Change, ChangeKind};

/// The result of a three-way merge.
#[derive(Debug, Clone)]
pub struct MergeResult {
    /// The merged JSON value (only fully valid when there are no conflicts).
    pub merged: serde_json::Value,
    /// Conflicts that could not be auto-resolved.
    pub conflicts: Vec<Conflict>,
    /// Strategy used for the merge.
    pub strategy: MergeStrategy,
}

impl MergeResult {
    /// Returns true if the merge completed without conflicts.
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
    }
}

/// A conflict between left and right changes.
#[derive(Debug, Clone)]
pub struct Conflict {
    /// The path where the conflict occurred.
    pub path: String,
    /// The base value (before either change).
    pub base: Option<String>,
    /// The left side's value.
    pub left: Option<String>,
    /// The right side's value.
    pub right: Option<String>,
}

/// Strategy for resolving merge conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Keep left side on conflict.
    Ours,
    /// Keep right side on conflict.
    Theirs,
    /// Report conflicts without resolving.
    Manual,
}

impl Default for MergeStrategy {
    fn default() -> Self {
        Self::Manual
    }
}

/// Three-way merge for structured data.
///
/// Computes `diff(base, left)` and `diff(base, right)`, then merges
/// non-conflicting changes and reports conflicts for overlapping paths.
pub fn merge_json(
    base: &serde_json::Value,
    left: &serde_json::Value,
    right: &serde_json::Value,
    strategy: MergeStrategy,
) -> MergeResult {
    let mut left_changes = Vec::new();
    diff_values(base, left, String::new(), &mut left_changes);

    let mut right_changes = Vec::new();
    diff_values(base, right, String::new(), &mut right_changes);

    // Index changes by path
    let left_map = index_changes(&left_changes);
    let right_map = index_changes(&right_changes);

    let all_paths: BTreeSet<&str> = left_map
        .keys()
        .chain(right_map.keys())
        .map(|s| s.as_str())
        .collect();

    let mut merged = base.clone();
    let mut conflicts = Vec::new();

    for path in all_paths {
        let left_change = left_map.get(path);
        let right_change = right_map.get(path);

        match (left_change, right_change) {
            (Some(lc), None) => {
                // Only left changed
                apply_change(&mut merged, lc);
            }
            (None, Some(rc)) => {
                // Only right changed
                apply_change(&mut merged, rc);
            }
            (Some(lc), Some(rc)) => {
                // Both changed — check if they agree
                if changes_agree(lc, rc) {
                    apply_change(&mut merged, lc);
                } else if lc.kind == ChangeKind::Unchanged {
                    apply_change(&mut merged, rc);
                } else if rc.kind == ChangeKind::Unchanged {
                    apply_change(&mut merged, lc);
                } else {
                    // Conflict
                    match strategy {
                        MergeStrategy::Ours => {
                            apply_change(&mut merged, lc);
                        }
                        MergeStrategy::Theirs => {
                            apply_change(&mut merged, rc);
                        }
                        MergeStrategy::Manual => {
                            conflicts.push(Conflict {
                                path: path.to_string(),
                                base: lc.old_value.clone(),
                                left: lc.new_value.clone().or_else(|| lc.old_value.clone()),
                                right: rc.new_value.clone().or_else(|| rc.old_value.clone()),
                            });
                        }
                    }
                }
            }
            (None, None) => {}
        }
    }

    MergeResult {
        merged,
        conflicts,
        strategy,
    }
}

/// Merge two string documents in the specified format.
pub fn merge(
    base: &str,
    left: &str,
    right: &str,
    format: FormatKind,
    strategy: MergeStrategy,
) -> Result<MergeResult, GudfError> {
    let parse = |s: &str, fmt: &FormatKind| -> Result<serde_json::Value, GudfError> {
        match fmt {
            FormatKind::Json => serde_json::from_str(s)
                .map_err(|e| GudfError::ParseError(format!("JSON: {e}"))),
            FormatKind::Toml => {
                let toml_val: toml::Value = s
                    .parse()
                    .map_err(|e: toml::de::Error| GudfError::ParseError(format!("TOML: {e}")))?;
                Ok(crate::formats::toml::toml_to_json(toml_val))
            }
            FormatKind::Yaml => serde_yaml::from_str(s)
                .map_err(|e| GudfError::ParseError(format!("YAML: {e}"))),
            _ => Err(GudfError::UnsupportedFormat(
                "Three-way merge only supports structured formats (JSON, TOML, YAML)".to_string(),
            )),
        }
    };

    let base_val = parse(base, &format)?;
    let left_val = parse(left, &format)?;
    let right_val = parse(right, &format)?;

    Ok(merge_json(&base_val, &left_val, &right_val, strategy))
}

fn index_changes(changes: &[Change]) -> BTreeMap<String, &Change> {
    let mut map = BTreeMap::new();
    for change in changes {
        if let Some(path) = &change.path {
            map.insert(path.clone(), change);
        }
    }
    map
}

fn changes_agree(a: &Change, b: &Change) -> bool {
    a.kind == b.kind && a.new_value == b.new_value
}

fn apply_change(root: &mut serde_json::Value, change: &Change) {
    let Some(path) = &change.path else { return };
    if path == "$" {
        return;
    }

    match change.kind {
        ChangeKind::Added | ChangeKind::Modified => {
            if let Some(new_val_str) = &change.new_value {
                if let Ok(new_val) = serde_json::from_str(new_val_str) {
                    set_path(root, path, new_val);
                }
            }
        }
        ChangeKind::Removed => {
            remove_path(root, path);
        }
        _ => {}
    }
}

fn set_path(root: &mut serde_json::Value, path: &str, value: serde_json::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = root;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let serde_json::Value::Object(map) = current {
                map.insert(part.to_string(), value);
                return;
            }
        } else {
            if let serde_json::Value::Object(map) = current {
                if !map.contains_key(*part) {
                    map.insert(
                        part.to_string(),
                        serde_json::Value::Object(serde_json::Map::new()),
                    );
                }
                current = map.get_mut(*part).unwrap();
            } else {
                return;
            }
        }
    }
}

fn remove_path(root: &mut serde_json::Value, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = root;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let serde_json::Value::Object(map) = current {
                map.remove(*part);
            }
            return;
        }
        current = match current {
            serde_json::Value::Object(map) => {
                if let Some(v) = map.get_mut(*part) {
                    v
                } else {
                    return;
                }
            }
            _ => return,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_merge_disjoint_paths() {
        let base = serde_json::json!({"port": 3000, "host": "localhost"});
        let left = serde_json::json!({"port": 8080, "host": "localhost"});
        let right = serde_json::json!({"port": 3000, "host": "example.com"});

        let result = merge_json(&base, &left, &right, MergeStrategy::Manual);

        assert!(result.is_clean());
        assert_eq!(result.merged["port"], 8080);
        assert_eq!(result.merged["host"], "example.com");
    }

    #[test]
    fn test_conflict_detection() {
        let base = serde_json::json!({"port": 3000});
        let left = serde_json::json!({"port": 8080});
        let right = serde_json::json!({"port": 9090});

        let result = merge_json(&base, &left, &right, MergeStrategy::Manual);

        assert!(!result.is_clean());
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].path, "port");
    }

    #[test]
    fn test_conflict_ours_strategy() {
        let base = serde_json::json!({"port": 3000});
        let left = serde_json::json!({"port": 8080});
        let right = serde_json::json!({"port": 9090});

        let result = merge_json(&base, &left, &right, MergeStrategy::Ours);

        assert!(result.is_clean());
        assert_eq!(result.merged["port"], 8080);
    }

    #[test]
    fn test_conflict_theirs_strategy() {
        let base = serde_json::json!({"port": 3000});
        let left = serde_json::json!({"port": 8080});
        let right = serde_json::json!({"port": 9090});

        let result = merge_json(&base, &left, &right, MergeStrategy::Theirs);

        assert!(result.is_clean());
        assert_eq!(result.merged["port"], 9090);
    }

    #[test]
    fn test_same_change_both_sides() {
        let base = serde_json::json!({"port": 3000});
        let left = serde_json::json!({"port": 8080});
        let right = serde_json::json!({"port": 8080});

        let result = merge_json(&base, &left, &right, MergeStrategy::Manual);

        assert!(result.is_clean());
        assert_eq!(result.merged["port"], 8080);
    }

    #[test]
    fn test_one_side_adds() {
        let base = serde_json::json!({"a": 1});
        let left = serde_json::json!({"a": 1, "b": 2});
        let right = serde_json::json!({"a": 1});

        let result = merge_json(&base, &left, &right, MergeStrategy::Manual);

        assert!(result.is_clean());
        assert_eq!(result.merged["b"], 2);
    }

    #[test]
    fn test_one_side_removes() {
        let base = serde_json::json!({"a": 1, "b": 2});
        let left = serde_json::json!({"a": 1});
        let right = serde_json::json!({"a": 1, "b": 2});

        let result = merge_json(&base, &left, &right, MergeStrategy::Manual);

        assert!(result.is_clean());
        assert!(result.merged.get("b").is_none());
    }

    #[test]
    fn test_merge_from_strings() {
        let base = r#"{"a": 1, "b": 2}"#;
        let left = r#"{"a": 10, "b": 2}"#;
        let right = r#"{"a": 1, "b": 20}"#;

        let result = merge(base, left, right, FormatKind::Json, MergeStrategy::Manual).unwrap();

        assert!(result.is_clean());
        assert_eq!(result.merged["a"], 10);
        assert_eq!(result.merged["b"], 20);
    }

    #[test]
    fn test_nested_clean_merge() {
        let base = serde_json::json!({"config": {"port": 3000, "host": "localhost"}});
        let left = serde_json::json!({"config": {"port": 8080, "host": "localhost"}});
        let right = serde_json::json!({"config": {"port": 3000, "host": "example.com"}});

        let result = merge_json(&base, &left, &right, MergeStrategy::Manual);

        assert!(result.is_clean());
        assert_eq!(result.merged["config"]["port"], 8080);
        assert_eq!(result.merged["config"]["host"], "example.com");
    }
}
