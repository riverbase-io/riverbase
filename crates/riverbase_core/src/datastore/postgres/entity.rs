//! Postgres entity trait and registry dispatch (`pg_domain_entity!` lives in `crate::r#macro`).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use diesel_async::AsyncPgConnection;
use serde::de::DeserializeOwned;
use serde_json::Value;
use uuid::Uuid;

use super::dbpool::PgPool;
use crate::datastore::dsl::DataQuery;
use crate::datastore::error::DataResult;
use crate::datastore::store::{DataStore, ResourceName, ResourceRegistry};
use crate::datastore::transaction::{CommandTransaction, CommandUnitOfWork};

/// Parse `_etag` from a fetched row without row-level locking (weak concurrency opt-out).
pub fn lock_version_from_row(row: Option<Value>) -> DataResult<Option<Uuid>> {
    Ok(row
        .and_then(|row| row.get("_etag").and_then(Value::as_str).map(str::to_string))
        .and_then(|etag| Uuid::parse_str(&etag).ok()))
}

/// Append-only / read-only entities with no optimistic concurrency.
pub fn lock_version_none() -> DataResult<Option<Uuid>> {
    Ok(None)
}

/// Type-erased Postgres entity for registry dispatch (JSON in/out).
///
/// Prefer [`pg_domain_entity!`](crate::pg_domain_entity) for ordinary tables. Manual impls must
/// still provide the security- and correctness-critical surface:
///
/// | Method | Why it matters |
/// | --- | --- |
/// | [`lock_version`](Self::lock_version) | Optimistic concurrency ([DAT-04]) |
/// | [`remove`](Self::remove) / [`invalidate`](Self::invalidate) | Store delete primitives (ADR 005) |
/// | [`enforces_policy_filter`](Self::enforces_policy_filter) | Row-level auth ([SEC-02]) |
/// | [`debug_orderable_columns`](Self::debug_orderable_columns) | Sortable field coverage ([DX-04]) |
///
/// Remaining manual impls (enrichment facades, read-only audit projections, bespoke SQL) are
/// tracked in `docs/04-reference/12-manual-erased-entity.md`. Do not add new manuals without a
/// recorded reason.
#[async_trait]
pub trait ErasedEntity: Send + Sync {
    /// Resources.
    fn resources(&self) -> &[&'static str];
    /// Source.
    fn source(&self) -> &'static str;

    /// Must return `true` when this entity applies [`DataQuery::policy_filter`] ([SEC-02]).
    /// No default — manual impls cannot silently skip enforcement ([DX-03]).
    fn enforces_policy_filter(&self) -> bool;

    /// Physical column names accepted in [`DataQuery::order`] (debug spawn validation only).
    /// No default — empty slice must be stated explicitly ([DX-03]/DX-04]).
    fn debug_orderable_columns(&self) -> &'static [&'static str];

    /// Query list.
    async fn query_list(
        &self,
        conn: &mut AsyncPgConnection,
        query: &DataQuery,
    ) -> DataResult<(Vec<Value>, i64)>;
    /// Fetch.
    async fn fetch(&self, conn: &mut AsyncPgConnection, id: &str) -> DataResult<Option<Value>>;
    /// Lock version.
    async fn lock_version(
        &self,
        conn: &mut AsyncPgConnection,
        id: &str,
    ) -> DataResult<Option<Uuid>>;
    /// Upsert.
    async fn upsert(&self, conn: &mut AsyncPgConnection, id: &str, data: Value) -> DataResult<()>;
    /// Remove.
    async fn remove(&self, conn: &mut AsyncPgConnection, id: &str) -> DataResult<()>;
    /// Invalidate.
    async fn invalidate(&self, conn: &mut AsyncPgConnection, id: &str) -> DataResult<()>;
}

