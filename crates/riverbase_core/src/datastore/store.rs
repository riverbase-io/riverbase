use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::any::Any;
use std::collections::HashSet;

use super::dsl::{DataQuery, Expr, OrderSpec, Projection};
use super::error::DataResult;
use super::transaction::CommandUnitOfWork;

fn exactly_one_row<R>(rows: Vec<R>) -> DataResult<R> {
    match rows.len() {
        0 => Err(crate::errors::DAT_049.with_data("find_one query returned no rows")),
        1 => Ok(rows.into_iter().next().expect("single row exists")),
        _ => Err(crate::errors::DAT_050.with_data("find_one query returned multiple rows")),
    }
}

/// Shallow-merge a JSON object patch into an existing row.
pub fn merge_json(current: Value, patch: Value) -> DataResult<Value> {
    let mut merged = current
        .as_object()
        .cloned()
        .ok_or_else(|| crate::errors::DAT_043.with_data("merge requires object row"))?;
    let updates = patch
        .as_object()
        .cloned()
        .ok_or_else(|| crate::errors::DAT_044.with_data("merge requires object patch"))?;
    merged.extend(updates);
    Ok(Value::Object(merged))
}

/// Compile-time resource identity emitted by [`query_resource!`](crate::query_resource).
///
/// `NAME` is the wire resource; `SOURCE` is the datastore source key. Declare the string
/// once on the query resource; pass this type (or `K::NAME`) everywhere else.
pub trait ResourceKey {
    /// Canonical name.
    const NAME: &'static str;
    /// Datastore source key.
    const SOURCE: &'static str;
    /// Physical columns that must appear in the backing entity `order:` map.
    ///
    /// Built from sortable and `default_order` fields after binding. Hand-written
    /// impls keep the empty default and rely on spawn-time `QRY-126`.
    const SORT_COLUMNS: &'static [&'static str] = &[];
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// Resource name structure.
pub struct ResourceName(String);

impl ResourceName {
    /// Borrow as r.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ResourceName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[derive(Debug, Clone, Default)]
/// Resource registry structure.
pub struct ResourceRegistry {
    allowed: HashSet<String>,
}

impl ResourceRegistry {
    /// Construct a new value.
    pub fn new<I, S>(allowed: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed: allowed.into_iter().map(Into::into).collect(),
        }
    }

    /// Register.
    pub fn register(&mut self, resource: impl Into<String>) {
        self.allowed.insert(resource.into());
    }

    /// Registry containing a single compile-time resource key.
    pub fn of<K: ResourceKey>() -> Self {
        Self::new([K::NAME])
    }

    /// Parse.
    pub fn parse(&self, value: impl AsRef<str>) -> DataResult<ResourceName> {
        let value = value.as_ref();
        if self.allowed.contains(value) {
            Ok(ResourceName(value.to_string()))
        } else {
            Err(crate::errors::DAT_045.with_data(format!("resource {value} is not allowed")))
        }
    }
}

/// Persistence facade used by command and query engines.
#[async_trait]
pub trait DataStore: Send + Sync {
    /// Registry.
    fn registry(&self) -> &ResourceRegistry;

    /// Downcast to the concrete store. Report handlers use this to reach `PgDataStore`.
    fn as_any(&self) -> &dyn Any;

    /// Whether this source's executor applies [`DataQuery::policy_filter`] on every read path.
    fn enforces_policy_filter(&self, _source: &str) -> bool {
        false
    }

    /// Whether this backend is expected to provide a command-scoped transaction.
    ///
    /// When `true`, [`Self::begin_command_transaction`] must return a transactional
    /// [`CommandUnitOfWork`]. A transactional declaration with a non-transactional UoW is a
    /// misconfiguration and the command engine errors (`CMD-027`). Postgres stores that
    /// implement transactions return `true`. Intentionally non-transactional backends
    /// override to `false` and receive [`CommandUnitOfWork::NonTransactional`].
    fn supports_command_transactions(&self) -> bool {
        true
    }

    /// Begin a command-scoped unit of work for atomic command execution.
    ///
    /// PostgreSQL stores return a transactional UoW whose connection is shared with
    /// audit/idempotency stores via the same handle. Backends that set
    /// [`Self::supports_command_transactions`] to `false` return
    /// [`CommandUnitOfWork::NonTransactional`] and must document weaker consistency semantics.
    async fn begin_command_transaction(&self) -> DataResult<CommandUnitOfWork> {
        Ok(CommandUnitOfWork::non_transactional())
    }

