use std::fmt;
use std::fs;
use std::path::Path;

use sha1::{Digest, Sha1};

use crate::engine::DiffEngine;
use crate::error::GudfError;
use crate::format::FormatKind;
use crate::output::unified::UnifiedFormatter;
use crate::output::OutputFormatter;
use crate::patch::patch_as;
use crate::result::{Change, ChangeKind, DiffResult, DiffStats};

/// SHA-1 content hash of a document state, mirroring git's object model.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ContentSha([u8; 20]);

impl ContentSha {
    /// Compute the SHA-1 hash of the given content bytes.
    pub fn from_content(content: &str) -> Self {
        let mut hasher = Sha1::new();
        hasher.update(content.as_bytes());
        let result = hasher.finalize();
        Self(result.into())
    }

    /// Full hex-encoded SHA-1 (40 characters).
    pub fn full(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Short SHA — first 7 hex characters.
    pub fn short(&self) -> String {
        self.full()[..7].to_string()
    }

    /// Check whether a hex prefix (short or full) matches this sha.
    pub fn matches_prefix(&self, prefix: &str) -> bool {
        let full = self.full();
        full.starts_with(prefix)
    }

    /// Raw bytes.
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }
}

impl fmt::Debug for ContentSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentSha({})", self.short())
    }
}

impl fmt::Display for ContentSha {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short())
    }
}

/// A snapshot produced by a single mutation step.
#[derive(Debug, Clone)]
pub struct MutationState {
    /// The document content after this mutation.
    pub document: String,
    /// SHA-1 hash of the document content.
    pub sha: ContentSha,
    /// The diff that was applied to reach this state.
    pub diff: DiffResult,
}

/// An entry in the mutation log.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Step index (0 = original).
    pub step: usize,
    /// SHA of the document at this step.
    pub sha: ContentSha,
    /// Summary stats of the diff applied at this step (`None` for step 0).
    pub stats: Option<DiffStats>,
}

/// Chains multiple mutations on a document, tracking every intermediate state.
///
/// Each state (including the original) is identified by a `ContentSha` — a
/// SHA-1 hash of its content, like git blob objects. You can look up any
/// state by full sha, short sha, or prefix.
///
/// ```rust,ignore
/// use gudf::mutation::MutationChain;
/// use gudf::FormatKind;
///
/// let mut chain = MutationChain::new(r#"{"a":1}"#, FormatKind::Json);
/// println!("original: {}", chain.original_sha()); // e.g. "a3f1c2d"
///
/// chain.mutate(&diff)?;
/// println!("current:  {}", chain.current_sha());  // e.g. "b7e4f90"
///
/// // Look up by sha prefix
/// let (step, doc) = chain.find_by_sha("b7e4").unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct MutationChain {
    original: String,
    original_sha: ContentSha,
    format: FormatKind,
    states: Vec<MutationState>,
    redo_stack: Vec<MutationState>,
}

