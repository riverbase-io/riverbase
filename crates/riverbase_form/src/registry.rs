//! Spec registries with optional Postgres backing and an in-process read-through cache.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use riverbase_core::base::RiverbaseResult;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::code::validate_key;
use crate::config::{load_documents_from_dir, load_elements_from_dir, load_forms_from_dir};
use crate::spec::{DocumentSpec, ElementSpec, FormSpec};

pub struct SpecRegistry<T> {
    kind: &'static str,
    store: RwLock<HashMap<String, T>>,
}

impl<T: Clone> SpecRegistry<T> {
    pub fn new(kind: &'static str) -> Self {
        Self {
            kind,
            store: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, key: String, spec: T) -> RiverbaseResult<()> {
        validate_key(&key)?;
        let mut store = self.store.write().map_err(|_| {
            crate::errors::FRM_030.with_data(format!("{} registry poisoned", self.kind))
        })?;
        if store.contains_key(&key) {
            return Err(crate::errors::FRM_031.with_data(key));
        }
        store.insert(key, spec);
        Ok(())
    }

    /// Insert or replace a cached spec (used by Postgres reload / upsert).
    pub fn upsert(&self, key: String, spec: T) -> RiverbaseResult<()> {
        validate_key(&key)?;
        let mut store = self.store.write().map_err(|_| {
            crate::errors::FRM_030.with_data(format!("{} registry poisoned", self.kind))
        })?;
        store.insert(key, spec);
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<T> {
        self.store.read().ok()?.get(key).cloned()
    }

    pub fn keys(&self) -> Vec<String> {
        self.store
            .read()
            .map(|s| s.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn values(&self) -> Vec<T> {
        self.store
            .read()
            .map(|s| s.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.store.read().map(|s| s.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        if let Ok(mut store) = self.store.write() {
            store.clear();
        }
    }

    fn replace_all(&self, specs: HashMap<String, T>) -> RiverbaseResult<()> {
        let mut store = self.store.write().map_err(|_| {
            crate::errors::FRM_030.with_data(format!("{} registry poisoned", self.kind))
        })?;
        *store = specs;
        Ok(())
    }
}

pub type ElementRegistry = SpecRegistry<ElementSpec>;
pub type FormRegistry = SpecRegistry<FormSpec>;
pub type DocumentRegistry = SpecRegistry<DocumentSpec>;

static ELEMENT_REGISTRY: OnceLock<ElementRegistry> = OnceLock::new();
static FORM_REGISTRY: OnceLock<FormRegistry> = OnceLock::new();
static DOCUMENT_REGISTRY: OnceLock<DocumentRegistry> = OnceLock::new();
static REGISTRY_POOL: OnceLock<RwLock<Option<Arc<riverbase_core::datastore::PgPool>>>> =
    OnceLock::new();

fn registry_pool_slot() -> &'static RwLock<Option<Arc<riverbase_core::datastore::PgPool>>> {
    REGISTRY_POOL.get_or_init(|| RwLock::new(None))
}

pub fn element_registry() -> &'static ElementRegistry {
    ELEMENT_REGISTRY.get_or_init(|| ElementRegistry::new("element"))
}

pub fn form_registry() -> &'static FormRegistry {
    FORM_REGISTRY.get_or_init(|| FormRegistry::new("form"))
}

pub fn document_registry() -> &'static DocumentRegistry {
    DOCUMENT_REGISTRY.get_or_init(|| DocumentRegistry::new("document"))
}

/// Bind the process-wide registries to a Postgres pool (primary load path).
pub fn bind_registry_pool(pool: Arc<riverbase_core::datastore::PgPool>) {
    if let Ok(mut guard) = registry_pool_slot().write() {
        *guard = Some(pool);
    }
}

pub fn registry_pool() -> Option<Arc<riverbase_core::datastore::PgPool>> {
    registry_pool_slot().read().ok().and_then(|g| g.clone())
}

pub fn register_elements_from_dir(dir: &Path) -> RiverbaseResult<usize> {
    let specs = load_elements_from_dir(dir)?;
    let reg = element_registry();
    for spec in specs {
        if reg.get(&spec.key).is_some() {
            reg.upsert(spec.key.clone(), spec)?;
        } else {
            reg.register(spec.key.clone(), spec)?;
        }
    }
    Ok(reg.len())
}

pub fn register_forms_from_dir(dir: &Path) -> RiverbaseResult<usize> {
    let specs = load_forms_from_dir(dir)?;
    let reg = form_registry();
    for spec in specs {
        if reg.get(&spec.key).is_some() {
            reg.upsert(spec.key.clone(), spec)?;
        } else {
            reg.register(spec.key.clone(), spec)?;
        }
    }
    Ok(reg.len())
}

pub fn register_documents_from_dir(dir: &Path) -> RiverbaseResult<usize> {
    let specs = load_documents_from_dir(dir)?;
    let reg = document_registry();
    for spec in specs {
        if reg.get(&spec.key).is_some() {
            reg.upsert(spec.key.clone(), spec)?;
        } else {
            reg.register(spec.key.clone(), spec)?;
        }
    }
    Ok(reg.len())
}

pub fn register_all_from_base(base: &Path) -> RiverbaseResult<(usize, usize, usize)> {
    let elements = register_elements_from_dir(&base.join("elements"))?;
    let forms = register_forms_from_dir(&base.join("forms"))?;
    let documents = register_documents_from_dir(&base.join("documents"))?;
    Ok((elements, forms, documents))
}

/// Seed Postgres from HCL/YAML dirs, then reload the in-process cache from the DB.
#[cfg(feature = "postgres")]
pub async fn seed_registries_from_dir(
    pool: &riverbase_core::datastore::PgPool,
    base: &Path,
) -> RiverbaseResult<(usize, usize, usize)> {
    ensure_spec_columns(pool).await?;
    let elements = load_elements_from_dir(&base.join("elements"))?;
    let forms = load_forms_from_dir(&base.join("forms"))?;
    let documents = load_documents_from_dir(&base.join("documents"))?;

    for spec in &elements {
        upsert_element_spec(pool, spec).await?;
    }
    for spec in &forms {
        upsert_form_spec(pool, spec).await?;
    }
    for spec in &documents {
        upsert_document_spec(pool, spec).await?;
    }

    reload_registries_from_postgres(pool).await
}

/// Replace the process-wide caches from Postgres registry tables.
#[cfg(feature = "postgres")]
pub async fn reload_registries_from_postgres(
    pool: &riverbase_core::datastore::PgPool,
) -> RiverbaseResult<(usize, usize, usize)> {
    ensure_spec_columns(pool).await?;
    let elements = load_element_specs(pool).await?;
    let forms = load_form_specs(pool).await?;
    let documents = load_document_specs(pool).await?;

    let mut element_map = HashMap::new();
    for spec in elements {
        element_map.insert(spec.key.clone(), spec);
    }
    let mut form_map = HashMap::new();
    for spec in forms {
        form_map.insert(spec.key.clone(), spec);
    }
    let mut document_map = HashMap::new();
    for spec in documents {
        document_map.insert(spec.key.clone(), spec);
    }

    element_registry().replace_all(element_map)?;
    form_registry().replace_all(form_map)?;
    document_registry().replace_all(document_map)?;
    Ok((
        element_registry().len(),
        form_registry().len(),
        document_registry().len(),
    ))
}

/// Bind pool and reload caches (primary runtime entrypoint).
#[cfg(feature = "postgres")]
pub async fn load_registries_from_postgres(
    pool: Arc<riverbase_core::datastore::PgPool>,
) -> RiverbaseResult<(usize, usize, usize)> {
    bind_registry_pool(pool.clone());
    reload_registries_from_postgres(pool.as_ref()).await
}

#[cfg(feature = "postgres")]
const ENSURE_SPEC_COLUMNS_SQL: &str = r#"
ALTER TABLE riverbase_form.form_registry
    ADD COLUMN IF NOT EXISTS spec JSONB NOT NULL DEFAULT '{}'::jsonb;
ALTER TABLE riverbase_form.template_registry
    ADD COLUMN IF NOT EXISTS spec JSONB NOT NULL DEFAULT '{}'::jsonb;
"#;

#[cfg(feature = "postgres")]
async fn ensure_spec_columns(pool: &riverbase_core::datastore::PgPool) -> RiverbaseResult<()> {
    use diesel::sql_query;
    use diesel_async::RunQueryDsl;

    let mut conn = pool
        .get()
        .await
        .map_err(|e| crate::errors::FRM_040.with_data(e.to_string()))?;
    for stmt in ENSURE_SPEC_COLUMNS_SQL
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sql_query(stmt)
            .execute(&mut *conn)
            .await
            .map_err(|e| crate::errors::FRM_041.with_data(e.to_string()))?;
    }
    Ok(())
}

#[cfg(feature = "postgres")]
async fn load_element_specs(
    pool: &riverbase_core::datastore::PgPool,
) -> RiverbaseResult<Vec<ElementSpec>> {
    use diesel::prelude::*;
    use diesel_async::RunQueryDsl;

    use crate::postgres::entity::{element_registry_row_to_json, ElementRegistryRow};
    use crate::postgres::schema::element_registry;

    let mut conn = pool
        .get()
        .await
        .map_err(|e| crate::errors::FRM_040.with_data(e.to_string()))?;
    let rows: Vec<ElementRegistryRow> = element_registry::table
        .filter(element_registry::_deleted.is_null())
        .select(ElementRegistryRow::as_select())
        .load(&mut conn)
        .await
        .map_err(|e| {
            crate::errors::FRM_042.with_data(format!("Failed to load element_registry: {e}"))
        })?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let key = row.element_key.clone();
        let schema = row.element_schema.clone();
        if let Ok(mut spec) = serde_json::from_value::<ElementSpec>(schema.clone()) {
            if spec.key.is_empty() {
                spec.key = key;
            }
            if spec.title.is_empty() {
                spec.title = row
                    .element_label
                    .clone()
                    .unwrap_or_else(|| spec.key.clone());
            }
            out.push(spec);
            continue;
        }
        let mut value = element_registry_row_to_json(row);
        if let Some(obj) = schema.as_object() {
            for (k, v) in obj {
                value
                    .as_object_mut()
                    .expect("json object")
                    .insert(k.clone(), v.clone());
            }
        }
        value
            .as_object_mut()
            .expect("json object")
            .insert("key".into(), json!(key));
        if let Ok(spec) = serde_json::from_value::<ElementSpec>(value) {
            out.push(spec);
        }
    }
    Ok(out)
}

