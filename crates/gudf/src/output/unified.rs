use crate::output::OutputFormatter;
use crate::result::{ChangeKind, DiffResult};

/// Configuration for hunk generation.
#[derive(Debug, Clone)]
pub struct HunkConfig {
    /// Number of context lines around changes (default: 3).
    pub context_lines: usize,
}

impl Default for HunkConfig {
    fn default() -> Self {
        Self { context_lines: 3 }
    }
}

/// A unified diff hunk with proper `@@` header.
#[derive(Debug, Clone)]
pub struct Hunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<HunkLine>,
}

/// A single line within a hunk.
#[derive(Debug, Clone)]
pub struct HunkLine {
    pub kind: HunkLineKind,
    pub text: String,
    pub missing_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HunkLineKind {
    Context,
    Add,
    Remove,
}

/// Git-native unified diff formatter producing `@@ -a,b +c,d @@` hunk headers.
pub struct UnifiedFormatter {
    pub old_name: String,
    pub new_name: String,
    pub context_lines: usize,
}

impl UnifiedFormatter {
    pub fn new(old_name: impl Into<String>, new_name: impl Into<String>) -> Self {
        Self {
            old_name: old_name.into(),
            new_name: new_name.into(),
            context_lines: 3,
        }
    }

    /// Set the number of context lines around changes.
    pub fn context(mut self, lines: usize) -> Self {
        self.context_lines = lines;
        self
    }
}

impl Default for UnifiedFormatter {
    fn default() -> Self {
        Self::new("a", "b")
    }
}

/// Internal representation of a diff line for hunk building.
struct LineEntry {
    kind: LineKind,
    text: String,
    old_line: Option<usize>,
    new_line: Option<usize>,
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum LineKind {
    Context,
    Add,
    Remove,
}

impl OutputFormatter for UnifiedFormatter {
    fn format(&self, result: &DiffResult) -> String {
        let has_paths = result
            .changes
            .iter()
            .any(|c| c.path.is_some() && c.kind != ChangeKind::Unchanged);

        if has_paths {
            self.format_structured(result)
        } else {
            self.format_text(result)
        }
    }
}

impl UnifiedFormatter {
    fn format_text(&self, result: &DiffResult) -> String {
        // Build line entries with old/new line tracking
        let mut entries = Vec::new();
        let mut old_line = 0usize;
        let mut new_line = 0usize;

        for change in &result.changes {
            match change.kind {
                ChangeKind::Unchanged => {
                    old_line += 1;
                    new_line += 1;
                    entries.push(LineEntry {
                        kind: LineKind::Context,
                        text: change.old_value.clone().unwrap_or_default(),
                        old_line: Some(old_line),
                        new_line: Some(new_line),
                    });
                }
                ChangeKind::Added => {
                    new_line += 1;
                    entries.push(LineEntry {
                        kind: LineKind::Add,
                        text: change.new_value.clone().unwrap_or_default(),
                        old_line: None,
                        new_line: Some(new_line),
                    });
                }
                ChangeKind::Removed => {
                    old_line += 1;
                    entries.push(LineEntry {
                        kind: LineKind::Remove,
                        text: change.old_value.clone().unwrap_or_default(),
                        old_line: Some(old_line),
                        new_line: None,
                    });
                }
                ChangeKind::Modified => {
                    // Decompose Modified into Remove + Add
                    old_line += 1;
                    new_line += 1;
                    entries.push(LineEntry {
                        kind: LineKind::Remove,
                        text: change.old_value.clone().unwrap_or_default(),
                        old_line: Some(old_line),
                        new_line: None,
                    });
                    entries.push(LineEntry {
                        kind: LineKind::Add,
                        text: change.new_value.clone().unwrap_or_default(),
                        old_line: None,
                        new_line: Some(new_line),
                    });
                }
                ChangeKind::Moved | ChangeKind::Renamed => {
                    // Show as remove + add with annotation
                    old_line += 1;
                    new_line += 1;
                    entries.push(LineEntry {
                        kind: LineKind::Remove,
                        text: change.old_value.clone().unwrap_or_default(),
                        old_line: Some(old_line),
                        new_line: None,
                    });
                    entries.push(LineEntry {
                        kind: LineKind::Add,
                        text: change.new_value.clone().unwrap_or_default(),
                        old_line: None,
                        new_line: Some(new_line),
                    });
                }
            }
        }

        if entries.is_empty() || entries.iter().all(|e| e.kind == LineKind::Context) {
            return String::new();
        }

        let hunks = self.build_hunks(&entries);
        self.render_hunks(&hunks)
    }

