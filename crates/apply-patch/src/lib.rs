pub mod apply;
pub mod parser;
mod seek_sequence;

use anyhow::Result;
use std::path::Path;

pub use apply::AffectedPaths;

pub fn apply_patch_to_dir(patch_text: &str, base_dir: &Path) -> Result<AffectedPaths> {
    let patch = parser::parse_patch(patch_text)?;
    apply::apply_patch(&patch, base_dir)
}
