//! Git source: reads a single file from a repository over HTTPS, in memory.
//!
//! URI form: `git+https://<host>/<owner>/<repo>[.git]//<path/to/file>[@<ref>]`.
//! The `//` separates the repository from the file path; `@<ref>` selects a
//! branch/tag/commit (default `main`).
//!
//! Rather than cloning (which would touch disk), this resolves the provider's
//! raw-blob HTTPS endpoint and streams the file into memory. GitHub and GitLab
//! are recognized directly; other hosts use `RIVERBASE_CONFIG_GIT_RAW_TEMPLATE`
//! (placeholders `{host}`, `{repo}`, `{ref}`, `{path}`) or a best-effort
//! `/{repo}/raw/{ref}/{path}` default. Auth uses a bearer token from
//! `RIVERBASE_CONFIG_GIT_TOKEN`.

use async_trait::async_trait;
use zeroize::Zeroizing;

use super::ConfigSource;
use crate::base::RiverbaseResult;

const ENV_GIT_TOKEN: &str = "RIVERBASE_CONFIG_GIT_TOKEN";
const ENV_GIT_TEMPLATE: &str = "RIVERBASE_CONFIG_GIT_RAW_TEMPLATE";

/// Fetches a single file from a git repository via its raw HTTPS endpoint.
pub struct GitSource {
    raw_url: String,
}

impl GitSource {
    pub fn from_uri(uri: &str) -> RiverbaseResult<Self> {
        let invalid = |detail: String| crate::errors::CFG_167.with_data(detail);

        let scheme_stripped = uri.strip_prefix("git+").unwrap_or(uri);
        let (_scheme, rest) = scheme_stripped
            .split_once("://")
            .ok_or_else(|| invalid(format!("uri={uri}")))?;

        let (authority, after) = rest
            .split_once('/')
            .ok_or_else(|| invalid(format!("uri={uri} (missing path)")))?;
        let host = authority.rsplit('@').next().unwrap_or(authority);

        let (repo_part, file_part) = after
            .split_once("//")
            .ok_or_else(|| invalid(format!("uri={uri} (missing // before file path)")))?;
        let repo = repo_part.trim_end_matches('/').trim_end_matches(".git");

        let (file_path, reference) = match file_part.rsplit_once('@') {
            Some((path, r)) if !r.is_empty() => (path, r),
            _ => (file_part, "main"),
        };
        let file_path = file_path.trim_start_matches('/');

        if host.is_empty() || repo.is_empty() || file_path.is_empty() {
            return Err(invalid(format!("uri={uri}")));
        }

        let raw_url = build_raw_url(host, repo, reference, file_path);
        Ok(Self { raw_url })
    }
}

fn build_raw_url(host: &str, repo: &str, reference: &str, file_path: &str) -> String {
    if let Ok(template) = std::env::var(ENV_GIT_TEMPLATE) {
        if !template.trim().is_empty() {
            return template
                .replace("{host}", host)
                .replace("{repo}", repo)
                .replace("{ref}", reference)
                .replace("{path}", file_path);
        }
    }

    if host == "github.com" || host == "www.github.com" {
        format!("https://raw.githubusercontent.com/{repo}/{reference}/{file_path}")
    } else if host.contains("gitlab") {
        format!("https://{host}/{repo}/-/raw/{reference}/{file_path}")
    } else {
        // Best-effort default (Gitea/Bitbucket-style). Override with a template.
        format!("https://{host}/{repo}/raw/{reference}/{file_path}")
    }
}

#[async_trait]
impl ConfigSource for GitSource {
    async fn fetch(&self) -> RiverbaseResult<Zeroizing<Vec<u8>>> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| crate::errors::CFG_100.with_data(e.to_string()))?;

        let mut req = client.get(&self.raw_url);
        if let Ok(token) = std::env::var(ENV_GIT_TOKEN) {
            if !token.is_empty() {
                req = req.bearer_auth(token);
            }
        }

        let resp = req
            .send()
            .await
            .map_err(|e| crate::errors::CFG_194.with_data(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(
                crate::errors::CFG_209.with_data(format!("status={status} url={}", self.raw_url))
            );
        }

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| crate::errors::CFG_195.with_data(e.to_string()))?;
        Ok(Zeroizing::new(bytes.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_github_raw_url() {
        let src = GitSource::from_uri("git+https://github.com/acme/cfg//env/riverbase.toml@v1")
            .expect("parse");
        assert_eq!(
            src.raw_url,
            "https://raw.githubusercontent.com/acme/cfg/v1/env/riverbase.toml"
        );
    }

    #[test]
    fn defaults_ref_to_main() {
        let src =
            GitSource::from_uri("git+https://github.com/acme/cfg//riverbase.toml").expect("parse");
        assert_eq!(
            src.raw_url,
            "https://raw.githubusercontent.com/acme/cfg/main/riverbase.toml"
        );
    }

    #[test]
    fn requires_file_separator() {
        let err = match GitSource::from_uri("git+https://github.com/acme/cfg") {
            Ok(_) => panic!("expected a missing-file-separator error"),
            Err(e) => e,
        };
        assert_eq!(err.errcode.as_str(), "CFG-120");
    }
}
