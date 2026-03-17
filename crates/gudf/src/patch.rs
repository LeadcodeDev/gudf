use crate::error::GudfError;
use crate::format::{detect_format, FormatKind};
use crate::result::{Change, ChangeKind};

pub trait Patchable {
    fn apply(&self, original: &str, changes: &[Change]) -> Result<String, GudfError>;
}

pub fn patch(original: &str, changes: &[Change]) -> Result<String, GudfError> {
    let kind = detect_format(original);
    patch_as(kind, original, changes)
}

pub fn patch_as(
    kind: FormatKind,
    original: &str,
    changes: &[Change],
) -> Result<String, GudfError> {
    match kind {
        FormatKind::Text | FormatKind::Code(_) => patch_text(original, changes),
        FormatKind::Json => patch_json(original, changes),
        FormatKind::Toml => patch_toml(original, changes),
        FormatKind::Yaml => patch_yaml(original, changes),
    }
}

fn patch_text(original: &str, changes: &[Change]) -> Result<String, GudfError> {
    let mut lines: Vec<String> = original.lines().map(|l| l.to_string()).collect();
    let mut offset: isize = 0;

    for change in changes {
        let line_idx = change
            .location
            .as_ref()
            .map(|l| l.line as isize - 1 + offset)
            .unwrap_or(0) as usize;

        match change.kind {
            ChangeKind::Added => {
                let value = change
                    .new_value
                    .as_deref()
                    .unwrap_or("")
                    .trim_end_matches('\n')
                    .to_string();
                if line_idx <= lines.len() {
                    lines.insert(line_idx, value);
                    offset += 1;
                }
            }
            ChangeKind::Removed => {
                if line_idx < lines.len() {
                    lines.remove(line_idx);
                    offset -= 1;
                }
            }
            ChangeKind::Modified | ChangeKind::Moved | ChangeKind::Renamed => {
                let value = change
                    .new_value
                    .as_deref()
                    .unwrap_or("")
                    .trim_end_matches('\n')
                    .to_string();
                if line_idx < lines.len() {
                    lines[line_idx] = value;
                }
            }
            ChangeKind::Unchanged => {}
        }
    }

    let mut result = lines.join("\n");
    if original.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

fn patch_json(original: &str, changes: &[Change]) -> Result<String, GudfError> {
    let mut value: serde_json::Value =
        serde_json::from_str(original).map_err(|e| GudfError::ParseError(e.to_string()))?;

    for change in changes {
        if matches!(change.kind, ChangeKind::Unchanged) {
            continue;
        }
        let path = match &change.path {
            Some(p) if p != "$" => p.clone(),
            _ => continue,
        };

        match change.kind {
            ChangeKind::Added | ChangeKind::Modified | ChangeKind::Moved | ChangeKind::Renamed => {
                let new_val: serde_json::Value = change
                    .new_value
                    .as_deref()
                    .and_then(|v| serde_json::from_str(v).ok())
                    .unwrap_or(serde_json::Value::Null);
                set_json_path(&mut value, &path, new_val)?;
            }
            ChangeKind::Removed => {
                remove_json_path(&mut value, &path)?;
            }
            ChangeKind::Unchanged => {}
        }
    }

    serde_json::to_string_pretty(&value).map_err(|e| GudfError::PatchError(e.to_string()))
}

fn patch_toml(original: &str, changes: &[Change]) -> Result<String, GudfError> {
    let json_str =
        serde_json::to_string(&original.parse::<toml::Value>().map_err(|e| {
            GudfError::ParseError(e.to_string())
        })?)
        .map_err(|e| GudfError::ParseError(e.to_string()))?;

    let patched_json = patch_json(&json_str, changes)?;

    let json_val: serde_json::Value =
        serde_json::from_str(&patched_json).map_err(|e| GudfError::ParseError(e.to_string()))?;
    let toml_val: toml::Value =
        serde_json::from_value(json_val).map_err(|e| GudfError::PatchError(e.to_string()))?;

    Ok(toml::to_string_pretty(&toml_val).map_err(|e| GudfError::PatchError(e.to_string()))?)
}

fn patch_yaml(original: &str, changes: &[Change]) -> Result<String, GudfError> {
    let json_val: serde_json::Value =
        serde_yaml::from_str(original).map_err(|e| GudfError::ParseError(e.to_string()))?;
    let json_str =
        serde_json::to_string(&json_val).map_err(|e| GudfError::ParseError(e.to_string()))?;

    let patched_json = patch_json(&json_str, changes)?;

    let patched_val: serde_json::Value =
        serde_json::from_str(&patched_json).map_err(|e| GudfError::ParseError(e.to_string()))?;

    serde_yaml::to_string(&patched_val).map_err(|e| GudfError::PatchError(e.to_string()))
}

fn set_json_path(
    root: &mut serde_json::Value,
    path: &str,
    new_value: serde_json::Value,
) -> Result<(), GudfError> {
    let parts = parse_path(path);
    let mut current = root;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            match part {
                PathPart::Key(key) => {
                    if let serde_json::Value::Object(map) = current {
                        map.insert(key.clone(), new_value);
                        return Ok(());
                    }
                }
                PathPart::Index(idx) => {
                    if let serde_json::Value::Array(arr) = current {
                        if *idx < arr.len() {
                            arr[*idx] = new_value;
                            return Ok(());
                        }
                    }
                }
            }
            return Err(GudfError::PatchError(format!(
                "Cannot set value at path: {path}"
            )));
        }

        current = match part {
            PathPart::Key(key) => current
                .get_mut(key.as_str())
                .ok_or_else(|| GudfError::PatchError(format!("Path not found: {path}")))?,
            PathPart::Index(idx) => current
                .get_mut(*idx)
                .ok_or_else(|| GudfError::PatchError(format!("Index out of bounds: {path}")))?,
        };
    }

    Ok(())
}