impl MutationChain {
    /// Create a new chain starting from `original` with the given format.
    pub fn new(original: impl Into<String>, format: FormatKind) -> Self {
        let original = original.into();
        let original_sha = ContentSha::from_content(&original);
        Self {
            original,
            original_sha,
            format,
            states: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    // ── Mutation ────────────────────────────────────────────────────────

    /// Apply a `DiffResult` as the next mutation.
    /// Clears the redo stack (new mutation forks the history).
    /// Returns the new document content.
    pub fn mutate(&mut self, diff: &DiffResult) -> Result<&str, GudfError> {
        let base = self.current();
        let document = apply_diff(base, &self.format, diff)?;
        let sha = ContentSha::from_content(&document);
        self.redo_stack.clear();
        self.states.push(MutationState {
            document,
            sha,
            diff: diff.clone(),
        });
        Ok(&self.states.last().unwrap().document)
    }

    /// Apply raw changes as the next mutation.
    /// Clears the redo stack (new mutation forks the history).
    pub fn apply(&mut self, changes: &[Change]) -> Result<&str, GudfError> {
        let base = self.current();
        let stats = DiffStats::from_changes(changes);
        let diff = DiffResult {
            changes: changes.to_vec(),
            format: self.format.clone(),
            stats,
        };
        let document = apply_diff(base, &self.format, &diff)?;
        let sha = ContentSha::from_content(&document);
        self.redo_stack.clear();
        self.states.push(MutationState {
            document,
            sha,
            diff,
        });
        Ok(&self.states.last().unwrap().document)
    }

    // ── State access ───────────────────────────────────────────────────

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

    // ── SHA ────────────────────────────────────────────────────────────

    /// SHA of the original document.
    pub fn original_sha(&self) -> &ContentSha {
        &self.original_sha
    }

    /// SHA of the current document state.
    pub fn current_sha(&self) -> &ContentSha {
        self.states
            .last()
            .map(|s| &s.sha)
            .unwrap_or(&self.original_sha)
    }

    /// SHA at a specific step (0 = original).
    pub fn sha_at(&self, step: usize) -> Option<&ContentSha> {
        if step == 0 {
            Some(&self.original_sha)
        } else {
            self.states.get(step - 1).map(|s| &s.sha)
        }
    }

    /// All SHAs from original through current, in order.
    pub fn shas(&self) -> Vec<&ContentSha> {
        let mut v = vec![&self.original_sha];
        for state in &self.states {
            v.push(&state.sha);
        }
        v
    }

    /// Find a state by full or partial SHA prefix.
    /// Returns `(step, document_content)` or `None` if no match / ambiguous.
    pub fn find_by_sha(&self, prefix: &str) -> Option<(usize, &str)> {
        let matches: Vec<(usize, &str)> = self
            .history()
            .into_iter()
            .enumerate()
            .filter(|(step, _)| {
                self.sha_at(*step)
                    .map_or(false, |sha| sha.matches_prefix(prefix))
            })
            .collect();

        if matches.len() == 1 {
            Some(matches[0])
        } else {
            None // no match or ambiguous
        }
    }

    /// Produce a git-log-style summary of all states.
    pub fn log(&self) -> Vec<LogEntry> {
        let mut entries = vec![LogEntry {
            step: 0,
            sha: self.original_sha.clone(),
            stats: None,
        }];
        for (i, state) in self.states.iter().enumerate() {
            entries.push(LogEntry {
                step: i + 1,
                sha: state.sha.clone(),
                stats: Some(state.diff.stats.clone()),
            });
        }
        entries
    }

    // ── Expressions ─────────────────────────────────────────────────────

    /// Resolve a git-like expression to a `(step, document)` pair.
    ///
    /// Supported expressions:
    /// - `HEAD`            — current state
    /// - `HEAD~N`          — N steps back from HEAD
    /// - `HEAD^`           — parent of HEAD (same as `HEAD~1`)
    /// - `HEAD^^`          — grandparent (same as `HEAD~2`)
    /// - `ORIG`            — the original document (step 0)
    /// - `@N`              — step N directly (e.g. `@0`, `@3`)
    /// - `<sha-prefix>`    — lookup by full or short SHA
    pub fn resolve(&self, expr: &str) -> Option<(usize, &str)> {
        let expr = expr.trim();

        // ORIG
        if expr.eq_ignore_ascii_case("ORIG") {
            return Some((0, &self.original));
        }

        // @N — direct step index
        if let Some(rest) = expr.strip_prefix('@') {
            if let Ok(step) = rest.parse::<usize>() {
                return self.at(step).map(|doc| (step, doc));
            }
        }

        // HEAD variants
        if expr.eq_ignore_ascii_case("HEAD") {
            let step = self.states.len();
            return Some((step, self.current()));
        }

        if let Some(head_expr) = expr
            .strip_prefix("HEAD")
            .or_else(|| expr.strip_prefix("head"))
        {
            // HEAD~N
            if let Some(rest) = head_expr.strip_prefix('~') {
                let n: usize = rest.parse().ok()?;
                let current_step = self.states.len();
                let target = current_step.checked_sub(n)?;
                return self.at(target).map(|doc| (target, doc));
            }

            // HEAD^^^... — count carets
            if head_expr.starts_with('^') && head_expr.chars().all(|c| c == '^') {
                let n = head_expr.len();
                let current_step = self.states.len();
                let target = current_step.checked_sub(n)?;
                return self.at(target).map(|doc| (target, doc));
            }
        }

        // Fall through: try as SHA prefix
        self.find_by_sha(expr)
    }

    /// Resolve an expression and return the document content, or an error.
    pub fn resolve_or_err(&self, expr: &str) -> Result<(usize, &str), GudfError> {
        self.resolve(expr).ok_or_else(|| {
            GudfError::PatchError(format!("Cannot resolve expression: '{expr}'"))
        })
    }

    /// Compose the diff between two expressions.
    ///
    /// ```rust,ignore
    /// let diff = chain.diff_expr("HEAD~3", "HEAD")?;
    /// let diff = chain.diff_expr("ORIG", "HEAD~1")?;
    /// let diff = chain.diff_expr("@1", "@3")?;
    /// ```
    pub fn diff_expr(&self, from: &str, to: &str) -> Result<DiffResult, GudfError> {
        let (from_step, _) = self.resolve_or_err(from)?;
        let (to_step, _) = self.resolve_or_err(to)?;
        self.compose_range(from_step, to_step)
    }

    // ── File I/O ──────────────────────────────────────────────────────

    /// Create a chain from a file. Format is auto-detected from extension.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, GudfError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(GudfError::Io)?;
        let format = format_from_extension(path);
        Ok(Self::new(content, format))
    }

    /// Create a chain from a file with an explicit format.
    pub fn from_file_as(path: impl AsRef<Path>, format: FormatKind) -> Result<Self, GudfError> {
        let content = fs::read_to_string(path).map_err(GudfError::Io)?;
        Ok(Self::new(content, format))
    }

    /// Write the current state to a file.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), GudfError> {
        fs::write(path, self.current()).map_err(GudfError::Io)
    }

