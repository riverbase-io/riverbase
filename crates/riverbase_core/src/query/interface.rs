//! Front-end query **interface** — the storage-agnostic contract a resource exposes to
//! clients and query-builder UIs. Pure `&'static` data: no Diesel/Postgres dependency.
//!
//! See `docs/02-design/query/10-query-engine-design.md` §3.1 / §4 / §7.1.

use serde_json::{json, Map, Value};

use super::binding::QueryBinding;
use super::primitives::SortDirection;
use crate::base::ScopeMeta;
use crate::openapi_meta::OpenApiMeta;

/// Filter operators understood by the statement grammar and presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// Eq.
    Eq,
    /// Ne.
    Ne,
    /// Gt.
    Gt,
    /// Gte.
    Gte,
    /// Lt.
    Lt,
    /// Lte.
    Lte,
    /// In.
    In,
    /// Not in.
    NotIn,
    /// Substring / contains (maps to `ILIKE '%v%'`).
    Has,
    /// Array overlap (`&&`).
    Ov,
    /// Between.
    Between,
}

impl Operator {
    /// Canonical wire token, e.g. `Operator::Gte` → `"gte"`.
    pub fn token(self) -> &'static str {
        match self {
            Operator::Eq => "eq",
            Operator::Ne => "ne",
            Operator::Gt => "gt",
            Operator::Gte => "gte",
            Operator::Lt => "lt",
            Operator::Lte => "lte",
            Operator::In => "in",
            Operator::NotIn => "notin",
            Operator::Has => "has",
            Operator::Ov => "ov",
            Operator::Between => "between",
        }
    }

    /// Parse a wire token into an operator (case-insensitive).
    pub fn parse(token: &str) -> Option<Operator> {
        match token.to_ascii_lowercase().as_str() {
            "eq" => Some(Operator::Eq),
            "ne" => Some(Operator::Ne),
            "gt" => Some(Operator::Gt),
            "gte" => Some(Operator::Gte),
            "lt" => Some(Operator::Lt),
            "lte" => Some(Operator::Lte),
            "in" => Some(Operator::In),
            "notin" => Some(Operator::NotIn),
            "has" | "cs" => Some(Operator::Has),
            "ov" => Some(Operator::Ov),
            "between" => Some(Operator::Between),
            _ => None,
        }
    }
}

/// One operator a field exposes, with display/input metadata.
#[derive(Debug, Clone, Copy)]
pub struct FilterDef {
    /// Op.
    pub op: Operator,
    /// Label.
    pub label: &'static str,
    /// Dtype.
    pub dtype: &'static str,
    /// Input.
    pub input: &'static str,
}

/// A named operator set with exactly one default operator.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    /// Name.
    pub name: &'static str,
    /// Default value.
    pub default: Operator,
    /// Operators.
    pub operators: &'static [FilterDef],
    /// Default client-sortability for fields using this preset.
    pub sortable: bool,
}

impl Preset {
    /// Whether the preset exposes `op` (as a listed operator or its default).
    pub fn allows(&self, op: Operator) -> bool {
        self.default == op || self.operators.iter().any(|f| f.op == op)
    }
}

/// One queryable field as the frontend sees it (logical name only, no column/source).
#[derive(Debug, Clone, Copy)]
pub struct FieldDef {
    /// Name.
    pub name: &'static str,
    /// Label.
    pub label: &'static str,
    /// Preset.
    pub preset: &'static Preset,
    /// Sortable.
    pub sortable: bool,
    /// Hidden.
    pub hidden: bool,
    /// Identifier.
    pub identifier: bool,
}

/// One report input parameter as the frontend sees it.
#[derive(Debug, Clone, Copy)]
pub struct ParamDef {
    /// Name.
    pub name: &'static str,
    /// Label.
    pub label: &'static str,
    /// Preset.
    pub preset: &'static Preset,
    /// Required.
    pub required: bool,
    /// Default value.
    pub default: Option<&'static str>,
}

impl ParamDef {
    /// Construct a new value.
    pub const fn new(name: &'static str, label: &'static str, preset: &'static Preset) -> Self {
        Self {
            name,
            label,
            preset,
            required: false,
            default: None,
        }
    }

