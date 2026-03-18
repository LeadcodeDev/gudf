//! Filter and query diff results with the pipeline API.

use gudf::{Gudf, ChangeKind};

fn main() {
    let old = r#"{
        "config": {
            "database": {"host": "localhost", "port": 5432, "password": "secret"},
            "server": {"host": "0.0.0.0", "port": 3000}
        }
    }"#;
    let new = r#"{
        "config": {
            "database": {"host": "db.prod.com", "port": 5432, "password": "new-secret"},
            "server": {"host": "0.0.0.0", "port": 8080},
            "cache": {"ttl": 300}
        }
    }"#;

    let result = Gudf::diff(old, new).execute().unwrap();

    // Filter: only modifications
    let modifications = result
        .pipeline()
        .filter_kind(ChangeKind::Modified)
        .collect();

    println!("=== Modifications ===");
    for change in &modifications {
        println!(
            "  {} : {} → {}",
            change.path.as_deref().unwrap_or("?"),
            change.old_value.as_deref().unwrap_or(""),
            change.new_value.as_deref().unwrap_or(""),
        );
    }

    // Filter: only changes under config.database
    let db_changes = result
        .pipeline()
        .exclude_unchanged()
        .filter_path("config.database.**")
        .collect();

    println!("\n=== Database changes ===");
    for change in &db_changes {
        println!(
            "  [{:?}] {}",
            change.kind,
            change.path.as_deref().unwrap_or("?"),
        );
    }

    // Filter: only additions
    let additions = result
        .pipeline()
        .filter_kind(ChangeKind::Added)
        .collect();

    println!("\n=== Additions ===");
    for change in &additions {
        println!(
            "  {} = {}",
            change.path.as_deref().unwrap_or("?"),
            change.new_value.as_deref().unwrap_or(""),
        );
    }

    // Stats
    println!("\n=== Stats ===");
    println!("  +{} -{} ~{}",
        result.stats.additions,
        result.stats.deletions,
        result.stats.modifications,
    );
}
