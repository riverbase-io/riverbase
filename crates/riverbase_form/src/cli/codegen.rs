//! Code generation from element HCL specs.

use std::fs;
use std::path::Path;

use riverbase_core::base::RiverbaseResult;
use riverbase_core::DOMAIN_FIELDS_DDL;

use crate::config::load_elements_from_dir;
use crate::spec::{ElementSpec, FieldDef};

#[derive(Debug, Clone)]
struct FieldMapping {
    rust_name: String,
    diesel_type: String,
    sql_type: String,
}

pub fn codegen_elements(
    elements_dir: &Path,
    out_rs_dir: &Path,
    out_migrations_dir: &Path,
) -> RiverbaseResult<()> {
    let specs = load_elements_from_dir(elements_dir)?;
    if specs.is_empty() {
        return Err(crate::errors::FRM_050.with_data(""));
    }

    fs::create_dir_all(out_rs_dir).map_err(|e| crate::errors::FRM_051.with_data(e.to_string()))?;
    fs::create_dir_all(out_migrations_dir)
        .map_err(|e| crate::errors::FRM_068.with_data(e.to_string()))?;

    let schema_rs = generate_schema_rs(&specs);
    let entities_rs = generate_entities_rs(&specs);
    let mod_rs = r#"//! Generated element tables and entities.

pub mod entities;
pub mod schema;
"#;

    fs::write(out_rs_dir.join("mod.rs"), mod_rs)
        .map_err(|e| crate::errors::FRM_069.with_data(e.to_string()))?;
    fs::write(out_rs_dir.join("schema.rs"), schema_rs)
        .map_err(|e| crate::errors::FRM_070.with_data(e.to_string()))?;
    fs::write(out_rs_dir.join("entities.rs"), entities_rs)
        .map_err(|e| crate::errors::FRM_071.with_data(e.to_string()))?;

    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let mig_dir = out_migrations_dir.join(format!("{ts}_form_elements"));
    fs::create_dir_all(&mig_dir).map_err(|e| crate::errors::FRM_072.with_data(e.to_string()))?;
    fs::write(mig_dir.join("up.sql"), generate_up_sql(&specs))
        .map_err(|e| crate::errors::FRM_073.with_data(e.to_string()))?;
    fs::write(mig_dir.join("down.sql"), generate_down_sql(&specs))
        .map_err(|e| crate::errors::FRM_074.with_data(e.to_string()))?;

    Ok(())
}

fn extract_fields(spec: &ElementSpec) -> RiverbaseResult<Vec<FieldMapping>> {
    let mut fields = Vec::new();
    for (name, def) in &spec.field {
        let nullable = !def.required;
        let mapping = field_def_to_mapping(name, def, nullable)?;
        fields.push(mapping);
    }
    fields.sort_by(|a, b| a.rust_name.cmp(&b.rust_name));
    Ok(fields)
}

fn field_def_to_mapping(name: &str, def: &FieldDef, nullable: bool) -> RiverbaseResult<FieldMapping> {
    let ty = def.field_type.as_str();
    let format = def.format.as_deref();

    let (diesel_type, sql_type) = match (ty, format) {
        ("string", Some("uuid")) => ("Uuid", "UUID"),
        ("string", Some("date-time")) => ("Timestamptz", "TIMESTAMPTZ"),
        ("string", _) => ("Text", "TEXT"),
        ("integer", _) => ("Int8", "BIGINT"),
        ("number", _) => ("Float8", "DOUBLE PRECISION"),
        ("boolean", _) => ("Bool", "BOOLEAN"),
        ("object", _) | ("array", _) => ("Jsonb", "JSONB"),
        _ => ("Jsonb", "JSONB"),
    };

    Ok(FieldMapping {
        rust_name: name.to_string(),
        diesel_type: if nullable {
            format!("Nullable<{diesel_type}>")
        } else {
            diesel_type.to_string()
        },
        sql_type: if nullable {
            sql_type.to_string()
        } else {
            format!("{sql_type} NOT NULL")
        },
    })
}

fn ident(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn pascal(name: &str) -> String {
    ident(name)
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                None => String::new(),
                Some(f) => {
                    let upper = f.to_ascii_uppercase();
                    let mut s = upper.to_string();
                    s.push_str(c.as_str());
                    s
                }
            }
        })
        .collect()
}

