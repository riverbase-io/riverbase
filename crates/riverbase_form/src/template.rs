//! Template -> document generation.

use serde_json::Value;

use crate::registry::form_registry;
use crate::result::FormResult;
use crate::spec::{Document, DocumentNode, DocumentSpec, RuntimeDocumentNode};

/// Generate a runtime document from a template spec and optional input data.
pub fn generate(template: &DocumentSpec, input: &Value) -> FormResult<Document> {
    let children = template
        .nodes
        .iter()
        .map(|node| generate_node(node, input))
        .collect::<FormResult<Vec<_>>>()?;

    Ok(Document {
        id: uuid::Uuid::new_v4().to_string(),
        document_key: template.key.clone(),
        document_name: template.title.clone(),
        template_key: Some(template.key.clone()),
        desc: template.desc.clone(),
        version: template.version.unwrap_or(1),
        children,
    })
}

fn generate_node(node: &DocumentNode, input: &Value) -> FormResult<RuntimeDocumentNode> {
    match node {
        DocumentNode::Section {
            title,
            desc,
            order,
            children,
        } => {
            let child_nodes = children
                .iter()
                .map(|c| generate_node(c, input))
                .collect::<FormResult<Vec<_>>>()?;
            Ok(RuntimeDocumentNode::Section {
                node_key: format!("section-{order}"),
                title: render_template_string(title, input),
                desc: desc.as_ref().map(|d| render_template_string(d, input)),
                children: child_nodes,
            })
        }
        DocumentNode::Content {
            title,
            content,
            ctype,
            order,
        } => Ok(RuntimeDocumentNode::Content {
            node_key: format!("content-{order}"),
            title: render_template_string(title, input),
            content: content.as_ref().map(|c| render_template_string(c, input)),
            ctype: ctype.clone(),
        }),
        DocumentNode::Form {
            form_key,
            title,
            attrs: _,
            data,
            order,
        } => {
            if form_registry().get(form_key).is_none() {
                return Err(crate::errors::FRM_084.with_data(form_key.clone()));
            }
            let elements = build_form_elements(form_key, data.as_ref(), input);
            Ok(RuntimeDocumentNode::Form {
                node_key: format!("form-{order}"),
                form_key: form_key.clone(),
                title: title.clone(),
                status: Some("draft".to_string()),
                elements,
            })
        }
    }
}

fn build_form_elements(
    form_key: &str,
    seed: Option<&Value>,
    input: &Value,
) -> std::collections::HashMap<String, Value> {
    let mut out = std::collections::HashMap::new();
    if let Some(form) = form_registry().get(form_key) {
        for (name, _group, _elem) in form.iter_elements() {
            let mut data = Value::Null;
            if let Some(seed) = seed {
                if let Some(v) = seed.get(name) {
                    data = v.clone();
                }
            }
            if data.is_null() {
                if let Some(v) = input.get(name) {
                    data = v.clone();
                }
            }
            out.insert(name.to_string(), data);
        }
    }
    out
}

/// Simple `{{var}}` template substitution against a JSON input object.
pub fn render_template_string(template: &str, input: &Value) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        if let Some(end) = rest.find("}}") {
            let key = rest[..end].trim();
            if let Some(val) = lookup_path(input, key) {
                out.push_str(&value_to_string(val));
            }
            rest = &rest[end + 2..];
        } else {
            out.push_str("{{");
            break;
        }
    }
    out.push_str(rest);
    out
}

fn lookup_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::form_registry;
    use crate::spec::{FormElement, FormGroup, FormSpec};
    use indexmap::IndexMap;
    use serde_json::json;

    #[test]
    fn renders_template_string() {
        let input = json!({ "name": "Alice" });
        assert_eq!(
            render_template_string("Hello {{name}}!", &input),
            "Hello Alice!"
        );
    }

    #[test]
    fn generates_document_from_template() {
        form_registry().clear();
        form_registry()
            .register(
                "FRM-0001".to_string(),
                FormSpec {
                    key: "FRM-0001".to_string(),
                    title: "Test Form".to_string(),
                    desc: None,
                    header: None,
                    footer: None,
                    element: IndexMap::new(),
                    group: IndexMap::from([(
                        "basic".to_string(),
                        FormGroup {
                            title: None,
                            desc: None,
                            element: IndexMap::from([(
                                "full_name".to_string(),
                                FormElement {
                                    key: Some("TXT-0001".to_string()),
                                    required: false,
                                    title: None,
                                    desc: None,
                                    schema: None,
                                },
                            )]),
                        },
                    )]),
                    constraint: IndexMap::new(),
                },
            )
            .unwrap();

        let template = DocumentSpec {
            key: "DOC-0001".to_string(),
            title: "Sample Doc".to_string(),
            desc: None,
            version: Some(1),
            types: None,
            nodes: vec![
                DocumentNode::Content {
                    title: "Header".to_string(),
                    content: Some("Welcome {{name}}".to_string()),
                    ctype: Some("text".to_string()),
                    order: 0,
                },
                DocumentNode::Form {
                    form_key: "FRM-0001".to_string(),
                    title: Some("Applicant".to_string()),
                    attrs: None,
                    data: None,
                    order: 1,
                },
            ],
        };

        let doc = generate(
            &template,
            &json!({ "name": "Bob", "full_name": "Bob Smith" }),
        )
        .unwrap();
        assert_eq!(doc.document_key, "DOC-0001");
        assert_eq!(doc.children.len(), 2);
    }
}