fn remove_json_path(root: &mut serde_json::Value, path: &str) -> Result<(), GudfError> {
    let parts = parse_path(path);
    let mut current = root;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            match part {
                PathPart::Key(key) => {
                    if let serde_json::Value::Object(map) = current {
                        map.remove(key.as_str());
                        return Ok(());
                    }
                }
                PathPart::Index(idx) => {
                    if let serde_json::Value::Array(arr) = current {
                        if *idx < arr.len() {
                            arr.remove(*idx);
                            return Ok(());
                        }
                    }
                }
            }
            return Err(GudfError::PatchError(format!(
                "Cannot remove at path: {path}"
            )));
        }

        current = match part {
            PathPart::Key(key) => current
                .get_mut(key.as_str())
                .ok_or_else(|| GudfError::PatchError(format!("Path not found: {path}")))?,
            PathPart::Index(idx) => current
                .get_mut(*idx)
                .ok_or_else(|| GudfError::PatchError(format!("Index out of bounds: {path}")))?,
        };
    }

    Ok(())
}

enum PathPart {
    Key(String),
    Index(usize),
}

fn parse_path(path: &str) -> Vec<PathPart> {
    let mut parts = Vec::new();
    let mut current = String::new();

    for ch in path.chars() {
        match ch {
            '.' => {
                if !current.is_empty() {
                    parts.push(PathPart::Key(current.clone()));
                    current.clear();
                }
            }
            '[' => {
                if !current.is_empty() {
                    parts.push(PathPart::Key(current.clone()));
                    current.clear();
                }
            }
            ']' => {
                if let Ok(idx) = current.parse::<usize>() {
                    parts.push(PathPart::Index(idx));
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        parts.push(PathPart::Key(current));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::Location;

    #[test]
    fn test_patch_text_add() {
        let original = "line1\nline2\n";
        let changes = vec![Change {
            kind: ChangeKind::Added,
            path: None,
            old_value: None,
            new_value: Some("new_line\n".to_string()),
            location: Some(Location {
                line: 2,
                column: None,
            }),
            annotations: Vec::new(),
        }];
        let result = patch_text(original, &changes).unwrap();
        assert!(result.contains("new_line"));
    }

    #[test]
    fn test_patch_json_modify() {
        let original = r#"{"name": "Alice", "age": 30}"#;
        let changes = vec![Change {
            kind: ChangeKind::Modified,
            path: Some("name".to_string()),
            old_value: Some("\"Alice\"".to_string()),
            new_value: Some("\"Bob\"".to_string()),
            location: None,
            annotations: Vec::new(),
        }];
        let result = patch_json(original, &changes).unwrap();
        assert!(result.contains("Bob"));
        assert!(!result.contains("Alice"));
    }

    #[test]
    fn test_patch_json_remove() {
        let original = r#"{"name": "Alice", "age": 30}"#;
        let changes = vec![Change {
            kind: ChangeKind::Removed,
            path: Some("age".to_string()),
            old_value: Some("30".to_string()),
            new_value: None,
            location: None,
            annotations: Vec::new(),
        }];
        let result = patch_json(original, &changes).unwrap();
        assert!(!result.contains("age"));
    }

    #[test]
    fn test_roundtrip_json() {
        let old = r#"{"a": 1, "b": 2}"#;
        let new = r#"{"a": 1, "b": 3, "c": 4}"#;

        let engine = crate::engine::DiffEngine::new();
        let diff_result = engine.diff_as(FormatKind::Json, old, new).unwrap();
        let patched = patch_json(old, &diff_result.changes).unwrap();
        let patched_val: serde_json::Value = serde_json::from_str(&patched).unwrap();
        let expected_val: serde_json::Value = serde_json::from_str(new).unwrap();
        assert_eq!(patched_val, expected_val);
    }
}
