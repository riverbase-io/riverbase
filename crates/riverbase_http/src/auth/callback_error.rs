//! OAuth callback failure: local logout, then JSON or a static HTML card.

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use axum_extra::extract::CookieJar;
use tower_sessions::Session;

use super::session_establish::id_token_removal_cookie;
use crate::base::RiverbaseError;
use crate::config::AuthConfig;

const AUTH_ERROR_CSS: &str = include_str!("auth_error.css");

/// True when `Accept` lists `application/json` as a media type.
pub fn accept_includes_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|part| {
                part.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("application/json")
            })
        })
}

/// Flush the browser session, drop the ID-token cookie, and render the failure.
///
/// Does not redirect to the IdP or the SPA.
pub async fn callback_failure_response(
    session: &Session,
    config: &AuthConfig,
    jar: CookieJar,
    headers: &HeaderMap,
    err: RiverbaseError,
) -> Response {
    let logged_in = session
        .get::<serde_json::Value>(&config.ses_user_field)
        .await
        .ok()
        .flatten()
        .is_some()
        || jar.get(&config.ses_id_token_field).is_some();
    let err = match err.errcode.as_str() {
        "AUT-150" | "AUT-151" if logged_in => replayed_callback_error(crate::errors::AUT_197, err),
        "AUT-150" | "AUT-151" => replayed_callback_error(crate::errors::AUT_198, err),
        _ => err,
    };
    let keep_session = err.errcode.as_str() == "AUT-197";
    let err = if keep_session {
        err
    } else {
        match session.flush().await {
            Ok(()) => err,
            Err(flush_err) => crate::errors::AUT_159.with_data(serde_json::json!({
                "detail": flush_err.to_string(),
            })),
        }
    };
    let body = if accept_includes_json(headers) {
        crate::http_response::problem_json_response(err, None).into_response()
    } else {
        let status = StatusCode::from_u16(err.http_status).unwrap_or(StatusCode::FORBIDDEN);
        (
            status,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            Html(forbidden_card_html(config, &err)),
        )
            .into_response()
    };
    let jar = if keep_session {
        jar
    } else {
        jar.remove(id_token_removal_cookie(config))
    };
    (jar, body).into_response()
}

/// Keep the original failure on the rewritten callback error so the card can show it.
fn replayed_callback_error(spec: crate::base::ErrorSpec, original: RiverbaseError) -> RiverbaseError {
    let mut data = match original.errdata {
        serde_json::Value::Object(map) => map,
        serde_json::Value::Null => serde_json::Map::new(),
        other => {
            let mut map = serde_json::Map::new();
            map.insert("detail".to_string(), other);
            map
        }
    };
    data.entry("cause".to_string())
        .or_insert_with(|| serde_json::Value::String(original.errmesg));
    data.entry("error".to_string())
        .or_insert_with(|| serde_json::Value::String(original.errcode.as_str().to_string()));
    spec.with_data(serde_json::Value::Object(data))
}

fn forbidden_card_html(config: &AuthConfig, err: &RiverbaseError) -> String {
    let problem = crate::base::ProblemDetails::from_riverbase_error(err, Some("/".to_string()));
    let report = serde_json::to_string_pretty(&problem).unwrap_or_else(|_| "{}".to_string());
    auth_error_document(
        config,
        err.http_status,
        &err.errmesg,
        err.errcode.as_str(),
        &err.errdata,
        "/",
        &report,
    )
}

