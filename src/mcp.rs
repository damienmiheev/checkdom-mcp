//! Minimal MCP server over stdio.
//!
//! Transport is newline-delimited JSON-RPC 2.0: one JSON message per line on
//! stdin, one response per line on stdout. Logs go to stderr so they never
//! corrupt the protocol stream.

use crate::tools;
use anyhow::Result;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Protocol version used when the client does not specify one.
const DEFAULT_PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC error as `(code, message)`.
type RpcError = (i64, String);

/// Run the server until stdin closes.
pub async fn serve(allow_writes: bool) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    tracing::info!(allow_writes, "checkdom-mcp server started");

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("ignoring invalid JSON on stdin: {e}");
                continue;
            }
        };

        let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();

        // Requests carry an `id`; notifications do not and get no response.
        let Some(id) = msg.get("id").cloned() else {
            tracing::debug!("notification: {method}");
            continue;
        };

        let response = match handle_request(&method, msg.get("params"), allow_writes).await {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
            }
        };

        let mut out = serde_json::to_string(&response)?;
        out.push('\n');
        stdout.write_all(out.as_bytes()).await?;
        stdout.flush().await?;
    }

    tracing::info!("stdin closed; shutting down");
    Ok(())
}

async fn handle_request(
    method: &str,
    params: Option<&Value>,
    allow_writes: bool,
) -> std::result::Result<Value, RpcError> {
    match method {
        "initialize" => {
            let protocol = params
                .and_then(|p| p.get("protocolVersion"))
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_PROTOCOL_VERSION)
                .to_string();
            Ok(json!({
                "protocolVersion": protocol,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "checkdom-mcp", "version": env!("CARGO_PKG_VERSION") }
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools::list(allow_writes) })),
        "tools/call" => {
            let params = params.ok_or((-32602, "missing params".to_string()))?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or((-32602, "missing tool name".to_string()))?;
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));

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
        other => Err((-32601, format!("method not found: {other}"))),
    }
}