    /// Write the state at a given expression to a file.
    pub fn save_expr(&self, expr: &str, path: impl AsRef<Path>) -> Result<(), GudfError> {
        let (_, doc) = self.resolve_or_err(expr)?;
        fs::write(path, doc).map_err(GudfError::Io)
    }

    /// Read a file, diff it against the current state, and apply the diff as
    /// the next mutation. Returns the new document content.
    pub fn mutate_file(&mut self, path: impl AsRef<Path>) -> Result<&str, GudfError> {
        let new_content = fs::read_to_string(path).map_err(GudfError::Io)?;
        let engine = DiffEngine::new();
        let diff = engine.diff_as(self.format.clone(), self.current(), &new_content)?;
        self.mutate(&diff)
    }

    // ── Undo / Redo ────────────────────────────────────────────────────

    /// Undo the last mutation, pushing it onto the redo stack.
    pub fn undo(&mut self) -> bool {
        if let Some(state) = self.states.pop() {
            self.redo_stack.push(state);
            true
        } else {
            false
        }
    }

    /// Redo the last undone mutation.
    pub fn redo(&mut self) -> bool {
        if let Some(state) = self.redo_stack.pop() {
            self.states.push(state);
            true
        } else {
            false
        }
    }

    /// Undo `n` mutations at once.
    pub fn undo_n(&mut self, n: usize) -> usize {
        let mut count = 0;
        for _ in 0..n {
            if self.undo() {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Redo `n` mutations at once.
    pub fn redo_n(&mut self, n: usize) -> usize {
        let mut count = 0;
        for _ in 0..n {
            if self.redo() {
                count += 1;
            } else {
                break;
            }
        }
        count
    }

    /// Redo all undone mutations.
    pub fn redo_all(&mut self) -> usize {
        let n = self.redo_stack.len();
        self.redo_n(n)
    }

    /// True if there is at least one mutation to undo.
    pub fn can_undo(&self) -> bool {
        !self.states.is_empty()
    }

    /// True if there is at least one mutation to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Number of mutations available for redo.
    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }

    /// Undo back to a specific step (0 = original).
    /// Undone states are pushed onto the redo stack.
    pub fn rewind(&mut self, step: usize) {
        while self.states.len() > step {
            if !self.undo() {
                break;
            }
        }
    }

    // ── Compose / Squash ───────────────────────────────────────────────

    /// Compose all mutations into a single `DiffResult` (original → current).
    pub fn compose(&self) -> Result<DiffResult, GudfError> {
        if self.states.is_empty() {
            return Ok(DiffResult {
                changes: Vec::new(),
                format: self.format.clone(),
                stats: DiffStats {
                    additions: 0,
                    deletions: 0,
                    modifications: 0,
                    moves: 0,
                    renames: 0,
                },
            });
        }
        let engine = DiffEngine::new();
        engine.diff_as(self.format.clone(), &self.original, self.current())
    }

    /// Compose the diff between two steps.
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

    /// Squash all mutations into one. Clears redo stack.
    pub fn squash(&mut self) -> Result<&DiffResult, GudfError> {
        let composed = self.compose()?;
        let document = self.current().to_string();
        let sha = ContentSha::from_content(&document);
        self.states.clear();
        self.redo_stack.clear();
        self.states.push(MutationState {
            document,
            sha,
            diff: composed,
        });
        Ok(&self.states[0].diff)
    }

    // ── Expression-based formatting ───────────────────────────────────

    /// Start building a unified diff between two expressions.
    ///
    /// ```rust,ignore
    /// let output = chain.unified("HEAD~1", "HEAD")
    ///     .context(5)
    ///     .render()?;
    /// ```
    pub fn unified<'a>(&'a self, from: &str, to: &str) -> ExprDiffBuilder<'a> {
        ExprDiffBuilder::new(self, from, to)
    }

    /// Render a diff between two expressions with any `OutputFormatter`.
    ///
    /// ```rust,ignore
    /// let output = chain.format_expr("ORIG", "HEAD", &InlineFormatter)?;
    /// ```
    pub fn format_expr(
        &self,
        from: &str,
        to: &str,
        formatter: &dyn OutputFormatter,
    ) -> Result<String, GudfError> {
        let diff = self.diff_expr(from, to)?;
        Ok(formatter.format(&diff))
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

// ── ExprDiffBuilder ───────────────────────────────────────────────────

/// Builder for creating formatted diffs between two expressions.
///
/// Created via [`MutationChain::unified`]. Resolves expressions, computes
/// the diff, and renders with configurable context lines.
///
/// ```rust,ignore
/// let output = chain.unified("ORIG", "HEAD")
///     .context(5)
///     .render()?;
///
/// // Use a custom formatter instead of unified:
/// let output = chain.unified("HEAD~2", "HEAD")
///     .render_with(&InlineFormatter)?;
/// ```
pub struct ExprDiffBuilder<'a> {
    chain: &'a MutationChain,
    from: String,
    to: String,
    context_lines: usize,
}

impl<'a> ExprDiffBuilder<'a> {
    fn new(chain: &'a MutationChain, from: &str, to: &str) -> Self {
        Self {
            chain,
            from: from.to_string(),
            to: to.to_string(),
            context_lines: 3,
        }
    }

