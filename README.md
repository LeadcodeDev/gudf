# GUDF

**Git Unified Diff Format** — universal diff library and CLI for text, JSON, TOML, YAML, and code (30+ languages).

gudf goes beyond simple text comparison: it understands structure. JSON keys, TOML tables, YAML mappings, and AST nodes are diffed semantically, not line-by-line. The output is git-native unified diff, consumable by `git apply`, `patch`, GitHub, and GitLab.

## Installation

```bash
cargo install --path crates/gudf-cli
```

Or add the library to your project:

```toml
[dependencies]
gudf = { path = "crates/gudf" }
```

## Quick Start

### CLI

```bash
# Text diff
gudf old.txt new.txt

# JSON structural diff
gudf config.old.json config.new.json

# Code diff with language-aware AST parsing
gudf old.rs new.rs --lang rust

# Directory diff (parallel, format-aware per file)
gudf src/ src-new/

# Cross-format diff (TOML vs YAML)
gudf config.toml config.yaml --cross-format

# Three-way merge
gudf merge base.json left.json right.json

# Semantic move/rename detection
gudf old.json new.json --semantic
```

### Library

```rust
use gudf::{diff, diff_json, DiffEngine, FormatKind};

// Auto-detect format and diff
let result = gudf::diff(old_text, new_text)?;

// Explicit format
let result = gudf::diff_json(old_json, new_json)?;

// Code diff (30+ languages)
let result = gudf::diff_code(old_code, new_code, "rust")?;

// Unified output
use gudf::output::unified::UnifiedFormatter;
use gudf::OutputFormatter;

let formatter = UnifiedFormatter::new("a/file.rs", "b/file.rs").context(3);
println!("{}", formatter.format(&result));
```

## Features

### Git-Native Unified Output

Produces byte-for-byte compatible unified diffs with proper `@@ -a,b +c,d @@` hunk headers, configurable context lines, and `\ No newline at end of file` markers.

```bash
gudf old.txt new.txt -U 5   # 5 lines of context
```

```rust
use gudf::output::unified::UnifiedFormatter;
use gudf::OutputFormatter;

let formatter = UnifiedFormatter::new("a/file.txt", "b/file.txt")
    .context(3);  // default: 3 lines of context
let output = formatter.format(&result);
// --- a/file.txt
// +++ b/file.txt
// @@ -1,4 +1,5 @@
//  unchanged line
// -old line
// +new line
//  unchanged line
```

Text and code diffs produce multiple hunks when changes are far apart. Structured diffs (JSON/TOML/YAML) show path-based changes within hunks.

### Cross-Format Diff

Compare documents of different formats by normalizing both to a common representation. Answer "did the content change, or just the format?"

```bash
# Explicit
gudf config.toml config.yaml --cross-format

# Auto-detected when extensions differ
gudf config.toml config.yaml
```

```rust
use gudf::{diff_cross, CrossFormatKind};

let result = diff_cross(
    toml_content, CrossFormatKind::Toml,
    yaml_content, CrossFormatKind::Yaml,
)?;
// Only reports actual value differences, not format differences
```

Supported cross-format pairs: JSON, TOML, YAML (any combination).

### Diff Pipelines

Builder-pattern API for filtering, transforming, and querying changes like an iterator chain.

```rust
use gudf::ChangeKind;

let result = gudf::diff_json(old, new)?;

// Filter to only modified database config entries
let db_changes = result.pipeline()
    .filter_kind(ChangeKind::Modified)
    .filter_path("config.database.**")
    .collect();

// Count additions
let addition_count = result.pipeline()
    .filter_kind(ChangeKind::Added)
    .count();

// Exclude unchanged, get first match
let first_change = result.pipeline()
    .exclude_unchanged()
    .first();

// Filter by annotation
let sensitive = result.pipeline()
    .filter_annotation("sensitive")
    .collect();
```

Path matching supports globs:

- `config.database.*` — matches one level (`config.database.host`, `config.database.port`)
- `config.**` — matches any depth (`config.database.host`, `config.server.tls.cert`)
- `config.*.host` — wildcard at a specific level

### Diff Annotations

Attach semantic metadata to changes — severity, category, tags — transforming diffs from "what changed" to "what changed and why it matters".

