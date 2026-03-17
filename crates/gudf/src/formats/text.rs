use similar::{ChangeTag, TextDiff};

use crate::error::GudfError;
use crate::format::{Format, FormatKind};
use crate::result::{Change, ChangeKind, DiffResult, DiffStats, Location};

pub struct TextFormat;

impl Format for TextFormat {
    fn kind(&self) -> FormatKind {
        FormatKind::Text
    }

    fn diff(&self, old: &str, new: &str) -> Result<DiffResult, GudfError> {
        let text_diff = TextDiff::from_lines(old, new);
        let mut changes = Vec::new();
        let mut line = 0usize;

        for op in text_diff.ops() {
            for change in text_diff.iter_changes(op) {
                let value = change.value().to_string();
                let kind = match change.tag() {
                    ChangeTag::Equal => {
                        line += 1;
                        changes.push(Change {
                            kind: ChangeKind::Unchanged,
                            path: None,
                            old_value: Some(value.clone()),
                            new_value: Some(value),
                            location: Some(Location {
                                line,
                                column: None,
                            }),
                            annotations: Vec::new(),
                        });
                        continue;
                    }
                    ChangeTag::Insert => {
                        line += 1;
                        ChangeKind::Added
                    }
                    ChangeTag::Delete => ChangeKind::Removed,
                };

                let (old_val, new_val) = match &kind {
                    ChangeKind::Added => (None, Some(value)),
                    ChangeKind::Removed => (Some(value), None),
                    _ => unreachable!(),
                };

                changes.push(Change {
                    kind,
                    path: None,
                    old_value: old_val,
                    new_value: new_val,
                    location: Some(Location {
                        line,
                        column: None,
                    }),
                    annotations: Vec::new(),
                });
            }
        }

        let stats = DiffStats::from_changes(&changes);
        Ok(DiffResult {
            changes,
            format: FormatKind::Text,
            stats,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_text() {
        let format = TextFormat;
        let result = format.diff("hello\n", "hello\n").unwrap();
        assert_eq!(result.stats.additions, 0);
        assert_eq!(result.stats.deletions, 0);
    }

    #[test]
    fn test_added_line() {
        let format = TextFormat;
        let result = format.diff("hello\n", "hello\nworld\n").unwrap();
        assert_eq!(result.stats.additions, 1);
        assert_eq!(result.stats.deletions, 0);
    }

    #[test]
    fn test_removed_line() {
        let format = TextFormat;
        let result = format.diff("hello\nworld\n", "hello\n").unwrap();
        assert_eq!(result.stats.additions, 0);
        assert_eq!(result.stats.deletions, 1);
    }

    #[test]
    fn test_modified_line() {
        let format = TextFormat;
        let result = format.diff("hello\n", "world\n").unwrap();
        assert_eq!(result.stats.additions, 1);
        assert_eq!(result.stats.deletions, 1);
    }
}
