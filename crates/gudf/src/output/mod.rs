pub mod inline;
pub mod json;
pub mod json_patch;
pub mod unified;

use crate::result::DiffResult;

pub trait OutputFormatter {
    fn format(&self, result: &DiffResult) -> String;
}
