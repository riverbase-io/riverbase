//! Config loading helpers.

use std::path::Path;

use riverbase_core::base::RiverbaseResult;
use riverbase_core::cfgfmt::read_config_text;

use crate::schema::{parse_document_spec, parse_element_spec, parse_form_spec};
use crate::spec::{DocumentSpec, ElementSpec, FormSpec};

pub fn load_element_from_path(path: &Path) -> RiverbaseResult<ElementSpec> {
    let (text, format) = read_config_text(path)?;
    parse_element_spec(&text, format)
}

pub fn load_form_from_path(path: &Path) -> RiverbaseResult<FormSpec> {
    let (text, format) = read_config_text(path)?;
    parse_form_spec(&text, format)
}

pub fn load_document_from_path(path: &Path) -> RiverbaseResult<DocumentSpec> {
    let (text, format) = read_config_text(path)?;
    parse_document_spec(&text, format)
}

pub fn load_elements_from_dir(dir: &Path) -> RiverbaseResult<Vec<ElementSpec>> {
    load_specs_from_dir(dir, load_element_from_path)
}

pub fn load_forms_from_dir(dir: &Path) -> RiverbaseResult<Vec<FormSpec>> {
    load_specs_from_dir(dir, load_form_from_path)
}

pub fn load_documents_from_dir(dir: &Path) -> RiverbaseResult<Vec<DocumentSpec>> {
    load_specs_from_dir(dir, load_document_from_path)
}

fn load_specs_from_dir<T, F>(dir: &Path, loader: F) -> RiverbaseResult<Vec<T>>
where
    F: Fn(&Path) -> RiverbaseResult<T>,
{
    let mut specs = Vec::new();
    if !dir.is_dir() {
        return Ok(specs);
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| crate::errors::FRM_020.with_data(e.to_string()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("hcl") {
            specs.push(loader(&path)?);
        }
    }
    Ok(specs)
}