#[cfg(feature = "postgres")]
async fn load_form_specs(pool: &riverbase_core::datastore::PgPool) -> RiverbaseResult<Vec<FormSpec>> {
    use diesel::sql_types::{Jsonb, Nullable, Text};
    use diesel::{sql_query, QueryableByName};
    use diesel_async::RunQueryDsl;

    #[derive(QueryableByName)]
    struct FormRow {
        #[diesel(sql_type = Text)]
        form_key: String,
        #[diesel(sql_type = Text)]
        title: String,
        #[diesel(sql_type = Nullable<Text>)]
        desc: Option<String>,
        #[diesel(sql_type = Jsonb)]
        spec: Value,
    }

    let mut conn = pool
        .get()
        .await
        .map_err(|e| crate::errors::FRM_040.with_data(e.to_string()))?;
    let rows: Vec<FormRow> = sql_query(
        r#"
        SELECT form_key, title, "desc", COALESCE(spec, '{}'::jsonb) AS spec
        FROM riverbase_form.form_registry
        WHERE _deleted IS NULL
        "#,
    )
    .load(&mut conn)
    .await
    .map_err(|e| crate::errors::FRM_043.with_data(format!("Failed to load form_registry: {e}")))?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if row.spec.as_object().is_some_and(|o| !o.is_empty()) {
            if let Ok(mut spec) = serde_json::from_value::<FormSpec>(row.spec) {
                if spec.key.is_empty() {
                    spec.key = row.form_key;
                }
                if spec.title.is_empty() {
                    spec.title = row.title;
                }
                if spec.desc.is_none() {
                    spec.desc = row.desc;
                }
                out.push(spec);
                continue;
            }
        }
        out.push(FormSpec {
            key: row.form_key,
            title: row.title,
            desc: row.desc,
            header: None,
            footer: None,
            element: Default::default(),
            group: Default::default(),
            constraint: Default::default(),
        });
    }
    Ok(out)
}

