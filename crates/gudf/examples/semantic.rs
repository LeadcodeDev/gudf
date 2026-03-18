//! Semantic analysis: detect moves and renames between JSON keys.

use gudf::{Gudf, OutputKind};

fn main() {
    // Key renamed: "userName" → "user_name" (same parent, same value = rename)
    let old = r#"{"user": {"userName": "Alice", "age": 30}}"#;
    let new = r#"{"user": {"user_name": "Alice", "age": 30}}"#;

    let output = Gudf::diff(old, new)
        .semantic(true)
        .output(OutputKind::Json)
        .run()
        .unwrap();

    println!("=== Rename detection ===");
    println!("{output}");

    // Key moved: "host" moved from "old_section" to "new_section" (different parent = move)
    let old = r#"{"old_section": {"host": "localhost"}, "new_section": {}}"#;
    let new = r#"{"old_section": {}, "new_section": {"host": "localhost"}}"#;

    let output = Gudf::diff(old, new)
        .semantic(true)
        .output(OutputKind::Json)
        .run()
        .unwrap();

    println!("\n=== Move detection ===");
    println!("{output}");
}