fn underlying_error(errdata: &serde_json::Value, headline: &str) -> Option<String> {
    let obj = errdata.as_object()?;
    let text = |key: &str| -> String {
        match obj.get(key) {
            Some(serde_json::Value::String(value)) => value.trim().to_string(),
            Some(serde_json::Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        }
    };
    let description = text("description");
    let cause = text("cause");
    let extra = text("detail");
    let code = text("error");
    let headline = headline.trim();
    let mut primary = String::new();
    for candidate in [&description, &cause, &extra] {
        if !candidate.is_empty() && candidate.as_str() != headline {
            primary = candidate.clone();
            break;
        }
    }
    if primary.is_empty() {
        return None;
    }
    let mut parts = vec![primary.clone()];
    for candidate in [&cause, &extra] {
        if candidate.is_empty()
            || candidate.as_str() == headline
            || parts.iter().any(|part| part == candidate)
        {
            continue;
        }
        if primary.contains(candidate.as_str()) || candidate.contains(primary.as_str()) {
            if candidate.len() > primary.len() {
                parts[0] = candidate.clone();
                primary = candidate.clone();
            }
            continue;
        }
        parts.push(candidate.clone());
    }
    let mut body = parts.join(" — ");
    if body.chars().count() > 500 {
        body = body.chars().take(499).collect();
        body.push('…');
    }
    if !code.is_empty() && !body.contains(&code) {
        body = format!("{code}: {body}");
    }
    if body == headline {
        return None;
    }
    Some(body)
}

fn normalize_notice(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn same_notice(left: &str, right: &str) -> bool {
    let a = normalize_notice(left);
    let b = normalize_notice(right);
    !a.is_empty() && a == b
}

fn generic_notice(value: &str) -> bool {
    matches!(
        normalize_notice(value).as_str(),
        "signincouldnotbecompleted"
            | "yourenotsignedinoryoursessionhasexpired"
            | "theserverrejectedthesessionrequest"
            | "couldntloadyoursession"
            | "startsigninagain"
            | "gohometocontinue"
            | "accessdenied"
    )
}

fn visible_message(title: &str, detail: &str, errdata: &serde_json::Value) -> String {
    if let Some(underlying) = underlying_error(errdata, title) {
        if !same_notice(&underlying, title) {
            return underlying;
        }
    }
    let detail = detail.trim();
    if !detail.is_empty() && !same_notice(detail, title) && !generic_notice(detail) {
        return detail.to_string();
    }
    String::new()
}

/// Browser card for an `/auth/*` error. JSON clients never see this.
pub fn auth_error_document(
    config: &AuthConfig,
    status: u16,
    detail: &str,
    errcode: &str,
    errdata: &serde_json::Value,
    path: &str,
    report_json: &str,
) -> String {
    let home = {
        let configured = config.default_logout_redirect_uri.trim();
        if configured.is_empty() { "/" } else { configured }
    };
    let prefix = config.base_path.trim_end_matches('/');
    let prefix = if prefix.is_empty() { "/auth" } else { prefix };
    let sign_in = format!(
        "{prefix}/sign-in?next={}",
        urlencoding_minimal(path)
    );
    let sign_out = format!(
        "{prefix}/sign-out?redirect_uri={}",
        urlencoding_minimal(home)
    );
    let realm = realm_block(errdata);
    let (kind, icon, title, action) = if status == 401 {
        (
            "unauthorized",
            LOCK_SVG,
            "Sign in to continue",
            button(&sign_in, "Sign in", LOGIN_SVG),
        )
    } else if status == 403 && !realm.is_empty() {
        (
            "forbidden-realm",
            SHIELD_SVG,
            "Access denied",
            button(&sign_out, "Sign out", LOGOUT_SVG),
        )
    } else if status == 403 {
        (
            "forbidden",
            SHIELD_SVG,
            "Access denied",
            button(&sign_out, "Sign out", LOGOUT_SVG),
        )
    } else {
        (
            "failed",
            ALERT_SVG,
            if detail.is_empty() {
                "Couldn't load your session"
            } else {
                detail
            },
            button(home, "Home", ""),
        )
    };
    let message = visible_message(title, detail, errdata);
    let message_html = if message.is_empty() {
        String::new()
    } else {
        format!(
            r#"<p class="ld__card-message">{}</p>"#,
            html_escape(&message)
        )
    };
    // The failure action is already a Home button. A second Home control
    // belongs in the footer only when the primary action is Sign in.
    let footer_aside = if errcode == "AUT-197" {
        format!(
            r#"<a class="ld__card-signout" href="{href}">{icon}Sign out</a>"#,
            href = html_escape(&sign_out),
            icon = LOGOUT_SVG,
        )
    } else if status == 401 {
        format!(
            r#"<a class="ld__card-home" href="{href}">Home</a>"#,
            href = html_escape(home),
        )
    } else {
        String::new()
    };
    let identity = identity_html(errdata);
    let report_attr = html_escape(report_json);
    let mut html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
{css}
</style>
</head>
<body class="app-error">
<div id="static-loader-screen" class="loader-screen" data-rjs-error="{kind}">
  <main class="ld__card" role="alert">
    <div class="ld__card-header">
      <div class="ld__card-icon">{icon}</div>
      <h1 class="ld__card-title">{title}</h1>
      {message_html}
    </div>
    {identity}
    {realm}
    <div class="ld__card-actions">
      {action}
    </div>
    <footer class="ld__card-footer">
      <div class="ld__card-meta">
        <span class="ld__card-code"># {code}</span>
        <span class="ld__card-sep" aria-hidden="true"></span>
        <button type="button" class="ld__card-report" aria-label="Report" data-report="{report_attr}"><span aria-hidden="true">{flag}</span><span class="ld__card-report-label">Report</span></button>
        {footer_aside}
      </div>
    </footer>
  </main>
  <p class="ld__screen-note">This notice is from Riverbase authorization service.</p>
</div>
</body>
</html>"#,
        title = html_escape(title),
        css = AUTH_ERROR_CSS,
        kind = kind,
        icon = icon,
        message_html = message_html,
        identity = identity,
        realm = realm,
        action = action,
        code = html_escape(errcode),
        flag = FLAG_SVG,
        report_attr = report_attr,
        footer_aside = footer_aside,
    );
    html.push_str(AUTH_ERROR_SCRIPT);
    html
}

/// Rewrite an auth-route problem JSON error into the HTML card when `Accept` is not JSON.
pub async fn negotiate_auth_error_html(
    State(config): State<AuthConfig>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers().clone();
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    if accept_includes_json(&headers) || !crate::api_path::is_auth_route(&path, &config.base_path) {
        return response;
    }
    let status = response.status();
    if status.is_success() || status.is_redirection() {
        return response;
    }
    if response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .starts_with("text/html")
    {
        return response;
    }
    let cookies: Vec<_> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .cloned()
        .collect();
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, 64 * 1024).await.unwrap_or_default();
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    if value.get("errcode").is_none() && value.get("detail").is_none() {
        return Response::from_parts(parts, Body::from(bytes));
    }
    let detail = value
        .get("detail")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    let errcode = value
        .get("errcode")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    let code = if errcode.is_empty() {
        format!("HTTP {}", status.as_u16())
    } else {
        errcode.to_string()
    };
    let errdata = value.get("errdata").cloned().unwrap_or(serde_json::Value::Null);
    let report = serde_json::to_string_pretty(&value)
        .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
    let html = auth_error_document(&config, status.as_u16(), detail, &code, &errdata, &path, &report);
    let mut rendered = (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(html),
    )
        .into_response();
    for cookie in cookies {
        rendered.headers_mut().append(header::SET_COOKIE, cookie);
    }
    rendered
}

