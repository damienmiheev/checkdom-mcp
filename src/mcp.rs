//! Minimal MCP server over stdio.
//!
//! Transport is newline-delimited JSON-RPC 2.0: one JSON message per line on
//! stdin, one response per line on stdout. Logs go to stderr so they never
//! corrupt the protocol stream.

use crate::tools;
use anyhow::Result;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// MCP protocol versions this server speaks, newest first. The server only
/// uses `tools` with text content, which is the same in all of them.
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

// JSON-RPC 2.0 error codes.
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// JSON-RPC error as `(code, message)`.
type RpcError = (i64, String);

/// Run the server until stdin closes.
pub async fn serve(allow_writes: bool) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    tracing::info!(allow_writes, "checkdom-mcp server started");

    while let Some(line) = lines.next_line().await? {
        let Some(response) = handle_line(&line, allow_writes).await else {
            continue;
        };
        let mut out = serde_json::to_string(&response)?;
        out.push('\n');
        stdout.write_all(out.as_bytes()).await?;
        stdout.flush().await?;
    }

    tracing::info!("stdin closed; shutting down");
    Ok(())
}

/// Handle one line of input. Returns the response to write, or `None` for
/// blank lines and notifications.
async fn handle_line(line: &str, allow_writes: bool) -> Option<Value> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("invalid JSON on stdin: {e}");
            return Some(error_response(Value::Null, PARSE_ERROR, "parse error"));
        }
    };

    let Some(obj) = msg.as_object() else {
        return Some(error_response(
            Value::Null,
            INVALID_REQUEST,
            "expected a JSON-RPC object",
        ));
    };

    // Requests carry an `id`; notifications do not and get no response.
    let Some(id) = obj.get("id").cloned() else {
        tracing::debug!("notification: {:?}", obj.get("method"));
        return None;
    };

    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Some(error_response(id, INVALID_REQUEST, "missing method"));
    };

    Some(
        match handle_request(method, obj.get("params"), allow_writes).await {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error_response(id, code, &message),
        },
    )
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

async fn handle_request(
    method: &str,
    params: Option<&Value>,
    allow_writes: bool,
) -> std::result::Result<Value, RpcError> {
    match method {
        "initialize" => {
            let requested = params
                .and_then(|p| p.get("protocolVersion"))
                .and_then(Value::as_str);
            Ok(json!({
                "protocolVersion": negotiate_protocol(requested),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "checkdom-mcp", "version": env!("CARGO_PKG_VERSION") }
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::list(allow_writes) })),
        "tools/call" => {
            let params = params.ok_or((INVALID_PARAMS, "missing params".to_string()))?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or((INVALID_PARAMS, "missing tool name".to_string()))?;
            if !tools::exists(name) {
                return Err((INVALID_PARAMS, format!("unknown tool: {name}")));
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));

            match tools::call(name, args, allow_writes).await {
                Ok(text) => Ok(json!({ "content": [ { "type": "text", "text": text } ] })),
                // Tool failures are returned as a tool result with `isError`, so the
                // model can read the message and react, rather than as a protocol error.
                Err(e) => Ok(json!({
                    "content": [ { "type": "text", "text": e.to_string() } ],
                    "isError": true
                })),
            }
        }
        other => Err((METHOD_NOT_FOUND, format!("method not found: {other}"))),
    }
}

/// Echo the client's version when supported, otherwise offer our newest
/// (the client then decides whether to continue).
fn negotiate_protocol(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|r| SUPPORTED_PROTOCOL_VERSIONS.iter().find(|v| **v == r))
        .copied()
        .unwrap_or(SUPPORTED_PROTOCOL_VERSIONS[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn reply(line: &str) -> Value {
        handle_line(line, false).await.unwrap()
    }

    #[test]
    fn negotiates_supported_or_newest_version() {
        assert_eq!(negotiate_protocol(Some("2024-11-05")), "2024-11-05");
        assert_eq!(
            negotiate_protocol(Some("1999-01-01")),
            SUPPORTED_PROTOCOL_VERSIONS[0]
        );
        assert_eq!(negotiate_protocol(None), SUPPORTED_PROTOCOL_VERSIONS[0]);
    }

    #[tokio::test]
    async fn initialize_reports_tools_capability() {
        let r = reply(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        )
        .await;
        assert_eq!(r["id"], 1);
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert_eq!(r["result"]["serverInfo"]["name"], "checkdom-mcp");
    }

    #[tokio::test]
    async fn blank_lines_and_notifications_get_no_reply() {
        assert!(handle_line("   ", false).await.is_none());
        let n = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert!(handle_line(n, false).await.is_none());
    }

    #[tokio::test]
    async fn invalid_json_is_a_parse_error() {
        let r = reply("{not json").await;
        assert_eq!(r["error"]["code"], PARSE_ERROR);
        assert_eq!(r["id"], Value::Null);
    }

    #[tokio::test]
    async fn non_object_and_missing_method_are_invalid_requests() {
        assert_eq!(reply("[1,2]").await["error"]["code"], INVALID_REQUEST);
        let r = reply(r#"{"jsonrpc":"2.0","id":"a"}"#).await;
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
        assert_eq!(r["id"], "a");
    }

    #[tokio::test]
    async fn unknown_method_and_tool_are_protocol_errors() {
        let r = reply(r#"{"jsonrpc":"2.0","id":2,"method":"resources/list"}"#).await;
        assert_eq!(r["error"]["code"], METHOD_NOT_FOUND);

        let r = reply(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"payByClientCard"}}"#,
        )
        .await;
        assert_eq!(r["error"]["code"], INVALID_PARAMS);
    }

    #[tokio::test]
    async fn write_tool_is_refused_without_allow_writes() {
        let r = reply(
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"comment_on_support_request","arguments":{"request_id":1,"comment_text":"x"}}}"#,
        )
        .await;
        assert_eq!(r["result"]["isError"], true);
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("--allow-writes"), "{text}");
    }

    #[tokio::test]
    async fn tools_list_hides_write_tools_by_default() {
        let r = reply(r#"{"jsonrpc":"2.0","id":5,"method":"tools/list"}"#).await;
        let names: Vec<&str> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"list_support_requests"));
        assert!(!names.contains(&"create_support_request"));
    }
}