fn generate_schema_rs(specs: &[ElementSpec]) -> String {
    let mut out = String::from("//! Generated Diesel schemas for element data tables.\n\n");
    for spec in specs {
        let fields = extract_fields(spec).unwrap_or_default();
        out.push_str(&format!(
            "riverbase_core::diesel_table_with_domain_fields! {{\n    riverbase_form.{} (_id) {{\n",
            ident(&spec.table_name)
        ));
        for f in &fields {
            out.push_str(&format!(
                "        {} -> {},\n",
                ident(&f.rust_name),
                f.diesel_type
            ));
        }
        out.push_str("    }\n}\n\n");
    }
    out
}

fn generate_entities_rs(specs: &[ElementSpec]) -> String {
    let mut out = String::from(
        "//! Generated entity wiring for element data tables.\n\n\
         use chrono::Utc;\n\
         use diesel::prelude::*;\n\
         use diesel_async::RunQueryDsl;\n\
         use riverbase_core::base::domain_fields_from_payload;\n\
         use riverbase_core::datastore::error::DataResult;\n\
         use serde_json::{json, Value};\n\
         use uuid::Uuid;\n\n\
         use crate::generated::schema::*;\n\n",
    );

    for spec in specs {
        let fields = extract_fields(spec).unwrap_or_default();
        let table = ident(&spec.table_name);
        let row = format!("{}Row", pascal(&spec.table_name));
        let entity = format!("{}Entity", pascal(&spec.table_name));

        out.push_str(&format!(
            "riverbase_core::domain_row! {{\n    #[derive(Debug, Clone, Queryable, Selectable)]\n    #[diesel(table_name = {table})]\n    pub struct {row} {{\n"
        ));
        for f in &fields {
            let rust_ty = diesel_to_rust_type(&f.diesel_type);
            out.push_str(&format!(
                "        pub {}: {rust_ty},\n",
                ident(&f.rust_name)
            ));
        }
        out.push_str("    }\n}\n\n");

        out.push_str(&format!(
            "pub fn {}_row_to_json(row: {row}) -> Value {{\n    json!({{\n",
            table
        ));
        out.push_str("        \"id\": row._id.to_string(),\n");
        for f in &fields {
            out.push_str(&format!(
                "        \"{}\": row.{},\n",
                f.rust_name,
                ident(&f.rust_name)
            ));
        }
        out.push_str("    })\n}\n\n");

        out.push_str(&format!(
            "pub async fn {}_upsert_from_json(conn: &mut diesel_async::AsyncPgConnection, id: Uuid, data: &Value) -> DataResult<()> {{\n",
            table
        ));
        for f in &fields {
            let extract = if f.diesel_type.contains("Bool") {
                format!(
                    "let {} = data.get(\"{}\").and_then(Value::as_bool).unwrap_or(false);",
                    ident(&f.rust_name),
                    f.rust_name
                )
            } else if f.diesel_type.contains("Int8") {
                format!(
                    "let {} = data.get(\"{}\").and_then(Value::as_i64).unwrap_or(0);",
                    ident(&f.rust_name),
                    f.rust_name
                )
            } else if f.diesel_type.contains("Float8") {
                format!(
                    "let {} = data.get(\"{}\").and_then(Value::as_f64).unwrap_or(0.0);",
                    ident(&f.rust_name),
                    f.rust_name
                )
            } else if f.diesel_type.contains("Jsonb") {
                format!(
                    "let {} = data.get(\"{}\").cloned().unwrap_or(Value::Null);",
                    ident(&f.rust_name),
                    f.rust_name
                )
            } else {
                format!(
                    "let {} = data.get(\"{}\").and_then(Value::as_str).unwrap_or_default();",
                    ident(&f.rust_name),
                    f.rust_name
                )
            };
            out.push_str(&format!("    {extract}\n"));
        }
        out.push_str(&format!(
            "    let exists: bool = diesel::select(diesel::dsl::exists({table}::table.filter({table}::_id.eq(id)))).get_result(conn).await?;\n"
        ));
        out.push_str("    if exists {\n");
        out.push_str(&format!(
            "        diesel::update({table}::table.filter({table}::_id.eq(id))).set((\n"
        ));
        for f in &fields {
            out.push_str(&format!(
                "            {table}::{}.eq({}),\n",
                ident(&f.rust_name),
                ident(&f.rust_name)
            ));
        }
        out.push_str("        )).execute(conn).await?;\n");
        out.push_str("    } else {\n");
        out.push_str("        let domain = domain_fields_from_payload(data, id);\n");
        out.push_str(&format!(
            "        diesel::insert_into({table}::table).values(riverbase_core::domain_insert_values!(\n            {table},\n            domain,\n"
        ));
        for f in &fields {
            out.push_str(&format!(
                "            {table}::{}.eq({}),\n",
                ident(&f.rust_name),
                ident(&f.rust_name)
            ));
        }
        out.push_str("        )).execute(conn).await?;\n");
        out.push_str("    }\n    Ok(())\n}\n\n");

        out.push_str(&format!(
            "riverbase_core::pg_domain_entity! {{\n    {entity} {{\n"
        ));
        out.push_str(&format!("        schema: {table},\n"));
        out.push_str(&format!("        row: {row},\n"));
        out.push_str(&format!(
            "        resources: [\"{}\", \"{}\"],\n",
            spec.key, spec.table_name
        ));
        out.push_str(&format!("        source: \"{}\",\n", spec.table_name));
        out.push_str(&format!("        row_to_json: {table}_row_to_json,\n"));
        out.push_str(&format!(
            "        upsert_from_json: {table}_upsert_from_json,\n"
        ));
        out.push_str("        order: [\n");
        out.push_str("            \"_id\" => _id,\n");
        for f in &fields {
            out.push_str(&format!(
                "            \"{}\" => {},\n",
                f.rust_name,
                ident(&f.rust_name)
            ));
        }
        out.push_str("        ],\n    }\n}\n\n");
    }

    out.push_str("pub fn register_element_entities() -> Vec<std::sync::Arc<dyn riverbase_core::datastore::ErasedEntity>> {\n    vec![\n");
    for spec in specs {
        let entity = format!("{}Entity", pascal(&spec.table_name));
        out.push_str(&format!("        std::sync::Arc::new({entity}) as std::sync::Arc<dyn riverbase_core::datastore::ErasedEntity>,\n"));
    }
    out.push_str("    ]\n}\n");
    out
}