    /// Required.
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Default value.
    pub const fn default_value(mut self, value: &'static str) -> Self {
        self.default = Some(value);
        self
    }
}

impl FieldDef {
    /// Construct a new value.
    pub const fn new(name: &'static str, label: &'static str, preset: &'static Preset) -> Self {
        Self {
            name,
            label,
            preset,
            sortable: preset.sortable,
            hidden: false,
            identifier: false,
        }
    }

    /// Identifier.
    pub const fn identifier(mut self) -> Self {
        self.identifier = true;
        self
    }

    /// Hidden.
    pub const fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    /// Sortable.
    pub const fn sortable(mut self) -> Self {
        self.sortable = true;
        self
    }

    /// No sort.
    pub const fn no_sort(mut self) -> Self {
        self.sortable = false;
        self
    }
}

/// The front-end contract for a queryable resource.
pub trait QueryInterface: Send + Sync {
    /// Fields.
    fn fields(&self) -> &'static [FieldDef];
    /// Params.
    fn params(&self) -> &'static [ParamDef] {
        &[]
    }
    /// Allow text search.
    fn allow_text_search(&self) -> bool {
        false
    }
    /// Default order.
    fn default_order(&self) -> &'static [(&'static str, SortDirection)] {
        &[("id", SortDirection::Desc)]
    }
    /// Sole client-sort allowlist. When non-empty, supersedes field `sortable` flags.
    fn order_fields(&self) -> &'static [&'static str] {
        &[]
    }
    /// Title.
    fn title(&self) -> &'static str {
        ""
    }
    /// Description.
    fn description(&self) -> &'static str {
        ""
    }

    /// Whether list/item routes require a path scope segment (mirrors [`CommandMeta::scope`](crate::command::CommandMeta::scope)).
    fn scope(&self) -> ScopeMeta {
        ScopeMeta::none()
    }

    /// OpenAPI catalog / explorer overrides for HTTP route documentation.
    fn openapi(&self) -> OpenApiMeta {
        OpenApiMeta::default()
    }

    /// Whitelist of deployment zones where this query/report route is registered.
    fn allowed_zones(&self) -> Vec<String> {
        Vec::new()
    }

    /// Declarative profile-role requirements ([APP-01] / D12).
    fn roles_required(&self) -> &'static [&'static str] {
        &[]
    }

    /// The single identifier field name (falls back to `"id"`).
    fn identifier_field(&self) -> &'static str {
        self.fields()
            .iter()
            .find(|f| f.identifier)
            .map(|f| f.name)
            .unwrap_or("id")
    }

    /// Field.
    fn field(&self, name: &str) -> Option<&'static FieldDef> {
        self.fields().iter().find(|f| f.name == name)
    }
}

// --- Built-in presets (§4.1) ---

macro_rules! filter_def {
    ($op:expr, $label:expr, $dtype:expr, $input:expr) => {
        FilterDef {
            op: $op,
            label: $label,
            dtype: $dtype,
            input: $input,
        }
    };
}

/// Preset None static.
pub static PRESET_NONE: Preset = Preset {
    name: "none",
    default: Operator::Eq,
    operators: &[filter_def!(Operator::Eq, "Equals", "string", "text")],
    sortable: false,
};

/// Preset Textsearch static.
pub static PRESET_TEXTSEARCH: Preset = Preset {
    name: "textsearch",
    default: Operator::Eq,
    operators: &[filter_def!(Operator::Eq, "Matches", "string", "text")],
    sortable: false,
};

/// Preset Uuid static.
pub static PRESET_UUID: Preset = Preset {
    name: "uuid",
    default: Operator::Eq,
    operators: &[
        filter_def!(Operator::Eq, "Is", "uuid", "text"),
        filter_def!(Operator::In, "In", "uuid", "tags"),
        filter_def!(Operator::Ne, "Is not", "uuid", "text"),
    ],
    sortable: true,
};

