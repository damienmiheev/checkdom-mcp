//! HTTP client for the CheckDom dashboard's private RPC API.
//!
//! Every call is `POST <base><path>` with a JSON body of the shape
//! `{"data": {"action": "<action>", "langModule": "", ...params}}` and the
//! session token sent as the `session` cookie. See `docs/API.md`.

use crate::config;
use crate::error::{CheckDomError, Result};
use reqwest::cookie::{CookieStore, Jar};
use reqwest::{Client, Url};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

const BASE: &str = "https://checkdom.in.ua";
const HOST: &str = "checkdom.in.ua";
const MAX_REDIRECTS: usize = 5;
const USER_AGENT: &str = concat!("checkdom-mcp/", env!("CARGO_PKG_VERSION"));
/// Upper bound for one whole request, so a stalled server can't hang a tool call.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A client bound to (optionally) a session token.
pub struct CheckDomClient {
    http: Client,
    token: Option<String>,
}

impl CheckDomClient {
    /// A client with no token, used only for `login`.
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: build_http()?,
            token: None,
        })
    }

    /// A client that authenticates with an explicit token.
    pub fn with_token(token: String) -> Result<Self> {
        Ok(Self {
            http: build_http()?,
            token: Some(token),
        })
    }

    /// A client that reads the token stored locally by `login`.
    pub fn from_stored() -> Result<Self> {
        let token = config::load_token()
            .map_err(|e| CheckDomError::Unexpected(e.to_string()))?
            .ok_or(CheckDomError::NotLoggedIn)?;
        Self::with_token(token)
    }

    /// Perform the login flow and return the session token.
    pub async fn login(&self, username: &str, password: &str) -> Result<String> {
        // A dedicated client with a cookie jar, so the CSRF cookie set by the
        // landing page is sent back with the login POST.
        let jar = Arc::new(Jar::default());
        let http = http_builder().cookie_provider(jar.clone()).build()?;

        // 1. Fetch the landing page and read the hidden CSRF token.
        let landing = http
            .get(BASE)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let csrf = extract_csrf(&landing).ok_or_else(|| {
            CheckDomError::Login("could not find the CSRF token on the login page".into())
        })?;

        // 2. Submit the login form.
        let resp = http
            .post(format!("{BASE}/login"))
            .form(&[
                ("_csrf-landing", csrf.as_str()),
                ("LoginForm[username]", username),
                ("LoginForm[password]", password),
            ])
            .send()
            .await?;
        // Drain the body; the cookies we care about are in the jar.
        let _ = resp.text().await?;

        // 3. Read the `session` cookie the jar captured.
        let url = Url::parse(BASE).map_err(|e| CheckDomError::Unexpected(e.to_string()))?;
        let session = jar
            .cookies(&url)
            .and_then(|hv| hv.to_str().ok().map(str::to_owned))
            .and_then(|cookies| cookie_value(&cookies, "session"))
            .ok_or_else(|| {
                CheckDomError::Login(
                    "no session cookie returned — check your CIF/email and password".into(),
                )
            })?;
        Ok(session)
    }

    /// Call an RPC action and return its `data` payload on success.
    pub async fn rpc(&self, path: &str, action: &str, params: Value) -> Result<Value> {
        let token = self.token.as_deref().ok_or(CheckDomError::NotLoggedIn)?;

        let body = rpc_body(action, params);

        let resp = self
            .http
            .post(format!("{BASE}{path}"))
            .header(reqwest::header::COOKIE, format!("session={token}"))
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;
        parse_rpc_response(status, &text)
            .map_err(|e| e.with_context(&format!("{path} (action {action})")))
    }

    /// Cheap authenticated call to tell a valid session from an expired one.
    pub async fn check_session(&self) -> Result<bool> {
        match self
            .rpc(
                "/modules/private/api/apiIndexController.php",
                "getUserHousekeepers",
                json!({}),
            )
            .await
        {
            Ok(_) => Ok(true),
            Err(CheckDomError::SessionExpired | CheckDomError::NotLoggedIn) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

fn http_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .user_agent(USER_AGENT)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if follows_redirect_to(attempt.url()) {
                attempt.follow()
            } else {
                // A 307/308 would replay the login form, password included.
                attempt.stop()
            }
        }))
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
}

fn build_http() -> Result<Client> {
    Ok(http_builder().build()?)
}

/// Redirects are followed only to CheckDom itself, over HTTPS.
fn follows_redirect_to(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some(HOST) && url.port().is_none()
}