#[cfg(feature = "postgres")]
async fn load_document_specs(
    pool: &riverbase_core::datastore::PgPool,
) -> RiverbaseResult<Vec<DocumentSpec>> {
    use diesel::sql_types::{Integer, Jsonb, Nullable, Text};
    use diesel::{sql_query, QueryableByName};
    use diesel_async::RunQueryDsl;

    #[derive(QueryableByName)]
    struct TemplateRow {
        #[diesel(sql_type = Text)]
        template_key: String,
        #[diesel(sql_type = Text)]
        template_name: String,
        #[diesel(sql_type = Nullable<Text>)]
        desc: Option<String>,
        #[diesel(sql_type = Integer)]
        version: i32,
        #[diesel(sql_type = Nullable<Text>)]
        types: Option<String>,
        #[diesel(sql_type = Jsonb)]
        spec: Value,
    }

    let mut conn = pool
        .get()
        .await
        .map_err(|e| crate::errors::FRM_040.with_data(e.to_string()))?;
    let rows: Vec<TemplateRow> = sql_query(
        r#"
        SELECT template_key, template_name, "desc", version, types,
               COALESCE(spec, '{}'::jsonb) AS spec
        FROM riverbase_form.template_registry
        WHERE _deleted IS NULL
        "#,
    )
    .load(&mut conn)
    .await
    .map_err(|e| {
        crate::errors::FRM_044.with_data(format!("Failed to load template_registry: {e}"))
    })?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if row.spec.as_object().is_some_and(|o| !o.is_empty()) {
            if let Ok(mut spec) = serde_json::from_value::<DocumentSpec>(row.spec) {
                if spec.key.is_empty() {
                    spec.key = row.template_key;
                }
                if spec.title.is_empty() {
                    spec.title = row.template_name;
                }
                if spec.desc.is_none() {
                    spec.desc = row.desc;
                }
                if spec.version.is_none() {
                    spec.version = Some(row.version);
                }
                if spec.types.is_none() {
                    spec.types = row.types;
                }
                out.push(spec);
                continue;
            }
        }
        out.push(DocumentSpec {
            key: row.template_key,
            title: row.template_name,
            desc: row.desc,
            version: Some(row.version),
            types: row.types,
            nodes: Vec::new(),
        });
    }
    Ok(out)
}