/// Preset String static.
pub static PRESET_STRING: Preset = Preset {
    name: "string",
    default: Operator::Has,
    operators: &[
        filter_def!(Operator::Has, "Contains", "string", "text"),
        filter_def!(Operator::Eq, "Equals", "string", "text"),
        filter_def!(Operator::Ne, "Not equals", "string", "text"),
    ],
    sortable: true,
};

/// Preset Integer static.
pub static PRESET_INTEGER: Preset = Preset {
    name: "integer",
    default: Operator::Eq,
    operators: &[
        filter_def!(Operator::Eq, "Equals", "integer", "number"),
        filter_def!(Operator::Gt, "Greater than", "integer", "number"),
        filter_def!(Operator::Lt, "Less than", "integer", "number"),
        filter_def!(Operator::Lte, "At most", "integer", "number"),
        filter_def!(Operator::Gte, "At least", "integer", "number"),
    ],
    sortable: true,
};

/// Preset Number static.
pub static PRESET_NUMBER: Preset = Preset {
    name: "number",
    default: Operator::Eq,
    operators: &[
        filter_def!(Operator::Eq, "Equals", "number", "number"),
        filter_def!(Operator::Gt, "Greater than", "number", "number"),
        filter_def!(Operator::Lt, "Less than", "number", "number"),
        filter_def!(Operator::Lte, "At most", "number", "number"),
        filter_def!(Operator::Gte, "At least", "number", "number"),
    ],
    sortable: true,
};

/// Preset Json static.
pub static PRESET_JSON: Preset = Preset {
    name: "json",
    default: Operator::Eq,
    operators: &[
        filter_def!(Operator::Eq, "Equals", "json", "text"),
        filter_def!(Operator::Ne, "Not equals", "json", "text"),
    ],
    sortable: false,
};

/// Preset Array static.
pub static PRESET_ARRAY: Preset = Preset {
    name: "array",
    default: Operator::Eq,
    operators: &[
        filter_def!(Operator::Eq, "Equals", "array", "tags"),
        filter_def!(Operator::Ov, "Overlaps", "array", "tags"),
    ],
    sortable: false,
};

/// Preset Boolean static.
pub static PRESET_BOOLEAN: Preset = Preset {
    name: "boolean",
    default: Operator::Eq,
    operators: &[
        filter_def!(Operator::Eq, "Is", "boolean", "checkbox"),
        filter_def!(Operator::Ne, "Is not", "boolean", "checkbox"),
    ],
    sortable: true,
};

/// Preset Datetime static.
pub static PRESET_DATETIME: Preset = Preset {
    name: "datetime",
    default: Operator::Eq,
    operators: &[
        filter_def!(Operator::Eq, "On", "datetime", "datetime"),
        filter_def!(Operator::Gt, "After", "datetime", "datetime"),
        filter_def!(Operator::Lt, "Before", "datetime", "datetime"),
        filter_def!(Operator::Lte, "On or before", "datetime", "datetime"),
        filter_def!(Operator::Gte, "On or after", "datetime", "datetime"),
        filter_def!(Operator::Between, "Between", "datetime", "datetime-range"),
    ],
    sortable: true,
};

/// Preset Date static.
pub static PRESET_DATE: Preset = Preset {
    name: "date",
    default: Operator::Eq,
    operators: &[
        filter_def!(Operator::Eq, "On", "date", "date"),
        filter_def!(Operator::Gt, "After", "date", "date"),
        filter_def!(Operator::Lt, "Before", "date", "date"),
        filter_def!(Operator::Lte, "On or before", "date", "date"),
        filter_def!(Operator::Gte, "On or after", "date", "date"),
        filter_def!(Operator::Between, "Between", "date", "date-range"),
    ],
    sortable: true,
};

/// Preset Enum static.
pub static PRESET_ENUM: Preset = Preset {
    name: "enum",
    default: Operator::Eq,
    operators: &[
        filter_def!(Operator::Eq, "Is", "enum", "select"),
        filter_def!(Operator::Ne, "Is not", "enum", "select"),
        filter_def!(Operator::In, "In", "enum", "multiselect"),
    ],
    sortable: true,
};

