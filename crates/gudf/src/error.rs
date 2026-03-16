use thiserror::Error;

#[derive(Debug, Error)]
pub enum GudfError {
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Patch error: {0}")]
    PatchError(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