    /// Map an API resource name to the storage source key used by [`DataQuery::source`].
    fn query_source(&self, resource: &ResourceName) -> String {
        resource.as_str().to_string()
    }

    /// Orderable physical columns for a storage source (`None` = skip debug sort coverage check).
    fn debug_orderable_columns(&self, _source: &str) -> Option<&'static [&'static str]> {
        None
    }

    /// Fetch a row as [`Value`] by resource name and id (string helper over [`Self::fetch`]).
    async fn state_fetch(
        &self,
        uow: Option<&CommandUnitOfWork>,
        resource: &str,
        id: &str,
    ) -> DataResult<Option<Value>> {
        let resource = self.registry().parse(resource)?;
        if let Some(uow) = uow {
            return self.fetch_in_unit_of_work(uow, resource, id).await;
        }
        self.fetch::<Value>(resource, id).await
    }

    /// Fetch within a command unit of work (memory staged tables; Postgres uses the pool).
    async fn fetch_in_unit_of_work<R>(
        &self,
        _uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
    ) -> DataResult<Option<R>>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        self.fetch(resource, id).await
    }

    /// Insert or replace a row addressed by `resource`/`id` (string helper over [`Self::upsert`]).
    async fn state_upsert(
        &self,
        uow: &CommandUnitOfWork,
        resource: &str,
        id: &str,
        data: Value,
    ) -> DataResult<()> {
        let resource = self.registry().parse(resource)?;
        self.upsert(uow, resource, id, data).await
    }

    /// Create a row; the payload must carry a string `id` (string helper over [`Self::state_upsert`]).
    async fn state_create(
        &self,
        uow: &CommandUnitOfWork,
        resource: &str,
        data: Value,
    ) -> DataResult<Value> {
        let id = data
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| crate::errors::DAT_046.with_data("create payload requires id"))?
            .to_string();
        self.state_upsert(uow, resource, &id, data.clone()).await?;
        Ok(data)
    }

    /// Query list.
    async fn query_list<R>(&self, query: &DataQuery) -> DataResult<Vec<R>>
    where
        R: DeserializeOwned + Send + Sync + 'static;

    /// List rows plus total matching count (`total` is `-1` when the store does not count).
    async fn query_list_with_total<R>(&self, query: &DataQuery) -> DataResult<(Vec<R>, i64)>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        let rows = self.query_list(query).await?;
        Ok((rows, -1))
    }

    /// Fetch a single row by id using a list query (default scans serialized `id`).
    async fn query_item<R>(&self, query: &DataQuery, id: &str) -> DataResult<R>
    where
        R: DeserializeOwned + Serialize + Send + Sync + 'static,
    {
        let rows = self.query_list::<R>(query).await?;
        for row in rows {
            let value = serde_json::to_value(&row)
                .map_err(|e| crate::errors::DAT_047.with_data(e.to_string()))?;
            if value.get("id").and_then(Value::as_str) == Some(id) {
                return Ok(row);
            }
        }
        Err(crate::errors::DAT_048.with_data(format!("resource {id}")))
    }

    /// Fetch.
    async fn fetch<R>(&self, resource: ResourceName, id: &str) -> DataResult<Option<R>>
    where
        R: DeserializeOwned + Send + Sync + 'static;

    /// Return exactly one row matching `filter`, or an error if none or many match.
    async fn find_one<R>(
        &self,
        resource: ResourceName,
        filter: Option<Expr>,
        order: Option<Vec<OrderSpec>>,
        projection: Option<Projection>,
    ) -> DataResult<R>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        exactly_one_row(self.find_all(resource, filter, order, projection).await?)
    }

    /// [`Self::find_one`] with a server-side [`DataQuery::policy_filter`] (not column-allowlisted).
    async fn find_one_with_policy<R>(
        &self,
        resource: ResourceName,
        filter: Option<Expr>,
        policy_filter: Option<Expr>,
        order: Option<Vec<OrderSpec>>,
        projection: Option<Projection>,
    ) -> DataResult<R>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        exactly_one_row(
            self.find_all_with_policy(resource, filter, policy_filter, order, projection)
                .await?,
        )
    }

    /// Return every row matching `filter`.
    async fn find_all<R>(
        &self,
        resource: ResourceName,
        filter: Option<Expr>,
        order: Option<Vec<OrderSpec>>,
        projection: Option<Projection>,
    ) -> DataResult<Vec<R>>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        self.query(resource, filter, order, projection).await
    }

    /// [`Self::find_all`] with a server-side [`DataQuery::policy_filter`] (not column-allowlisted).
    async fn find_all_with_policy<R>(
        &self,
        resource: ResourceName,
        filter: Option<Expr>,
        policy_filter: Option<Expr>,
        order: Option<Vec<OrderSpec>>,
        projection: Option<Projection>,
    ) -> DataResult<Vec<R>>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        self.query_with_policy(resource, filter, policy_filter, order, projection)
            .await
    }

    /// Run a list query built from resource, filter, order, and projection.
    async fn query<R>(
        &self,
        resource: ResourceName,
        filter: Option<Expr>,
        order: Option<Vec<OrderSpec>>,
        projection: Option<Projection>,
    ) -> DataResult<Vec<R>>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        self.query_with_policy(resource, filter, None, order, projection)
            .await
    }

    /// [`Self::query`] with a server-side [`DataQuery::policy_filter`].
    async fn query_with_policy<R>(
        &self,
        resource: ResourceName,
        filter: Option<Expr>,
        policy_filter: Option<Expr>,
        order: Option<Vec<OrderSpec>>,
        projection: Option<Projection>,
    ) -> DataResult<Vec<R>>
    where
        R: DeserializeOwned + Send + Sync + 'static,
    {
        let mut query = DataQuery::list(self.query_source(&resource));
        query.filter = filter;
        query.policy_filter = policy_filter;
        query.order = order.unwrap_or_default();
        query.projection = projection;
        self.query_list(&query).await
    }

    /// Insert a row that already carries a string `id`.
    async fn insert<R>(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        data: R,
    ) -> DataResult<R>
    where
        R: Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        let value = serde_json::to_value(&data)
            .map_err(|e| crate::errors::DAT_051.with_data(e.to_string()))?;
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| crate::errors::DAT_052.with_data("insert requires id field"))?
            .to_string();
        self.upsert(uow, resource, &id, value).await?;
        Ok(data)
    }

    /// Upsert.
    async fn upsert<R>(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
        data: R,
    ) -> DataResult<()>
    where
        R: Serialize + Send + Sync + 'static;

    /// Update only when the persisted `_etag` still matches the aggregate snapshot.
    async fn compare_and_swap<R>(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
        expected_etag: &str,
        data: R,
    ) -> DataResult<()>
    where
        R: Serialize + Send + Sync + 'static,
    {
        let current = self
            .fetch::<Value>(resource.clone(), id)
            .await?
            .ok_or_else(|| {
                crate::errors::DAT_053.with_data(format!("{}:{id}", resource.as_str()))
            })?;
        if current.get("_etag").and_then(Value::as_str) != Some(expected_etag) {
            return Err(crate::errors::RDT_301
                .with_data(format!("stale _etag for {}:{id}", resource.as_str())));
        }
        self.upsert(uow, resource, id, data).await
    }

    /// Merge `data` onto the persisted row and write the result.
    async fn update<R>(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
        data: R,
    ) -> DataResult<R>
    where
        R: Serialize + DeserializeOwned + Send + Sync + 'static,
    {
        let patch = serde_json::to_value(&data)
            .map_err(|e| crate::errors::DAT_054.with_data(e.to_string()))?;
        let current = self
            .fetch::<Value>(resource.clone(), id)
            .await?
            .ok_or_else(|| {
                crate::errors::DAT_055.with_data(format!("{}:{id}", resource.as_str()))
            })?;
        let merged = merge_json(current, patch)?;
        self.upsert(uow, resource, id, merged.clone()).await?;
        serde_json::from_value(merged).map_err(|e| crate::errors::DAT_056.with_data(e.to_string()))
    }

    /// Physically remove a row (ADR 005 store primitive).
    async fn remove(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
    ) -> DataResult<()>;

    /// Soft-delete a row by setting `_deleted` ([ADR 005] primitive).
    async fn invalidate(
        &self,
        uow: &CommandUnitOfWork,
        resource: ResourceName,
        id: &str,
    ) -> DataResult<()>;
}
