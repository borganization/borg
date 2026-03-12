pub mod apply;
pub mod parser;

use anyhow::Result;
use std::path::Path;

pub fn apply_patch_to_dir(patch_text: &str, base_dir: &Path) -> Result<Vec<String>> {
    let patch = parser::parse_patch(patch_text)?;
    apply::apply_patch(&patch, base_dir)
}
