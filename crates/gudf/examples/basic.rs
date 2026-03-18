//! Basic JSON diff with the builder API.

use gudf::{Gudf, OutputKind};

fn main() {
    let old = r#"{"items": [{"name": "a"}, {"name": "b"}]}"#;
    let new = r#"{"name": "test", "items": [{"name": "a"}, {"name": "d"}, {"name": "c"}]}"#;

    let output = Gudf::diff(old, new)
        .output(OutputKind::Json)
        .run()
        .unwrap();

    println!("{output}");
}
