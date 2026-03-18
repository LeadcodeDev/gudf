use crate::error::GudfError;
use crate::result::DiffResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatKind {
    Text,
    Json,
    Toml,
    Yaml,
    Code(String),
}

pub trait Format {
    fn kind(&self) -> FormatKind;
    fn diff(&self, old: &str, new: &str) -> Result<DiffResult, GudfError>;
}

pub fn detect_format(content: &str) -> FormatKind {
    let trimmed = content.trim();

    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return FormatKind::Json;
    }

    if trimmed.parse::<toml::Value>().is_ok() && trimmed.contains('=') {
        return FormatKind::Toml;
    }

    // YAML detection: require either a front-matter marker or multiple key-value lines
    if !trimmed.is_empty() && serde_yaml::from_str::<serde_yaml::Value>(trimmed).is_ok() {
        // Check the parsed result is a mapping (not a scalar)
        if let Ok(val) = serde_yaml::from_str::<serde_yaml::Value>(trimmed) {
            let is_mapping = val.is_mapping();

            if trimmed.starts_with("---") && is_mapping {
                return FormatKind::Yaml;
            }

            // Count lines with "key: value" or "key:\n" patterns
            let kv_lines = trimmed
                .lines()
                .filter(|line| {
                    let l = line.trim();
                    !l.is_empty() && !l.starts_with('#') && (l.contains(": ") || l.ends_with(':'))
                })
                .count();

            if is_mapping && kv_lines >= 2 {
                return FormatKind::Yaml;
            }
        }
    }

    FormatKind::Text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_single_kv_as_text() {
        // A single "key: value" line should NOT be detected as YAML
        assert_eq!(detect_format("note: this is text"), FormatKind::Text);
    }

    #[test]
    fn test_detect_multiple_kv_as_yaml() {
        assert_eq!(
            detect_format("key: value\nother: data\n"),
            FormatKind::Yaml
        );
    }

    #[test]
    fn test_detect_front_matter_as_yaml() {
        assert_eq!(detect_format("---\nname: test\n"), FormatKind::Yaml);
    }

    #[test]
    fn test_detect_json() {
        assert_eq!(detect_format(r#"{"a": 1}"#), FormatKind::Json);
    }

    #[test]
    fn test_detect_toml() {
        assert_eq!(detect_format("[server]\nhost = \"localhost\"\n"), FormatKind::Toml);
    }

    #[test]
    fn test_detect_plain_text() {
        assert_eq!(detect_format("hello world"), FormatKind::Text);
    }
}
