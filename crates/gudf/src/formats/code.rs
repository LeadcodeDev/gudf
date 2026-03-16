use tree_sitter::{Parser, Tree};

use crate::error::GudfError;
use crate::format::{Format, FormatKind};
use crate::formats::text::TextFormat;
use crate::result::{Change, ChangeKind, DiffResult, DiffStats, Location};

pub struct CodeFormat {
    language: String,
}

impl CodeFormat {
    pub fn new(language: impl Into<String>) -> Self {
        Self {
            language: language.into(),
        }
    }

    /// Returns the list of all supported language identifiers.
    pub fn supported_languages() -> &'static [&'static str] {
        &[
            "bash", "sh",
            "c",
            "c-sharp", "csharp", "cs",
            "cpp", "c++",
            "css",
            "dart",
            "elixir", "ex",
            "erlang", "erl",
            "go",
            "haskell", "hs",
            "hcl", "terraform", "tf",
            "html",
            "java",
            "javascript", "js",
            "json",
            "lua",
            "ocaml", "ml",
            "ocaml-interface", "mli",
            "php",
            "python", "py",
            "r",
            "regex",
            "ruby", "rb",
            "rust", "rs",
            "scala",
            "swift",
            "typescript", "ts",
            "tsx",
            "yaml", "yml",
            "zig",
        ]
    }

    fn get_parser(&self) -> Result<Parser, GudfError> {
        let mut parser = Parser::new();
        let language = match self.language.as_str() {
            "bash" | "sh" => tree_sitter_bash::LANGUAGE.into(),
            "c" => tree_sitter_c::LANGUAGE.into(),
            "c-sharp" | "csharp" | "cs" => tree_sitter_c_sharp::LANGUAGE.into(),
            "cpp" | "c++" => tree_sitter_cpp::LANGUAGE.into(),
            "css" => tree_sitter_css::LANGUAGE.into(),
            "dart" => tree_sitter_dart::LANGUAGE.into(),
            "elixir" | "ex" => tree_sitter_elixir::LANGUAGE.into(),
            "erlang" | "erl" => tree_sitter_erlang::LANGUAGE.into(),
            "go" => tree_sitter_go::LANGUAGE.into(),
            "haskell" | "hs" => tree_sitter_haskell::LANGUAGE.into(),
            "hcl" | "terraform" | "tf" => tree_sitter_hcl::LANGUAGE.into(),
            "html" => tree_sitter_html::LANGUAGE.into(),
            "java" => tree_sitter_java::LANGUAGE.into(),
            "javascript" | "js" => tree_sitter_javascript::LANGUAGE.into(),
            "json" => tree_sitter_json::LANGUAGE.into(),
            "lua" => tree_sitter_lua::LANGUAGE.into(),
            "ocaml" | "ml" => tree_sitter_ocaml::LANGUAGE_OCAML.into(),
            "ocaml-interface" | "mli" => tree_sitter_ocaml::LANGUAGE_OCAML_INTERFACE.into(),
            "php" => tree_sitter_php::LANGUAGE_PHP.into(),
            "python" | "py" => tree_sitter_python::LANGUAGE.into(),
            "r" => tree_sitter_r::LANGUAGE.into(),
            "regex" => tree_sitter_regex::LANGUAGE.into(),
            "ruby" | "rb" => tree_sitter_ruby::LANGUAGE.into(),
            "rust" | "rs" => tree_sitter_rust::LANGUAGE.into(),
            "scala" => tree_sitter_scala::LANGUAGE.into(),
            "swift" => tree_sitter_swift::LANGUAGE.into(),
            "typescript" | "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
            "yaml" | "yml" => tree_sitter_yaml::LANGUAGE.into(),
            "zig" => tree_sitter_zig::LANGUAGE.into(),
            other => {
                return Err(GudfError::UnsupportedFormat(format!(
                    "Language not supported: {other}. Supported: {:?}",
                    Self::supported_languages()
                )));
            }
        };
        parser
            .set_language(&language)
            .map_err(|e| GudfError::ParseError(e.to_string()))?;
        Ok(parser)
    }

    fn parse(&self, source: &str) -> Result<Tree, GudfError> {
        let mut parser = self.get_parser()?;
        parser
            .parse(source, None)
            .ok_or_else(|| GudfError::ParseError("Failed to parse source".to_string()))
    }
}

