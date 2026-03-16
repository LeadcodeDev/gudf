#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
    Unchanged,
}

#[derive(Debug, Clone)]
pub struct Change {
    pub kind: ChangeKind,
    pub path: Option<String>,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub location: Option<Location>,
}

#[derive(Debug, Clone)]
pub struct Location {
    pub line: usize,
    pub column: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DiffResult {
    pub changes: Vec<Change>,
    pub format: FormatKind,
    pub stats: DiffStats,
}

#[derive(Debug, Clone)]
pub struct DiffStats {
    pub additions: usize,
    pub deletions: usize,
    pub modifications: usize,
}

impl DiffStats {
    pub fn from_changes(changes: &[Change]) -> Self {
        let mut stats = DiffStats {
            additions: 0,
            deletions: 0,
            modifications: 0,
        };
        for change in changes {
            match change.kind {
                ChangeKind::Added => stats.additions += 1,
                ChangeKind::Removed => stats.deletions += 1,
                ChangeKind::Modified => stats.modifications += 1,
                ChangeKind::Unchanged => {}
            }
        }
        stats
    }
}

use crate::format::FormatKind;
