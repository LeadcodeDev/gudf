//! Patch: compute a diff then apply it to reconstruct the target document.

use gudf::{Gudf, FormatKind};

fn main() {
    let old = r#"{"name": "Alice", "age": 30, "city": "Paris"}"#;
    let new = r#"{"name": "Bob", "age": 30, "city": "Lyon", "active": true}"#;

    // Compute the diff
    let result = Gudf::diff(old, new).execute().unwrap();

    println!("=== Changes ===");
    for change in &result.changes {
        if change.kind != gudf::ChangeKind::Unchanged {
            println!(
                "  [{:?}] {} : {:?} → {:?}",
                change.kind,
                change.path.as_deref().unwrap_or("?"),
                change.old_value.as_deref(),
                change.new_value.as_deref(),
            );
        }
    }

    // Apply the changes to old → should produce new
    let patched = gudf::patch_as(FormatKind::Json, old, &result.changes).unwrap();
    let patched_val: serde_json::Value = serde_json::from_str(&patched).unwrap();
    let expected_val: serde_json::Value = serde_json::from_str(new).unwrap();

    println!("\n=== Patched ===");
    println!("{}", serde_json::to_string_pretty(&patched_val).unwrap());

    println!("\n=== Roundtrip OK: {}", patched_val == expected_val);
}