/// Whether a logical field is sortable for clients.
///
/// [`QueryInterface::order_fields`], when non-empty, is the sole source and
/// supersedes field `sortable` flags. Otherwise the field flag (preset default)
/// is used.
pub fn field_sortable(iface: &dyn QueryInterface, field_name: &str) -> bool {
    let order_fields = iface.order_fields();
    if !order_fields.is_empty() {
        return order_fields.iter().any(|name| *name == field_name);
    }
    iface
        .field(field_name)
        .map(|field| field.sortable)
        .unwrap_or(false)
}

/// Build the `.meta` descriptor (§7.1) from the interface and optional binding.
pub fn build_resource_meta(
    name: &str,
    iface: &dyn QueryInterface,
    _binding: Option<&dyn QueryBinding>,
) -> Value {
    let mut fields = Vec::new();
    let mut filters = Map::new();

    for field in iface.fields() {
        fields.push(json!({
            "name": field.name,
            "label": field.label,
            "desc": "",
            "noop": field.preset.default.token(),
            "sortable": field_sortable(iface, field.name),
            "hidden": field.hidden,
            "finput": field.preset.operators.first().map(|f| f.input).unwrap_or("text"),
            "dtype": field.preset.operators.first().map(|f| f.dtype).unwrap_or("string"),
            "ftype": field.preset.name,
        }));
        for op in field.preset.operators {
            let key = format!("{}.{}", field.name, op.op.token());
            filters.insert(
                key,
                json!({
                    "field": field.name,
                    "label": op.label,
                    "dtype": op.dtype,
                    "input": op.input,
                }),
            );
        }
    }

    json!({
        "name": name,
        "resource": name,
        "title": iface.title(),
        "desc": iface.description(),
        "allow_text_search": iface.allow_text_search(),
        "idfield": iface.identifier_field(),
        "fields": fields,
        "filters": Value::Object(filters),
        "params": build_params_meta(iface),
        "composites": {
            ".and": { "label": "AND Group" },
            ".or": { "label": "OR Group" },
        },
    })
}

