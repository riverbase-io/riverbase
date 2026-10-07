//! Local `file://` source. Useful for tests and local development; reads an
//! existing file only.

use std::path::PathBuf;

use async_trait::async_trait;
use zeroize::Zeroizing;

use super::ConfigSource;
use crate::base::RiverbaseResult;

/// Reads config bytes from a local path.
pub struct FileSource {
    path: PathBuf,
}

impl FileSource {
    /// Build from a `file://` URI. Supports `file:///abs/path` and
    /// `file://localhost/abs/path`.
    pub fn from_uri(uri: &str) -> RiverbaseResult<Self> {
        let rest = uri.strip_prefix("file://").unwrap_or(uri);
        let path = if let Some(stripped) = rest.strip_prefix("localhost/") {
            format!("/{stripped}")
        } else {
            rest.to_string()
        };
        if path.is_empty() {
            return Err(crate::errors::CFG_165.with_data(format!("uri={uri}")));
        }
        Ok(Self {
            path: PathBuf::from(path),
        })
    }

    /// Build directly from a filesystem path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait]
impl ConfigSource for FileSource {
    async fn fetch(&self) -> RiverbaseResult<Zeroizing<Vec<u8>>> {
        let bytes = tokio::fs::read(&self.path).await.map_err(|e| {
            crate::errors::CFG_189.with_data(format!("path={}: {e}", self.path.display()))
        })?;
        Ok(Zeroizing::new(bytes))
    }
}