fn button(href: &str, label: &str, icon: &str) -> String {
    format!(
        r#"<a class="ld__card-btn ld__card-btn--primary" href="{href}">{icon}{label}</a>"#,
        href = html_escape(href),
        icon = icon,
        label = html_escape(label),
    )
}

fn realm_block(errdata: &serde_json::Value) -> String {
    let raw = if errdata.is_array() {
        Some(errdata)
    } else {
        errdata.get("realm_urls")
    };
    let Some(list) = raw.and_then(|value| value.as_array()) else {
        return String::new();
    };
    let mut items = String::new();
    for entry in list {
        let (href, name) = if let Some(href) = entry.as_str() {
            (href, href)
        } else {
            let href = entry.get("url").and_then(|value| value.as_str()).unwrap_or("");
            let name = entry
                .get("portal_name")
                .or_else(|| entry.get("name"))
                .and_then(|value| value.as_str())
                .unwrap_or(href);
            (href, name)
        };
        if !(href.starts_with("http://") || href.starts_with("https://")) {
            continue;
        }
        let host = href.split("://").nth(1).unwrap_or(href).split('/').next().unwrap_or(href);
        items.push_str(&format!(
            r#"<li><a class="ld__card-realm" href="{href}" rel="noopener noreferrer"><span class="ld__card-realm-text"><span class="ld__card-realm-name">{name}</span><span class="ld__card-realm-host">{host}</span></span></a></li>"#,
            href = html_escape(href),
            name = html_escape(name),
            host = html_escape(host),
        ));
    }
    if items.is_empty() {
        return String::new();
    }
    format!(
        r#"<div class="ld__card-realms"><p class="ld__card-realms-label">You have access to</p><ul class="ld__card-realm-list">{items}</ul></div>"#
    )
}

