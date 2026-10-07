//! S3 source (`s3://bucket/key`). Credentials and region are resolved from the
//! standard AWS provider chain (environment, shared profile, web identity, IMDS).

use async_trait::async_trait;
use zeroize::Zeroizing;

use super::ConfigSource;
use crate::base::RiverbaseResult;

/// Fetches a config object from Amazon S3.
pub struct S3Source {
    bucket: String,
    key: String,
}

impl S3Source {
    /// Build from an `s3://bucket/key` URI. Both bucket and key are required.
    pub fn from_uri(uri: &str) -> RiverbaseResult<Self> {
        let rest = uri
            .strip_prefix("s3://")
            .ok_or_else(|| crate::errors::CFG_120.with_data(format!("uri={uri}")))?;
        let (bucket, key) = rest
            .split_once('/')
            .ok_or_else(|| crate::errors::CFG_164.with_data(format!("uri={uri}")))?;
        if bucket.is_empty() || key.is_empty() {
            return Err(crate::errors::CFG_166.with_data(format!("uri={uri}")));
        }
        Ok(Self {
            bucket: bucket.to_string(),
            key: key.to_string(),
        })
    }
}

#[async_trait]
impl ConfigSource for S3Source {
    async fn fetch(&self) -> RiverbaseResult<Zeroizing<Vec<u8>>> {
        let conf = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = aws_sdk_s3::Client::new(&conf);

        let output = client
            .get_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
            .map_err(|e| {
                crate::errors::CFG_190
                    .with_data(format!("bucket={} key={}: {e}", self.bucket, self.key))
            })?;

        let data = output.body.collect().await.map_err(|e| {
            crate::errors::CFG_192
                .with_data(format!("bucket={} key={}: {e}", self.bucket, self.key))
        })?;
        Ok(Zeroizing::new(data.into_bytes().to_vec()))
    }
}