    /// Set the number of context lines (default: 3).
    pub fn context(mut self, lines: usize) -> Self {
        self.context_lines = lines;
        self
    }

    /// Resolve expressions and compute the diff.
    pub fn diff(&self) -> Result<DiffResult, GudfError> {
        self.chain.diff_expr(&self.from, &self.to)
    }

    /// Render the diff as a unified format string.
    pub fn render(&self) -> Result<String, GudfError> {
        let diff = self.diff()?;
        let from_label = self.label(&self.from);
        let to_label = self.label(&self.to);
        let formatter = UnifiedFormatter::new(from_label, to_label).context(self.context_lines);
        Ok(formatter.format(&diff))
    }

    /// Render with a custom `OutputFormatter`.
    pub fn render_with(&self, formatter: &dyn OutputFormatter) -> Result<String, GudfError> {
        let diff = self.diff()?;
        Ok(formatter.format(&diff))
    }

    /// Build a label for the `---`/`+++` header from an expression.
    /// Appends the short-sha for context.
    fn label(&self, expr: &str) -> String {
        if let Some((step, _)) = self.chain.resolve(expr) {
            if let Some(sha) = self.chain.sha_at(step) {
                return format!("{expr} ({})", sha.short());
            }
        }
        expr.to_string()
    }
}

// ── Internal helpers ───────────────────────────────────────────────────

/// Detect format from file extension.
fn format_from_extension(path: &Path) -> FormatKind {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "json" => FormatKind::Json,
        "toml" => FormatKind::Toml,
        "yaml" | "yml" => FormatKind::Yaml,
        _ => FormatKind::Text,
    }
}

fn apply_diff(base: &str, format: &FormatKind, diff: &DiffResult) -> Result<String, GudfError> {
    match format {
        FormatKind::Text | FormatKind::Code(_) => Ok(reconstruct_text(&diff.changes)),
        _ => patch_as(format.clone(), base, &diff.changes),
    }
}

