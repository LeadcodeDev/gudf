use crate::engine::DiffEngine;
use crate::error::GudfError;
use crate::format::FormatKind;
use crate::patch::patch_as;
use crate::result::{Change, ChangeKind, DiffResult, DiffStats};

/// A snapshot produced by a single mutation step.
#[derive(Debug, Clone)]
pub struct MutationState {
    /// The document content after this mutation.
    pub document: String,
    /// The diff that was applied to reach this state.
    pub diff: DiffResult,
}

/// Chains multiple mutations on a document, tracking every intermediate state.
///
/// ```rust,ignore
/// use gudf::mutation::MutationChain;
/// use gudf::FormatKind;
///
/// let mut chain = MutationChain::new(r#"{"a":1,"b":2}"#, FormatKind::Json);
/// chain.mutate(&diff1)?;
/// chain.mutate(&diff2)?;
///
/// // current state after all mutations
/// let current = chain.current();
///
/// // full history: original → state1 → state2
/// let history = chain.history();
///
/// // compose every mutation into a single diff (original → current)
/// let composed = chain.compose()?;
///
/// // undo the last mutation
/// chain.undo();
/// ```
#[derive(Debug, Clone)]
pub struct MutationChain {
    original: String,
    format: FormatKind,
    states: Vec<MutationState>,
}

impl MutationChain {
    /// Create a new chain starting from `original` with the given format.
    pub fn new(original: impl Into<String>, format: FormatKind) -> Self {
        Self {
            original: original.into(),
            format,
            states: Vec::new(),
        }
    }

    /// Apply a `DiffResult` as the next mutation.
    /// Returns the new document content.
    pub fn mutate(&mut self, diff: &DiffResult) -> Result<&str, GudfError> {
        let base = self.current();
        let document = apply_diff(base, &self.format, diff)?;
        self.states.push(MutationState {
            document,
            diff: diff.clone(),
        });
        Ok(&self.states.last().unwrap().document)
    }

    /// Apply raw changes as the next mutation.
    /// The changes are wrapped in a `DiffResult` for bookkeeping.
    pub fn apply(&mut self, changes: &[Change]) -> Result<&str, GudfError> {
        let base = self.current();
        let stats = DiffStats::from_changes(changes);
        let diff = DiffResult {
            changes: changes.to_vec(),
            format: self.format.clone(),
            stats,
        };
        let document = apply_diff(base, &self.format, &diff)?;
        self.states.push(MutationState { document, diff });
        Ok(&self.states.last().unwrap().document)
    }

    /// Current document content (after all applied mutations, or the original).
    pub fn current(&self) -> &str {
        self.states
            .last()
            .map(|s| s.document.as_str())
            .unwrap_or(&self.original)
    }

    /// The original document content.
    pub fn original(&self) -> &str {
        &self.original
    }

    /// Document content at a specific step.
    /// Step 0 is the original, step 1 is after the first mutation, etc.
    pub fn at(&self, step: usize) -> Option<&str> {
        if step == 0 {
            Some(&self.original)
        } else {
            self.states.get(step - 1).map(|s| s.document.as_str())
        }
    }

    /// The diff applied at a specific step (1-indexed).
    pub fn diff_at(&self, step: usize) -> Option<&DiffResult> {
        if step == 0 {
            None
        } else {
            self.states.get(step - 1).map(|s| &s.diff)
        }
    }

    /// Number of mutations applied so far.
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// True if no mutation has been applied yet.
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// All document states including the original.
    /// Returns `[original, after_mutation_1, after_mutation_2, ...]`.
    pub fn history(&self) -> Vec<&str> {
        let mut h = vec![self.original.as_str()];
        for state in &self.states {
            h.push(&state.document);
        }
        h
    }

    /// All applied diffs in order.
    pub fn diffs(&self) -> Vec<&DiffResult> {
        self.states.iter().map(|s| &s.diff).collect()
    }

    /// Undo the last mutation. Returns the removed state, or `None` if empty.
    pub fn undo(&mut self) -> Option<MutationState> {
        self.states.pop()
    }

