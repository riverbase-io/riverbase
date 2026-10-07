use serde::Deserialize;
use serde_json::json;

use crate::RiverbaseResult;

#[derive(Debug, Clone, Deserialize)]
pub struct OpenIdConfiguration {
    pub issuer: String,
    pub jwks_uri: String,
}

pub async fn fetch_openid_configuration(
    discovery_url: &str,
    client: &reqwest::Client,
) -> RiverbaseResult<OpenIdConfiguration> {
    let doc = client
        .get(discovery_url)
        .send()
        .await
        .map_err(|e| {
            crate::errors::AUT_011.with_data(json!({
                "operation": "oidc_discovery_request",
                "discovery_url": discovery_url,
                "cause": e.to_string(),
            }))
        })?
        .error_for_status()
        .map_err(|e| {
            crate::errors::AUT_184.with_data(json!({
                "operation": "oidc_discovery_http_status",
                "discovery_url": discovery_url,
                "cause": e.to_string(),
            }))
        })?
        .json::<OpenIdConfiguration>()
        .await
        .map_err(|e| {
            crate::errors::AUT_012.with_data(json!({
                "operation": "oidc_discovery_parse",
                "discovery_url": discovery_url,
                "cause": e.to_string(),
            }))
        })?;
    Ok(doc)
}
