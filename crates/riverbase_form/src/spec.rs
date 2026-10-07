//! Spec types for elements, forms, and documents.

use std::collections::{BTreeMap, HashMap};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Document node type discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Section,
    Content,
    Form,
}

/// Expression constraint on a field or element (`constraint "name" { expr, message }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintRule {
    pub expr: String,
    #[serde(default)]
    pub message: Option<String>,
}

/// Form-level constraint: JSON Schema conditional or expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormConstraintRule {
    #[serde(default)]
    pub expr: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(flatten)]
    pub schema: BTreeMap<String, Value>,
}

impl FormConstraintRule {
    pub fn has_expr(&self) -> bool {
        self.expr.as_ref().is_some_and(|s| !s.is_empty())
    }

    pub fn has_schema(&self) -> bool {
        !self.schema.is_empty()
    }
}

/// JSON-Schema-like field declaration for an element data column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub validation: Option<Value>,
    #[serde(default)]
    pub constraint: IndexMap<String, ConstraintRule>,
}

/// Inline anonymous element schema within a form element block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineElement {
    #[serde(default)]
    pub field: BTreeMap<String, FieldDef>,
    #[serde(default)]
    pub validation: Option<Value>,
    #[serde(default)]
    pub constraint: IndexMap<String, ConstraintRule>,
}

/// A data element reference or inline schema within a form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormElement {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub schema: Option<InlineElement>,
}

/// Data element specification (from `elements/*.hcl`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementSpec {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub desc: Option<String>,
    pub table_name: String,
    #[serde(default)]
    pub validation: Option<Value>,
    #[serde(default)]
    pub constraint: IndexMap<String, ConstraintRule>,
    #[serde(default)]
    pub field: BTreeMap<String, FieldDef>,
}

/// A layout group of form elements (non-nestable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormGroup {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub element: IndexMap<String, FormElement>,
}

/// Form specification (from `forms/*.hcl`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormSpec {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub footer: Option<String>,
    /// Ungrouped element references or inline anonymous elements.
    #[serde(default)]
    pub element: IndexMap<String, FormElement>,
    /// Named groups containing elements.
    #[serde(default)]
    pub group: IndexMap<String, FormGroup>,
    #[serde(default)]
    pub constraint: IndexMap<String, FormConstraintRule>,
}

impl FormSpec {
    /// Flat iteration: `(element_name, optional_group_key, element)`.
    /// Order: all top-level elements, then each group's elements in map order.
    pub fn iter_elements(&self) -> impl Iterator<Item = (&str, Option<&str>, &FormElement)> {
        let top = self
            .element
            .iter()
            .map(|(name, elem)| (name.as_str(), None, elem));
        let grouped = self.group.iter().flat_map(|(group_key, group)| {
            group
                .element
                .iter()
                .map(move |(name, elem)| (name.as_str(), Some(group_key.as_str()), elem))
        });
        top.chain(grouped)
    }
}

/// A node within a document template.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "node_type", rename_all = "lowercase")]
pub enum DocumentNode {
    Section {
        title: String,
        #[serde(default)]
        desc: Option<String>,
        #[serde(default)]
        order: i32,
        #[serde(default)]
        children: Vec<DocumentNode>,
    },
    Content {
        title: String,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        ctype: Option<String>,
        #[serde(default)]
        order: i32,
    },
    Form {
        form_key: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        attrs: Option<Value>,
        #[serde(default)]
        data: Option<Value>,
        #[serde(default)]
        order: i32,
    },
}

/// Document / template specification (from `documents/*.hcl`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSpec {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub version: Option<i32>,
    #[serde(default)]
    pub types: Option<String>,
    #[serde(default)]
    pub nodes: Vec<DocumentNode>,
}

/// Runtime document assembled from persisted data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub document_key: String,
    pub document_name: String,
    #[serde(default)]
    pub template_key: Option<String>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub version: i32,
    pub children: Vec<RuntimeDocumentNode>,
}

/// Runtime document node with resolved data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "node_type", rename_all = "lowercase")]
pub enum RuntimeDocumentNode {
    Section {
        node_key: String,
        title: String,
        #[serde(default)]
        desc: Option<String>,
        #[serde(default)]
        children: Vec<RuntimeDocumentNode>,
    },
    Content {
        node_key: String,
        title: String,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        ctype: Option<String>,
    },
    Form {
        node_key: String,
        form_key: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        elements: HashMap<String, Value>,
    },
}

impl Document {
    /// Nested JSON dump for rendering (e.g. via `river_duckweed`).
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        json!({
            "node_type": "Document",
            "document_id": self.id,
            "document_key": self.document_key,
            "title": self.document_name,
            "template_key": self.template_key,
            "desc": self.desc,
            "version": self.version,
            "children": self.children.iter().map(runtime_node_to_json).collect::<Vec<_>>(),
        })
    }
}

fn runtime_node_to_json(node: &RuntimeDocumentNode) -> serde_json::Value {
    use serde_json::json;
    match node {
        RuntimeDocumentNode::Section {
            node_key,
            title,
            desc,
            children,
        } => json!({
            "node_type": "Section",
            "node_key": node_key,
            "title": title,
            "desc": desc,
            "children": children.iter().map(runtime_node_to_json).collect::<Vec<_>>(),
        }),
        RuntimeDocumentNode::Content {
            node_key,
            title,
            content,
            ctype,
        } => json!({
            "node_type": "Content",
            "node_key": node_key,
            "title": title,
            "content": content,
            "ctype": ctype,
        }),
        RuntimeDocumentNode::Form {
            node_key,
            form_key,
            title,
            status,
            elements,
        } => json!({
            "node_type": "Form",
            "node_key": node_key,
            "form_key": form_key,
            "title": title,
            "status": status,
            "elements": elements,
        }),
    }
}
