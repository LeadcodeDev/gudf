//! Three-way merge: resolve concurrent changes to a JSON document.

use gudf::{merge_json, MergeStrategy};

fn main() {
    let base = serde_json::json!({
        "name": "my-app",
        "version": "1.0.0",
        "config": {
            "port": 3000,
            "host": "localhost",
            "debug": false
        }
    });

    // Alice changes the port
    let alice = serde_json::json!({
        "name": "my-app",
        "version": "1.0.0",
        "config": {
            "port": 8080,
            "host": "localhost",
            "debug": false
        }
    });

    // Bob changes the host and enables debug
    let bob = serde_json::json!({
        "name": "my-app",
        "version": "1.0.0",
        "config": {
            "port": 3000,
            "host": "0.0.0.0",
            "debug": true
        }
    });

    // Clean merge: no conflicts (different paths)
    let result = merge_json(&base, &alice, &bob, MergeStrategy::Manual);

    println!("=== Clean merge ===");
    println!("Conflicts: {}", result.conflicts.len());
    println!("{}", serde_json::to_string_pretty(&result.merged).unwrap());

    // Conflicting merge: both change the same field
    let alice_v2 = serde_json::json!({
        "name": "my-app",
        "version": "2.0.0",
        "config": { "port": 3000, "host": "localhost", "debug": false }
    });
    let bob_v2 = serde_json::json!({
        "name": "my-app",
        "version": "3.0.0",
        "config": { "port": 3000, "host": "localhost", "debug": false }
    });

    let result = merge_json(&base, &alice_v2, &bob_v2, MergeStrategy::Manual);

    println!("\n=== Conflict (manual) ===");
    for conflict in &result.conflicts {
        println!(
            "  CONFLICT at '{}': left={}, right={}",
            conflict.path,
            conflict.left.as_deref().unwrap_or("(none)"),
            conflict.right.as_deref().unwrap_or("(none)"),
        );
    }

    // Auto-resolve with "theirs" strategy
    let result = merge_json(&base, &alice_v2, &bob_v2, MergeStrategy::Theirs);

    println!("\n=== Conflict (theirs) ===");
    println!("Conflicts: {}", result.conflicts.len());
    println!("version = {}", result.merged["version"]);
}
