//! Config sources: fetch raw (possibly encrypted) bytes into memory.
//!
//! Every implementation MUST keep data in memory and never write fetched bytes
//! to disk. The local [`FileSource`] is the only non-remote source and reads an
//! existing file (it never writes).

use async_trait::async_trait;
use zeroize::Zeroizing;

use crate::base::{RiverbaseError, RiverbaseResult};

mod file;
pub use file::FileSource;

#[cfg(feature = "cfgfetch-https")]
mod https;
#[cfg(feature = "cfgfetch-https")]
pub use https::HttpsSource;

#[cfg(feature = "cfgfetch-git")]
mod git;
#[cfg(feature = "cfgfetch-git")]
pub use git::GitSource;

#[cfg(feature = "cfgfetch-s3")]
mod s3;
#[cfg(feature = "cfgfetch-s3")]
pub use s3::S3Source;

#[cfg(feature = "cfgfetch-oci")]
mod oci;
#[cfg(feature = "cfgfetch-oci")]
pub use oci::OciSource;

/// A remote (or local) origin for config bytes.
#[async_trait]
pub trait ConfigSource: Send + Sync {
    /// Fetch raw bytes into a buffer that is zeroized on drop.
    async fn fetch(&self) -> RiverbaseResult<Zeroizing<Vec<u8>>>;
}

/// Extract the scheme component of a URI (`scheme:rest`).
fn uri_scheme(uri: &str) -> Option<&str> {
    let end = uri.find(':')?;
    let scheme = &uri[..end];
    let valid = !scheme.is_empty()
        && scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.');
    valid.then_some(scheme)
}

/// Resolve a URI to a concrete [`ConfigSource`] based on its scheme.
pub fn resolve_source(uri: &str) -> RiverbaseResult<Box<dyn ConfigSource>> {
    let scheme =
        uri_scheme(uri).ok_or_else(|| crate::errors::CFG_168.with_data(format!("uri={uri}")))?;

    match scheme {
        "file" => Ok(Box::new(FileSource::from_uri(uri)?)),
        "http" | "https" => https_source(uri),
        "s3" => s3_source(uri),
        s if s == "git" || s.starts_with("git+") => git_source(uri),
        "oci" => oci_source(uri),
        other => Err(crate::errors::CFG_169.with_data(format!("scheme={other}"))),
    }
}

#[allow(dead_code)]
fn feature_disabled(scheme: &str, feature: &str) -> RiverbaseError {
    crate::errors::CFG_121.with_data(format!("scheme={scheme} requires feature `{feature}`"))
}

#[cfg(feature = "cfgfetch-https")]
fn https_source(uri: &str) -> RiverbaseResult<Box<dyn ConfigSource>> {
    Ok(Box::new(HttpsSource::from_uri(uri)?))
}
#[cfg(not(feature = "cfgfetch-https"))]
fn https_source(_uri: &str) -> RiverbaseResult<Box<dyn ConfigSource>> {
    Err(feature_disabled("http(s)", "cfgfetch-https"))
}

#[cfg(feature = "cfgfetch-git")]
fn git_source(uri: &str) -> RiverbaseResult<Box<dyn ConfigSource>> {
    Ok(Box::new(GitSource::from_uri(uri)?))
}
#[cfg(not(feature = "cfgfetch-git"))]
fn git_source(_uri: &str) -> RiverbaseResult<Box<dyn ConfigSource>> {
    Err(feature_disabled("git", "cfgfetch-git"))
}

#[cfg(feature = "cfgfetch-s3")]
fn s3_source(uri: &str) -> RiverbaseResult<Box<dyn ConfigSource>> {
    Ok(Box::new(S3Source::from_uri(uri)?))
}
#[cfg(not(feature = "cfgfetch-s3"))]
fn s3_source(_uri: &str) -> RiverbaseResult<Box<dyn ConfigSource>> {
    Err(feature_disabled("s3", "cfgfetch-s3"))
}

#[cfg(feature = "cfgfetch-oci")]
fn oci_source(uri: &str) -> RiverbaseResult<Box<dyn ConfigSource>> {
    Ok(Box::new(OciSource::from_uri(uri)?))
}
#[cfg(not(feature = "cfgfetch-oci"))]
fn oci_source(_uri: &str) -> RiverbaseResult<Box<dyn ConfigSource>> {
    Err(feature_disabled("oci", "cfgfetch-oci"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_scheme() {
        assert_eq!(uri_scheme("s3://bucket/key"), Some("s3"));
        assert_eq!(uri_scheme("git+https://host/x"), Some("git+https"));
        assert_eq!(uri_scheme("oci://reg/repo:tag"), Some("oci"));
        assert_eq!(uri_scheme("nocolon"), None);
        assert_eq!(uri_scheme("://noscheme"), None);
    }

    #[test]
    fn file_scheme_resolves() {
        assert!(resolve_source("file:///tmp/riverbase.toml").is_ok());
    }

    #[cfg(not(feature = "cfgfetch-s3"))]
    #[test]
    fn disabled_scheme_reports_feature() {
        let err = match resolve_source("s3://bucket/key") {
            Ok(_) => panic!("expected a feature-disabled error"),
            Err(e) => e,
        };
        assert_eq!(err.errcode.as_str(), "CFG-121");
    }
}
