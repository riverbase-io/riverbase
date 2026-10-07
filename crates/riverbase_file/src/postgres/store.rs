use std::sync::Arc;

use async_trait::async_trait;
use riverbase_core::datastore::{DataStore, PgDataStore, PgPool, ResourceName, ResourceRegistry};
use uuid::Uuid;

use super::entity::MediaEntity;
use crate::metadata::MediaMetadataStore;
use crate::model::{MediaEntry, MediaQuery};
use riverbase_core::base::RiverbaseResult;

#[derive(Clone)]
pub struct PostgresMediaMetadataStore {
    store: PgDataStore,
}

impl PostgresMediaMetadataStore {
    pub fn new(dbpool: PgPool) -> Self {
        Self {
            store: PgDataStore::with_registry(dbpool, ResourceRegistry::new(["media_entry"]))
                .register_entity(Arc::new(MediaEntity)),
        }
    }

    fn resource(&self) -> RiverbaseResult<ResourceName> {
        self.store.registry().parse("media_entry")
    }
}

#[async_trait]
impl MediaMetadataStore for PostgresMediaMetadataStore {
    async fn upsert(&self, row: MediaEntry) -> RiverbaseResult<()> {
        let id = row.id().to_string();
        let value = serde_json::to_value(row)
            .map_err(|e| crate::errors::MED_020.with_data(e.to_string()))?;
        let uow = self.store.begin_command_transaction().await?;
        self.store
            .upsert(&uow, self.resource()?, &id, value)
            .await?;
        uow.commit().await
    }

    async fn get(&self, id: &Uuid) -> RiverbaseResult<MediaEntry> {
        self.store
            .fetch(self.resource()?, &id.to_string())
            .await?
            .ok_or_else(|| crate::errors::MED_024.with_data(id.to_string()))
    }

    async fn remove(&self, id: &Uuid) -> RiverbaseResult<MediaEntry> {
        let row = self.get(id).await?;
        let uow = self.store.begin_command_transaction().await?;
        self.store
            .remove(&uow, self.resource()?, &id.to_string())
            .await?;
        uow.commit().await?;
        Ok(row)
    }

    async fn list(&self, query: &MediaQuery) -> RiverbaseResult<Vec<MediaEntry>> {
        use riverbase_core::datastore::DataQuery;
        let mut q = DataQuery::list("media_entries");
        q.page = Some(riverbase_core::datastore::PageSpec {
            offset: query.offset as u64,
            limit: query.limit as u64,
        });
        let rows: Vec<MediaEntry> = self.store.query_list(&q).await?;
        Ok(rows
            .into_iter()
            .filter(|row| {
                query
                    .resource
                    .as_ref()
                    .map(|r| row.resource.as_deref() == Some(r.as_str()))
                    .unwrap_or(true)
                    && query
                        .resource_id
                        .map(|rid| row.resource_id == Some(rid))
                        .unwrap_or(true)
            })
            .collect())
    }
}