/// Build `{"data": {...params, "action", "langModule"}}`.
///
/// `action` and `langModule` are inserted last, so a tool argument can never
/// override which action is called (the controller path is fixed by the tool).
fn rpc_body(action: &str, params: Value) -> Value {
    let mut data = match params {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    data.insert("action".into(), Value::String(action.to_string()));
    data.insert("langModule".into(), Value::String(String::new()));
    json!({ "data": Value::Object(data) })
}

/// Map an RPC response to its `data` payload or a typed error (docs/API.md).
///
/// The raw body is never put into an error: it could carry account data the
/// model should only see through a tool's own result.
fn parse_rpc_response(status: reqwest::StatusCode, body: &str) -> Result<Value> {
    if !status.is_success() {
        return Err(CheckDomError::Unexpected(format!("HTTP {status}")));
    }
    let mut parsed: Value = serde_json::from_str(body)
        .map_err(|_| CheckDomError::Unexpected("non-JSON response".into()))?;

    match parsed.get("code").and_then(Value::as_str).unwrap_or("") {
        "000" => Ok(parsed
            .get_mut("data")
            .map(Value::take)
            .unwrap_or(Value::Null)),
        "205" => Err(CheckDomError::SessionExpired),
        other => Err(CheckDomError::Api {
            code: other.to_string(),
            message: parsed
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string(),
        }),
    }
}

/// Read one cookie value out of a `k=v; k2=v2` header string.
fn cookie_value(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

/// Extract the hidden `_csrf-landing` token from the login page HTML: the
/// `value` attribute of the tag that has `name="_csrf-landing"`, in either
/// attribute order.
fn extract_csrf(html: &str) -> Option<String> {
    html.match_indices(r#"name="_csrf-landing""#)
        .find_map(|(at, _)| {
            let start = html[..at].rfind('<')?;
            let end = at + html[at..].find('>')?;
            attr_value(&html[start..end], "value")
        })
        .filter(|v| !v.is_empty())
}

/// The double-quoted value of attribute `name` inside one HTML tag.
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    tag.match_indices(&needle).find_map(|(at, _)| {
        // Must be a whole attribute name, not the tail of e.g. `data-value`.
        if !tag[..at].ends_with(|c: char| c.is_ascii_whitespace()) {
            return None;
        }
        let rest = &tag[at + needle.len()..];
        rest.find('"').map(|len| rest[..len].to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn reads_a_named_cookie() {
        let h = "site_lang=ua; session=abc.def; name=x";
        assert_eq!(cookie_value(h, "session").as_deref(), Some("abc.def"));
        assert_eq!(cookie_value(h, "missing"), None);
    }

    #[test]
    fn finds_csrf_token_either_attribute_order() {
        let a = r#"<input type="hidden" name="_csrf-landing" value="TOKEN123==">"#;
        assert_eq!(extract_csrf(a).as_deref(), Some("TOKEN123=="));
        let b = r#"<input value="ABC==" name="_csrf-landing">"#;
        assert_eq!(extract_csrf(b).as_deref(), Some("ABC=="));
        assert_eq!(extract_csrf("<p>no token here</p>"), None);
    }

    #[test]
    fn csrf_ignores_other_tags_and_lookalike_attributes() {
        let html = r#"<meta name="csrf-param" content="_csrf-landing">
<meta name="csrf-token" content="META">
<form><input type="hidden" data-value="WRONG"
    name="_csrf-landing"
    value="RIGHT=="></form>"#;
        assert_eq!(extract_csrf(html).as_deref(), Some("RIGHT=="));
        assert_eq!(
            extract_csrf(r#"<input name="_csrf-landing" value="">"#),
            None
        );
        assert_eq!(
            extract_csrf(r#"<input name="_csrf-landing" value="unterminated>"#),
            None
        );
    }

    #[test]
    fn rpc_success_returns_data() {
        let body = r#"{"code":"000","message":"Success","data":{"result":[1,2],"total":2}}"#;
        let data = parse_rpc_response(StatusCode::OK, body).unwrap();
        assert_eq!(data["total"], 2);
    }

    #[test]
    fn rpc_success_without_data_is_null() {
        let data = parse_rpc_response(StatusCode::OK, r#"{"code":"000"}"#).unwrap();
        assert_eq!(data, Value::Null);
    }

    #[test]
    fn rpc_code_205_is_session_expired() {
        let err = parse_rpc_response(StatusCode::OK, r#"{"code":"205","message":"x"}"#);
        assert!(matches!(err, Err(CheckDomError::SessionExpired)));
    }

    #[test]
    fn rpc_other_code_is_api_error_with_message() {
        let err = parse_rpc_response(StatusCode::OK, r#"{"code":"101","message":"Помилка"}"#);
        match err {
            Err(CheckDomError::Api { code, message }) => {
                assert_eq!(code, "101");
                assert_eq!(message, "Помилка");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn rpc_http_error_and_html_never_leak_the_body() {
        let secret = "<html>account 12345</html>";
        for status in [StatusCode::INTERNAL_SERVER_ERROR, StatusCode::OK] {
            let err = parse_rpc_response(status, secret).unwrap_err().to_string();
            assert!(!err.contains("12345"), "{err}");
        }
    }

    #[test]
    fn redirects_only_within_checkdom_over_https() {
        let ok = |u: &str| follows_redirect_to(&Url::parse(u).unwrap());
        assert!(ok("https://checkdom.in.ua/dash/"));
        assert!(!ok("http://checkdom.in.ua/dash/"));
        assert!(!ok("https://checkdom.in.ua:8443/"));
        assert!(!ok("https://evil.example/login"));
        assert!(!ok("https://checkdom.in.ua.evil.example/"));
        assert!(!ok("https://be.checkdom.in.ua/api"));
    }

    #[test]
    fn tool_params_cannot_override_the_action() {
        let body = rpc_body(
            "getCommentsByRequestId",
            json!({ "id": 7, "action": "changeStatus", "langModule": "x" }),
        );
        assert_eq!(body["data"]["action"], "getCommentsByRequestId");
        assert_eq!(body["data"]["langModule"], "");
        assert_eq!(body["data"]["id"], 7);
    }
}