fn reconstruct_text(changes: &[Change]) -> String {
    let mut out = String::new();
    for change in changes {
        match change.kind {
            ChangeKind::Unchanged
            | ChangeKind::Added
            | ChangeKind::Modified
            | ChangeKind::Moved
            | ChangeKind::Renamed => {
                if let Some(v) = &change.new_value {
                    out.push_str(v);
                }
            }
            ChangeKind::Removed => {}
        }
    }
    out
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Format;

    // ── ContentSha ─────────────────────────────────────────────────────

    #[test]
    fn test_sha_deterministic() {
        let a = ContentSha::from_content("hello");
        let b = ContentSha::from_content("hello");
        assert_eq!(a, b);
        assert_eq!(a.full(), b.full());
    }

    #[test]
    fn test_sha_different_content() {
        let a = ContentSha::from_content("hello");
        let b = ContentSha::from_content("world");
        assert_ne!(a, b);
        assert_ne!(a.full(), b.full());
        assert_ne!(a.short(), b.short());
    }

    #[test]
    fn test_sha_format() {
        let sha = ContentSha::from_content("test");
        assert_eq!(sha.full().len(), 40);
        assert_eq!(sha.short().len(), 7);
        assert!(sha.full().starts_with(&sha.short()));
    }

    #[test]
    fn test_sha_display() {
        let sha = ContentSha::from_content("test");
        let display = format!("{sha}");
        assert_eq!(display, sha.short());
    }

    #[test]
    fn test_sha_matches_prefix() {
        let sha = ContentSha::from_content("test");
        let full = sha.full();
        assert!(sha.matches_prefix(&full));
        assert!(sha.matches_prefix(&full[..7]));
        assert!(sha.matches_prefix(&full[..4]));
        assert!(!sha.matches_prefix("0000000"));
    }

    // ── Chain SHA tracking ─────────────────────────────────────────────

    #[test]
    fn test_original_sha() {
        let chain = MutationChain::new("hello\n", FormatKind::Text);
        let sha = chain.original_sha();
        assert_eq!(sha, &ContentSha::from_content("hello\n"));
        assert_eq!(chain.current_sha(), sha);
    }

    #[test]
    fn test_sha_changes_on_mutate() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        let sha0 = chain.current_sha().clone();

        let d = gudf_diff_text("a\n", "b\n");
        chain.mutate(&d).unwrap();
        let sha1 = chain.current_sha().clone();

        assert_ne!(sha0, sha1);
        assert_eq!(sha1, ContentSha::from_content("b\n"));
    }

    #[test]
    fn test_sha_at() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        let d = gudf_diff_text("a\n", "b\n");
        chain.mutate(&d).unwrap();

        assert_eq!(chain.sha_at(0), Some(&ContentSha::from_content("a\n")));
        assert_eq!(chain.sha_at(1), Some(&ContentSha::from_content("b\n")));
        assert_eq!(chain.sha_at(2), None);
    }

    #[test]
    fn test_shas() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();

        let shas = chain.shas();
        assert_eq!(shas.len(), 3);
        assert_eq!(shas[0], &ContentSha::from_content("a\n"));
        assert_eq!(shas[1], &ContentSha::from_content("b\n"));
        assert_eq!(shas[2], &ContentSha::from_content("c\n"));
    }

    #[test]
    fn test_find_by_sha_full() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        let target_sha = ContentSha::from_content("b\n").full();
        let found = chain.find_by_sha(&target_sha);
        assert!(found.is_some());
        let (step, doc) = found.unwrap();
        assert_eq!(step, 1);
        assert_eq!(doc, "b\n");
    }

    #[test]
    fn test_find_by_sha_short() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        let prefix = ContentSha::from_content("a\n").short();
        let found = chain.find_by_sha(&prefix);
        assert!(found.is_some());
        let (step, doc) = found.unwrap();
        assert_eq!(step, 0);
        assert_eq!(doc, "a\n");
    }

    #[test]
    fn test_find_by_sha_not_found() {
        let chain = MutationChain::new("a\n", FormatKind::Text);
        assert!(chain.find_by_sha("0000000").is_none());
    }

    #[test]
    fn test_log() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "a\nb\n")).unwrap();
        chain.mutate(&gudf_diff_text("a\nb\n", "a\nb\nc\n")).unwrap();

        let log = chain.log();
        assert_eq!(log.len(), 3);

        assert_eq!(log[0].step, 0);
        assert!(log[0].stats.is_none());

        assert_eq!(log[1].step, 1);
        let stats1 = log[1].stats.as_ref().unwrap();
        assert_eq!(stats1.additions, 1);

        assert_eq!(log[2].step, 2);
        let stats2 = log[2].stats.as_ref().unwrap();
        assert_eq!(stats2.additions, 1);
    }

    #[test]
    fn test_sha_survives_undo_redo() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        let sha_b = chain.current_sha().clone();

        chain.undo();
        assert_eq!(chain.current_sha(), &ContentSha::from_content("a\n"));

        chain.redo();
        assert_eq!(chain.current_sha(), &sha_b);
    }

    #[test]
    fn test_mutation_state_has_sha() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        let state = &chain.states[0];
        assert_eq!(state.sha, ContentSha::from_content("b\n"));
    }

    // ── Text chain ─────────────────────────────────────────────────────

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
        chain.mutate(&gudf_diff_text("v0\n", "v1\n")).unwrap();
        chain.mutate(&gudf_diff_text("v1\n", "v2\n")).unwrap();

        let h = chain.history();
        assert_eq!(h, vec!["v0\n", "v1\n", "v2\n"]);
    }

    #[test]
    fn test_at() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        assert_eq!(chain.at(0), Some("a\n"));
        assert_eq!(chain.at(1), Some("b\n"));
        assert_eq!(chain.at(2), None);
    }

    #[test]
    fn test_undo() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        assert!(chain.can_undo());
        assert!(!chain.can_redo());

        assert!(chain.undo());
        assert_eq!(chain.current(), "a\n");
        assert!(!chain.can_undo());
        assert!(chain.can_redo());
        assert!(!chain.undo());
    }

    #[test]
    fn test_redo() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        chain.undo();
        assert!(chain.can_redo());
        assert!(chain.redo());
        assert_eq!(chain.current(), "b\n");
        assert!(!chain.can_redo());
        assert!(!chain.redo());
    }

    #[test]
    fn test_undo_redo_multiple() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        for (f, t) in [("a\n", "b\n"), ("b\n", "c\n"), ("c\n", "d\n")] {
            chain.mutate(&gudf_diff_text(f, t)).unwrap();
        }
        assert_eq!(chain.undo_n(2), 2);
        assert_eq!(chain.current(), "b\n");
        assert!(chain.redo());
        assert_eq!(chain.current(), "c\n");
        assert_eq!(chain.redo_all(), 1);
        assert_eq!(chain.current(), "d\n");
    }

    #[test]
    fn test_redo_cleared_on_new_mutation() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();

        chain.undo();
        assert_eq!(chain.redo_len(), 1);
        chain.mutate(&gudf_diff_text("b\n", "x\n")).unwrap();
        assert_eq!(chain.redo_len(), 0);
    }

    #[test]
    fn test_rewind() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        for (f, t) in [("a\n", "b\n"), ("b\n", "c\n"), ("c\n", "d\n")] {
            chain.mutate(&gudf_diff_text(f, t)).unwrap();
        }
        chain.rewind(1);
        assert_eq!(chain.current(), "b\n");
        assert_eq!(chain.redo_len(), 2);
        chain.redo_all();
        assert_eq!(chain.current(), "d\n");
    }

    #[test]
    fn test_compose() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();
        let composed = chain.compose().unwrap();
        assert!(composed.stats.additions > 0 || composed.stats.deletions > 0);
    }

    #[test]
    fn test_compose_range() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();
        chain.mutate(&gudf_diff_text("c\n", "d\n")).unwrap();
        let range = chain.compose_range(1, 3).unwrap();
        assert!(range.stats.additions > 0 || range.stats.deletions > 0);
    }

    #[test]
    fn test_squash() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();
        chain.squash().unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain.current(), "c\n");
    }

    #[test]
    fn test_total_stats() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "a\nb\n")).unwrap();
        chain.mutate(&gudf_diff_text("a\nb\n", "a\nb\nc\n")).unwrap();
        assert_eq!(chain.total_stats().additions, 2);
    }

    #[test]
    fn test_empty_compose() {
        let chain = MutationChain::new("hello\n", FormatKind::Text);
        let composed = chain.compose().unwrap();
        assert!(composed.changes.is_empty());
    }

    #[test]
    fn test_text_multiline_chain() {
        let v0 = "line1\nline2\nline3\n";
        let v1 = "line1\nmodified\nline3\n";
        let v2 = "line1\nmodified\nline3\nline4\n";
        let v3 = "header\nline1\nmodified\nline3\nline4\n";

        let mut chain = MutationChain::new(v0, FormatKind::Text);
        chain.mutate(&gudf_diff_text(v0, v1)).unwrap();
        chain.mutate(&gudf_diff_text(v1, v2)).unwrap();
        chain.mutate(&gudf_diff_text(v2, v3)).unwrap();
        assert_eq!(chain.current(), v3);

        let composed = chain.compose().unwrap();
        assert_eq!(reconstruct_text(&composed.changes), v3);
    }

    // ── JSON ───────────────────────────────────────────────────────────

    #[test]
    fn test_json_chain() {
        let mut chain = MutationChain::new(r#"{"a":1,"b":2}"#, FormatKind::Json);
        chain
            .mutate(&gudf_diff_json(r#"{"a":1,"b":2}"#, r#"{"a":10,"b":2}"#))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(v["a"], 10);

        chain
            .mutate(&gudf_diff_json(chain.current(), r#"{"a":10,"b":20,"c":30}"#))
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(v["b"], 20);
        assert_eq!(v["c"], 30);
    }

    #[test]
    fn test_json_compose_roundtrip() {
        let orig = r#"{"x":1}"#;
        let mut chain = MutationChain::new(orig, FormatKind::Json);
        chain.mutate(&gudf_diff_json(orig, r#"{"x":2}"#)).unwrap();
        chain
            .mutate(&gudf_diff_json(chain.current(), r#"{"x":2,"y":3}"#))
            .unwrap();

        let composed = chain.compose().unwrap();
        let reconstructed = patch_as(FormatKind::Json, orig, &composed.changes).unwrap();
        let expected: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        let got: serde_json::Value = serde_json::from_str(&reconstructed).unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn test_json_undo_redo_cycle() {
        let mut chain = MutationChain::new(r#"{"count":0}"#, FormatKind::Json);
        for i in 1..=3 {
            let prev = chain.current().to_string();
            chain
                .mutate(&gudf_diff_json(&prev, &format!(r#"{{"count":{i}}}"#)))
                .unwrap();
        }
        assert_eq!(chain.undo_n(2), 2);
        let v: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(v["count"], 1);

        chain.redo();
        let v: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(v["count"], 2);

        chain.redo();
        let v: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(v["count"], 3);
        assert!(!chain.can_redo());
    }

    #[test]
    fn test_apply_raw_changes() {
        let mut chain = MutationChain::new(r#"{"a":1}"#, FormatKind::Json);
        chain
            .apply(&[Change {
                kind: ChangeKind::Modified,
                path: Some("a".to_string()),
                old_value: Some("1".to_string()),
                new_value: Some("99".to_string()),
                location: None,
                annotations: Vec::new(),
            }])
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(v["a"], 99);
    }

    #[test]
    fn test_diffs() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();
        assert_eq!(chain.diffs().len(), 2);
    }

    // ── Expressions ─────────────────────────────────────────────────────

    #[test]
    fn test_resolve_head() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();

        let (step, doc) = chain.resolve("HEAD").unwrap();
        assert_eq!(step, 2);
        assert_eq!(doc, "c\n");
    }

    #[test]
    fn test_resolve_head_tilde() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();
        chain.mutate(&gudf_diff_text("c\n", "d\n")).unwrap();

        let (step, doc) = chain.resolve("HEAD~1").unwrap();
        assert_eq!(step, 2);
        assert_eq!(doc, "c\n");

        let (step, doc) = chain.resolve("HEAD~3").unwrap();
        assert_eq!(step, 0);
        assert_eq!(doc, "a\n");

        // Out of range
        assert!(chain.resolve("HEAD~99").is_none());
    }

    #[test]
    fn test_resolve_head_caret() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();

        let (step, doc) = chain.resolve("HEAD^").unwrap();
        assert_eq!(step, 1);
        assert_eq!(doc, "b\n");

        let (step, doc) = chain.resolve("HEAD^^").unwrap();
        assert_eq!(step, 0);
        assert_eq!(doc, "a\n");
    }

    #[test]
    fn test_resolve_orig() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        let (step, doc) = chain.resolve("ORIG").unwrap();
        assert_eq!(step, 0);
        assert_eq!(doc, "a\n");
    }

    #[test]
    fn test_resolve_at_step() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();

        let (step, doc) = chain.resolve("@0").unwrap();
        assert_eq!(step, 0);
        assert_eq!(doc, "a\n");

        let (step, doc) = chain.resolve("@2").unwrap();
        assert_eq!(step, 2);
        assert_eq!(doc, "c\n");

        assert!(chain.resolve("@99").is_none());
    }

    #[test]
    fn test_resolve_sha() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        let sha = chain.sha_at(1).unwrap().short();
        let (step, doc) = chain.resolve(&sha).unwrap();
        assert_eq!(step, 1);
        assert_eq!(doc, "b\n");
    }

    #[test]
    fn test_resolve_not_found() {
        let chain = MutationChain::new("a\n", FormatKind::Text);
        assert!(chain.resolve("NOPE").is_none());
        assert!(chain.resolve("@999").is_none());
        assert!(chain.resolve("HEAD~1").is_none()); // no mutations
    }

    #[test]
    fn test_resolve_or_err() {
        let chain = MutationChain::new("a\n", FormatKind::Text);
        assert!(chain.resolve_or_err("HEAD").is_ok());
        assert!(chain.resolve_or_err("NOPE").is_err());
    }

    #[test]
    fn test_diff_expr() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();
        chain.mutate(&gudf_diff_text("c\n", "d\n")).unwrap();

        // ORIG → HEAD
        let diff = chain.diff_expr("ORIG", "HEAD").unwrap();
        assert!(diff.stats.additions > 0 || diff.stats.deletions > 0);

        // HEAD~2 → HEAD
        let diff = chain.diff_expr("HEAD~2", "HEAD").unwrap();
        assert!(diff.stats.additions > 0 || diff.stats.deletions > 0);

        // @1 → @2
        let diff = chain.diff_expr("@1", "@2").unwrap();
        assert!(diff.stats.additions > 0 || diff.stats.deletions > 0);
    }

    // ── ExprDiffBuilder ────────────────────────────────────────────────

    #[test]
    fn test_unified_builder_render() {
        let mut chain = MutationChain::new("a\nb\nc\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\nb\nc\n", "a\nx\nc\n")).unwrap();

        let output = chain.unified("ORIG", "HEAD").render().unwrap();
        assert!(output.contains("@@"));
        assert!(output.contains("-b"));
        assert!(output.contains("+x"));
        // Headers contain expression + short-sha
        assert!(output.contains("ORIG"));
        assert!(output.contains("HEAD"));
    }

    #[test]
    fn test_unified_builder_context() {
        let mut chain = MutationChain::new(
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
            FormatKind::Text,
        );
        chain
            .mutate(&gudf_diff_text(
                "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
                "1\n2\n3\n4\nX\n6\n7\n8\n9\n10\n",
            ))
            .unwrap();

        let narrow = chain.unified("ORIG", "HEAD").context(1).render().unwrap();
        let wide = chain.unified("ORIG", "HEAD").context(5).render().unwrap();
        // More context = more lines
        assert!(wide.len() > narrow.len());
    }

    #[test]
    fn test_unified_builder_with_tilde() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();
        chain.mutate(&gudf_diff_text("c\n", "d\n")).unwrap();

        // Diff from step 1 to step 3
        let output = chain.unified("HEAD~2", "HEAD").render().unwrap();
        assert!(output.contains("@@"));
        assert!(output.contains("-b"));
        assert!(output.contains("+d"));
    }

    #[test]
    fn test_unified_builder_sha_in_header() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        let output = chain.unified("ORIG", "HEAD").render().unwrap();
        let orig_short = chain.original_sha().short();
        let head_short = chain.current_sha().short();
        assert!(output.contains(&orig_short));
        assert!(output.contains(&head_short));
    }

    #[test]
    fn test_unified_builder_diff() {
        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        let diff = chain.unified("ORIG", "HEAD").diff().unwrap();
        assert!(diff.stats.additions > 0 || diff.stats.deletions > 0);
    }

    #[test]
    fn test_unified_builder_render_with() {
        use crate::output::inline::InlineFormatter;

        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        let output = chain
            .unified("ORIG", "HEAD")
            .render_with(&InlineFormatter)
            .unwrap();
        assert!(output.contains("[-]"));
        assert!(output.contains("[+]"));
    }

    #[test]
    fn test_format_expr() {
        use crate::output::inline::InlineFormatter;

        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();

        let output = chain.format_expr("ORIG", "HEAD", &InlineFormatter).unwrap();
        assert!(output.contains("[-]"));
        assert!(output.contains("[+]"));
    }

    #[test]
    fn test_unified_builder_json() {
        let mut chain = MutationChain::new(r#"{"a":1}"#, FormatKind::Json);
        chain
            .mutate(&gudf_diff_json(r#"{"a":1}"#, r#"{"a":2,"b":3}"#))
            .unwrap();

        let output = chain.unified("ORIG", "HEAD").render().unwrap();
        assert!(output.contains("@@"));
    }

    // ── File I/O ──────────────────────────────────────────────────────

    #[test]
    fn test_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        fs::write(&path, r#"{"a": 1}"#).unwrap();

        let chain = MutationChain::from_file(&path).unwrap();
        assert_eq!(chain.current(), r#"{"a": 1}"#);
        assert_eq!(chain.format, FormatKind::Json);
    }

    #[test]
    fn test_from_file_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "hello\n").unwrap();

        let chain = MutationChain::from_file(&path).unwrap();
        assert_eq!(chain.format, FormatKind::Text);
    }

    #[test]
    fn test_from_file_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        fs::write(&path, "name: test\n").unwrap();

        let chain = MutationChain::from_file(&path).unwrap();
        assert_eq!(chain.format, FormatKind::Yaml);
    }

    #[test]
    fn test_from_file_as() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data");
        fs::write(&path, r#"{"a": 1}"#).unwrap();

        let chain = MutationChain::from_file_as(&path, FormatKind::Json).unwrap();
        assert_eq!(chain.format, FormatKind::Json);
    }

    #[test]
    fn test_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.json");

        let mut chain = MutationChain::new(r#"{"a":1}"#, FormatKind::Json);
        chain
            .mutate(&gudf_diff_json(r#"{"a":1}"#, r#"{"a":2}"#))
            .unwrap();

        chain.save(&path).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["a"], 2);
    }

    #[test]
    fn test_save_expr() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");

        let mut chain = MutationChain::new("a\n", FormatKind::Text);
        chain.mutate(&gudf_diff_text("a\n", "b\n")).unwrap();
        chain.mutate(&gudf_diff_text("b\n", "c\n")).unwrap();

        // Save step 1 (not current)
        chain.save_expr("@1", &path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "b\n");

        // Save HEAD^
        chain.save_expr("HEAD^", &path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "b\n");
    }

    #[test]
    fn test_mutate_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("next.json");

        let mut chain = MutationChain::new(r#"{"a":1}"#, FormatKind::Json);
        fs::write(&path, r#"{"a":2,"b":3}"#).unwrap();

        chain.mutate_file(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(chain.current()).unwrap();
        assert_eq!(v["a"], 2);
        assert_eq!(v["b"], 3);
        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn test_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.json");
        let v1 = dir.path().join("v1.json");
        let v2 = dir.path().join("v2.json");

        fs::write(&src, r#"{"x":1}"#).unwrap();
        fs::write(&v1, r#"{"x":2}"#).unwrap();
        fs::write(&v2, r#"{"x":2,"y":3}"#).unwrap();

        let mut chain = MutationChain::from_file(&src).unwrap();
        chain.mutate_file(&v1).unwrap();
        chain.mutate_file(&v2).unwrap();

        let out = dir.path().join("out.json");
        chain.save(&out).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(result["x"], 2);
        assert_eq!(result["y"], 3);

        // Save original via expression
        let orig_out = dir.path().join("orig.json");
        chain.save_expr("ORIG", &orig_out).unwrap();
        assert_eq!(fs::read_to_string(&orig_out).unwrap(), r#"{"x":1}"#);
    }

    // ── helpers ─────────────────────────────────────────────────────────

    fn gudf_diff_text(old: &str, new: &str) -> DiffResult {
        crate::formats::text::TextFormat.diff(old, new).unwrap()
    }

    fn gudf_diff_json(old: &str, new: &str) -> DiffResult {
        crate::formats::json::JsonFormat.diff(old, new).unwrap()
    }
}