#[cfg(feature = "postgres")]
async fn upsert_element_spec(
    pool: &riverbase_core::datastore::PgPool,
    spec: &ElementSpec,
) -> RiverbaseResult<()> {
    use diesel::prelude::*;
    use diesel_async::RunQueryDsl;

    use crate::postgres::entity::element_registry_upsert_from_json;
    use crate::postgres::schema::element_registry;

    let mut conn = pool
        .get()
        .await
        .map_err(|e| crate::errors::FRM_040.with_data(e.to_string()))?;
    let existing: Option<Uuid> = element_registry::table
        .filter(element_registry::element_key.eq(&spec.key))
        .filter(element_registry::_deleted.is_null())
        .select(element_registry::_id)
        .first(&mut conn)
        .await
        .optional()
        .map_err(|e| {
            crate::errors::FRM_045.with_data(format!("element_registry lookup failed: {e}"))
        })?;
    let id = existing.unwrap_or_else(Uuid::new_v4);
    let payload = json!({
        "element_key": spec.key,
        "element_label": spec.title,
        "element_schema": serde_json::to_value(spec).unwrap_or_else(|_| json!({})),
    });
    element_registry_upsert_from_json(&mut conn, id, &payload)
        .await
        .map_err(|e| {
            crate::errors::FRM_046.with_data(format!("element_registry upsert failed: {e}"))
        })?;
    Ok(())
}