    /// Undo all mutations back to a specific step (0 = original).
    pub fn rewind(&mut self, step: usize) {
        self.states.truncate(step);
    }

    /// Compose all mutations into a single `DiffResult` (original → current).
    /// Re-diffs the original against the current state.
    pub fn compose(&self) -> Result<DiffResult, GudfError> {
        if self.states.is_empty() {
            let stats = DiffStats {
                additions: 0,
                deletions: 0,
                modifications: 0,
                moves: 0,
                renames: 0,
            };
            return Ok(DiffResult {
                changes: Vec::new(),
                format: self.format.clone(),
                stats,
            });
        }

        let engine = DiffEngine::new();
        engine.diff_as(self.format.clone(), &self.original, self.current())
    }

    /// Compose the diff between two steps into a single `DiffResult`.
    /// `from` and `to` are step indices (0 = original).
    pub fn compose_range(&self, from: usize, to: usize) -> Result<DiffResult, GudfError> {
        let from_doc = self
            .at(from)
            .ok_or_else(|| GudfError::PatchError(format!("Step {from} does not exist")))?;
        let to_doc = self
            .at(to)
            .ok_or_else(|| GudfError::PatchError(format!("Step {to} does not exist")))?;

        let engine = DiffEngine::new();
        engine.diff_as(self.format.clone(), from_doc, to_doc)
    }

    /// Squash: compose all mutations into a single diff and replace the history.
    /// After squash, `len() == 1` and the chain holds only original → current.
    pub fn squash(&mut self) -> Result<&DiffResult, GudfError> {
        let composed = self.compose()?;
        let document = self.current().to_string();
        self.states.clear();
        self.states.push(MutationState {
            document,
            diff: composed,
        });
        Ok(&self.states[0].diff)
    }

    /// Cumulative stats across all mutations.
    pub fn total_stats(&self) -> DiffStats {
        let mut total = DiffStats {
            additions: 0,
            deletions: 0,
            modifications: 0,
            moves: 0,
            renames: 0,
        };
        for state in &self.states {
            total.additions += state.diff.stats.additions;
            total.deletions += state.diff.stats.deletions;
            total.modifications += state.diff.stats.modifications;
            total.moves += state.diff.stats.moves;
            total.renames += state.diff.stats.renames;
        }
        total
    }
}

/// Apply a diff to a document, choosing the right strategy per format.
///
/// - **Text / Code**: the diff contains every line (Unchanged, Added, Removed)
///   so the new document is reconstructed by collecting the new-side values.
/// - **Structured (JSON, TOML, YAML)**: changes are path-based, so we delegate
///   to the existing `patch_as` which mutates the parsed document in place.
fn apply_diff(base: &str, format: &FormatKind, diff: &DiffResult) -> Result<String, GudfError> {
    match format {
        FormatKind::Text | FormatKind::Code(_) => Ok(reconstruct_text(&diff.changes)),
        _ => patch_as(format.clone(), base, &diff.changes),
    }
}

