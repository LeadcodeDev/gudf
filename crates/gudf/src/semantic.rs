use std::collections::HashSet;

use crate::result::{Change, ChangeKind, DiffResult, DiffStats};

/// Configuration for semantic analysis.
#[derive(Debug, Clone)]
pub struct SemanticOptions {
    /// Enable move detection (same value at different path).
    pub move_detection: bool,
    /// Enable rename detection (different key, same value).
    pub rename_detection: bool,
    /// Similarity threshold for fuzzy matching (0.0 to 1.0).
    /// V1 only uses exact match (threshold = 1.0).
    pub rename_threshold: f64,
}

impl Default for SemanticOptions {
    fn default() -> Self {
        Self {
            move_detection: true,
            rename_detection: true,
            rename_threshold: 1.0,
        }
    }
}

/// Post-processor that detects moves and renames in a diff result.
pub struct SemanticAnalyzer {
    options: SemanticOptions,
}

impl SemanticAnalyzer {
    pub fn new(options: SemanticOptions) -> Self {
        Self { options }
    }

    pub fn with_defaults() -> Self {
        Self::new(SemanticOptions::default())
    }

    /// Analyze a diff result and convert Remove+Add pairs into Moved/Renamed where appropriate.
    pub fn analyze(&self, result: DiffResult) -> DiffResult {
        let mut changes = result.changes;
        self.detect_moves_and_renames(&mut changes);

        let stats = DiffStats::from_changes(&changes);
        DiffResult {
            changes,
            format: result.format,
            stats,
        }
    }

    fn detect_moves_and_renames(&self, changes: &mut Vec<Change>) {
        // Collect indices of Removed and Added changes
        let removed_indices: Vec<usize> = changes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == ChangeKind::Removed)
            .map(|(i, _)| i)
            .collect();