```rust
use gudf::{DiffEngine, SensitiveFieldAnnotator, PathDepthAnnotator, AstNodeAnnotator};

let engine = DiffEngine::new()
    .with_annotators(vec![
        Box::new(SensitiveFieldAnnotator),  // flags password, secret, token, api_key
        Box::new(PathDepthAnnotator),       // nesting depth, leaf vs branch
        Box::new(AstNodeAnnotator),         // tree-sitter node type for code diffs
    ]);

let result = engine.diff(old, new)?;

for change in &result.changes {
    for annotation in &change.annotations {
        println!("{}: {:?}", annotation.key, annotation.value);
    }
}
```

Built-in annotators:

| Annotator                 | Key                            | Description                                                             |
| ------------------------- | ------------------------------ | ----------------------------------------------------------------------- |
| `SensitiveFieldAnnotator` | `sensitive`, `sensitive_field` | Flags changes to `password`, `secret`, `token`, `api_key`, etc.         |
| `PathDepthAnnotator`      | `depth`, `node_type`           | Path nesting depth and `leaf`/`branch` classification                   |
| `AstNodeAnnotator`        | `ast_node`                     | Tree-sitter node type (`function_definition`, `import_statement`, etc.) |

Implement the `Annotator` trait for custom annotators:

```rust
use gudf::{Annotator, Annotation, AnnotationValue, Change};

struct MyAnnotator;
impl Annotator for MyAnnotator {
    fn annotate(&self, change: &Change) -> Vec<Annotation> {
        // your logic here
        Vec::new()
    }
}
```

### Semantic Move/Rename Detection

Automatically detect when a key was renamed or a value moved to a different path, instead of reporting separate Remove + Add.

```bash
gudf old.json new.json --semantic
```

```rust
use gudf::{DiffEngine, SemanticAnalyzer, SemanticOptions, ChangeKind};

let engine = DiffEngine::new();
let result = engine.diff(old, new)?;

let analyzer = SemanticAnalyzer::new(SemanticOptions {
    move_detection: true,
    rename_detection: true,
    rename_threshold: 1.0,  // exact match (V1)
});

let result = analyzer.analyze(result);

for change in &result.changes {
    match change.kind {
        ChangeKind::Renamed => {
            // e.g. "userName" -> "user_name" with same value
            println!("Renamed: {} -> {}",
                change.path.as_deref().unwrap_or(""),
                change.new_value.as_deref().unwrap_or(""));
        }
        ChangeKind::Moved => {
            // e.g. "old_section.key" -> "new_section.key" with same value
            println!("Moved: {} -> {}",
                change.path.as_deref().unwrap_or(""),
                change.new_value.as_deref().unwrap_or(""));
        }
        _ => {}
    }
}
```

Detection logic (V1 — exact match):

- **Renamed**: same parent path, different key name, identical value
- **Moved**: different parent path, identical value

### Directory Tree Diff

Recursive diff of two directory trees with format-aware per-file diffing and parallel execution.

```bash
# Auto-detected when both arguments are directories
gudf old-project/ new-project/
```

```rust
use gudf_dir::{diff_dirs, format_dir_diff};
use std::path::Path;

let result = diff_dirs(Path::new("old/"), Path::new("new/"))?;

println!("Added: {}", result.summary.files_added);
println!("Removed: {}", result.summary.files_removed);
println!("Modified: {}", result.summary.files_modified);

// Unified multi-file output (like git diff)
print!("{}", format_dir_diff(&result));
```

Capabilities:

- `.gitignore`-aware file walking
- Binary file detection (skipped with `Binary files differ` marker)
- Extension-to-format mapping (30+ languages, JSON, TOML, YAML, text)
- Parallel file processing via `rayon`
- Per-file `diff --git a/path b/path` headers

### Three-Way Merge

Structural three-way merge for JSON, TOML, and YAML. Non-conflicting changes are auto-merged; conflicts are reported with paths.

```bash
# Merge with conflict detection
gudf merge base.json left.json right.json

# Auto-resolve with a strategy
gudf merge base.json left.json right.json --strategy ours
gudf merge base.json left.json right.json --strategy theirs
```

```rust
use gudf::{merge, merge_json, MergeStrategy, FormatKind};

// From strings
let result = merge(base, left, right, FormatKind::Json, MergeStrategy::Manual)?;

if result.is_clean() {
    println!("{}", serde_json::to_string_pretty(&result.merged)?);
} else {
    for conflict in &result.conflicts {
        println!("CONFLICT at '{}': left={:?}, right={:?}",
            conflict.path, conflict.left, conflict.right);
    }
}

// From serde_json::Value
let result = merge_json(&base_val, &left_val, &right_val, MergeStrategy::Ours);
```

