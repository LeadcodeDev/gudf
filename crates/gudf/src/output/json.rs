use crate::format::FormatKind;
use crate::output::OutputFormatter;
use crate::result::{ChangeKind, DiffResult};

/// Outputs diff as a JSON array using gudf's native dot-notation paths.
///
/// Unlike `JsonPatchFormatter` (RFC 6902 with JSON Pointer `/a/b/0`),
/// this uses human-readable paths: `a.b[0]`.
pub struct JsonFormatter;

impl JsonFormatter {
    fn parse_value(raw: Option<&str>) -> serde_json::Value {
        let Some(v) = raw else {
            return serde_json::Value::Null;
        };
        serde_json::from_str(v).unwrap_or_else(|_| {
            serde_json::Value::String(v.trim_end_matches('\n').to_string())
        })
    }

    fn text_line(change: &crate::result::Change) -> usize {
        change
            .location
            .as_ref()
            .map(|loc| loc.line.saturating_sub(1))
            .unwrap_or(0)
    }

    fn format_text(&self, result: &DiffResult) -> Vec<serde_json::Value> {
        let changes: Vec<_> = result
            .changes
            .iter()
            .filter(|c| c.kind != ChangeKind::Unchanged)
            .collect();

        if changes.is_empty() {
            return Vec::new();
        }

        let mut blocks: Vec<TextBlock> = Vec::new();

        for change in &changes {
            let line = Self::text_line(change);
            let value = match change.kind {
                ChangeKind::Removed => change
                    .old_value
                    .as_deref()
                    .unwrap_or("")
                    .trim_end_matches('\n')
                    .to_string(),
                _ => change
                    .new_value
                    .as_deref()
                    .unwrap_or("")
                    .trim_end_matches('\n')
                    .to_string(),
            };

            let can_extend = blocks.last().map_or(false, |b| b.kind == change.kind);
            if can_extend {
                blocks.last_mut().unwrap().lines.push(value);
            } else {
                blocks.push(TextBlock {
                    kind: change.kind.clone(),
                    start_line: line,
                    lines: vec![value],
                });
            }
        }

        let mut ops: Vec<serde_json::Value> = Vec::new();
        let mut i = 0;
        while i < blocks.len() {
            let block = &blocks[i];

            if block.kind == ChangeKind::Removed {
                if let Some(next) = blocks.get(i + 1) {
                    if next.kind == ChangeKind::Added {
                        let rm_count = block.lines.len();
                        let add_count = next.lines.len();
                        let paired = rm_count.min(add_count);

                        // Emit replace for the paired portion
                        if paired > 0 {
                            ops.push(serde_json::json!({
                                "op": "replace",
                                "line": block.start_line,
                                "value": {
                                    "before": block.lines[..paired].join("\n"),
                                    "after": next.lines[..paired].join("\n"),
                                },
                            }));
                        }

                        // Emit remaining removes (more removed than added)
                        if rm_count > paired {
                            ops.push(serde_json::json!({
                                "op": "remove",
                                "line": block.start_line + paired,
                                "value": block.lines[paired..].join("\n"),
                            }));
                        }

                        // Emit remaining adds (more added than removed)
                        if add_count > paired {
                            ops.push(serde_json::json!({
                                "op": "add",
                                "line": next.start_line + paired,
                                "value": next.lines[paired..].join("\n"),
                            }));
                        }

                        i += 2;
                        continue;
                    }
                }
                ops.push(serde_json::json!({
                    "op": "remove",
                    "line": block.start_line,
                    "value": block.lines.join("\n"),
                }));
            } else if block.kind == ChangeKind::Added {
                ops.push(serde_json::json!({
                    "op": "add",
                    "line": block.start_line,
                    "value": block.lines.join("\n"),
                }));
            }

            i += 1;
        }

        ops
    }
}

struct TextBlock {
    kind: ChangeKind,
    start_line: usize,
    lines: Vec<String>,
}

