//! Validate HCL configs under a base directory.

use std::path::Path;

use riverbase_core::base::RiverbaseResult;

use crate::config::{load_documents_from_dir, load_elements_from_dir, load_forms_from_dir};
use crate::registry::register_all_from_base;

pub fn validate_dir(dir: &Path) -> RiverbaseResult<()> {
    let _ = load_elements_from_dir(&dir.join("elements"))?;
    let _ = load_forms_from_dir(&dir.join("forms"))?;
    let _ = load_documents_from_dir(&dir.join("documents"))?;
    Ok(())
}

pub fn validate_and_register(dir: &Path) -> RiverbaseResult<(usize, usize, usize)> {
    register_all_from_base(dir)
}
