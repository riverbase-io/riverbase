use std::collections::HashMap;
use std::time::Instant;

use jsonwebtoken::DecodingKey;
use serde::Deserialize;
use serde_json::json;

use crate::RiverbaseResult;

#[derive(Debug, Deserialize)]
pub struct JwksDocument {
    pub keys: Vec<Jwk>,
}

#[derive(Debug, Deserialize)]
pub struct Jwk {
    pub kid: Option<String>,
    pub kty: String,
    #[serde(rename = "use")]
    pub key_use: Option<String>,
    pub alg: Option<String>,
    pub n: Option<String>,
    pub e: Option<String>,
}

pub struct JwksCache {
    keys: HashMap<String, DecodingKey>,
    fetched_at: Instant,
}

impl JwksCache {
    pub fn empty() -> Self {
        Self {
            keys: HashMap::new(),
            fetched_at: Instant::now(),
        }
    }

    pub fn is_stale(&self, ttl_secs: u64) -> bool {
        self.fetched_at.elapsed().as_secs() >= ttl_secs
    }

    pub fn get(&self, kid: &str) -> Option<&DecodingKey> {
        self.keys.get(kid)
    }

    pub fn replace(&mut self, keys: HashMap<String, DecodingKey>) {
        self.keys = keys;
        self.fetched_at = Instant::now();
    }
}

pub async fn fetch_jwks(jwks_uri: &str, client: &reqwest::Client) -> RiverbaseResult<JwksDocument> {
    let doc = client
        .get(jwks_uri)
        .send()
        .await
        .map_err(|e| {
            crate::errors::AUT_020.with_data(json!({
                "operation": "jwks_request",
                "jwks_uri": jwks_uri,
                "cause": e.to_string(),
            }))
        })?
        .error_for_status()
        .map_err(|e| {
            crate::errors::AUT_021.with_data(json!({
                "operation": "jwks_http_status",
                "jwks_uri": jwks_uri,
                "cause": e.to_string(),
            }))
        })?
        .json::<JwksDocument>()
        .await
        .map_err(|e| {
            crate::errors::AUT_022.with_data(json!({
                "operation": "jwks_parse",
                "jwks_uri": jwks_uri,
                "cause": e.to_string(),
            }))
        })?;
    Ok(doc)
}

pub fn jwks_to_decoding_keys(doc: &JwksDocument) -> RiverbaseResult<HashMap<String, DecodingKey>> {
    let mut map = HashMap::new();
    for key in &doc.keys {
        if key.kty != "RSA" {
            continue;
        }
        if key.key_use.as_deref() == Some("enc") {
            continue;
        }
        if let Some(alg) = &key.alg {
            if alg != "RS256" {
                continue;
            }
        }
        let kid = key.kid.clone().ok_or_else(|| {
            crate::errors::AUT_023.with_data(json!({
                "operation": "jwks_to_decoding_keys",
                "kty": key.kty,
                "alg": key.alg,
            }))
        })?;
        let n = key.n.as_deref().ok_or_else(|| {
            crate::errors::AUT_024.with_data(json!({
                "operation": "jwks_to_decoding_keys",
                "kid": kid,
            }))
        })?;
        let e = key.e.as_deref().ok_or_else(|| {
            crate::errors::AUT_025.with_data(json!({
                "operation": "jwks_to_decoding_keys",
                "kid": kid,
            }))
        })?;
        let decoding = DecodingKey::from_rsa_components(n, e).map_err(|err| {
            crate::errors::AUT_026.with_data(json!({
                "operation": "jwks_to_decoding_keys",
                "kid": kid,
                "cause": err.to_string(),
            }))
        })?;
        map.insert(kid, decoding);
    }
    if map.is_empty() {
        return Err(crate::errors::AUT_027.with_data(json!({
            "operation": "jwks_to_decoding_keys",
            "keys_count": doc.keys.len(),
        })));
    }
    Ok(map)
}
