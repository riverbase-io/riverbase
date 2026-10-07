//! Live integration tests for remote config sources.
//!
//! These talk to real endpoints and only run when the corresponding env var is
//! set (and the matching `cfgfetch-*` feature is enabled); otherwise they skip.
//! Provide cloud credentials via the standard provider chains and, for encrypted
//! payloads, the relevant SOPS key material (e.g. `SOPS_AGE_KEY`).
//!
//! Examples:
//!   RIVERBASE_TEST_HTTPS_URI=https://example.com/flrs.yaml \
//!     cargo test -p riverbase_core --features cfgfetch-https --test cfgfetch_remote
//!   RIVERBASE_TEST_S3_URI=s3://my-bucket/riverbase.toml \
//!     cargo test -p riverbase_core --features cfgfetch-s3 --test cfgfetch_remote
//!   RIVERBASE_TEST_OCI_URI=oci://us-docker.pkg.dev/proj/repo/cfg:1.0.0//riverbase.toml \
//!     cargo test -p riverbase_core --features cfgfetch-oci --test cfgfetch_remote

#![cfg(feature = "cfgfetch")]

#[allow(unused_imports)]
use riverbase_core::{fetch_and_decrypt, RemoteConfigSpec};

#[allow(dead_code)]
fn env_uri(var: &str) -> Option<String> {
    match std::env::var(var) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("skipping: {var} not set");
            None
        }
    }
}

#[cfg(feature = "cfgfetch-https")]
#[tokio::test]
async fn https_remote_fetch() {
    let Some(uri) = env_uri("RIVERBASE_TEST_HTTPS_URI") else {
        return;
    };
    let spec = RemoteConfigSpec::new(uri);
    let bytes = fetch_and_decrypt(&spec).await.expect("fetch https config");
    assert!(!bytes.is_empty(), "fetched config was empty");
}

#[cfg(feature = "cfgfetch-s3")]
#[tokio::test]
async fn s3_remote_fetch() {
    let Some(uri) = env_uri("RIVERBASE_TEST_S3_URI") else {
        return;
    };
    let spec = RemoteConfigSpec::new(uri);
    let bytes = fetch_and_decrypt(&spec).await.expect("fetch s3 config");
    assert!(!bytes.is_empty(), "fetched config was empty");
}

#[cfg(feature = "cfgfetch-git")]
#[tokio::test]
async fn git_remote_fetch() {
    let Some(uri) = env_uri("RIVERBASE_TEST_GIT_URI") else {
        return;
    };
    let spec = RemoteConfigSpec::new(uri);
    let bytes = fetch_and_decrypt(&spec).await.expect("fetch git config");
    assert!(!bytes.is_empty(), "fetched config was empty");
}

#[cfg(feature = "cfgfetch-oci")]
#[tokio::test]
async fn oci_remote_fetch() {
    let Some(uri) = env_uri("RIVERBASE_TEST_OCI_URI") else {
        return;
    };
    let spec = RemoteConfigSpec::new(uri);
    let bytes = fetch_and_decrypt(&spec).await.expect("fetch oci config");
    assert!(!bytes.is_empty(), "fetched config was empty");
}