fn diesel_to_rust_type(diesel: &str) -> String {
    if diesel.starts_with("Nullable<") {
        let inner = diesel.trim_start_matches("Nullable<").trim_end_matches('>');
        return format!("Option<{}>", diesel_inner_to_rust(inner));
    }
    diesel_inner_to_rust(diesel)
}

fn diesel_inner_to_rust(diesel: &str) -> String {
    match diesel {
        "Text" => "String".to_string(),
        "Int8" => "i64".to_string(),
        "Float8" => "f64".to_string(),
        "Bool" => "bool".to_string(),
        "Uuid" => "uuid::Uuid".to_string(),
        "Timestamptz" => "chrono::DateTime<chrono::Utc>".to_string(),
        "Jsonb" => "serde_json::Value".to_string(),
        other => other.to_string(),
    }
}

fn generate_up_sql(specs: &[ElementSpec]) -> String {
    let mut out = String::from(
        "-- Generated element data tables.\n\nCREATE SCHEMA IF NOT EXISTS riverbase_form;\n\n",
    );
    for spec in specs {
        let fields = extract_fields(spec).unwrap_or_default();
        out.push_str(&format!(
            "CREATE TABLE IF NOT EXISTS riverbase_form.{} (\n",
            spec.table_name
        ));
        out.push_str(DOMAIN_FIELDS_DDL.trim());
        for f in &fields {
            out.push_str(&format!(",\n    {} {}", ident(&f.rust_name), f.sql_type));
        }
        out.push_str("\n);\n\n");
    }
    out
}

fn generate_down_sql(specs: &[ElementSpec]) -> String {
    let mut out = String::from("-- Drop generated element data tables.\n\n");
    for spec in specs.iter().rev() {
        out.push_str(&format!(
            "DROP TABLE IF EXISTS riverbase_form.{};\n",
            spec.table_name
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use std::collections::BTreeMap;

    #[test]
    fn maps_element_fields() {
        let mut field = BTreeMap::new();
        field.insert(
            "value".to_string(),
            FieldDef {
                field_type: "string".to_string(),
                format: None,
                required: true,
                validation: None,
                constraint: IndexMap::new(),
            },
        );
        field.insert(
            "count".to_string(),
            FieldDef {
                field_type: "integer".to_string(),
                format: None,
                required: false,
                validation: None,
                constraint: IndexMap::new(),
            },
        );
        let spec = ElementSpec {
            key: "TXT-0001".to_string(),
            title: "Test".to_string(),
            desc: None,
            table_name: "test_data".to_string(),
            validation: None,
            constraint: IndexMap::new(),
            field,
        };
        let fields = extract_fields(&spec).unwrap();
        assert_eq!(fields.len(), 2);
        assert!(fields
            .iter()
            .any(|f| { f.rust_name == "value" && f.sql_type.ends_with("NOT NULL") }));
        assert!(fields
            .iter()
            .any(|f| { f.rust_name == "count" && !f.sql_type.ends_with("NOT NULL") }));
    }
}
