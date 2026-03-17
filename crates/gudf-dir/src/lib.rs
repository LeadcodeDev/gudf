use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use rayon::prelude::*;

use gudf::formats::code::CodeFormat;
use gudf::{DiffEngine, DiffResult, Format, FormatKind, GudfError};

/// Status of a file in a directory diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Removed,
    Modified,
    Unchanged,
    Binary,
}

/// A single file's diff entry.
#[derive(Debug)]
pub struct FileDiffEntry {
    /// Relative path of the file.
    pub path: PathBuf,
    /// Status of the file.
    pub status: FileStatus,
    /// The diff result, if applicable.
    pub diff: Option<DiffResult>,
}

/// Summary statistics for a directory diff.
#[derive(Debug, Clone, Default)]
pub struct DirDiffSummary {
    pub files_added: usize,
    pub files_removed: usize,
    pub files_modified: usize,
    pub files_unchanged: usize,
    pub files_binary: usize,
}

/// Result of diffing two directories.
#[derive(Debug)]
pub struct DirDiffResult {
    pub file_results: Vec<FileDiffEntry>,
    pub summary: DirDiffSummary,
}

/// Detect format from file extension.
fn format_from_extension(path: &Path) -> FormatKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "json" => FormatKind::Json,
        "toml" => FormatKind::Toml,
        "yaml" | "yml" => FormatKind::Yaml,
        "rs" => FormatKind::Code("rust".to_string()),
        "py" => FormatKind::Code("python".to_string()),
        "js" => FormatKind::Code("javascript".to_string()),
        "ts" => FormatKind::Code("typescript".to_string()),
        "tsx" => FormatKind::Code("tsx".to_string()),
        "go" => FormatKind::Code("go".to_string()),
        "java" => FormatKind::Code("java".to_string()),
        "c" | "h" => FormatKind::Code("c".to_string()),
        "cpp" | "cc" | "cxx" | "hpp" => FormatKind::Code("cpp".to_string()),
        "cs" => FormatKind::Code("c-sharp".to_string()),
        "rb" => FormatKind::Code("ruby".to_string()),
        "php" => FormatKind::Code("php".to_string()),
        "swift" => FormatKind::Code("swift".to_string()),
        "scala" => FormatKind::Code("scala".to_string()),
        "zig" => FormatKind::Code("zig".to_string()),
        "lua" => FormatKind::Code("lua".to_string()),
        "dart" => FormatKind::Code("dart".to_string()),
        "ex" | "exs" => FormatKind::Code("elixir".to_string()),
        "erl" => FormatKind::Code("erlang".to_string()),
        "hs" => FormatKind::Code("haskell".to_string()),
        "ml" => FormatKind::Code("ocaml".to_string()),
        "html" | "htm" => FormatKind::Code("html".to_string()),
        "css" => FormatKind::Code("css".to_string()),
        "sh" | "bash" => FormatKind::Code("bash".to_string()),
        "r" => FormatKind::Code("r".to_string()),
        "hcl" | "tf" => FormatKind::Code("hcl".to_string()),
        _ => FormatKind::Text,
    }
}

/// Check if a file appears to be binary (contains null bytes in first 8KB).
fn is_binary(path: &Path) -> bool {
    if let Ok(bytes) = fs::read(path) {
        let check_len = bytes.len().min(8192);
        bytes[..check_len].contains(&0)
    } else {
        false
    }
}

/// Collect all file paths under a directory, respecting .gitignore.
fn collect_files(dir: &Path) -> BTreeMap<PathBuf, PathBuf> {
    let mut files = BTreeMap::new();
    let walker = WalkBuilder::new(dir)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker.flatten() {
        if entry.file_type().map_or(false, |ft| ft.is_file()) {
            let full_path = entry.path().to_path_buf();
            if let Ok(rel) = full_path.strip_prefix(dir) {
                files.insert(rel.to_path_buf(), full_path);
            }
        }
    }
    files
}