/// Append-only entity (log tables): insert-only, no domain fields.
#[async_trait]
pub trait AppendOnlyEntity: Send + Sync {
    /// Table name.
    fn table_name(&self) -> &'static str;
    /// Append.
    async fn append(&self, conn: &mut AsyncPgConnection, record: Value) -> DataResult<()>;
}

/// Parse uuid id.
pub fn parse_uuid_id(id: &str) -> DataResult<Uuid> {
    Uuid::parse_str(id).map_err(|e| crate::errors::DAT_033.with_data(e.to_string()))
}

async fn pool_conn(pool: &PgPool) -> DataResult<super::transaction::PgConnectionGuard> {
    super::transaction::pool_connection(pool).await
}

/// Registry-backed Postgres [`DataStore`] (automodel equivalent).
#[derive(Clone)]
pub struct PgDataStore {
    dbpool: PgPool,
    registry: ResourceRegistry,
    entities: HashMap<String, Arc<dyn ErasedEntity>>,
}

impl PgDataStore {
    /// Construct a new value.
    pub fn new(dbpool: PgPool) -> Self {
        Self::with_registry(dbpool, ResourceRegistry::default())
    }

    /// Set registry and return self.
    pub fn with_registry(dbpool: PgPool, registry: ResourceRegistry) -> Self {
        Self {
            dbpool,
            registry,
            entities: HashMap::new(),
        }
    }

    /// Dbpool.
    pub fn dbpool(&self) -> &PgPool {
        &self.dbpool
    }

    /// Register entity.
    pub fn register_entity(mut self, entity: Arc<dyn ErasedEntity>) -> Self {
        for resource in entity.resources() {
            self.entities.insert(resource.to_string(), entity.clone());
            self.registry.register(*resource);
        }
        self.entities.insert(entity.source().to_string(), entity);
        self
    }

    fn entity_for_resource(&self, resource: &ResourceName) -> DataResult<Arc<dyn ErasedEntity>> {
        self.entities
            .get(resource.as_str())
            .cloned()
            .ok_or_else(|| {
                crate::errors::DAT_034
                    .with_data(format!("resource {} is not registered", resource.as_str()))
            })
    }

    /// Unsupported.
    pub fn unsupported(
        operation: &str,
        detail: impl std::fmt::Display,
    ) -> crate::base::RiverbaseError {
        crate::errors::DAT_035.with_data(format!("{operation} is not implemented: {detail}"))
    }
}

#[async_trait]
impl DataStore for PgDataStore {
    fn registry(&self) -> &ResourceRegistry {
        &self.registry
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn enforces_policy_filter(&self, source: &str) -> bool {
        self.entities
            .get(source)
            .is_some_and(|entity| entity.enforces_policy_filter())
    }

    async fn begin_command_transaction(&self) -> DataResult<CommandUnitOfWork> {
        super::transaction::PgTransaction::begin(&self.dbpool)
            .await
            .map(CommandTransaction::from)
            .map(CommandUnitOfWork::transactional)
    }

    fn query_source(&self, resource: &ResourceName) -> String {
        self.entities
            .get(resource.as_str())
            .map(|e| e.source().to_string())
            .unwrap_or_else(|| resource.as_str().to_string())
    }

    fn debug_orderable_columns(&self, source: &str) -> Option<&'static [&'static str]> {
        self.entities
            .get(source)
            .map(|entity| entity.debug_orderable_columns())
    }