fn identity_html(errdata: &serde_json::Value) -> String {
    let Some(line) = identity_line(errdata) else {
        return String::new();
    };
    let initial = line.chars().next().unwrap_or('?').to_uppercase().to_string();
    format!(
        r#"<div class="ld__card-user"><span class="ld__card-avatar" aria-hidden="true">{initial}</span><div class="ld__card-user-text"><p class="ld__card-user-name">{line}</p></div></div>"#,
        initial = html_escape(&initial),
        line = html_escape(&line),
    )
}

fn urlencoding_minimal(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

const LOCK_SVG: &str = r#"<svg viewBox="0 0 24 24" aria-hidden="true"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>"#;
const LOGIN_SVG: &str = r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 3h4a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2h-4"/><polyline points="10 17 15 12 10 7"/><line x1="15" x2="3" y1="12" y2="12"/></svg>"#;
const LOGOUT_SVG: &str = r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><polyline points="16 17 21 12 16 7"/><line x1="21" x2="9" y1="12" y2="12"/></svg>"#;
const SHIELD_SVG: &str = r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m2 2 20 20"/><path d="M5 5a1 1 0 0 0-1 1v7c0 5 3.5 7.5 7.67 8.94a1 1 0 0 0 .67.01c2.35-.82 4.48-1.97 5.9-3.71"/></svg>"#;
const ALERT_SVG: &str = r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/></svg>"#;
const FLAG_SVG: &str = r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1z"/><line x1="4" x2="4" y1="22" y2="15"/></svg>"#;
const AUTH_ERROR_SCRIPT: &str = r#"<script>
(function () {
  var report = document.querySelector(".ld__card-report");
  if (report) {
    var label = report.querySelector(".ld__card-report-label");
    report.addEventListener("click", function () {
      var text = report.getAttribute("data-report") || "";
      var done = function (ok) {
        if (label) label.textContent = ok ? "Copied" : "Copy failed";
        setTimeout(function () { if (label) label.textContent = "Report"; }, 2000);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(function () { done(true); }, function () { done(false); });
      } else {
        done(false);
      }
    });
  }
})();
</script>
"#;

fn identity_line(errdata: &serde_json::Value) -> Option<String> {
    let principal = errdata.get("principal")?;
    let username = text_field(principal, "preferred_username");
    let email = text_field(principal, "email");
    match (username, email) {
        (Some(username), Some(email)) if username != email => Some(format!("{username} · {email}")),
        (_, Some(email)) => Some(email),
        (Some(username), _) => Some(username),
        _ => None,
    }
}

fn text_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::body::to_bytes;
    use axum::http::HeaderValue;
    use serde_json::json;
    use tower_sessions::{MemoryStore, Session};

    fn test_session() -> Session {
        Session::new(None, Arc::new(MemoryStore::default()), None)
    }

    fn headers(accept: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(accept) = accept {
            headers.insert(header::ACCEPT, HeaderValue::from_str(accept).unwrap());
        }
        headers
    }

    #[test]
    fn json_accept_is_exact_media_type() {
        assert!(accept_includes_json(&headers(Some("application/json"))));
        assert!(accept_includes_json(&headers(Some(
            "text/html, application/json;q=0.9"
        ))));
        assert!(!accept_includes_json(&headers(Some(
            "text/html,application/xhtml+xml"
        ))));
        assert!(!accept_includes_json(&headers(None)));
    }

    #[test]
    fn html_card_escapes_and_shows_errcode() {
        let mut config = AuthConfig::default();
        config.default_logout_redirect_uri = "/".into();
        let err = crate::errors::AUT_196.with_data(json!({
            "principal": {
                "sub": "user-1",
                "preferred_username": "alex",
                "email": "a<b@abx.example"
            }
        }));
        let html = forbidden_card_html(&config, &err);
        assert!(html.contains("# AUT-196"));
        assert!(html.contains("Access denied"));
        assert!(html.contains("a&lt;b@abx.example"));
        assert!(html.contains("Sign out"));
        assert_eq!(html.matches(">Home<").count(), 0);
        assert!(html.contains("ld__card-report"));
        assert!(html.contains("data-report"));
        assert!(!html.contains("a<b@abx.example"));
    }

    #[test]
    fn bad_request_card_has_one_home_control() {
        let mut config = AuthConfig::default();
        config.default_logout_redirect_uri = "/".into();
        let err = crate::errors::AUT_153.with_data(json!({ "detail": "Unknown mock user." }));
        let html = forbidden_card_html(&config, &err);
        assert!(html.contains("Missing authorization code"));
        assert!(html.contains("Unknown mock user."));
        assert_eq!(html.matches(">Home<").count(), 1);
        assert!(!html.contains("class=\"ld__card-home\""));
    }

    #[tokio::test]
    async fn callback_failure_returns_html_without_json_accept() {
        let session = test_session();
        session
            .insert("ses_user", json!({"sub": "user-1"}))
            .await
            .unwrap();
        let err = crate::errors::AUT_196.with_data(json!({}));
        let response = callback_failure_response(
            &session,
            &AuthConfig::default(),
            CookieJar::new(),
            &headers(Some("text/html")),
            err,
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(content_type.starts_with("text/html"));
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("AUT-196"));
        assert!(session
            .get::<serde_json::Value>("ses_user")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn callback_failure_returns_json_when_requested() {
        let session = test_session();
        let err = crate::errors::AUT_196.with_data(json!({"required_realm_role": "abx-stratify"}));
        let response = callback_failure_response(
            &session,
            &AuthConfig::default(),
            CookieJar::new(),
            &headers(Some("application/json")),
            err,
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(content_type.contains("json"));
        let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["errcode"], "AUT-196");
    }

    #[tokio::test]
    async fn replayed_callback_keeps_session() {
        let config = AuthConfig::default();
        let session = test_session();
        session
            .insert(&config.ses_user_field, json!({"sub": "user-1"}))
            .await
            .unwrap();
        let response = callback_failure_response(
            &session,
            &config,
            CookieJar::new(),
            &headers(Some("text/html")),
            crate::errors::AUT_151.with_data(json!({"detail": "invalid_grant"})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("AUT-197"));
        assert!(html.contains("You are already signed in."));
        assert!(html.contains(
            r#"<p class="ld__card-message">AUT-151: OAuth token exchange failed. — invalid_grant</p>"#
        ));
        assert!(!html.contains(r#"<p class="ld__card-message">You are already signed in.</p>"#));
        assert!(html.contains("ld__card-signout"));
        assert!(!html.contains("class=\"ld__card-home\""));
        assert!(html.contains("ld__card-sep"));
        assert!(!html.contains("ld__card-toggle"));
        assert!(html.contains("Riverbase authorization service."));
        assert!(session
            .get::<serde_json::Value>(&config.ses_user_field)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn failed_callback_without_session_offers_sign_in() {
        let session = test_session();
        let response = callback_failure_response(
            &session,
            &AuthConfig::default(),
            CookieJar::new(),
            &headers(Some("text/html")),
            crate::errors::AUT_150.with_data(json!({})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("AUT-198"));
        assert!(html.contains("sign-in"));
        assert!(html.contains("class=\"ld__card-home\""));
    }
}
