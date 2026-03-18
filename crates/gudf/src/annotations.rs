use crate::result::Change;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub key: String,
    pub value: AnnotationValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnotationValue {
    Tag(String),
    Severity(Severity),
    Category(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

pub trait Annotator {
    fn annotate(&self, change: &Change) -> Vec<Annotation>;
}

/// Tags changes with the AST node type from tree-sitter (stored in the `path` field for code diffs).
pub struct AstNodeAnnotator;

impl Annotator for AstNodeAnnotator {
    fn annotate(&self, change: &Change) -> Vec<Annotation> {
        let Some(path) = &change.path else {
            return Vec::new();
        };

        let node_types = [
            "function_definition",
            "function_item",
            "function_declaration",
            "method_definition",
            "method_declaration",
            "class_definition",
            "class_declaration",
            "struct_item",
            "enum_item",
            "impl_item",
            "trait_item",
            "interface_declaration",
            "comment",
            "line_comment",
            "block_comment",
            "import_statement",
            "use_declaration",
            "variable_declaration",
            "let_declaration",
            "const_item",
            "type_alias",
        ];

        let mut annotations = Vec::new();
        for node_type in &node_types {
            if path == *node_type {
                annotations.push(Annotation {
                    key: "ast_node".to_string(),
                    value: AnnotationValue::Tag(node_type.to_string()),
                });
                break;
            }
        }

        annotations
    }
}

/// Flags changes to fields whose names suggest sensitive data.
pub struct SensitiveFieldAnnotator {
    patterns: Vec<String>,
}

const DEFAULT_SENSITIVE_PATTERNS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "api-key",
    "private_key",
    "private-key",
    "credential",
    "auth",
    "authorization",
    "session",
    "cookie",
    "jwt",
    "bearer",
    "client_secret",
];

impl SensitiveFieldAnnotator {
    pub fn new(patterns: Vec<String>) -> Self {
        Self { patterns }
    }
}

impl Default for SensitiveFieldAnnotator {
    fn default() -> Self {
        Self {
            patterns: DEFAULT_SENSITIVE_PATTERNS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

impl Annotator for SensitiveFieldAnnotator {
    fn annotate(&self, change: &Change) -> Vec<Annotation> {
        let Some(path) = &change.path else {
            return Vec::new();
        };

        let lower = path.to_lowercase();
        for pattern in &self.patterns {
            if lower.contains(pattern.as_str()) {
                return vec![
                    Annotation {
                        key: "sensitive".to_string(),
                        value: AnnotationValue::Severity(Severity::Warning),
                    },
                    Annotation {
                        key: "sensitive_field".to_string(),
                        value: AnnotationValue::Tag(pattern.to_string()),
                    },
                ];
            }
        }

        Vec::new()
    }
}

/// Annotates changes with their path nesting depth and whether they are leaf or branch nodes.
pub struct PathDepthAnnotator;

impl Annotator for PathDepthAnnotator {
    fn annotate(&self, change: &Change) -> Vec<Annotation> {
        let Some(path) = &change.path else {
            return Vec::new();
        };

        let depth = path.matches('.').count() + path.matches('[').count() + 1;
        let is_leaf = change.old_value.as_ref().map_or(true, |v| {
            !v.starts_with('{') && !v.starts_with('[')
        }) && change.new_value.as_ref().map_or(true, |v| {
            !v.starts_with('{') && !v.starts_with('[')
        });

        vec![
            Annotation {
                key: "depth".to_string(),
                value: AnnotationValue::Tag(depth.to_string()),
            },
            Annotation {
                key: "node_type".to_string(),
                value: AnnotationValue::Category(if is_leaf {
                    "leaf".to_string()
                } else {
                    "branch".to_string()
                }),
            },
        ]
    }
}

/// Apply a set of annotators to all changes in a diff result.
pub fn annotate_changes(changes: &mut [Change], annotators: &[Box<dyn Annotator>]) {
    for change in changes.iter_mut() {
        for annotator in annotators {
            let new_annotations = annotator.annotate(change);
            change.annotations.extend(new_annotations);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::ChangeKind;

    fn make_change(kind: ChangeKind, path: Option<&str>, old: Option<&str>, new: Option<&str>) -> Change {
        Change {
            kind,
            path: path.map(|s| s.to_string()),
            old_value: old.map(|s| s.to_string()),
            new_value: new.map(|s| s.to_string()),
            location: None,
            annotations: Vec::new(),
        }
    }

    #[test]
    fn test_sensitive_field_annotator() {
        let annotator = SensitiveFieldAnnotator::default();
        let change = make_change(ChangeKind::Modified, Some("database.password"), Some("old"), Some("new"));
        let annotations = annotator.annotate(&change);
        assert!(!annotations.is_empty());
        assert!(annotations.iter().any(|a| a.key == "sensitive"));
    }

    #[test]
    fn test_sensitive_field_no_match() {
        let annotator = SensitiveFieldAnnotator::default();
        let change = make_change(ChangeKind::Modified, Some("database.host"), Some("old"), Some("new"));
        let annotations = annotator.annotate(&change);
        assert!(annotations.is_empty());
    }

    #[test]
    fn test_path_depth_annotator() {
        let annotator = PathDepthAnnotator;
        let change = make_change(ChangeKind::Modified, Some("a.b.c"), Some("1"), Some("2"));
        let annotations = annotator.annotate(&change);
        let depth = annotations.iter().find(|a| a.key == "depth").unwrap();
        assert_eq!(depth.value, AnnotationValue::Tag("3".to_string()));
    }

    #[test]
    fn test_ast_node_annotator() {
        let annotator = AstNodeAnnotator;
        let change = make_change(ChangeKind::Modified, Some("function_definition"), Some("old"), Some("new"));
        let annotations = annotator.annotate(&change);
        assert!(!annotations.is_empty());
        assert!(annotations.iter().any(|a| a.key == "ast_node"));
    }

    #[test]
    fn test_annotate_changes() {
        let mut changes = vec![
            make_change(ChangeKind::Modified, Some("config.api_key"), Some("old"), Some("new")),
            make_change(ChangeKind::Modified, Some("config.host"), Some("old"), Some("new")),
        ];
        let annotators: Vec<Box<dyn Annotator>> = vec![
            Box::new(SensitiveFieldAnnotator::default()),
            Box::new(PathDepthAnnotator),
        ];
        annotate_changes(&mut changes, &annotators);
        assert!(!changes[0].annotations.is_empty());
        assert!(changes[0].annotations.iter().any(|a| a.key == "sensitive"));
        assert!(!changes[1].annotations.is_empty());
        assert!(changes[1].annotations.iter().all(|a| a.key != "sensitive"));
    }

    #[test]
    fn test_sensitive_new_patterns() {
        let annotator = SensitiveFieldAnnotator::default();
        for field in &["session_id", "cookie_value", "jwt_token", "bearer_auth", "client_secret"] {
            let change = make_change(ChangeKind::Modified, Some(field), Some("old"), Some("new"));
            let annotations = annotator.annotate(&change);
            assert!(
                annotations.iter().any(|a| a.key == "sensitive"),
                "Expected {field} to be flagged as sensitive"
            );
        }
    }

    #[test]
    fn test_sensitive_custom_patterns() {
        let annotator = SensitiveFieldAnnotator::new(vec!["custom_field".to_string()]);
        let change = make_change(ChangeKind::Modified, Some("my_custom_field"), Some("old"), Some("new"));
        let annotations = annotator.annotate(&change);
        assert!(annotations.iter().any(|a| a.key == "sensitive"));

        // Default patterns should NOT match with custom-only
        let change2 = make_change(ChangeKind::Modified, Some("password"), Some("old"), Some("new"));
        let annotations2 = annotator.annotate(&change2);
        assert!(annotations2.is_empty());
    }

    #[test]
    fn test_ast_node_no_substring_match() {
        let annotator = AstNodeAnnotator;
        // "my_function_definition_v2" should NOT match "function_definition"
        let change = make_change(ChangeKind::Modified, Some("my_function_definition_v2"), Some("old"), Some("new"));
        let annotations = annotator.annotate(&change);
        assert!(annotations.is_empty(), "Should not match by substring");
    }
}
