//! OCI artifact registry source (`oci://<registry>/<repo>:<tag>`), supporting
//! Amazon ECR and Google Artifact Registry (and generic OCI registries).
//!
//! Config is stored as an artifact blob (e.g. pushed with `oras push`). This
//! performs an in-memory OCI distribution pull: resolve auth, GET the manifest,
//! select a layer (by `org.opencontainers.image.title` when a `//<file>`
//! selector is given), then GET the blob. Nothing is written to disk.
//!
//! Auth selection by registry host:
//! - `*.dkr.ecr.<region>.amazonaws.com` -> `aws-sdk-ecr` GetAuthorizationToken (Basic).
//! - `*-docker.pkg.dev`, `gcr.io` -> `gcp_auth` access token (Bearer).
//! - otherwise -> `RIVERBASE_CONFIG_OCI_TOKEN` (Bearer) or anonymous token exchange.

use std::collections::HashMap;

use async_trait::async_trait;
use base64::Engine;
use reqwest::header::{ACCEPT, AUTHORIZATION, WWW_AUTHENTICATE};
use serde::Deserialize;
use url::Url;
use zeroize::Zeroizing;

use super::ConfigSource;
use crate::base::RiverbaseResult;

const ENV_OCI_TOKEN: &str = "RIVERBASE_CONFIG_OCI_TOKEN";
const TITLE_ANNOTATION: &str = "org.opencontainers.image.title";
const EMPTY_CONFIG_MEDIA_TYPE: &str = "application/vnd.oci.empty.v1+json";
const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.manifest.v1+json, \
application/vnd.docker.distribution.manifest.v2+json, \
application/vnd.oci.image.index.v1+json, \
application/vnd.docker.distribution.manifest.list.v2+json";

/// Fetches a config blob from an OCI artifact registry.
pub struct OciSource {
    registry: String,
    repository: String,
    reference: String,
    file: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    layers: Vec<Descriptor>,
    #[serde(default)]
    manifests: Vec<Descriptor>,
}

#[derive(Debug, Deserialize)]
struct Descriptor {
    digest: String,
    #[serde(default, rename = "mediaType")]
    media_type: String,
    #[serde(default)]
    annotations: HashMap<String, String>,
}

enum OciAuth {
    None,
    Basic(String),
    Bearer(String),
}

impl OciSource {
    pub fn from_uri(uri: &str) -> RiverbaseResult<Self> {
        let invalid = |detail: String| crate::errors::CFG_170.with_data(detail);

        let rest = uri
            .strip_prefix("oci://")
            .ok_or_else(|| invalid(format!("uri={uri}")))?;

        let (ref_part, file) = match rest.split_once("//") {
            Some((r, f)) if !f.is_empty() => (r, Some(f.to_string())),
            _ => (rest, None),
        };

        let (registry, remainder) = ref_part
            .split_once('/')
            .ok_or_else(|| invalid(format!("uri={uri} (missing repository)")))?;

        let (repository, reference) = if let Some((repo, digest)) = remainder.split_once('@') {
            (repo.to_string(), digest.to_string())
        } else if let Some(idx) = remainder.rfind(':') {
            (
                remainder[..idx].to_string(),
                remainder[idx + 1..].to_string(),
            )
        } else {
            (remainder.to_string(), "latest".to_string())
        };

        if registry.is_empty() || repository.is_empty() || reference.is_empty() {
            return Err(invalid(format!("uri={uri}")));
        }

        Ok(Self {
            registry: registry.to_string(),
            repository,
            reference,
            file,
        })
    }

    fn manifest_url(&self, reference: &str) -> String {
        format!(
            "https://{}/v2/{}/manifests/{}",
            self.registry, self.repository, reference
        )
    }

    fn blob_url(&self, digest: &str) -> String {
        format!(
            "https://{}/v2/{}/blobs/{}",
            self.registry, self.repository, digest
        )
    }

