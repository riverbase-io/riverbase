//! HCL parsing via [`hcl-rs`](https://docs.rs/hcl-rs/latest/hcl/).

use serde_json::Value;

use crate::base::RiverbaseResult;

/// Deserialize HCL text into a `serde_json::Value` (HCL JSON specification).
pub fn parse_hcl_to_json(text: &str) -> RiverbaseResult<Value> {
    hcl::from_str(text).map_err(|e| crate::errors::CFG_010.with_data(e.to_string()))
}

/// Parse an HCL file into one or more process definition JSON documents.
pub fn parse_hcl_process_documents(text: &str) -> RiverbaseResult<Vec<Value>> {
    let root = parse_hcl_to_json(text)?;
    let Some(obj) = root.as_object() else {
        return Err(crate::errors::CFG_011.with_data(root.to_string()));
    };

    if let Some(processes) = obj.get("process") {
        return labeled_process_blocks(processes);
    }

    Ok(vec![root])
}

fn labeled_process_blocks(value: &Value) -> RiverbaseResult<Vec<Value>> {
    let Some(map) = value.as_object() else {
        return Err(crate::errors::CFG_012.with_data(value.to_string()));
    };
    let mut docs = Vec::with_capacity(map.len());
    for (name, body) in map {
        let mut doc = body.clone();
        if let Some(obj) = doc.as_object_mut() {
            obj.entry("name".to_string())
                .or_insert_with(|| Value::String(name.clone()));
        }
        docs.push(doc);
    }
    Ok(docs)
}