/// Diff two directories recursively, using format-aware diffing per file.
pub fn diff_dirs(old_dir: &Path, new_dir: &Path) -> Result<DirDiffResult, GudfError> {
    let old_files = collect_files(old_dir);
    let new_files = collect_files(new_dir);

    // Collect all unique relative paths
    let mut all_paths: Vec<PathBuf> = old_files
        .keys()
        .chain(new_files.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    all_paths.sort();

    // Process files in parallel
    let file_results: Vec<FileDiffEntry> = all_paths
        .par_iter()
        .filter_map(|rel_path| {
            let old_full = old_files.get(rel_path);
            let new_full = new_files.get(rel_path);

            match (old_full, new_full) {
                (None, Some(new_path)) => {
                    // File added
                    if is_binary(new_path) {
                        Some(FileDiffEntry {
                            path: rel_path.clone(),
                            status: FileStatus::Binary,
                            diff: None,
                        })
                    } else {
                        Some(FileDiffEntry {
                            path: rel_path.clone(),
                            status: FileStatus::Added,
                            diff: None,
                        })
                    }
                }
                (Some(old_path), None) => {
                    // File removed
                    if is_binary(old_path) {
                        Some(FileDiffEntry {
                            path: rel_path.clone(),
                            status: FileStatus::Binary,
                            diff: None,
                        })
                    } else {
                        Some(FileDiffEntry {
                            path: rel_path.clone(),
                            status: FileStatus::Removed,
                            diff: None,
                        })
                    }
                }
                (Some(old_path), Some(new_path)) => {
                    // Both exist — check binary
                    if is_binary(old_path) || is_binary(new_path) {
                        return Some(FileDiffEntry {
                            path: rel_path.clone(),
                            status: FileStatus::Binary,
                            diff: None,
                        });
                    }

                    let old_content = fs::read_to_string(old_path).ok()?;
                    let new_content = fs::read_to_string(new_path).ok()?;

                    if old_content == new_content {
                        return Some(FileDiffEntry {
                            path: rel_path.clone(),
                            status: FileStatus::Unchanged,
                            diff: None,
                        });
                    }

                    let format = format_from_extension(rel_path);
                    let diff_result = match &format {
                        FormatKind::Code(lang) => {
                            let code_format = CodeFormat::new(lang.as_str());
                            code_format.diff(&old_content, &new_content).ok()
                        }
                        _ => {
                            let engine = DiffEngine::new();
                            engine.diff_as(format, &old_content, &new_content).ok()
                        }
                    };

                    Some(FileDiffEntry {
                        path: rel_path.clone(),
                        status: FileStatus::Modified,
                        diff: diff_result,
                    })
                }
                (None, None) => None,
            }
        })
        .collect();

    let summary = DirDiffSummary {
        files_added: file_results
            .iter()
            .filter(|e| e.status == FileStatus::Added)
            .count(),
        files_removed: file_results
            .iter()
            .filter(|e| e.status == FileStatus::Removed)
            .count(),
        files_modified: file_results
            .iter()
            .filter(|e| e.status == FileStatus::Modified)
            .count(),
        files_unchanged: file_results
            .iter()
            .filter(|e| e.status == FileStatus::Unchanged)
            .count(),
        files_binary: file_results
            .iter()
            .filter(|e| e.status == FileStatus::Binary)
            .count(),
    };

    Ok(DirDiffResult {
        file_results,
        summary,
    })
}

/// Format a directory diff result in unified diff format.
pub fn format_dir_diff(result: &DirDiffResult) -> String {
    use gudf::output::unified::UnifiedFormatter;
    use gudf::output::OutputFormatter;

    let mut output = String::new();

    for entry in &result.file_results {
        let path_str = entry.path.display().to_string();

        match entry.status {
            FileStatus::Added => {
                output.push_str(&format!("diff --git a/{path_str} b/{path_str}\n"));
                output.push_str("new file\n");
                output.push_str(&format!("--- /dev/null\n+++ b/{path_str}\n"));
            }
            FileStatus::Removed => {
                output.push_str(&format!("diff --git a/{path_str} b/{path_str}\n"));
                output.push_str("deleted file\n");
                output.push_str(&format!("--- a/{path_str}\n+++ /dev/null\n"));
            }
            FileStatus::Modified => {
                output.push_str(&format!("diff --git a/{path_str} b/{path_str}\n"));
                if let Some(ref diff) = entry.diff {
                    let formatter = UnifiedFormatter::new(
                        format!("a/{path_str}"),
                        format!("b/{path_str}"),
                    );
                    output.push_str(&formatter.format(diff));
                }
            }
            FileStatus::Binary => {
                output.push_str(&format!(
                    "diff --git a/{path_str} b/{path_str}\nBinary files differ\n"
                ));
            }
            FileStatus::Unchanged => {}
        }
    }

    // Summary
    output.push_str(&format!(
        "\n{} file(s) changed: {} added, {} removed, {} modified, {} unchanged, {} binary\n",
        result.summary.files_added
            + result.summary.files_removed
            + result.summary.files_modified,
        result.summary.files_added,
        result.summary.files_removed,
        result.summary.files_modified,
        result.summary.files_unchanged,
        result.summary.files_binary,
    ));

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_temp_dirs() -> (tempfile::TempDir, tempfile::TempDir) {
        (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap())
    }

    #[test]
    fn test_format_from_extension() {
        assert_eq!(
            format_from_extension(Path::new("file.json")),
            FormatKind::Json
        );
        assert_eq!(
            format_from_extension(Path::new("file.rs")),
            FormatKind::Code("rust".to_string())
        );
        assert_eq!(
            format_from_extension(Path::new("file.txt")),
            FormatKind::Text
        );
    }

    #[test]
    fn test_diff_dirs_identical() {
        let (old_dir, new_dir) = create_temp_dirs();
        fs::write(old_dir.path().join("test.txt"), "hello\n").unwrap();
        fs::write(new_dir.path().join("test.txt"), "hello\n").unwrap();

        let result = diff_dirs(old_dir.path(), new_dir.path()).unwrap();
        assert_eq!(result.summary.files_unchanged, 1);
        assert_eq!(result.summary.files_modified, 0);
    }

    #[test]
    fn test_diff_dirs_added_file() {
        let (old_dir, new_dir) = create_temp_dirs();
        fs::write(new_dir.path().join("new.txt"), "hello\n").unwrap();

        let result = diff_dirs(old_dir.path(), new_dir.path()).unwrap();
        assert_eq!(result.summary.files_added, 1);
    }

    #[test]
    fn test_diff_dirs_removed_file() {
        let (old_dir, new_dir) = create_temp_dirs();
        fs::write(old_dir.path().join("old.txt"), "hello\n").unwrap();

        let result = diff_dirs(old_dir.path(), new_dir.path()).unwrap();
        assert_eq!(result.summary.files_removed, 1);
    }

    #[test]
    fn test_diff_dirs_modified_file() {
        let (old_dir, new_dir) = create_temp_dirs();
        fs::write(old_dir.path().join("test.json"), r#"{"a": 1}"#).unwrap();
        fs::write(new_dir.path().join("test.json"), r#"{"a": 2}"#).unwrap();

        let result = diff_dirs(old_dir.path(), new_dir.path()).unwrap();
        assert_eq!(result.summary.files_modified, 1);

        let modified = result
            .file_results
            .iter()
            .find(|e| e.status == FileStatus::Modified)
            .unwrap();
        assert!(modified.diff.is_some());
    }
}