Merge strategies:

| Strategy | Behavior                            |
| -------- | ----------------------------------- |
| `Manual` | Reports conflicts without resolving |
| `Ours`   | Left side wins on conflict          |
| `Theirs` | Right side wins on conflict         |

### Mutation Chains

Reconstruct a document through a sequence of diffs, with full history tracking, undo/redo, and SHA-based state identification.

Every state (including the original) is identified by a `ContentSha` — a SHA-1 hash of its content, like git blob objects. States can be looked up by full sha, short sha (7 chars), or any unambiguous prefix.

#### SHA-based identification

```rust
use gudf::{MutationChain, ContentSha, FormatKind};

let mut chain = MutationChain::new(r#"{"version": 1}"#, FormatKind::Json);

println!("original: {}", chain.original_sha());       // e.g. "a3f1c2d"
println!("full:     {}", chain.original_sha().full()); // 40-char hex

chain.mutate(&diff)?;
println!("current:  {}", chain.current_sha());

// Look up state by SHA prefix
let (step, doc) = chain.find_by_sha("a3f1").unwrap();
let sha = chain.sha_at(1).unwrap();
let all_shas = chain.shas();

// Git-log-style summary
for entry in chain.log() {
    match entry.stats {
        None => println!("{} (initial)", entry.sha),
        Some(s) => println!("{} +{} -{} ~{}", entry.sha, s.additions, s.deletions, s.modifications),
    }
}
```

#### Git-like expressions

Navigate the chain with expressions instead of step indices:

```rust
// Resolve any expression to (step, document)
let (step, doc) = chain.resolve("HEAD").unwrap();
let (step, doc) = chain.resolve("HEAD~2").unwrap();   // 2 steps back
let (step, doc) = chain.resolve("HEAD^^").unwrap();    // grandparent
let (step, doc) = chain.resolve("ORIG").unwrap();      // step 0
let (step, doc) = chain.resolve("@3").unwrap();        // step 3 directly
let (step, doc) = chain.resolve("a3f1c2d").unwrap();   // by SHA prefix

// Diff between two expressions
let diff = chain.diff_expr("ORIG", "HEAD")?;
let diff = chain.diff_expr("HEAD~3", "HEAD")?;
let diff = chain.diff_expr("@1", "@3")?;

// Save a specific expression to disk
chain.save_expr("HEAD~1", "previous.json")?;
```

| Expression    | Resolves to                          |
| ------------- | ------------------------------------ |
| `HEAD`        | Current state                        |
| `HEAD~N`      | N steps back from HEAD               |
| `HEAD^`       | Parent of HEAD (= `HEAD~1`)          |
| `HEAD^^`      | Grandparent (= `HEAD~2`)            |
| `ORIG`        | Original document (step 0)           |
| `@N`          | Step N directly                      |
| `<sha>`       | Lookup by full or short SHA prefix   |

#### File I/O

Load from files, apply file mutations, and save states to disk:

```rust
use gudf::MutationChain;

// Create chain from file (format auto-detected from extension)
let mut chain = MutationChain::from_file("config.json")?;
// or with explicit format:
let mut chain = MutationChain::from_file_as("data", FormatKind::Json)?;

// Apply a file as the next mutation (diffs against current, then applies)
chain.mutate_file("config_v2.json")?;
chain.mutate_file("config_v3.json")?;

// Save current state
chain.save("output.json")?;

// Save any expression to a file
chain.save_expr("ORIG", "original.json")?;
chain.save_expr("HEAD~1", "previous.json")?;
```

#### Undo / redo / compose

```rust
chain.undo();                   // push to redo stack
chain.redo();                   // pop from redo stack
chain.undo_n(2);                // undo 2 at once
chain.redo_all();               // redo everything

chain.rewind(1);                // undo to step 1 (redoable)

chain.mutate(&diff)?;           // forks: clears redo stack

let composed = chain.compose()?;         // original → current
let range = chain.compose_range(1, 3)?;  // step 1 → step 3
chain.squash()?;                         // collapse to single mutation
```

Key capabilities:

