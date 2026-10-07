use async_trait::async_trait;

use crate::base::RiverbaseResult;

/// Narrow read port for command decisions. Public read models remain query resources.
#[async_trait]
pub trait DecisionReadPort<Q, R>: Send + Sync
where
    Q: Send + Sync,
    R: Send + Sync,
{
    /// Read.
    async fn read(&self, query: &Q) -> RiverbaseResult<R>;
}