    async fn query_list<R>(&self, query: &DataQuery) -> DataResult<Vec<R>>
    where
        R: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        let (rows, _) = self.query_list_with_total(query).await?;
        Ok(rows)
    }

    async fn query_list_with_total<R>(&self, query: &DataQuery) -> DataResult<(Vec<R>, i64)>
    where
        R: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        let entity = self.entities.get(&query.source).cloned().ok_or_else(|| {
            Self::unsupported("query_list_with_total", format!("source {}", query.source))
        })?;
        let mut conn = pool_conn(&self.dbpool).await?;
        let (values, total) = entity.query_list(&mut conn, query).await?;
        let rows = values
            .into_iter()
            .map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| crate::errors::DAT_036.with_data(e.to_string()))
            })
            .collect::<DataResult<Vec<R>>>()?;
        Ok((rows, total))
    }

    async fn query_item<R>(&self, query: &DataQuery, _id: &str) -> DataResult<R>
    where
        R: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        let (rows, _) = self.query_list_with_total::<R>(query).await?;
        rows.into_iter()
            .next()
            .ok_or_else(|| crate::errors::DAT_037.with_data("query item returned no rows"))
    }

    async fn fetch<R>(&self, resource: ResourceName, id: &str) -> DataResult<Option<R>>
    where
        R: serde::de::DeserializeOwned + Send + Sync + 'static,
    {
        let entity = self.entity_for_resource(&resource)?;
        let mut conn = pool_conn(&self.dbpool).await?;
        let value = entity.fetch(&mut conn, id).await?;
        value
            .map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| crate::errors::DAT_038.with_data(e.to_string()))
            })
            .transpose()
    }

    /// Read through the command transaction so uncommitted upserts are visible.
    async fn fetch_in_unit_of_work<R>(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
    ) -> DataResult<Option<R>>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        let entity = self.entity_for_resource(&resource)?;
        let mut conn = uow.postgres_connection().await?;
        let value = entity.fetch(&mut conn, id).await?;
        value
            .map(|v| {
                serde_json::from_value(v)
                    .map_err(|e| crate::errors::DAT_039.with_data(e.to_string()))
            })
            .transpose()
    }

    async fn upsert<R>(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
        data: R,
    ) -> DataResult<()>
    where
        R: serde::Serialize + Send + Sync + 'static,
    {
        let entity = self.entity_for_resource(&resource)?;
        let data = serde_json::to_value(data)
            .map_err(|e| crate::errors::DAT_040.with_data(e.to_string()))?;
        let mut conn = uow.postgres_connection().await?;
        entity.upsert(&mut conn, id, data).await
    }

    async fn compare_and_swap<R>(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
        expected_etag: &str,
        data: R,
    ) -> DataResult<()>
    where
        R: serde::Serialize + Send + Sync + 'static,
    {
        let entity = self.entity_for_resource(&resource)?;
        // Non-UUID / stale If-Match values are precondition failures (412 RDT-300),
        // not unsupported-argument 422s.
        let expected_etag = Uuid::parse_str(expected_etag).ok();
        let data = serde_json::to_value(data)
            .map_err(|e| crate::errors::DAT_041.with_data(e.to_string()))?;
        let mut conn = uow.postgres_connection().await?;
        let actual_etag = entity.lock_version(&mut conn, id).await?.ok_or_else(|| {
            crate::errors::DAT_042.with_data(format!("{}:{id}", resource.as_str()))
        })?;
        if Some(actual_etag) != expected_etag {
            return Err(crate::errors::RDT_300
                .with_data(format!("stale _etag for {}:{id}", resource.as_str())));
        }
        entity.upsert(&mut conn, id, data).await
    }

    async fn remove(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
    ) -> DataResult<()> {
        let entity = self.entity_for_resource(&resource)?;
        let mut conn = uow.postgres_connection().await?;
        entity.remove(&mut conn, id).await
    }

    async fn invalidate(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
    ) -> DataResult<()> {
        let entity = self.entity_for_resource(&resource)?;
        let mut conn = uow.postgres_connection().await?;
        entity.invalidate(&mut conn, id).await
    }
}

/// Run an append-only insert on the command transaction connection.
pub async fn append_record(
    uow: &CommandUnitOfWork,
    entity: Arc<dyn AppendOnlyEntity>,
    record: Value,
) -> DataResult<()> {
    let mut conn = uow.postgres_connection().await?;
    entity.append(&mut conn, record).await
}