| Method                           | Description                                         |
| -------------------------------- | --------------------------------------------------- |
| `mutate(diff)` / `apply(changes)`| Apply diff or raw changes as next mutation           |
| `mutate_file(path)`              | Read file, diff against current, apply               |
| `from_file(path)`                | Create chain from file (auto-detect format)          |
| `save(path)` / `save_expr(e, p)` | Write current or expression state to file           |
| `resolve(expr)`                  | Resolve expression to `(step, doc)`                  |
| `diff_expr(from, to)`            | Diff between two expressions                         |
| `current_sha()` / `sha_at(step)` | SHA of current or any step                          |
| `find_by_sha(prefix)`            | Lookup by SHA prefix                                 |
| `log()`                          | Git-log-style `Vec<LogEntry>`                        |
| `undo()` / `redo()`              | Single undo/redo                                     |
| `undo_n(n)` / `redo_n(n)` / `redo_all()` | Batch undo/redo                             |
| `compose()` / `compose_range()`  | Compose diffs across steps                           |
| `squash()`                       | Collapse all mutations into one                      |

## Supported Formats

### Structured

| Format | Detection            | Features                                       |
| ------ | -------------------- | ---------------------------------------------- |
| JSON   | Auto (valid JSON)    | Recursive key/value diff, array index tracking |
| TOML   | Auto (contains `=`)  | Normalized to JSON internally                  |
| YAML   | Auto (contains `: `) | Normalized to JSON internally                  |

### Code (30+ languages via tree-sitter)

| Language      | Aliases           |
| ------------- | ----------------- |
| Bash          | `sh`              |
| C             |                   |
| C#            | `csharp`, `cs`    |
| C++           | `c++`             |
| CSS           |                   |
| Dart          |                   |
| Elixir        | `ex`              |
| Erlang        | `erl`             |
| Go            |                   |
| Haskell       | `hs`              |
| HCL/Terraform | `terraform`, `tf` |
| HTML          |                   |
| Java          |                   |
| JavaScript    | `js`              |
| JSON          |                   |
| Lua           |                   |
| OCaml         | `ml`, `mli`       |
| PHP           |                   |
| Python        | `py`              |
| R             |                   |
| Regex         |                   |
| Ruby          | `rb`              |
| Rust          | `rs`              |
| Scala         |                   |
| Swift         |                   |
| TypeScript    | `ts`, `tsx`       |
| YAML          | `yml`             |
| Zig           |                   |

## Output Formats

```bash
gudf old new --output unified     # default: git-compatible unified diff
gudf old new --output inline      # inline with [+] [-] [~] [M] [R] markers
gudf old new --output json-patch  # RFC 6902 JSON Patch operations
```

## CLI Reference

```
gudf [OPTIONS] <FILE1> <FILE2>
gudf merge <BASE> <LEFT> <RIGHT> [OPTIONS]

Arguments:
  <FILE1>    First file or directory (use '-' for stdin)
  <FILE2>    Second file or directory

Options:
  -f, --format <FORMAT>    Force format (text, json, toml, yaml)
  -o, --output <OUTPUT>    Output format: unified, inline, json-patch [default: unified]
  -l, --lang <LANG>        Language for code diff (rust, python, js, etc.)
  -U, --context <N>        Context lines for unified output [default: 3]
      --cross-format       Enable cross-format diff
      --semantic           Enable move/rename detection
      --annotate           Enable sensitive field annotations

Merge options:
  -s, --strategy <STRAT>   Merge strategy: manual, ours, theirs [default: manual]
  -f, --format <FORMAT>    Force format (json, toml, yaml)
```

## Architecture

```
crates/
  gudf/              Core library
    src/
      lib.rs           Public API and convenience functions
      engine.rs        DiffEngine — format dispatch, annotators, semantic
      format.rs        Format trait, FormatKind, auto-detection
      result.rs        Change, ChangeKind, DiffResult, DiffStats
      error.rs         GudfError
      patch.rs         Apply changes to reconstruct documents
      pipeline.rs      DiffPipeline — composable filtering and querying
      annotations.rs   Annotation types, Annotator trait, built-in annotators
      semantic.rs      SemanticAnalyzer — move/rename detection
      merge.rs         Three-way structural merge
      mutation.rs      MutationChain — sequential diff replay and composition
      formats/
        text.rs        Line-by-line diff (similar crate)
        json.rs        Recursive JSON diff
        toml.rs        TOML → JSON normalization
        yaml.rs        YAML → JSON normalization
        code.rs        Tree-sitter AST diff (30+ languages)
        cross.rs       Cross-format normalization and diff
      output/
        unified.rs     Git-native unified diff with @@ hunks
        inline.rs      Inline format with markers
        json_patch.rs  RFC 6902 JSON Patch
  gudf-cli/          CLI binary
  gudf-dir/          Directory tree diff (rayon, walkdir, ignore)
```

## License

MIT
