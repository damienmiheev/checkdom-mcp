//! HTTP client for the CheckDom dashboard's private RPC API.
//!
//! Every call is `POST <base><path>` with a JSON body of the shape
//! `{"data": {"action": "<action>", "langModule": "", ...params}}` and the
//! session token sent as the `session` cookie. See `docs/API.md`.

use crate::config;
use crate::error::{CheckDomError, Result};
use reqwest::header::{HeaderMap, SET_COOKIE};
use reqwest::{Client, Url};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::time::Duration;

const BASE: &str = "https://checkdom.in.ua";
const HOST: &str = "checkdom.in.ua";
/// The login RPC the dashboard's login page calls (`userLogin`). The HTML form
/// that posts to `/login` is not how the site logs in.
const LOGIN_RPC: &str = "/modules/front/api/loginController.php";
const MAX_REDIRECTS: usize = 5;
/// The dashboard page; it embeds the logged-in user's id (`curUser.id = '…'`).
const DASHBOARD_PATH: &str = "/dash/";

/// The user id behind a session token, so the dashboard page is fetched once per
/// token rather than once per tool call.
static USER_ID_CACHE: Mutex<Option<(String, i64)>> = Mutex::new(None);
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

    /// Log in with the dashboard's `userLogin` RPC and return the session token,
    /// which CheckDom sets as the `session` cookie on the response.
    pub async fn login(&self, username: &str, password: &str) -> Result<String> {
        let resp = self
            .http
            .post(format!("{BASE}{LOGIN_RPC}"))
            .json(&rpc_body(
                "userLogin",
                json!({ "login": username, "pass": password }),
            ))
            .send()
            .await?;
        let status = resp.status();
        let session = session_from_set_cookie(resp.headers());
        let text = resp.text().await?;

        match parse_rpc_response(status, &text) {
            Ok(_) => {}
            Err(CheckDomError::Api { .. }) => {
                return Err(CheckDomError::Login(
                    "CheckDom rejected the CIF/email or password".into(),
                ));
            }
            Err(e) => return Err(e),
        }
        session.ok_or_else(|| {
            CheckDomError::Login("CheckDom accepted the login but set no session cookie".into())
        })
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

    /// The id of the logged-in user, which many actions take as `user_id` or
    /// `role_owner_id`. Read from the dashboard page and cached per token.
    pub async fn user_id(&self) -> Result<i64> {
        let token = self.token.as_deref().ok_or(CheckDomError::NotLoggedIn)?;
        if let Ok(cache) = USER_ID_CACHE.lock()
            && let Some((cached_token, id)) = cache.as_ref()
            && cached_token == token
        {
            return Ok(*id);
        }

        let resp = self
            .http
            .get(format!("{BASE}{DASHBOARD_PATH}"))
            .header(reqwest::header::COOKIE, format!("session={token}"))
            .send()
            .await?;
        // Without a valid session the dashboard redirects to the landing page.
        if !resp.url().path().starts_with(DASHBOARD_PATH) {
            return Err(CheckDomError::SessionExpired);
        }
        let status = resp.status();
        let html = resp.text().await?;
        if !status.is_success() {
            return Err(CheckDomError::Unexpected(format!(
                "HTTP {status} from {DASHBOARD_PATH}"
            )));
        }
        let id = parse_user_id(&html).ok_or_else(|| {
            CheckDomError::Unexpected("could not find your user id on the dashboard page".into())
        })?;
        if let Ok(mut cache) = USER_ID_CACHE.lock() {
            *cache = Some((token.to_string(), id));
        }
        Ok(id)
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

/// The user id from the dashboard page's inline `curUser.id = '1234'`.
fn parse_user_id(html: &str) -> Option<i64> {
    html.match_indices("curUser.id").find_map(|(at, needle)| {
        let rest = html[at + needle.len()..].trim_start().strip_prefix('=')?;
        let rest = rest.trim_start().trim_start_matches(['\'', '"']);
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok().filter(|id| *id > 0)
    })
}

/// The value of the `session` cookie set by a response, if any. A cleared
/// cookie (`session=deleted` or empty) does not count.
fn session_from_set_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next()?.trim().split_once('='))
        .filter(|(name, value)| *name == "session" && !value.is_empty() && *value != "deleted")
        .map(|(_, value)| value.to_string())
        .next_back()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn session_cookie_is_read_from_set_cookie_headers() {
        use reqwest::header::HeaderValue;
        let mut h = HeaderMap::new();
        for v in [
            "site_lang=ua; path=/",
            "session=abc.DEF-123; path=/; secure; HttpOnly",
            "name=%D0%94; path=/",
        ] {
            h.append(SET_COOKIE, HeaderValue::from_static(v));
        }
        assert_eq!(session_from_set_cookie(&h).as_deref(), Some("abc.DEF-123"));

        let mut cleared = HeaderMap::new();
        cleared.append(
            SET_COOKIE,
            HeaderValue::from_static("session=deleted; expires=Thu, 01 Jan 1970 00:00:01 GMT"),
        );
        cleared.append(SET_COOKIE, HeaderValue::from_static("session=; path=/"));
        assert_eq!(session_from_set_cookie(&cleared), None);
        assert_eq!(session_from_set_cookie(&HeaderMap::new()), None);
    }

    #[test]
    fn user_id_is_read_from_the_dashboard_page() {
        let page = "<script>\nvar curUser = {}\ncurUser.id = '4321'\ncurUser.name = 'A'\n</script>";
        assert_eq!(parse_user_id(page), Some(4321));
        assert_eq!(parse_user_id(r#"curUser.id="77";"#), Some(77));
        // Comparisons and empty values elsewhere in the page are not assignments.
        assert_eq!(
            parse_user_id("if (x == curUser.id) {}\ncurUser.id = '9'"),
            Some(9)
        );
        assert_eq!(parse_user_id("curUser.id = ''"), None);
        assert_eq!(parse_user_id("curUser.id = '0'"), None);
        assert_eq!(parse_user_id("<html>landing page</html>"), None);
    }

    #[test]
    fn login_body_is_a_user_login_rpc() {
        let body = rpc_body("userLogin", json!({ "login": "u", "pass": "p" }));
        assert_eq!(body["data"]["action"], "userLogin");
        assert_eq!(body["data"]["login"], "u");
        assert_eq!(body["data"]["pass"], "p");
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
