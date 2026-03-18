//! Mutation chain: track document evolution with undo/redo and SHA addressing.

use gudf::{DiffEngine, FormatKind, MutationChain};
use gudf::output::json::JsonFormatter;
use gudf::output::OutputFormatter;

fn main() {
    let engine = DiffEngine::new();

    // Create a chain starting from a base JSON document
    let v0 = r#"{"name": "app", "version": "1.0.0", "port": 3000}"#;
    let v1 = r#"{"name": "app", "version": "1.1.0", "port": 3000}"#;
    let v2 = r#"{"name": "app", "version": "1.1.0", "port": 8080}"#;
    let v3 = r#"{"name": "my-app", "version": "2.0.0", "port": 8080}"#;

    let mut chain = MutationChain::new(v0, FormatKind::Json);

    println!("=== Original ===");
    println!("SHA: {}", chain.original_sha());

    // Apply mutations via diff
    for new_doc in [v1, v2, v3] {
        let diff = engine.diff_as(FormatKind::Json, chain.current(), new_doc).unwrap();
        chain.mutate(&diff).unwrap();
    }

    // Log
    println!("\n=== Mutation log ===");
    for entry in chain.log() {
        match &entry.stats {
            None => println!("  {} @{} (origin)", entry.sha.short(), entry.step),
            Some(stats) => println!(
                "  {} @{} +{} -{} ~{}",
                entry.sha.short(), entry.step,
                stats.additions, stats.deletions, stats.modifications,
            ),
        }
    }

    // Expression resolution
    println!("\n=== Expressions ===");
    if let Some((step, doc)) = chain.resolve("HEAD~1") {
        println!("HEAD~1 = @{}: {}", step, doc.trim());
    }
    if let Some((step, doc)) = chain.resolve("ORIG") {
        println!("ORIG   = @{}: {}", step, doc.trim());
    }

    // Diff between expressions
    println!("\n=== Diff ORIG..HEAD ===");
    let diff = chain.diff_expr("ORIG", "HEAD").unwrap();
    let output = JsonFormatter.format(&diff);
    println!("{output}");

    // Undo/redo
    chain.undo();
    println!("\n=== After undo ===");
    println!("{}", chain.current().trim());

    chain.redo();
    println!("\n=== After redo ===");
    println!("{}", chain.current().trim());

    // Lookup by SHA prefix
    let sha = chain.original_sha();
    let prefix = &sha.to_string()[..7];
    if let Some((step, _)) = chain.find_by_sha(prefix) {
        println!("\n=== SHA lookup ===");
        println!("Found '{}' at @{}", prefix, step);
    }
}
