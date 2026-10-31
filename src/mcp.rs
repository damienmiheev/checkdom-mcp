//! Minimal MCP server over stdio.
//!
//! Transport is newline-delimited JSON-RPC 2.0: one JSON message per line on
//! stdin, one response per line on stdout. Logs go to stderr so they never
//! corrupt the protocol stream.

use crate::tools;
use anyhow::Result;
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};

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

/// Longest accepted input line. Longer lines are answered with an error and
/// discarded, so one message can't grow memory without bound.
const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Run the server until stdin closes.
pub async fn serve(allow_writes: bool) -> Result<()> {
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();

    eprintln!(
        "checkdom-mcp: server started (writes {})",
        if allow_writes { "enabled" } else { "disabled" }
    );

    let mut line = Vec::new();
    while read_line_capped(&mut stdin, &mut line).await? {
        let response = if line.len() > MAX_LINE_BYTES {
            eprintln!("checkdom-mcp: discarded an input line over {MAX_LINE_BYTES} bytes");
            Some(error_response(
                Value::Null,
                INVALID_REQUEST,
                "message too large",
            ))
        } else {
            handle_line(&line, allow_writes).await
        };
        if let Some(response) = response {
            let mut out = serde_json::to_vec(&response)?;
            out.push(b'\n');
            stdout.write_all(&out).await?;
            stdout.flush().await?;
        }
    }

    eprintln!("checkdom-mcp: stdin closed; shutting down");
    Ok(())
}

/// Read one `\n`-terminated line into `buf`, keeping at most
/// `MAX_LINE_BYTES + 1` bytes of it (enough to tell it was too long).
/// Returns `false` at end of input.
async fn read_line_capped<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> std::io::Result<bool> {
    buf.clear();
    let mut read_any = false;
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(read_any);
        }
        read_any = true;
        let (part, done) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (&chunk[..i], Some(i + 1)),
            None => (chunk, None),
        };
        let room = (MAX_LINE_BYTES + 1).saturating_sub(buf.len());
        buf.extend_from_slice(&part[..part.len().min(room)]);
        let consumed = done.unwrap_or(chunk.len());
        reader.consume(consumed);
        if done.is_some() {
            return Ok(true);
        }
    }
}

/// Handle one line of input. Returns the response to write, or `None` for
/// blank lines, notifications and client responses.
async fn handle_line(line: &[u8], allow_writes: bool) -> Option<Value> {
    if line.trim_ascii().is_empty() {
        return None;
    }

    let msg: Value = match serde_json::from_slice(line) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("checkdom-mcp: invalid JSON on stdin: {e}");
            return Some(error_response(Value::Null, PARSE_ERROR, "parse error"));
        }
    };

    match msg {
        // JSON-RPC batch (MCP 2025-03-26): one array of replies, none if all
        // were notifications.
        Value::Array(batch) if batch.is_empty() => {
            Some(error_response(Value::Null, INVALID_REQUEST, "empty batch"))
        }
        Value::Array(batch) => {
            let mut replies = Vec::new();
            for m in batch {
                if let Some(r) = handle_message(m, allow_writes).await {
                    replies.push(r);
                }
            }
            (!replies.is_empty()).then_some(Value::Array(replies))
        }
        m => handle_message(m, allow_writes).await,
    }
}

/// Handle one JSON-RPC message (not a batch).
async fn handle_message(msg: Value, allow_writes: bool) -> Option<Value> {
    let Value::Object(obj) = msg else {
        return Some(error_response(
            Value::Null,
            INVALID_REQUEST,
            "expected a JSON-RPC object",
        ));
    };

    let method = obj.get("method").and_then(Value::as_str);
    let id = obj.get("id").cloned();

    // A response from the client (to a request we never send): never answered.
    if method.is_none() && (obj.contains_key("result") || obj.contains_key("error")) {
        return None;
    }
    // Notifications carry no `id` and get no response.
    let id = id?;
    // MCP ids are strings or integers, never null.
    if !(id.is_string() || id.is_i64() || id.is_u64()) {
        return Some(error_response(Value::Null, INVALID_REQUEST, "invalid id"));
    }
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(error_response(
            id,
            INVALID_REQUEST,
            "jsonrpc must be \"2.0\"",
        ));
    }
    let Some(method) = method else {
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
        handle_line(line.as_bytes(), false).await.unwrap()
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
        assert!(handle_line(b"   ", false).await.is_none());
        let n = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert!(handle_line(n.as_bytes(), false).await.is_none());
        // A client response is not answered either.
        let r = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
        assert!(handle_line(r.as_bytes(), false).await.is_none());
    }

    #[tokio::test]
    async fn invalid_utf8_is_a_parse_error_not_a_crash() {
        let r = handle_line(b"\xff\xfe{}", false).await.unwrap();
        assert_eq!(r["error"]["code"], PARSE_ERROR);
    }

    #[tokio::test]
    async fn null_or_object_ids_and_wrong_version_are_invalid() {
        for line in [
            r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":{"a":1},"method":"ping"}"#,
            r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#,
            r#"{"id":1,"method":"ping"}"#,
        ] {
            assert_eq!(
                reply(line).await["error"]["code"],
                INVALID_REQUEST,
                "{line}"
            );
        }
    }

    #[tokio::test]
    async fn batches_get_one_array_of_replies() {
        let r = reply(
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","id":"b","method":"nope"}]"#,
        )
        .await;
        let replies = r.as_array().unwrap();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[1]["error"]["code"], METHOD_NOT_FOUND);

        assert_eq!(reply("[]").await["error"]["code"], INVALID_REQUEST);
        let only_notifications = r#"[{"jsonrpc":"2.0","method":"notifications/initialized"}]"#;
        assert!(
            handle_line(only_notifications.as_bytes(), false)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn long_lines_are_capped_and_the_next_line_still_reads() {
        let mut input = vec![b'x'; MAX_LINE_BYTES + 10];
        input.extend_from_slice(b"\nnext\nlast");
        let mut reader = &input[..];
        let mut buf = Vec::new();
        assert!(read_line_capped(&mut reader, &mut buf).await.unwrap());
        assert_eq!(buf.len(), MAX_LINE_BYTES + 1);
        assert!(read_line_capped(&mut reader, &mut buf).await.unwrap());
        assert_eq!(buf, b"next");
        assert!(read_line_capped(&mut reader, &mut buf).await.unwrap());
        assert_eq!(buf, b"last");
        assert!(!read_line_capped(&mut reader, &mut buf).await.unwrap());
    }

    #[tokio::test]
    async fn invalid_json_is_a_parse_error() {
        let r = reply("{not json").await;
        assert_eq!(r["error"]["code"], PARSE_ERROR);
        assert_eq!(r["id"], Value::Null);
    }

    #[tokio::test]
    async fn non_object_and_missing_method_are_invalid_requests() {
        let r = reply("[1,2]").await;
        assert_eq!(r[0]["error"]["code"], INVALID_REQUEST);
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
