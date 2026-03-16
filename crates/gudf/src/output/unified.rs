use crate::output::OutputFormatter;
use crate::result::{ChangeKind, DiffResult};

pub struct UnifiedFormatter {
    pub old_name: String,
    pub new_name: String,
}

impl UnifiedFormatter {
    pub fn new(old_name: impl Into<String>, new_name: impl Into<String>) -> Self {
        Self {
            old_name: old_name.into(),
            new_name: new_name.into(),
        }
    }
}

impl Default for UnifiedFormatter {
    fn default() -> Self {
        Self::new("a", "b")
    }
}

impl OutputFormatter for UnifiedFormatter {
    fn format(&self, result: &DiffResult) -> String {
        let mut output = String::new();
        output.push_str(&format!("--- {}\n", self.old_name));
        output.push_str(&format!("+++ {}\n", self.new_name));

        let significant_changes: Vec<_> = result
            .changes
            .iter()
            .filter(|c| c.kind != ChangeKind::Unchanged)
            .collect();

        if significant_changes.is_empty() {
            return output;
        }

        for change in &result.changes {
            match change.kind {
                ChangeKind::Added => {
                    let val = change.new_value.as_deref().unwrap_or("");
                    let val = val.trim_end_matches('\n');
                    output.push_str(&format!("+{val}\n"));
                }
                ChangeKind::Removed => {
                    let val = change.old_value.as_deref().unwrap_or("");
                    let val = val.trim_end_matches('\n');
                    output.push_str(&format!("-{val}\n"));
                }
                ChangeKind::Modified => {
                    if let Some(path) = &change.path {
                        let old_val = change.old_value.as_deref().unwrap_or("");
                        let new_val = change.new_value.as_deref().unwrap_or("");
                        output.push_str(&format!("~{path}: {old_val} -> {new_val}\n"));
                    } else {
                        let old_val = change.old_value.as_deref().unwrap_or("");
                        let new_val = change.new_value.as_deref().unwrap_or("");
                        let old_val = old_val.trim_end_matches('\n');
                        let new_val = new_val.trim_end_matches('\n');
                        output.push_str(&format!("-{old_val}\n"));
                        output.push_str(&format!("+{new_val}\n"));
                    }
                }
                ChangeKind::Unchanged => {
                    let val = change.old_value.as_deref().unwrap_or("");
                    let val = val.trim_end_matches('\n');
                    output.push_str(&format!(" {val}\n"));
                }
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::text::TextFormat;
    use crate::format::Format;

    #[test]
    fn test_unified_output() {
        let format = TextFormat;
        let result = format.diff("hello\nworld\n", "hello\nrust\n").unwrap();
        let formatter = UnifiedFormatter::default();
        let output = formatter.format(&result);
        assert!(output.contains("--- a"));
        assert!(output.contains("+++ b"));
        assert!(output.contains("-world"));
        assert!(output.contains("+rust"));
    }
}