    fn format_structured(&self, result: &DiffResult) -> String {
        // For structured diffs, generate lines from path-based changes
        let significant: Vec<_> = result
            .changes
            .iter()
            .filter(|c| c.kind != ChangeKind::Unchanged)
            .collect();

        if significant.is_empty() {
            return String::new();
        }

        let mut entries = Vec::new();
        let mut old_line = 0usize;
        let mut new_line = 0usize;

        for change in &result.changes {
            let path = change.path.as_deref().unwrap_or("");
            match change.kind {
                ChangeKind::Added => {
                    new_line += 1;
                    let val = change.new_value.as_deref().unwrap_or("");
                    entries.push(LineEntry {
                        kind: LineKind::Add,
                        text: format!("{path}: {val}\n"),
                        old_line: None,
                        new_line: Some(new_line),
                    });
                }
                ChangeKind::Removed => {
                    old_line += 1;
                    let val = change.old_value.as_deref().unwrap_or("");
                    entries.push(LineEntry {
                        kind: LineKind::Remove,
                        text: format!("{path}: {val}\n"),
                        old_line: Some(old_line),
                        new_line: None,
                    });
                }
                ChangeKind::Modified => {
                    old_line += 1;
                    new_line += 1;
                    let old_val = change.old_value.as_deref().unwrap_or("");
                    let new_val = change.new_value.as_deref().unwrap_or("");
                    entries.push(LineEntry {
                        kind: LineKind::Remove,
                        text: format!("{path}: {old_val}\n"),
                        old_line: Some(old_line),
                        new_line: None,
                    });
                    entries.push(LineEntry {
                        kind: LineKind::Add,
                        text: format!("{path}: {new_val}\n"),
                        old_line: None,
                        new_line: Some(new_line),
                    });
                }
                ChangeKind::Unchanged => {
                    old_line += 1;
                    new_line += 1;
                    let val = change.old_value.as_deref().unwrap_or("");
                    entries.push(LineEntry {
                        kind: LineKind::Context,
                        text: format!("{path}: {val}\n"),
                        old_line: Some(old_line),
                        new_line: Some(new_line),
                    });
                }
                ChangeKind::Moved => {
                    old_line += 1;
                    new_line += 1;
                    let old_val = change.old_value.as_deref().unwrap_or("");
                    let new_path = change.new_value.as_deref().unwrap_or("");
                    entries.push(LineEntry {
                        kind: LineKind::Remove,
                        text: format!("{path}: {old_val}\n"),
                        old_line: Some(old_line),
                        new_line: None,
                    });
                    entries.push(LineEntry {
                        kind: LineKind::Add,
                        text: format!("{new_path}: {old_val} (moved from {path})\n"),
                        old_line: None,
                        new_line: Some(new_line),
                    });
                }
                ChangeKind::Renamed => {
                    old_line += 1;
                    new_line += 1;
                    let old_val = change.old_value.as_deref().unwrap_or("");
                    let new_path = change.new_value.as_deref().unwrap_or("");
                    entries.push(LineEntry {
                        kind: LineKind::Remove,
                        text: format!("{path}: {old_val}\n"),
                        old_line: Some(old_line),
                        new_line: None,
                    });
                    entries.push(LineEntry {
                        kind: LineKind::Add,
                        text: format!("{new_path}: {old_val} (renamed from {path})\n"),
                        old_line: None,
                        new_line: Some(new_line),
                    });
                }
            }
        }

        let hunks = self.build_hunks(&entries);
        self.render_hunks(&hunks)
    }

    fn build_hunks(&self, entries: &[LineEntry]) -> Vec<Hunk> {
        // Find indices of non-context lines
        let interesting: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind != LineKind::Context)
            .map(|(i, _)| i)
            .collect();

        if interesting.is_empty() {
            return Vec::new();
        }

        // Build ranges with context, merging overlapping ones
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for &idx in &interesting {
            let start = idx.saturating_sub(self.context_lines);
            let end = (idx + self.context_lines).min(entries.len() - 1);

            if let Some(last) = ranges.last_mut() {
                if start <= last.1 + 1 {
                    last.1 = end;
                } else {
                    ranges.push((start, end));
                }
            } else {
                ranges.push((start, end));
            }
        }

        // Convert ranges to Hunks
        ranges
            .iter()
            .map(|&(start, end)| {
                let slice = &entries[start..=end];

                let old_start = slice
                    .iter()
                    .find_map(|e| e.old_line)
                    .unwrap_or(1);
                let new_start = slice
                    .iter()
                    .find_map(|e| e.new_line)
                    .unwrap_or(1);
                let old_count = slice
                    .iter()
                    .filter(|e| matches!(e.kind, LineKind::Context | LineKind::Remove))
                    .count();
                let new_count = slice
                    .iter()
                    .filter(|e| matches!(e.kind, LineKind::Context | LineKind::Add))
                    .count();

                let lines = slice
                    .iter()
                    .map(|e| {
                        let has_newline = e.text.ends_with('\n');
                        HunkLine {
                            kind: match e.kind {
                                LineKind::Context => HunkLineKind::Context,
                                LineKind::Add => HunkLineKind::Add,
                                LineKind::Remove => HunkLineKind::Remove,
                            },
                            text: e.text.trim_end_matches('\n').to_string(),
                            missing_newline: !has_newline && !e.text.is_empty(),
                        }
                    })
                    .collect();

                Hunk {
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                    lines,
                }
            })
            .collect()
    }

