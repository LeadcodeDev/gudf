use crate::path::{self, PathSegment};
use crate::result::{Change, ChangeKind, DiffResult};

/// A composable pipeline for filtering and querying diff changes.
pub struct DiffPipeline<'a> {
    changes: Vec<&'a Change>,
}

impl<'a> DiffPipeline<'a> {
    pub fn new(changes: &'a [Change]) -> Self {
        Self {
            changes: changes.iter().collect(),
        }
    }

    /// Keep only changes matching the given kind.
    pub fn filter_kind(mut self, kind: ChangeKind) -> Self {
        self.changes.retain(|c| c.kind == kind);
        self
    }

    /// Keep only changes whose path matches a glob pattern.
    /// Supports `*` (single segment) and `**` (any depth).
    pub fn filter_path(mut self, pattern: &str) -> Self {
        let matcher = PathMatcher::new(pattern);
        self.changes
            .retain(|c| c.path.as_deref().map_or(false, |p| matcher.matches(p)));
        self
    }

    /// Exclude unchanged entries.
    pub fn exclude_unchanged(mut self) -> Self {
        self.changes.retain(|c| c.kind != ChangeKind::Unchanged);
        self
    }

    /// Apply a predicate to filter changes.
    pub fn filter<F>(mut self, predicate: F) -> Self
    where
        F: Fn(&&Change) -> bool,
    {
        self.changes.retain(|c| predicate(c));
        self
    }

    /// Map values through a transformation function, returning owned changes.
    pub fn map_values<F>(self, f: F) -> Vec<Change>
    where
        F: Fn(&Change) -> Change,
    {
        self.changes.into_iter().map(|c| f(c)).collect()
    }

    /// Collect the filtered changes as references.
    pub fn collect(self) -> Vec<&'a Change> {
        self.changes
    }

    /// Count the remaining changes.
    pub fn count(self) -> usize {
        self.changes.len()
    }

    /// Check if any changes remain.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Get the first matching change.
    pub fn first(self) -> Option<&'a Change> {
        self.changes.into_iter().next()
    }

    /// Keep only changes that have an annotation with the given key.
    pub fn filter_annotation(mut self, key: &str) -> Self {
        self.changes
            .retain(|c| c.annotations.iter().any(|a| a.key == key));
        self
    }
}

impl<'a> IntoIterator for DiffPipeline<'a> {
    type Item = &'a Change;
    type IntoIter = std::vec::IntoIter<&'a Change>;

    fn into_iter(self) -> Self::IntoIter {
        self.changes.into_iter()
    }
}

impl DiffResult {
    /// Create a pipeline for filtering and querying changes.
    pub fn pipeline(&self) -> DiffPipeline<'_> {
        DiffPipeline::new(&self.changes)
    }
}

/// Glob-style path matcher supporting `*` and `**`.
pub struct PathMatcher {
    segments: Vec<PatternSegment>,
}

#[derive(Debug)]
enum PatternSegment {
    Literal(String),
    Star,
    DoubleStar,
}

impl PathMatcher {
    pub fn new(pattern: &str) -> Self {
        let segments = pattern
            .split('.')
            .map(|s| match s {
                "**" => PatternSegment::DoubleStar,
                "*" => PatternSegment::Star,
                _ => PatternSegment::Literal(s.to_string()),
            })
            .collect();
        Self { segments }
    }

    pub fn matches(&self, input_path: &str) -> bool {
        // Use the shared path parser to decompose the path into string segments
        let parsed = path::parse_path(input_path);
        let parts: Vec<String> = parsed
            .iter()
            .map(|seg| match seg {
                PathSegment::Key(k) => k.clone(),
                PathSegment::Index(i) => i.to_string(),
            })
            .collect();
        let part_refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
        self.match_recursive(&self.segments, &part_refs)
    }

