use crate::output::OutputFormatter;
use crate::result::{ChangeKind, DiffResult};

/// Outputs diff in JSON Patch format (RFC 6902).
pub struct JsonPatchFormatter;

impl OutputFormatter for JsonPatchFormatter {
    fn format(&self, result: &DiffResult) -> String {
        let mut ops: Vec<serde_json::Value> = Vec::new();

        for change in &result.changes {
            let path = change
                .path
                .as_deref()
                .unwrap_or("")
                .replace('.', "/");

            let json_path = if path.is_empty() || path == "$" {
                String::new()
            } else {
                format!("/{path}")
            };

            match change.kind {
                ChangeKind::Added => {
                    let value = change
                        .new_value
                        .as_deref()
                        .and_then(|v| serde_json::from_str(v).ok())
                        .unwrap_or(serde_json::Value::Null);
                    ops.push(serde_json::json!({
                        "op": "add",
                        "path": json_path,
                        "value": value,
                    }));
                }
                ChangeKind::Removed => {
                    ops.push(serde_json::json!({
                        "op": "remove",
                        "path": json_path,
                    }));
                }
                ChangeKind::Modified => {
                    let value = change
                        .new_value
                        .as_deref()
                        .and_then(|v| serde_json::from_str(v).ok())
                        .unwrap_or(serde_json::Value::Null);
                    ops.push(serde_json::json!({
                        "op": "replace",
                        "path": json_path,
                        "value": value,
                    }));
                }
                ChangeKind::Unchanged => {}
            }
        }

        serde_json::to_string_pretty(&ops).unwrap_or_else(|_| "[]".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::json::JsonFormat;
    use crate::format::Format;

    #[test]
    fn test_json_patch_output() {
        let format = JsonFormat;
        let result = format
            .diff(r#"{"a": 1}"#, r#"{"a": 2, "b": 3}"#)
            .unwrap();
        let formatter = JsonPatchFormatter;
        let output = formatter.format(&result);
        let ops: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();
        assert!(ops.iter().any(|op| op["op"] == "replace"));
        assert!(ops.iter().any(|op| op["op"] == "add"));
    }
}