fn build_params_meta(iface: &dyn QueryInterface) -> Value {
    let params = iface.params();
    if params.is_empty() {
        return Value::Object(Map::new());
    }

    let mut out = Map::new();
    for param in params {
        let op = param.preset.operators.first();
        let mut entry = json!({
            "label": param.label,
            "dtype": op.map(|f| f.dtype).unwrap_or("string"),
            "input": op.map(|f| f.input).unwrap_or("text"),
            "required": param.required,
        });
        if let Some(default) = param.default {
            entry
                .as_object_mut()
                .expect("params meta entry is an object")
                .insert("default".to_string(), json!(default));
        }
        out.insert(param.name.to_string(), entry);
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ReportIface;

    impl QueryInterface for ReportIface {
        fn fields(&self) -> &'static [FieldDef] {
            &[]
        }

        fn params(&self) -> &'static [ParamDef] {
            static PARAMS: &[ParamDef] = &[
                ParamDef::new("from_date", "From date", &PRESET_DATE).required(),
                ParamDef::new("status", "Status", &PRESET_STRING).default_value("open"),
            ];
            PARAMS
        }
    }

    #[test]
    fn build_resource_meta_includes_params() {
        let meta = build_resource_meta("todo_summary", &ReportIface, None);
        let params = meta
            .get("params")
            .expect("params key")
            .as_object()
            .expect("params object");
        assert!(params.contains_key("from_date"));
        assert!(params.contains_key("status"));
        assert_eq!(params["from_date"]["required"], json!(true));
        assert_eq!(params["status"]["default"], json!("open"));
    }

    #[test]
    fn build_resource_meta_omits_empty_params_object() {
        struct PlainIface;

        impl QueryInterface for PlainIface {
            fn fields(&self) -> &'static [FieldDef] {
                &[]
            }
        }

        let meta = build_resource_meta("todo", &PlainIface, None);
        assert_eq!(meta["resource"], json!("todo"));
        assert_eq!(meta["params"], json!({}));
    }

    struct SortableIface;

    impl QueryInterface for SortableIface {
        fn fields(&self) -> &'static [FieldDef] {
            static FIELDS: &[FieldDef] = &[
                FieldDef::new("name", "Name", &PRESET_STRING),
                FieldDef::new("version", "Version", &PRESET_INTEGER).no_sort(),
                FieldDef::new("status", "Status", &PRESET_STRING),
            ];
            FIELDS
        }
    }

    #[test]
    fn build_resource_meta_uses_preset_sortable_and_field_overrides() {
        let meta = build_resource_meta("items", &SortableIface, None);
        let fields = meta["fields"].as_array().expect("fields array");
        let sortable: std::collections::HashMap<_, _> = fields
            .iter()
            .map(|field| {
                (
                    field["name"].as_str().expect("name"),
                    field["sortable"].as_bool().expect("sortable"),
                )
            })
            .collect();
        assert_eq!(sortable.get("name"), Some(&true));
        assert_eq!(sortable.get("version"), Some(&false));
        assert_eq!(sortable.get("status"), Some(&true));
    }

    struct JsonFieldIface;

    impl QueryInterface for JsonFieldIface {
        fn fields(&self) -> &'static [FieldDef] {
            static FIELDS: &[FieldDef] = &[
                FieldDef::new("payload", "Payload", &PRESET_JSON),
                FieldDef::new("payload_sortable", "Payload sortable", &PRESET_JSON).sortable(),
            ];
            FIELDS
        }
    }

    #[test]
    fn build_resource_meta_json_preset_defaults_and_sortable_override() {
        let meta = build_resource_meta("items", &JsonFieldIface, None);
        let fields = meta["fields"].as_array().expect("fields array");
        let sortable: std::collections::HashMap<_, _> = fields
            .iter()
            .map(|field| {
                (
                    field["name"].as_str().expect("name"),
                    field["sortable"].as_bool().expect("sortable"),
                )
            })
            .collect();
        assert_eq!(sortable.get("payload"), Some(&false));
        assert_eq!(sortable.get("payload_sortable"), Some(&true));
    }

    struct OrderFieldsIface;

    impl QueryInterface for OrderFieldsIface {
        fn fields(&self) -> &'static [FieldDef] {
            static FIELDS: &[FieldDef] = &[
                FieldDef::new("name", "Name", &PRESET_STRING),
                FieldDef::new("status", "Status", &PRESET_STRING),
                FieldDef::new("payload", "Payload", &PRESET_JSON),
            ];
            FIELDS
        }

        fn order_fields(&self) -> &'static [&'static str] {
            &["name", "payload"]
        }
    }

    #[test]
    fn order_fields_supersedes_field_sortable() {
        let iface = OrderFieldsIface;
        assert!(field_sortable(&iface, "name"));
        assert!(!field_sortable(&iface, "status"));
        assert!(field_sortable(&iface, "payload"));
        assert!(!field_sortable(&iface, "missing"));

        let meta = build_resource_meta("items", &iface, None);
        let fields = meta["fields"].as_array().expect("fields array");
        let sortable: std::collections::HashMap<_, _> = fields
            .iter()
            .map(|field| {
                (
                    field["name"].as_str().expect("name"),
                    field["sortable"].as_bool().expect("sortable"),
                )
            })
            .collect();
        assert_eq!(sortable.get("name"), Some(&true));
        assert_eq!(sortable.get("status"), Some(&false));
        assert_eq!(sortable.get("payload"), Some(&true));
    }

    #[test]
    fn query_resource_sort_preset_table_matches_statics() {
        // Keep in sync with `query_resource!` `@sort_if_preset`.
        assert!(PRESET_UUID.sortable);
        assert!(PRESET_STRING.sortable);
        assert!(PRESET_BOOLEAN.sortable);
        assert!(PRESET_DATETIME.sortable);
        assert!(PRESET_DATE.sortable);
        assert!(PRESET_INTEGER.sortable);
        assert!(PRESET_NUMBER.sortable);
        assert!(PRESET_ENUM.sortable);
        assert!(!PRESET_JSON.sortable);
        assert!(!PRESET_ARRAY.sortable);
        assert!(!PRESET_TEXTSEARCH.sortable);
        assert!(!PRESET_NONE.sortable);
    }
}