    fn match_recursive(&self, segments: &[PatternSegment], parts: &[&str]) -> bool {
        match (segments.first(), parts.first()) {
            (None, None) => true,
            (None, Some(_)) => false,
            (Some(PatternSegment::DoubleStar), _) => {
                // ** matches zero or more segments
                let rest = &segments[1..];
                if self.match_recursive(rest, parts) {
                    return true;
                }
                if !parts.is_empty() {
                    return self.match_recursive(segments, &parts[1..]);
                }
                false
            }
            (Some(_), None) => {
                // Remaining pattern segments with no parts left
                // Only match if all remaining segments are **
                segments
                    .iter()
                    .all(|s| matches!(s, PatternSegment::DoubleStar))
            }
            (Some(PatternSegment::Literal(lit)), Some(part)) => {
                if lit == part {
                    self.match_recursive(&segments[1..], &parts[1..])
                } else {
                    false
                }
            }
            (Some(PatternSegment::Star), Some(_)) => {
                self.match_recursive(&segments[1..], &parts[1..])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_change(kind: ChangeKind, path: Option<&str>) -> Change {
        Change {
            kind,
            path: path.map(|s| s.to_string()),
            old_value: Some("old".to_string()),
            new_value: Some("new".to_string()),
            location: None,
            annotations: Vec::new(),
        }
    }

    #[test]
    fn test_filter_kind() {
        let changes = vec![
            make_change(ChangeKind::Added, Some("a")),
            make_change(ChangeKind::Removed, Some("b")),
            make_change(ChangeKind::Modified, Some("c")),
        ];
        let pipeline = DiffPipeline::new(&changes);
        let result = pipeline.filter_kind(ChangeKind::Added).collect();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path.as_deref(), Some("a"));
    }

    #[test]
    fn test_filter_path() {
        let changes = vec![
            make_change(ChangeKind::Modified, Some("config.database.host")),
            make_change(ChangeKind::Modified, Some("config.database.port")),
            make_change(ChangeKind::Modified, Some("config.server.host")),
        ];
        let pipeline = DiffPipeline::new(&changes);
        let result = pipeline.filter_path("config.database.**").collect();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_filter_path_star() {
        let changes = vec![
            make_change(ChangeKind::Modified, Some("config.database")),
            make_change(ChangeKind::Modified, Some("config.server")),
        ];
        let pipeline = DiffPipeline::new(&changes);
        let result = pipeline.filter_path("config.*").collect();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_exclude_unchanged() {
        let changes = vec![
            make_change(ChangeKind::Unchanged, Some("a")),
            make_change(ChangeKind::Modified, Some("b")),
        ];
        let pipeline = DiffPipeline::new(&changes);
        let result = pipeline.exclude_unchanged().collect();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_chained_filters() {
        let changes = vec![
            make_change(ChangeKind::Added, Some("config.database.host")),
            make_change(ChangeKind::Modified, Some("config.database.port")),
            make_change(ChangeKind::Added, Some("config.server.host")),
        ];
        let pipeline = DiffPipeline::new(&changes);
        let result = pipeline
            .filter_kind(ChangeKind::Added)
            .filter_path("config.database.**")
            .collect();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path.as_deref(), Some("config.database.host"));
    }

    #[test]
    fn test_count() {
        let changes = vec![
            make_change(ChangeKind::Added, Some("a")),
            make_change(ChangeKind::Removed, Some("b")),
        ];
        let pipeline = DiffPipeline::new(&changes);
        assert_eq!(pipeline.count(), 2);
    }

    #[test]
    fn test_into_iterator() {
        let changes = vec![
            make_change(ChangeKind::Added, Some("a")),
            make_change(ChangeKind::Added, Some("b")),
        ];
        let pipeline = DiffPipeline::new(&changes);
        let collected: Vec<_> = pipeline.into_iter().collect();
        assert_eq!(collected.len(), 2);
    }

    #[test]
    fn test_path_matcher_exact() {
        let matcher = PathMatcher::new("config.database.host");
        assert!(matcher.matches("config.database.host"));
        assert!(!matcher.matches("config.database.port"));
    }

    #[test]
    fn test_path_matcher_double_star() {
        let matcher = PathMatcher::new("config.**");
        assert!(matcher.matches("config.database"));
        assert!(matcher.matches("config.database.host"));
        assert!(matcher.matches("config.database.host.deep"));
        assert!(!matcher.matches("other.database"));
    }

    #[test]
    fn test_path_matcher_star() {
        let matcher = PathMatcher::new("config.*.host");
        assert!(matcher.matches("config.database.host"));
        assert!(matcher.matches("config.server.host"));
        assert!(!matcher.matches("config.database.port"));
    }

    #[test]
    fn test_diff_result_pipeline() {
        let result = DiffResult {
            changes: vec![
                make_change(ChangeKind::Added, Some("a")),
                make_change(ChangeKind::Unchanged, Some("b")),
            ],
            format: crate::format::FormatKind::Json,
            stats: crate::result::DiffStats {
                additions: 1,
                deletions: 0,
                modifications: 0,
                moves: 0,
                renames: 0,
            },
        };
        let filtered = result.pipeline().exclude_unchanged().collect();
        assert_eq!(filtered.len(), 1);
    }
}
