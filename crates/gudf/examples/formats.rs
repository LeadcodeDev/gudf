//! Diff across multiple formats: text, JSON, TOML, YAML.

use gudf::{Gudf, FormatKind, OutputKind};

fn main() {
    // Text diff: replace "world" → "rust", and add "bar" at the end
    let old_text = "hello\nworld\nfoo\n";
    let new_text = "hello\nrust\nfoo\nbar\n";

    let output = Gudf::diff(old_text, new_text)
        .output(OutputKind::Json)
        .run()
        .unwrap();

    println!("=== Text ===");
    println!("{output}");

    // TOML diff
    let old_toml = "[server]\nhost = \"localhost\"\nport = 3000\n";
    let new_toml = "[server]\nhost = \"0.0.0.0\"\nport = 8080\n";

    let output = Gudf::diff(old_toml, new_toml)
        .format(FormatKind::Toml)
        .output(OutputKind::Json)
        .run()
        .unwrap();

    println!("\n=== TOML ===");
    println!("{output}");

    // YAML diff
    let old_yaml = "name: Alice\nage: 30\n";
    let new_yaml = "name: Bob\nage: 30\nemail: bob@test.com\n";

    let output = Gudf::diff(old_yaml, new_yaml)
        .format(FormatKind::Yaml)
        .output(OutputKind::Json)
        .run()
        .unwrap();

    println!("\n=== YAML ===");
    println!("{output}");
}