/// Reconstruct a text document from a full list of line-level changes.
///
/// Takes every line that should appear in the new document:
/// - `Unchanged` / `Added` / `Modified` → take `new_value`
/// - `Removed` → skip
fn reconstruct_text(changes: &[Change]) -> String {
    let mut out = String::new();
    for change in changes {
        match change.kind {
            ChangeKind::Unchanged | ChangeKind::Added | ChangeKind::Modified => {
                if let Some(v) = &change.new_value {
                    out.push_str(v);
                }
            }
            ChangeKind::Moved | ChangeKind::Renamed => {
                if let Some(v) = &change.new_value {
                    out.push_str(v);
                }
            }
            ChangeKind::Removed => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;

    // ── Text ────────────────────────────────────────────────────────────

    #[test]
    fn test_text_chain() {
        let mut chain = MutationChain::new("hello\nworld\n", FormatKind::Text);
        assert_eq!(chain.current(), "hello\nworld\n");
        assert!(chain.is_empty());

        let diff1 = gudf_diff_text("hello\nworld\n", "hello\nrust\n");
        chain.mutate(&diff1).unwrap();
        assert_eq!(chain.current(), "hello\nrust\n");
        assert_eq!(chain.len(), 1);

        let diff2 = gudf_diff_text("hello\nrust\n", "hello\nrust\nis great\n");
        chain.mutate(&diff2).unwrap();
        assert_eq!(chain.current(), "hello\nrust\nis great\n");
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn test_history() {
        let mut chain = MutationChain::new("v0\n", FormatKind::Text);

        let d1 = gudf_diff_text("v0\n", "v1\n");
        chain.mutate(&d1).unwrap();

        let d2 = gudf_diff_text("v1\n", "v2\n");
        chain.mutate(&d2).unwrap();

        let h = chain.history();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0], "v0\n");
        assert_eq!(h[1], "v1\n");
        assert_eq!(h[2], "v2\n");
    }

    #[test]
    fn test_at() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        let d = gudf_diff_text("a\n", "b\n");
        chain.mutate(&d).unwrap();

        assert_eq!(chain.at(0), Some("a\n"));
        assert_eq!(chain.at(1), Some("b\n"));
        assert_eq!(chain.at(2), None);
    }

    #[test]
    fn test_undo() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        let d = gudf_diff_text("a\n", "b\n");
        chain.mutate(&d).unwrap();
        assert_eq!(chain.current(), "b\n");

        chain.undo();
        assert_eq!(chain.current(), "a\n");
        assert!(chain.is_empty());
    }

    #[test]
    fn test_rewind() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);

        for (from, to) in [("a\n", "b\n"), ("b\n", "c\n"), ("c\n", "d\n")] {
            let d = gudf_diff_text(from, to);
            chain.mutate(&d).unwrap();
        }
        assert_eq!(chain.len(), 3);
        assert_eq!(chain.current(), "d\n");

        chain.rewind(1);
        assert_eq!(chain.len(), 1);
        assert_eq!(chain.current(), "b\n");
    }

    #[test]
    fn test_compose() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        let d1 = gudf_diff_text("a\n", "b\n");
        chain.mutate(&d1).unwrap();
        let d2 = gudf_diff_text("b\n", "c\n");
        chain.mutate(&d2).unwrap();

        let composed = chain.compose().unwrap();
        // composed is original("a\n") → current("c\n")
        assert!(composed.stats.additions > 0 || composed.stats.deletions > 0);
    }

    #[test]
    fn test_compose_range() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        let d1 = gudf_diff_text("a\n", "b\n");
        chain.mutate(&d1).unwrap();
        let d2 = gudf_diff_text("b\n", "c\n");
        chain.mutate(&d2).unwrap();
        let d3 = gudf_diff_text("c\n", "d\n");
        chain.mutate(&d3).unwrap();

        let range = chain.compose_range(1, 3).unwrap();
        // diff of step 1 ("b\n") → step 3 ("d\n")
        assert!(range.stats.additions > 0 || range.stats.deletions > 0);
    }

    #[test]
    fn test_squash() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        let d1 = gudf_diff_text("a\n", "b\n");
        chain.mutate(&d1).unwrap();
        let d2 = gudf_diff_text("b\n", "c\n");
        chain.mutate(&d2).unwrap();

        chain.squash().unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain.current(), "c\n");
        assert_eq!(chain.original(), "a\n");
    }

    #[test]
    fn test_total_stats() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        let d1 = gudf_diff_text("a\n", "a\nb\n");
        chain.mutate(&d1).unwrap();
        let d2 = gudf_diff_text("a\nb\n", "a\nb\nc\n");
        chain.mutate(&d2).unwrap();

        let stats = chain.total_stats();
        assert_eq!(stats.additions, 2);
    }

    // ── JSON ────────────────────────────────────────────────────────────

    #[test]
    fn test_json_chain() {
        let mut chain = MutationChain::new(r#"{"a":1,"b":2}"#, FormatKind::Json);

        let d1 = gudf_diff_json(r#"{"a":1,"b":2}"#, r#"{"a":10,"b":2}"#);
        chain.mutate(&d1).unwrap();

        let current: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(current["a"], 10);
        assert_eq!(current["b"], 2);

        let d2 = gudf_diff_json(chain.current(), r#"{"a":10,"b":20,"c":30}"#);
        chain.mutate(&d2).unwrap();

        let current: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(current["a"], 10);
        assert_eq!(current["b"], 20);
        assert_eq!(current["c"], 30);
    }

    #[test]
    fn test_json_compose_roundtrip() {
        let original = r#"{"x":1}"#;
        let mut chain = MutationChain::new(original, FormatKind::Json);

        let d1 = gudf_diff_json(original, r#"{"x":2}"#);
        chain.mutate(&d1).unwrap();
        let d2 = gudf_diff_json(chain.current(), r#"{"x":2,"y":3}"#);
        chain.mutate(&d2).unwrap();

        // Compose into a single diff, then apply it to original
        let composed = chain.compose().unwrap();
        let reconstructed = patch_as(FormatKind::Json, original, &composed.changes).unwrap();

        let expected: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        let got: serde_json::Value = serde_json::from_str(&reconstructed).unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn test_apply_raw_changes() {
        let mut chain = MutationChain::new(r#"{"a":1}"#, FormatKind::Json);

        let changes = vec![Change {
            kind: ChangeKind::Modified,
            path: Some("a".to_string()),
            old_value: Some("1".to_string()),
            new_value: Some("99".to_string()),
            location: None,
            annotations: Vec::new(),
        }];

        chain.apply(&changes).unwrap();
        let current: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(current["a"], 99);
    }

    #[test]
    fn test_empty_compose() {
        let chain = MutationChain::new("hello\n", FormatKind::Text);
        let composed = chain.compose().unwrap();
        assert!(composed.changes.is_empty());
    }

    #[test]
    fn test_diffs() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        let d1 = gudf_diff_text("a\n", "b\n");
        chain.mutate(&d1).unwrap();
        let d2 = gudf_diff_text("b\n", "c\n");
        chain.mutate(&d2).unwrap();

        let diffs = chain.diffs();
        assert_eq!(diffs.len(), 2);
    }

    #[test]
    fn test_text_multiline_chain() {
        let v0 = "line1\nline2\nline3\n";
        let v1 = "line1\nmodified\nline3\n";
        let v2 = "line1\nmodified\nline3\nline4\n";
        let v3 = "header\nline1\nmodified\nline3\nline4\n";

        let mut chain = MutationChain::new(v0, FormatKind::Text);

        chain.mutate(&gudf_diff_text(v0, v1)).unwrap();
        assert_eq!(chain.current(), v1);

        chain.mutate(&gudf_diff_text(v1, v2)).unwrap();
        assert_eq!(chain.current(), v2);

        chain.mutate(&gudf_diff_text(v2, v3)).unwrap();
        assert_eq!(chain.current(), v3);

        // Compose full chain
        let composed = chain.compose().unwrap();
        let rebuilt = reconstruct_text(&composed.changes);
        assert_eq!(rebuilt, v3);
    }

    #[test]
    fn test_json_undo_redo_pattern() {
        let original = r#"{"count":0}"#;
        let mut chain = MutationChain::new(original, FormatKind::Json);

        // Increment 3 times
        for i in 1..=3 {
            let prev = chain.current().to_string();
            let next = format!(r#"{{"count":{i}}}"#);
            let d = gudf_diff_json(&prev, &next);
            chain.mutate(&d).unwrap();
        }

        let val: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(val["count"], 3);

        // Undo twice → count should be 1
        chain.undo();
        chain.undo();
        let val: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(val["count"], 1);
    }

    // ── helpers ─────────────────────────────────────────────────────────

    fn gudf_diff_text(old: &str, new: &str) -> DiffResult {
        crate::formats::text::TextFormat.diff(old, new).unwrap()
    }

    fn gudf_diff_json(old: &str, new: &str) -> DiffResult {
        crate::formats::json::JsonFormat.diff(old, new).unwrap()
    }
}