    async fn resolve_auth(&self, client: &reqwest::Client) -> RiverbaseResult<OciAuth> {
        if let Ok(token) = std::env::var(ENV_OCI_TOKEN) {
            if !token.is_empty() {
                return Ok(OciAuth::Bearer(token));
            }
        }
        if is_ecr_host(&self.registry) {
            return ecr_basic_auth().await;
        }
        if is_gar_host(&self.registry) {
            return gar_bearer_auth().await;
        }

        // Generic registry: probe for an anonymous bearer challenge.
        let probe = client
            .get(format!("https://{}/v2/", self.registry))
            .send()
            .await;
        if let Ok(resp) = probe {
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
                if let Some(www) = resp
                    .headers()
                    .get(WWW_AUTHENTICATE)
                    .and_then(|v| v.to_str().ok())
                {
                    let scope = format!("repository:{}:pull", self.repository);
                    if let Some(auth) = fetch_anonymous_token(client, www, &scope).await {
                        return Ok(auth);
                    }
                }
            }
        }
        Ok(OciAuth::None)
    }

    async fn get_manifest(
        &self,
        client: &reqwest::Client,
        auth: &OciAuth,
        reference: &str,
    ) -> RiverbaseResult<Manifest> {
        let url = self.manifest_url(reference);
        let resp = apply_auth(client.get(&url), auth)
            .header(ACCEPT, MANIFEST_ACCEPT)
            .send()
            .await
            .map_err(|e| crate::errors::CFG_196.with_data(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(crate::errors::CFG_211.with_data(format!("status={status} url={url}")));
        }

        resp.json::<Manifest>()
            .await
            .map_err(|e| crate::errors::CFG_197.with_data(e.to_string()))
    }

    async fn resolve_layers(
        &self,
        client: &reqwest::Client,
        auth: &OciAuth,
        manifest: Manifest,
    ) -> RiverbaseResult<Vec<Descriptor>> {
        if !manifest.layers.is_empty() {
            return Ok(manifest.layers);
        }
        if let Some(child) = manifest.manifests.first() {
            let sub = self.get_manifest(client, auth, &child.digest).await?;
            return Ok(sub.layers);
        }
        Err(crate::errors::CFG_188.with_data(format!("repository={}", self.repository)))
    }

    fn select_layer<'a>(&self, layers: &'a [Descriptor]) -> RiverbaseResult<&'a Descriptor> {
        if layers.is_empty() {
            return Err(crate::errors::CFG_188.with_data(format!("repository={}", self.repository)));
        }
        if let Some(file) = &self.file {
            return layers
                .iter()
                .find(|d| {
                    d.annotations
                        .get(TITLE_ANNOTATION)
                        .is_some_and(|t| t == file)
                })
                .ok_or_else(|| {
                    crate::errors::CFG_198
                        .with_data(format!("file={file} repository={}", self.repository))
                });
        }
        Ok(layers
            .iter()
            .find(|d| d.media_type != EMPTY_CONFIG_MEDIA_TYPE)
            .unwrap_or(&layers[0]))
    }

    async fn get_blob(
        &self,
        client: &reqwest::Client,
        auth: &OciAuth,
        digest: &str,
    ) -> RiverbaseResult<Zeroizing<Vec<u8>>> {
        let url = self.blob_url(digest);
        let resp = apply_auth(client.get(&url), auth)
            .send()
            .await
            .map_err(|e| crate::errors::CFG_199.with_data(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(crate::errors::CFG_212.with_data(format!("status={status} url={url}")));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| crate::errors::CFG_200.with_data(e.to_string()))?;
        Ok(Zeroizing::new(bytes.to_vec()))
    }
}

#[async_trait]
impl ConfigSource for OciSource {
    async fn fetch(&self) -> RiverbaseResult<Zeroizing<Vec<u8>>> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| crate::errors::CFG_100.with_data(e.to_string()))?;

        let auth = self.resolve_auth(&client).await?;
        let reference = self.reference.clone();
        let manifest = self.get_manifest(&client, &auth, &reference).await?;
        let layers = self.resolve_layers(&client, &auth, manifest).await?;
        let digest = self.select_layer(&layers)?.digest.clone();
        self.get_blob(&client, &auth, &digest).await
    }
}

fn apply_auth(req: reqwest::RequestBuilder, auth: &OciAuth) -> reqwest::RequestBuilder {
    match auth {
        OciAuth::None => req,
        OciAuth::Basic(token) => req.header(AUTHORIZATION, format!("Basic {token}")),
        OciAuth::Bearer(token) => req.header(AUTHORIZATION, format!("Bearer {token}")),
    }
}

fn is_ecr_host(host: &str) -> bool {
    host.contains(".dkr.ecr.") && host.ends_with(".amazonaws.com")
}

fn is_gar_host(host: &str) -> bool {
    host == "gcr.io"
        || host.ends_with(".gcr.io")
        || host.ends_with("-docker.pkg.dev")
        || host.ends_with(".pkg.dev")
}

