use crate::output::OutputFormatter;
use crate::result::{ChangeKind, DiffResult};

pub struct InlineFormatter;

impl OutputFormatter for InlineFormatter {
    fn format(&self, result: &DiffResult) -> String {
        let mut output = String::new();

        for change in &result.changes {
            match change.kind {
                ChangeKind::Added => {
                    let val = change.new_value.as_deref().unwrap_or("");
                    let val = val.trim_end_matches('\n');
                    output.push_str(&format!("[+] {val}\n"));
                }
                ChangeKind::Removed => {
                    let val = change.old_value.as_deref().unwrap_or("");
                    let val = val.trim_end_matches('\n');
                    output.push_str(&format!("[-] {val}\n"));
                }
                ChangeKind::Modified => {
                    if let Some(path) = &change.path {
                        let old_val = change.old_value.as_deref().unwrap_or("");
                        let new_val = change.new_value.as_deref().unwrap_or("");
                        output.push_str(&format!("[~] {path}: {old_val} -> {new_val}\n"));
                    } else {
                        let old_val = change.old_value.as_deref().unwrap_or("");
                        let new_val = change.new_value.as_deref().unwrap_or("");
                        let old_val = old_val.trim_end_matches('\n');
                        let new_val = new_val.trim_end_matches('\n');
                        output.push_str(&format!("[-] {old_val}\n"));
                        output.push_str(&format!("[+] {new_val}\n"));
                    }
                }
                ChangeKind::Unchanged => {
                    let val = change.old_value.as_deref().unwrap_or("");
                    let val = val.trim_end_matches('\n');
                    output.push_str(&format!("    {val}\n"));
                }
            }
        }

        output.push_str(&format!(
            "\n{} addition(s), {} deletion(s), {} modification(s)\n",
            result.stats.additions, result.stats.deletions, result.stats.modifications
        ));

        output
    }
}
