//! HTTP(S) source with optional authentication.
//!
//! Auth precedence: a bearer token from `RIVERBASE_CONFIG_HTTP_TOKEN`, otherwise HTTP
//! basic credentials embedded in the URI userinfo.

use async_trait::async_trait;
use url::Url;
use zeroize::Zeroizing;

use super::ConfigSource;
use crate::base::RiverbaseResult;

const ENV_HTTP_TOKEN: &str = "RIVERBASE_CONFIG_HTTP_TOKEN";

/// Fetches config bytes over HTTP(S).
pub struct HttpsSource {
    url: Url,
}

impl HttpsSource {
    pub fn from_uri(uri: &str) -> RiverbaseResult<Self> {
        let url = Url::parse(uri)
            .map_err(|e| crate::errors::CFG_163.with_data(format!("uri={uri}: {e}")))?;
        Ok(Self { url })
    }
}

#[async_trait]
impl ConfigSource for HttpsSource {
    async fn fetch(&self) -> RiverbaseResult<Zeroizing<Vec<u8>>> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| crate::errors::CFG_100.with_data(e.to_string()))?;

        // Move any userinfo out of the request URL and apply it as basic auth.
        let username = self.url.username().to_string();
        let password = self.url.password().map(|p| p.to_string());
        let mut request_url = self.url.clone();
        let _ = request_url.set_username("");
        let _ = request_url.set_password(None);

        let mut req = client.get(request_url.clone());
        match std::env::var(ENV_HTTP_TOKEN) {
            Ok(token) if !token.is_empty() => {
                req = req.bearer_auth(token);
            }
            _ if !username.is_empty() => {
                req = req.basic_auth(username, password);
            }
            _ => {}
        }

        let resp = req
            .send()
            .await
            .map_err(|e| crate::errors::CFG_191.with_data(redact(&request_url, &e.to_string())))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(crate::errors::CFG_210
                .with_data(format!("status={status} {}", redact(&request_url, ""))));
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| crate::errors::CFG_193.with_data(e.to_string()))?;
        Ok(Zeroizing::new(bytes.to_vec()))
    }
}

/// Render a URL for error context without leaking userinfo or query secrets.
fn redact(url: &Url, suffix: &str) -> String {
    format!(
        "url={}://{}{} {suffix}",
        url.scheme(),
        url.host_str().unwrap_or(""),
        url.path()
    )
}