async fn ecr_basic_auth() -> RiverbaseResult<OciAuth> {
    let conf = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let client = aws_sdk_ecr::Client::new(&conf);
    let output = client
        .get_authorization_token()
        .send()
        .await
        .map_err(|e| crate::errors::CFG_101.with_data(e.to_string()))?;

    let token = output
        .authorization_data()
        .iter()
        .find_map(|d| d.authorization_token())
        .ok_or_else(|| crate::errors::CFG_201.with_data("authorization_data was empty"))?;
    Ok(OciAuth::Basic(token.to_string()))
}

async fn gar_bearer_auth() -> RiverbaseResult<OciAuth> {
    let provider = gcp_auth::provider()
        .await
        .map_err(|e| crate::errors::CFG_202.with_data(e.to_string()))?;
    let scopes = ["https://www.googleapis.com/auth/cloud-platform"];
    let token = provider
        .token(&scopes)
        .await
        .map_err(|e| crate::errors::CFG_203.with_data(e.to_string()))?;
    Ok(OciAuth::Bearer(token.as_str().to_string()))
}

/// Perform a docker-registry anonymous bearer token exchange from a
/// `WWW-Authenticate` challenge.
async fn fetch_anonymous_token(
    client: &reqwest::Client,
    challenge: &str,
    fallback_scope: &str,
) -> Option<OciAuth> {
    let params = parse_bearer_challenge(challenge)?;
    let realm = params.get("realm")?;
    let mut url = Url::parse(realm).ok()?;
    {
        let mut qp = url.query_pairs_mut();
        if let Some(service) = params.get("service") {
            qp.append_pair("service", service);
        }
        let scope = params
            .get("scope")
            .map(String::as_str)
            .unwrap_or(fallback_scope);
        qp.append_pair("scope", scope);
    }

    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let value: serde_json::Value = resp.json().await.ok()?;
    let token = value
        .get("token")
        .and_then(|t| t.as_str())
        .or_else(|| value.get("access_token").and_then(|t| t.as_str()))?;
    Some(OciAuth::Bearer(token.to_string()))
}

fn parse_bearer_challenge(header: &str) -> Option<HashMap<String, String>> {
    let trimmed = header.trim();
    if trimmed.len() < 6 || !trimmed[..6].eq_ignore_ascii_case("bearer") {
        return None;
    }
    let mut map = HashMap::new();
    for part in trimmed[6..].split(',') {
        if let Some((key, value)) = part.split_once('=') {
            map.insert(
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            );
        }
    }
    Some(map)
}

/// Decode an ECR-style base64 `user:password` token (exposed for completeness;
/// the registry accepts the raw token as Basic credentials directly).
#[allow(dead_code)]
fn decode_basic(token: &str) -> Option<(String, String)> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(token)
        .ok()?;
    let text = String::from_utf8(decoded).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tag_and_file_selector() {
        let src = OciSource::from_uri("oci://reg.example.com/team/config:1.2.3//riverbase.toml")
            .expect("parse");
        assert_eq!(src.registry, "reg.example.com");
        assert_eq!(src.repository, "team/config");
        assert_eq!(src.reference, "1.2.3");
        assert_eq!(src.file.as_deref(), Some("riverbase.toml"));
    }

    #[test]
    fn parses_digest_reference() {
        let src =
            OciSource::from_uri("oci://reg.example.com/team/config@sha256:abc").expect("parse");
        assert_eq!(src.reference, "sha256:abc");
        assert_eq!(src.file, None);
    }

    #[test]
    fn defaults_tag_to_latest() {
        let src = OciSource::from_uri("oci://reg.example.com/team/config").expect("parse");
        assert_eq!(src.reference, "latest");
    }

    #[test]
    fn detects_cloud_hosts() {
        assert!(is_ecr_host("123456789012.dkr.ecr.us-east-1.amazonaws.com"));
        assert!(is_gar_host("us-docker.pkg.dev"));
        assert!(is_gar_host("gcr.io"));
        assert!(!is_ecr_host("reg.example.com"));
    }

    #[test]
    fn parses_bearer_challenge_params() {
        let params = parse_bearer_challenge(
            r#"Bearer realm="https://auth.example.com/token",service="registry",scope="repository:x:pull""#,
        )
        .expect("challenge");
        assert_eq!(
            params.get("realm").unwrap(),
            "https://auth.example.com/token"
        );
        assert_eq!(params.get("service").unwrap(), "registry");
    }
}
