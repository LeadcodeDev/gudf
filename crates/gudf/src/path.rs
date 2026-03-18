/// Shared path parsing utilities for gudf.
///
/// Paths use dot notation for object keys and bracket notation for array indices:
/// - `key.nested` → Key("key"), Key("nested")
/// - `items[0].name` → Key("items"), Index(0), Key("name")
/// - `obj["my.key"][0].name` → Key("obj"), Key("my.key"), Index(0), Key("name")

/// A segment of a parsed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

/// Parse a dotted path string into segments, supporting:
/// - Dot-separated keys: `a.b.c`
/// - Array indices: `a[0].b`
/// - Bracket-quoted keys: `a["my.key"].b`
pub fn parse_path(path: &str) -> Vec<PathSegment> {
    let mut segments = Vec::new();
    let chars: Vec<char> = path.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        match chars[i] {
            '.' => {
                i += 1;
            }
            '[' => {
                i += 1; // skip '['
                if i < len && chars[i] == '"' {
                    // Bracket-quoted key: ["my.key"]
                    i += 1; // skip opening '"'
                    let start = i;
                    while i < len && chars[i] != '"' {
                        i += 1;
                    }
                    let key: String = chars[start..i].iter().collect();
                    segments.push(PathSegment::Key(key));
                    if i < len {
                        i += 1; // skip closing '"'
                    }
                    if i < len && chars[i] == ']' {
                        i += 1; // skip ']'
                    }
                } else {
                    // Numeric index: [0]
                    let start = i;
                    while i < len && chars[i] != ']' {
                        i += 1;
                    }
                    let idx_str: String = chars[start..i].iter().collect();
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        segments.push(PathSegment::Index(idx));
                    }
                    if i < len {
                        i += 1; // skip ']'
                    }
                }
            }
            _ => {
                // Regular key segment until next '.', '[', or end
                let start = i;
                while i < len && chars[i] != '.' && chars[i] != '[' {
                    i += 1;
                }
                let key: String = chars[start..i].iter().collect();
                if !key.is_empty() {
                    segments.push(PathSegment::Key(key));
                }
            }
        }
    }

    segments
}

/// Build a path string from segments, using bracket-quoting for keys containing `.` or `[`.
pub fn build_path(segments: &[PathSegment]) -> String {
    let mut result = String::new();
    for (i, seg) in segments.iter().enumerate() {
        match seg {
            PathSegment::Key(key) => {
                if key.contains('.') || key.contains('[') {
                    // Bracket-quoted key
                    if !result.is_empty() {
                        // No dot before bracket notation
                    }
                    result.push_str(&format!("[\"{key}\"]"));
                } else {
                    if i > 0 && !result.ends_with(']') {
                        result.push('.');
                    } else if i > 0 && result.ends_with(']') {
                        result.push('.');
                    }
                    result.push_str(key);
                }
            }
            PathSegment::Index(idx) => {
                result.push_str(&format!("[{idx}]"));
            }
        }
    }
    result
}

/// Convert a gudf path to a JSON Pointer (RFC 6901).
///
/// Examples:
/// - `name` → `/name`
/// - `items[0].name` → `/items/0/name`
/// - `obj["my.key"][0]` → `/obj/my.key/0`
/// - empty or `$` → ``
pub fn to_json_pointer(path: &str) -> String {
    if path.is_empty() || path == "$" {
        return String::new();
    }

    let segments = parse_path(path);
    let mut result = String::new();
    for seg in &segments {
        result.push('/');
        match seg {
            PathSegment::Key(key) => {
                // RFC 6901: escape ~ as ~0, / as ~1
                let escaped = key.replace('~', "~0").replace('/', "~1");
                result.push_str(&escaped);
            }
            PathSegment::Index(idx) => {
                result.push_str(&idx.to_string());
            }
        }
    }
    result
}

/// Check if a key needs bracket-quoting in a path (contains `.` or `[`).
pub fn needs_quoting(key: &str) -> bool {
    key.contains('.') || key.contains('[')
}

/// Format a key for inclusion in a path, applying bracket-quoting if needed.
pub fn format_key(key: &str) -> String {
    if needs_quoting(key) {
        format!("[\"{key}\"]")
    } else {
        key.to_string()
    }
}

/// Append a key to an existing path, applying bracket-quoting if needed.
pub fn append_key(path: &str, key: &str) -> String {
    if needs_quoting(key) {
        format!("{path}[\"{key}\"]")
    } else if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// Append an array index to an existing path.
pub fn append_index(path: &str, index: usize) -> String {
    if path.is_empty() {
        format!("[{index}]")
    } else {
        format!("{path}[{index}]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_path() {
        assert_eq!(
            parse_path("a.b.c"),
            vec![
                PathSegment::Key("a".into()),
                PathSegment::Key("b".into()),
                PathSegment::Key("c".into()),
            ]
        );
    }

    #[test]
    fn test_parse_array_index() {
        assert_eq!(
            parse_path("items[0].name"),
            vec![
                PathSegment::Key("items".into()),
                PathSegment::Index(0),
                PathSegment::Key("name".into()),
            ]
        );
    }

    #[test]
    fn test_parse_bracket_quoted_key() {
        assert_eq!(
            parse_path(r#"obj["my.key"][0].name"#),
            vec![
                PathSegment::Key("obj".into()),
                PathSegment::Key("my.key".into()),
                PathSegment::Index(0),
                PathSegment::Key("name".into()),
            ]
        );
    }

    #[test]
    fn test_parse_root_array() {
        assert_eq!(
            parse_path("[0]"),
            vec![PathSegment::Index(0)]
        );
    }

    #[test]
    fn test_parse_multiple_indices() {
        assert_eq!(
            parse_path("matrix[0][1]"),
            vec![
                PathSegment::Key("matrix".into()),
                PathSegment::Index(0),
                PathSegment::Index(1),
            ]
        );
    }

    #[test]
    fn test_to_json_pointer() {
        assert_eq!(to_json_pointer("items[0].name"), "/items/0/name");
        assert_eq!(to_json_pointer("a.b.c"), "/a/b/c");
        assert_eq!(to_json_pointer(""), "");
        assert_eq!(to_json_pointer("$"), "");
        assert_eq!(to_json_pointer(r#"obj["my.key"][0]"#), "/obj/my.key/0");
    }

    #[test]
    fn test_to_json_pointer_rfc6901_escaping() {
        // Keys containing / or ~ need escaping per RFC 6901
        assert_eq!(to_json_pointer(r#"["a/b"]"#), "/a~1b");
        assert_eq!(to_json_pointer(r#"["a~b"]"#), "/a~0b");
    }

    #[test]
    fn test_build_path() {
        let segs = vec![
            PathSegment::Key("obj".into()),
            PathSegment::Key("my.key".into()),
            PathSegment::Index(0),
            PathSegment::Key("name".into()),
        ];
        assert_eq!(build_path(&segs), r#"obj["my.key"][0].name"#);
    }

    #[test]
    fn test_append_key_simple() {
        assert_eq!(append_key("obj", "name"), "obj.name");
        assert_eq!(append_key("", "name"), "name");
    }

    #[test]
    fn test_append_key_dotted() {
        assert_eq!(append_key("obj", "my.key"), r#"obj["my.key"]"#);
    }

    #[test]
    fn test_append_index() {
        assert_eq!(append_index("items", 0), "items[0]");
        assert_eq!(append_index("", 0), "[0]");
    }
}