    fn render_hunks(&self, hunks: &[Hunk]) -> String {
        if hunks.is_empty() {
            return String::new();
        }

        let mut output = String::new();
        output.push_str(&format!("--- {}\n", self.old_name));
        output.push_str(&format!("+++ {}\n", self.new_name));

        for hunk in hunks {
            output.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
            ));

            for line in &hunk.lines {
                let prefix = match line.kind {
                    HunkLineKind::Context => " ",
                    HunkLineKind::Add => "+",
                    HunkLineKind::Remove => "-",
                };
                output.push_str(&format!("{prefix}{}\n", line.text));
                if line.missing_newline {
                    output.push_str("\\ No newline at end of file\n");
                }
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;
    use crate::formats::text::TextFormat;

    #[test]
    fn test_unified_output_with_hunks() {
        let format = TextFormat;
        let result = format.diff("hello\nworld\n", "hello\nrust\n").unwrap();
        let formatter = UnifiedFormatter::default();
        let output = formatter.format(&result);
        assert!(output.contains("--- a"));
        assert!(output.contains("+++ b"));
        assert!(output.contains("@@"));
        assert!(output.contains("-world"));
        assert!(output.contains("+rust"));
    }

    #[test]
    fn test_hunk_header_format() {
        let format = TextFormat;
        let result = format.diff("a\nb\nc\n", "a\nx\nc\n").unwrap();
        let formatter = UnifiedFormatter::new("old.txt", "new.txt").context(1);
        let output = formatter.format(&result);
        assert!(output.contains("--- old.txt"));
        assert!(output.contains("+++ new.txt"));
        // Should contain @@ header
        assert!(output.contains("@@ -"));
    }

    #[test]
    fn test_context_lines() {
        let format = TextFormat;
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let new = "1\n2\n3\n4\nX\n6\n7\n8\n9\n10\n";
        let formatter = UnifiedFormatter::default().context(2);
        let result = format.diff(old, new).unwrap();
        let output = formatter.format(&result);
        // Context of 2 means we see lines 3,4 before and 6,7 after the change
        assert!(output.contains(" 3"));
        assert!(output.contains(" 4"));
        assert!(output.contains("-5"));
        assert!(output.contains("+X"));
        assert!(output.contains(" 6"));
        assert!(output.contains(" 7"));
    }

    #[test]
    fn test_multiple_hunks() {
        let format = TextFormat;
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n";
        let new = "1\nX\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\nY\n15\n";
        let formatter = UnifiedFormatter::default().context(1);
        let result = format.diff(old, new).unwrap();
        let output = formatter.format(&result);
        // With context=1, changes at line 2 and line 14 should be in separate hunks
        let hunk_count = output.matches("@@").count();
        assert!(hunk_count >= 2, "Expected 2+ hunks, got {hunk_count}");
    }

    #[test]
    fn test_no_changes() {
        let format = TextFormat;
        let result = format.diff("hello\n", "hello\n").unwrap();
        let formatter = UnifiedFormatter::default();
        let output = formatter.format(&result);
        assert!(output.is_empty() || !output.contains("@@"));
    }

    #[test]
    fn test_no_newline_at_end() {
        let format = TextFormat;
        let result = format.diff("hello", "world").unwrap();
        let formatter = UnifiedFormatter::default();
        let output = formatter.format(&result);
        assert!(output.contains("\\ No newline at end of file"));
    }

    #[test]
    fn test_structured_diff_format() {
        use crate::formats::json::JsonFormat;
        let format = JsonFormat;
        let result = format
            .diff(r#"{"a": 1, "b": 2}"#, r#"{"a": 1, "b": 3}"#)
            .unwrap();
        let formatter = UnifiedFormatter::default();
        let output = formatter.format(&result);
        assert!(output.contains("@@"));
        assert!(output.contains("-b: 2"));
        assert!(output.contains("+b: 3"));
    }

    #[test]
    fn test_git_apply_compatible_format() {
        let format = TextFormat;
        let result = format
            .diff("line1\nline2\nline3\n", "line1\nmodified\nline3\n")
            .unwrap();
        let formatter = UnifiedFormatter::new("a/file.txt", "b/file.txt");
        let output = formatter.format(&result);

        // Verify it has all required parts for git apply
        assert!(output.contains("--- a/file.txt"));
        assert!(output.contains("+++ b/file.txt"));
        assert!(output.contains("@@ -"));
        assert!(output.contains(" @@\n")); // hunk header ends with @@\n
    }
}