        let added_indices: Vec<usize> = changes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == ChangeKind::Added)
            .map(|(i, _)| i)
            .collect();

        let mut matched_removed: Vec<bool> = vec![false; removed_indices.len()];
        let mut matched_added: Vec<bool> = vec![false; added_indices.len()];
        let mut to_remove: HashSet<usize> = HashSet::new();

        // For each removed+added pair, check for exact value match
        for (ri, &removed_idx) in removed_indices.iter().enumerate() {
            if matched_removed[ri] {
                continue;
            }

            let removed = &changes[removed_idx];
            let removed_value = removed.old_value.as_deref().unwrap_or("");
            let removed_path = removed.path.as_deref().unwrap_or("");

            if removed_value.is_empty() {
                continue;
            }

            for (ai, &added_idx) in added_indices.iter().enumerate() {
                if matched_added[ai] {
                    continue;
                }

                let added = &changes[added_idx];
                let added_value = added.new_value.as_deref().unwrap_or("");
                let added_path = added.path.as_deref().unwrap_or("");

                if removed_value != added_value {
                    continue;
                }

                // Same value found at different paths. Determine if it's a Move or Rename.
                if removed_path == added_path {
                    // Same path, same value — this is Unchanged, skip
                    continue;
                }

                let is_sibling = Self::are_siblings(removed_path, added_path);

                // Check flags: siblings → rename_detection, non-siblings → move_detection
                if is_sibling && !self.options.rename_detection {
                    continue;
                }
                if !is_sibling && !self.options.move_detection {
                    continue;
                }

                let kind = if is_sibling {
                    ChangeKind::Renamed
                } else {
                    ChangeKind::Moved
                };

                matched_removed[ri] = true;
                matched_added[ai] = true;

                // Update the removed entry to be a Move/Rename
                changes[removed_idx] = Change {
                    kind,
                    path: Some(removed_path.to_string()),
                    old_value: Some(removed_value.to_string()),
                    new_value: Some(added_path.to_string()),
                    location: changes[removed_idx].location.clone(),
                    annotations: changes[removed_idx].annotations.clone(),
                };

                // Mark the added entry for removal
                to_remove.insert(added_idx);
                break;
            }
        }

        // Remove the paired added entries by index
        let mut idx = 0;
        changes.retain(|_| {
            let keep = !to_remove.contains(&idx);
            idx += 1;
            keep
        });
    }

    /// Check if two paths are siblings (same parent, different leaf).
    fn are_siblings(path_a: &str, path_b: &str) -> bool {
        let parent_a = path_a.rsplit_once('.').map(|(p, _)| p);
        let parent_b = path_b.rsplit_once('.').map(|(p, _)| p);
        parent_a == parent_b && parent_a.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::FormatKind;

    fn make_result(changes: Vec<Change>) -> DiffResult {
        let stats = DiffStats::from_changes(&changes);
        DiffResult {
            changes,
            format: FormatKind::Json,
            stats,
        }
    }

    #[test]
    fn test_rename_detection() {
        let changes = vec![
            Change {
                kind: ChangeKind::Removed,
                path: Some("user.userName".to_string()),
                old_value: Some("\"Alice\"".to_string()),
                new_value: None,
                location: None,
                annotations: Vec::new(),
            },
            Change {
                kind: ChangeKind::Added,
                path: Some("user.user_name".to_string()),
                old_value: None,
                new_value: Some("\"Alice\"".to_string()),
                location: None,
                annotations: Vec::new(),
            },
        ];

        let analyzer = SemanticAnalyzer::with_defaults();
        let result = analyzer.analyze(make_result(changes));

        assert_eq!(result.stats.renames, 1);
        let renamed = result
            .changes
            .iter()
            .find(|c| c.kind == ChangeKind::Renamed)
            .expect("should have a Renamed change");
        assert_eq!(renamed.path.as_deref(), Some("user.userName"));
        assert_eq!(renamed.new_value.as_deref(), Some("user.user_name"));
    }

    #[test]
    fn test_move_detection() {
        let changes = vec![
            Change {
                kind: ChangeKind::Removed,
                path: Some("old_section.key".to_string()),
                old_value: Some("\"value\"".to_string()),
                new_value: None,
                location: None,
                annotations: Vec::new(),
            },
            Change {
                kind: ChangeKind::Added,
                path: Some("new_section.key".to_string()),
                old_value: None,
                new_value: Some("\"value\"".to_string()),
                location: None,
                annotations: Vec::new(),
            },
        ];

        let analyzer = SemanticAnalyzer::with_defaults();
        let result = analyzer.analyze(make_result(changes));

        assert_eq!(result.stats.moves, 1);
        let moved = result
            .changes
            .iter()
            .find(|c| c.kind == ChangeKind::Moved)
            .expect("should have a Moved change");
        assert_eq!(moved.path.as_deref(), Some("old_section.key"));
    }

    #[test]
    fn test_no_false_positives() {
        let changes = vec![
            Change {
                kind: ChangeKind::Removed,
                path: Some("key1".to_string()),
                old_value: Some("\"value1\"".to_string()),
                new_value: None,
                location: None,
                annotations: Vec::new(),
            },
            Change {
                kind: ChangeKind::Added,
                path: Some("key2".to_string()),
                old_value: None,
                new_value: Some("\"value2\"".to_string()),
                location: None,
                annotations: Vec::new(),
            },
        ];

        let analyzer = SemanticAnalyzer::with_defaults();
        let result = analyzer.analyze(make_result(changes));

        assert_eq!(result.stats.moves, 0);
        assert_eq!(result.stats.renames, 0);
    }

    #[test]
    fn test_disabled_detection() {
        let changes = vec![
            Change {
                kind: ChangeKind::Removed,
                path: Some("user.userName".to_string()),
                old_value: Some("\"Alice\"".to_string()),
                new_value: None,
                location: None,
                annotations: Vec::new(),
            },
            Change {
                kind: ChangeKind::Added,
                path: Some("user.user_name".to_string()),
                old_value: None,
                new_value: Some("\"Alice\"".to_string()),
                location: None,
                annotations: Vec::new(),
            },
        ];

        let options = SemanticOptions {
            move_detection: false,
            rename_detection: false,
            rename_threshold: 1.0,
        };
        let analyzer = SemanticAnalyzer::new(options);
        let result = analyzer.analyze(make_result(changes));

        assert_eq!(result.stats.renames, 0);
        assert_eq!(result.stats.moves, 0);
    }

    #[test]
    fn test_move_off_rename_on() {
        // Siblings should be detected as Renamed, non-siblings should be ignored
        let changes = vec![
            // Sibling pair (same parent "user")
            Change {
                kind: ChangeKind::Removed,
                path: Some("user.firstName".to_string()),
                old_value: Some("\"Alice\"".to_string()),
                new_value: None,
                location: None,
                annotations: Vec::new(),
            },
            Change {
                kind: ChangeKind::Added,
                path: Some("user.first_name".to_string()),
                old_value: None,
                new_value: Some("\"Alice\"".to_string()),
                location: None,
                annotations: Vec::new(),
            },
            // Non-sibling pair (different parents)
            Change {
                kind: ChangeKind::Removed,
                path: Some("old_section.key".to_string()),
                old_value: Some("\"val\"".to_string()),
                new_value: None,
                location: None,
                annotations: Vec::new(),
            },
            Change {
                kind: ChangeKind::Added,
                path: Some("new_section.key".to_string()),
                old_value: None,
                new_value: Some("\"val\"".to_string()),
                location: None,
                annotations: Vec::new(),
            },
        ];

        let options = SemanticOptions {
            move_detection: false,
            rename_detection: true,
            rename_threshold: 1.0,
        };
        let analyzer = SemanticAnalyzer::new(options);
        let result = analyzer.analyze(make_result(changes));

        assert_eq!(result.stats.renames, 1);
        assert_eq!(result.stats.moves, 0);
        // The non-sibling pair should remain as Added+Removed
        assert_eq!(result.stats.additions, 1);
        assert_eq!(result.stats.deletions, 1);
    }

    #[test]
    fn test_move_on_rename_off() {
        // Non-siblings should be detected as Moved, siblings should be ignored
        let changes = vec![
            // Sibling pair
            Change {
                kind: ChangeKind::Removed,
                path: Some("user.firstName".to_string()),
                old_value: Some("\"Alice\"".to_string()),
                new_value: None,
                location: None,
                annotations: Vec::new(),
            },
            Change {
                kind: ChangeKind::Added,
                path: Some("user.first_name".to_string()),
                old_value: None,
                new_value: Some("\"Alice\"".to_string()),
                location: None,
                annotations: Vec::new(),
            },
            // Non-sibling pair
            Change {
                kind: ChangeKind::Removed,
                path: Some("old_section.key".to_string()),
                old_value: Some("\"val\"".to_string()),
                new_value: None,
                location: None,
                annotations: Vec::new(),
            },
            Change {
                kind: ChangeKind::Added,
                path: Some("new_section.key".to_string()),
                old_value: None,
                new_value: Some("\"val\"".to_string()),
                location: None,
                annotations: Vec::new(),
            },
        ];

        let options = SemanticOptions {
            move_detection: true,
            rename_detection: false,
            rename_threshold: 1.0,
        };
        let analyzer = SemanticAnalyzer::new(options);
        let result = analyzer.analyze(make_result(changes));

        assert_eq!(result.stats.moves, 1);
        assert_eq!(result.stats.renames, 0);
        // The sibling pair should remain as Added+Removed
        assert_eq!(result.stats.additions, 1);
        assert_eq!(result.stats.deletions, 1);
    }
}