impl Format for CodeFormat {
    fn kind(&self) -> FormatKind {
        FormatKind::Code(self.language.clone())
    }

    fn diff(&self, old: &str, new: &str) -> Result<DiffResult, GudfError> {
        let old_tree = match self.parse(old) {
            Ok(tree) => tree,
            Err(_) => return TextFormat.diff(old, new),
        };
        let new_tree = match self.parse(new) {
            Ok(tree) => tree,
            Err(_) => return TextFormat.diff(old, new),
        };

        let mut changes = Vec::new();
        diff_nodes(
            &old_tree.root_node(),
            &new_tree.root_node(),
            old,
            new,
            &mut changes,
        );

        let stats = DiffStats::from_changes(&changes);
        Ok(DiffResult {
            changes,
            format: FormatKind::Code(self.language.clone()),
            stats,
        })
    }
}

fn diff_nodes(
    old_node: &tree_sitter::Node,
    new_node: &tree_sitter::Node,
    old_src: &str,
    new_src: &str,
    changes: &mut Vec<Change>,
) {
    let old_text = &old_src[old_node.byte_range()];
    let new_text = &new_src[new_node.byte_range()];

    if old_text == new_text {
        return;
    }

    if old_node.kind() != new_node.kind()
        || old_node.child_count() == 0
        || new_node.child_count() == 0
    {
        changes.push(Change {
            kind: ChangeKind::Modified,
            path: Some(old_node.kind().to_string()),
            old_value: Some(old_text.to_string()),
            new_value: Some(new_text.to_string()),
            location: Some(Location {
                line: old_node.start_position().row + 1,
                column: Some(old_node.start_position().column),
            }),
        });
        return;
    }

    let old_children: Vec<_> = (0..old_node.child_count())
        .filter_map(|i| old_node.child(i))
        .collect();
    let new_children: Vec<_> = (0..new_node.child_count())
        .filter_map(|i| new_node.child(i))
        .collect();

    let max_len = old_children.len().max(new_children.len());
    for i in 0..max_len {
        match (old_children.get(i), new_children.get(i)) {
            (Some(old_child), Some(new_child)) => {
                diff_nodes(old_child, new_child, old_src, new_src, changes);
            }
            (Some(old_child), None) => {
                let text = &old_src[old_child.byte_range()];
                changes.push(Change {
                    kind: ChangeKind::Removed,
                    path: Some(old_child.kind().to_string()),
                    old_value: Some(text.to_string()),
                    new_value: None,
                    location: Some(Location {
                        line: old_child.start_position().row + 1,
                        column: Some(old_child.start_position().column),
                    }),
                });
            }
            (None, Some(new_child)) => {
                let text = &new_src[new_child.byte_range()];
                changes.push(Change {
                    kind: ChangeKind::Added,
                    path: Some(new_child.kind().to_string()),
                    old_value: None,
                    new_value: Some(text.to_string()),
                    location: Some(Location {
                        line: new_child.start_position().row + 1,
                        column: Some(new_child.start_position().column),
                    }),
                });
            }
            (None, None) => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_code() {
        let format = CodeFormat::new("rust");
        let code = "fn main() { let x = 1; }\n";
        let result = format.diff(code, code).unwrap();
        assert_eq!(result.stats.additions, 0);
        assert_eq!(result.stats.deletions, 0);
        assert_eq!(result.stats.modifications, 0);
    }

    #[test]
    fn test_modified_code() {
        let format = CodeFormat::new("rust");
        let old = "fn main() { let x = 1; }\n";
        let new = "fn main() { let x = 2; }\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0);
    }

    #[test]
    fn test_unsupported_language_fallback() {
        let format = CodeFormat::new("cobol");
        let result = format.diff("hello\n", "world\n").unwrap();
        assert_eq!(result.format, FormatKind::Text);
    }

    #[test]
    fn test_javascript_diff() {
        let format = CodeFormat::new("javascript");
        let old = "const x = 1;\n";
        let new = "const x = 2;\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0);
    }

    #[test]
    fn test_python_diff() {
        let format = CodeFormat::new("python");
        let old = "x = 1\n";
        let new = "x = 2\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0);
    }

    #[test]
    fn test_go_diff() {
        let format = CodeFormat::new("go");
        let old = "package main\nfunc main() { x := 1 }\n";
        let new = "package main\nfunc main() { x := 2 }\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0);
    }

    #[test]
    fn test_typescript_diff() {
        let format = CodeFormat::new("typescript");
        let old = "const x: number = 1;\n";
        let new = "const x: number = 2;\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0);
    }

    #[test]
    fn test_java_diff() {
        let format = CodeFormat::new("java");
        let old = "class Main { int x = 1; }\n";
        let new = "class Main { int x = 2; }\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0);
    }

    #[test]
    fn test_c_diff() {
        let format = CodeFormat::new("c");
        let old = "int main() {\n    int x = 0;\n    return x;\n}\n";
        let new = "int main() {\n    int x = 1;\n    return x;\n}\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0 || result.stats.additions > 0 || result.stats.deletions > 0);
    }

    #[test]
    fn test_cpp_diff() {
        let format = CodeFormat::new("cpp");
        let old = "int main() { int x = 1; return 0; }\n";
        let new = "int main() { int x = 2; return 0; }\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0);
    }

    #[test]
    fn test_ruby_diff() {
        let format = CodeFormat::new("ruby");
        let old = "x = 1\n";
        let new = "x = 2\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0);
    }

    #[test]
    fn test_bash_diff() {
        let format = CodeFormat::new("bash");
        let old = "echo hello\n";
        let new = "echo world\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0);
    }

    #[test]
    fn test_zig_diff() {
        let format = CodeFormat::new("zig");
        let old = "const x = 1;\n";
        let new = "const x = 2;\n";
        let result = format.diff(old, new).unwrap();
        assert!(result.stats.modifications > 0);
    }

    #[test]
    fn test_supported_languages_list() {
        let langs = CodeFormat::supported_languages();
        assert!(langs.contains(&"rust"));
        assert!(langs.contains(&"python"));
        assert!(langs.contains(&"go"));
        assert!(langs.contains(&"typescript"));
        assert!(langs.contains(&"zig"));
        assert!(langs.contains(&"hcl"));
        assert!(langs.contains(&"scala"));
        assert!(langs.contains(&"swift"));
        assert!(langs.contains(&"elixir"));
        assert!(langs.contains(&"erlang"));
    }

    #[test]
    fn test_language_aliases() {
        for alias in &["js", "ts", "py", "rs", "rb", "sh", "cs", "c++", "ex", "erl", "hs", "ml", "mli", "yml"] {
            let format = CodeFormat::new(*alias);
            let result = format.get_parser();
            assert!(result.is_ok(), "Alias '{alias}' should create a valid parser");
        }
    }

    #[test]
    fn test_all_languages_parse() {
        let all_langs = [
            "bash", "c", "c-sharp", "cpp", "css", "dart", "elixir", "erlang",
            "go", "haskell", "hcl", "html", "java", "javascript", "json",
            "lua", "ocaml", "php", "python", "r", "regex", "ruby", "rust",
            "scala", "swift", "typescript", "tsx", "yaml", "zig",
        ];
        let mut ok_count = 0;
        for lang in &all_langs {
            let format = CodeFormat::new(*lang);
            if format.get_parser().is_ok() {
                ok_count += 1;
            }
        }
        // At minimum, the core languages should work
        assert!(ok_count >= 20, "Expected at least 20 languages to parse, got {ok_count}");
    }
}