#[cfg(feature = "postgres")]
async fn upsert_form_spec(
    pool: &riverbase_core::datastore::PgPool,
    spec: &FormSpec,
) -> RiverbaseResult<()> {
    use diesel::sql_types::{Jsonb, Nullable, Text, Uuid as SqlUuid};
    use diesel::{sql_query, OptionalExtension};
    use diesel_async::RunQueryDsl;

    let mut conn = pool
        .get()
        .await
        .map_err(|e| crate::errors::FRM_040.with_data(e.to_string()))?;
    #[derive(diesel::QueryableByName)]
    struct IdRow {
        #[diesel(sql_type = SqlUuid)]
        id: Uuid,
    }
    let existing: Option<IdRow> = sql_query(
        r#"
        SELECT _id AS id FROM riverbase_form.form_registry
        WHERE form_key = $1 AND _deleted IS NULL
        "#,
    )
    .bind::<Text, _>(&spec.key)
    .get_result(&mut conn)
    .await
    .optional()
    .map_err(|e| crate::errors::FRM_047.with_data(format!("form_registry lookup failed: {e}")))?;
    let id = existing.map(|r| r.id).unwrap_or_else(Uuid::new_v4);
    let payload = serde_json::to_value(spec).unwrap_or_else(|_| json!({}));
    sql_query(
        r#"
        INSERT INTO riverbase_form.form_registry (_id, form_key, title, "desc", spec)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (_id) DO UPDATE SET
            form_key = EXCLUDED.form_key,
            title = EXCLUDED.title,
            "desc" = EXCLUDED."desc",
            spec = EXCLUDED.spec,
            _updated = NOW()
        "#,
    )
    .bind::<SqlUuid, _>(id)
    .bind::<Text, _>(&spec.key)
    .bind::<Text, _>(&spec.title)
    .bind::<Nullable<Text>, _>(spec.desc.as_deref())
    .bind::<Jsonb, _>(&payload)
    .execute(&mut conn)
    .await
    .map_err(|e| crate::errors::FRM_048.with_data(format!("form_registry upsert failed: {e}")))?;
    Ok(())
}

#[cfg(feature = "postgres")]
async fn upsert_document_spec(
    pool: &riverbase_core::datastore::PgPool,
    spec: &DocumentSpec,
) -> RiverbaseResult<()> {
    use diesel::sql_types::{Integer, Jsonb, Nullable, Text, Uuid as SqlUuid};
    use diesel::{sql_query, OptionalExtension};
    use diesel_async::RunQueryDsl;

    let mut conn = pool
        .get()
        .await
        .map_err(|e| crate::errors::FRM_040.with_data(e.to_string()))?;
    #[derive(diesel::QueryableByName)]
    struct IdRow {
        #[diesel(sql_type = SqlUuid)]
        id: Uuid,
    }
    let existing: Option<IdRow> = sql_query(
        r#"
        SELECT _id AS id FROM riverbase_form.template_registry
        WHERE template_key = $1 AND _deleted IS NULL
        "#,
    )
    .bind::<Text, _>(&spec.key)
    .get_result(&mut conn)
    .await
    .optional()
    .map_err(|e| {
        crate::errors::FRM_049.with_data(format!("template_registry lookup failed: {e}"))
    })?;
    let id = existing.map(|r| r.id).unwrap_or_else(Uuid::new_v4);
    let payload = serde_json::to_value(spec).unwrap_or_else(|_| json!({}));
    let version = spec.version.unwrap_or(1);
    sql_query(
        r#"
        INSERT INTO riverbase_form.template_registry
            (_id, template_key, template_name, "desc", version, types, spec)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (_id) DO UPDATE SET
            template_key = EXCLUDED.template_key,
            template_name = EXCLUDED.template_name,
            "desc" = EXCLUDED."desc",
            version = EXCLUDED.version,
            types = EXCLUDED.types,
            spec = EXCLUDED.spec,
            _updated = NOW()
        "#,
    )
    .bind::<SqlUuid, _>(id)
    .bind::<Text, _>(&spec.key)
    .bind::<Text, _>(&spec.title)
    .bind::<Nullable<Text>, _>(spec.desc.as_deref())
    .bind::<Integer, _>(version)
    .bind::<Nullable<Text>, _>(spec.types.as_deref())
    .bind::<Jsonb, _>(&payload)
    .execute(&mut conn)
    .await
    .map_err(|e| {
        crate::errors::FRM_081.with_data(format!("template_registry upsert failed: {e}"))
    })?;
    Ok(())
}