impl OutputFormatter for JsonFormatter {
    fn format(&self, result: &DiffResult) -> String {
        let is_text = matches!(result.format, FormatKind::Text | FormatKind::Code(_));

        let ops = if is_text {
            self.format_text(result)
        } else {
            let mut ops: Vec<serde_json::Value> = Vec::new();

            for change in &result.changes {
                let path = change.path.as_deref().unwrap_or("");

                match change.kind {
                    ChangeKind::Added => {
                        ops.push(serde_json::json!({
                            "op": "add",
                            "path": path,
                            "value": Self::parse_value(change.new_value.as_deref()),
                        }));
                    }
                    ChangeKind::Removed => {
                        ops.push(serde_json::json!({
                            "op": "remove",
                            "path": path,
                            "value": Self::parse_value(change.old_value.as_deref()),
                        }));
                    }
                    ChangeKind::Modified => {
                        ops.push(serde_json::json!({
                            "op": "replace",
                            "path": path,
                            "value": {
                                "before": Self::parse_value(change.old_value.as_deref()),
                                "after": Self::parse_value(change.new_value.as_deref()),
                            },
                        }));
                    }
                    ChangeKind::Moved => {
                        ops.push(serde_json::json!({
                            "op": "move",
                            "from": path,
                            "path": change.new_value.as_deref().unwrap_or(""),
                        }));
                    }
                    ChangeKind::Renamed => {
                        ops.push(serde_json::json!({
                            "op": "rename",
                            "from": path,
                            "path": change.new_value.as_deref().unwrap_or(""),
                        }));
                    }
                    ChangeKind::Unchanged => {}
                }
            }

            ops
        };

        serde_json::to_string_pretty(&ops).unwrap_or_else(|_| "[]".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::formats::json::JsonFormat;
    use crate::formats::text::TextFormat;

    #[test]
    fn test_json_output_dot_notation() {
        let format = JsonFormat;
        let result = format
            .diff(r#"{"a": 1}"#, r#"{"a": 2, "b": 3}"#)
            .unwrap();
        let formatter = JsonFormatter;
        let output = formatter.format(&result);
        let ops: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();

        let replace_op = ops.iter().find(|op| op["op"] == "replace").unwrap();
        assert_eq!(replace_op["path"], "a");
        assert_eq!(replace_op["value"]["before"], 1);
        assert_eq!(replace_op["value"]["after"], 2);

        let add_op = ops.iter().find(|op| op["op"] == "add").unwrap();
        assert_eq!(add_op["path"], "b");
        assert_eq!(add_op["value"], 3);
    }

    #[test]
    fn test_json_output_nested() {
        let format = JsonFormat;
        let result = format
            .diff(
                r#"{"items": [{"name": "a"}, {"name": "b"}]}"#,
                r#"{"items": [{"name": "a"}, {"name": "c"}]}"#,
            )
            .unwrap();
        let formatter = JsonFormatter;
        let output = formatter.format(&result);
        let ops: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();

        let replace_op = ops.iter().find(|op| op["op"] == "replace").unwrap();
        assert_eq!(replace_op["path"], "items[1].name");
        assert_eq!(replace_op["value"]["before"], "b");
        assert_eq!(replace_op["value"]["after"], "c");
    }

    #[test]
    fn test_json_output_text_coalesced() {
        let format = TextFormat;
        let result = format.diff("hello\nworld\n", "hello\nrust\n").unwrap();
        let formatter = JsonFormatter;
        let output = formatter.format(&result);
        let ops: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();

        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0]["op"], "replace");
        assert_eq!(ops[0]["value"]["before"], "world");
        assert_eq!(ops[0]["value"]["after"], "rust");
    }

    #[test]
    fn test_json_output_remove() {
        let format = JsonFormat;
        let result = format
            .diff(r#"{"a": 1, "b": 2}"#, r#"{"a": 1}"#)
            .unwrap();
        let formatter = JsonFormatter;
        let output = formatter.format(&result);
        let ops: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();

        let remove_op = ops.iter().find(|op| op["op"] == "remove").unwrap();
        assert_eq!(remove_op["path"], "b");
        assert_eq!(remove_op["value"], 2);
    }

    #[test]
    fn test_json_output_add() {
        let format = JsonFormat;
        let result = format
            .diff(r#"{"a": 1}"#, r#"{"a": 1, "b": 2}"#)
            .unwrap();
        let formatter = JsonFormatter;
        let output = formatter.format(&result);
        let ops: Vec<serde_json::Value> = serde_json::from_str(&output).unwrap();

        let add_op = ops.iter().find(|op| op["op"] == "add").unwrap();
        assert_eq!(add_op["path"], "b");
        assert_eq!(add_op["value"], 2);
    }
}
