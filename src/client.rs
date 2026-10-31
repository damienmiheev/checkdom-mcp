//! HTTP client for the CheckDom dashboard's private RPC API.
//!
//! Every call is `POST <base><path>` with a JSON body of the shape
//! `{"data": {"action": "<action>", "langModule": "", ...params}}` and the
//! session token sent as the `session` cookie. See `docs/API.md`.

use crate::config;
use crate::error::{CheckDomError, Result};
use reqwest::cookie::{CookieStore, Jar};
use reqwest::{Client, Url};
use serde_json::{json, Value};
use std::sync::Arc;

const BASE: &str = "https://checkdom.in.ua";
const USER_AGENT: &str = concat!("checkdom-mcp/", env!("CARGO_PKG_VERSION"));

/// A client bound to (optionally) a session token.
pub struct CheckDomClient {
    http: Client,
    token: Option<String>,
}

impl CheckDomClient {
    /// A client with no token, used only for `login`.
    pub fn new() -> Result<Self> {
        Ok(Self { http: build_http()?, token: None })
    }

    /// A client that authenticates with an explicit token.
    pub fn with_token(token: String) -> Result<Self> {
        Ok(Self { http: build_http()?, token: Some(token) })
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
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(jar.clone())
            .build()?;

        // 1. Fetch the landing page and read the hidden CSRF token.
        let landing = http.get(BASE).send().await?.text().await?;
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

        // Build {"data": {"action", "langModule", ...params}}.
        let mut data = serde_json::Map::new();
        data.insert("action".into(), Value::String(action.to_string()));
        data.insert("langModule".into(), Value::String(String::new()));
        if let Value::Object(extra) = params {
            for (k, v) in extra {
                data.insert(k, v);
            }
        }
        let body = json!({ "data": Value::Object(data) });

        let resp = self
            .http
            .post(format!("{BASE}{path}"))
            .header(reqwest::header::COOKIE, format!("session={token}"))
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(CheckDomError::Unexpected(format!(
                "HTTP {status} from {path} (action {action})"
            )));
        }

        let parsed: Value = serde_json::from_str(&text).map_err(|_| {
            CheckDomError::Unexpected(format!("non-JSON response from {path} (action {action})"))
        })?;

        match parsed.get("code").and_then(Value::as_str).unwrap_or("") {
            "000" => Ok(parsed.get("data").cloned().unwrap_or(Value::Null)),
            "205" => Err(CheckDomError::SessionExpired),
            other => {
                let message = parsed
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
                    .to_string();
                Err(CheckDomError::Api { code: other.to_string(), message })
            }
        }
    }

    /// Cheap authenticated call to tell a valid session from an expired one.
    pub async fn check_session(&self) -> Result<bool> {
        match self
            .rpc("/modules/private/api/apiIndexController.php", "getUserHousekeepers", json!({}))
            .await
        {
            Ok(_) => Ok(true),
            Err(CheckDomError::SessionExpired | CheckDomError::NotLoggedIn) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

fn build_http() -> Result<Client> {
    Ok(Client::builder().user_agent(USER_AGENT).build()?)
}

/// Read one cookie value out of a `k=v; k2=v2` header string.
fn cookie_value(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

/// Extract the hidden `_csrf-landing` token from the login page HTML.
fn extract_csrf(html: &str) -> Option<String> {
    use regex::Regex;
    use std::sync::OnceLock;
    static RES: OnceLock<Vec<Regex>> = OnceLock::new();
    let res = RES.get_or_init(|| {
        vec![
            Regex::new(r#"name="_csrf-landing"[^>]*?value="([^"]*)""#).unwrap(),
            Regex::new(r#"value="([^"]*)"[^>]*?name="_csrf-landing""#).unwrap(),
        ]
    });
    res.iter()
        .find_map(|re| re.captures(html).and_then(|c| c.get(1)).map(|m| m.as_str().to_string()))
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
