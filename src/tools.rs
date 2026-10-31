//! MCP tool definitions and dispatch.
//!
//! Read tools are always available. Write tools are only listed and callable
//! when the server is started with `--allow-writes`.

use crate::client::CheckDomClient;
use crate::error::{CheckDomError, Result};
use serde_json::{json, Value};

// RPC controller paths (see docs/API.md).
const IDX: &str = "/modules/private/api/apiIndexController.php";
const PROP: &str = "/modules/dash/api/apiProperty.php";
const DEBT: &str = "/modules/houseKeeper/api/apiDebtStatusController.php";
const INV: &str = "/modules/private/api/apiInvoicesController.php";
const SUP: &str = "/modules/supportRequests/api/apiSupportRequest.php";
const MSG: &str = "/modules/internalMessages/api/apiInternalMessages.php";
const VOTE: &str = "/modules/voting/api/apiVotingController.php";

/// The tool list advertised over `tools/list`.
pub fn list(allow_writes: bool) -> Value {
    let mut tools = read_tools();
    if allow_writes {
        tools.extend(write_tools());
    }
    Value::Array(tools)
}

/// Dispatch a `tools/call`. Returns the pretty-printed JSON payload as text.
pub async fn call(name: &str, args: Value, allow_writes: bool) -> Result<String> {
    // Gate writes before touching the network.
    if is_write_tool(name) && !allow_writes {
        return Err(CheckDomError::Unexpected(
            "write tools are disabled; restart the server with --allow-writes to enable them"
                .into(),
        ));
    }

    let client = CheckDomClient::from_stored()?;

    let data = match name {
        "list_housekeepers" => client.rpc(IDX, "getUserHousekeepers", json!({})).await?,
        "list_houses" => client.rpc(PROP, "getUserHouses", json!({})).await?,
        "get_debt_status" => {
            let params = json!({
                "hk_id": req_i64(&args, "hk_id")?,
                "house_id": opt(&args, "house_id"),
                "section": opt(&args, "section"),
                "floor": opt(&args, "floor"),
            });
            client.rpc(DEBT, "getDebtStatus", params).await?
        }
        "list_invoices" => {
            client.rpc(INV, "getIncomingInvoicesPagination", pagination(&args)).await?
        }
        "list_support_requests" => {
            client.rpc(SUP, "getSupportRequestsPagination", pagination(&args)).await?
        }
        "get_support_request_comments" => {
            client.rpc(SUP, "getCommentsByRequestId", json!({ "id": req_i64(&args, "id")? })).await?
        }
        "list_messages" => {
            let mut p = pagination(&args);
            if let Value::Object(ref mut m) = p {
                m.insert("role_owner_id".into(), opt(&args, "role_owner_id"));
                m.insert("role_id".into(), opt(&args, "role_id"));
            }
            client.rpc(MSG, "getMessagesPagination", p).await?
        }
        "list_votings" => client.rpc(VOTE, "getVotingsPagination", pagination(&args)).await?,

        "create_support_request" => {
            let params = as_object(args)?;
            client.rpc(SUP, "saveSupportRequest", params).await?
        }
        "comment_on_support_request" => {
            let params = json!({
                "request_id": req_i64(&args, "request_id")?,
                "comment_text": req_str(&args, "comment_text")?,
                "comment_type": 0,
            });
            client.rpc(SUP, "saveComment", params).await?
        }

        other => return Err(CheckDomError::Unexpected(format!("unknown tool: {other}"))),
    };

    serde_json::to_string_pretty(&data).map_err(CheckDomError::from)
}

fn is_write_tool(name: &str) -> bool {
    matches!(name, "create_support_request" | "comment_on_support_request")
}

fn read_tools() -> Vec<Value> {
    vec![
        tool(
            "list_housekeepers",
            "List your building managers (домоуправителі / ОСББ) and their ids. Many other \
             tools take an `hk_id` from here.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "list_houses",
            "List your properties / houses registered on CheckDom.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "get_debt_status",
            "Get the balance / debt state (платіжний радар) for a building.",
            json!({
                "type": "object",
                "properties": {
                    "hk_id": { "type": "integer", "description": "Housekeeper id from list_housekeepers." },
                    "house_id": { "type": "integer", "description": "Optional house filter." },
                    "section": { "type": "string", "description": "Optional section filter." },
                    "floor": { "type": "string", "description": "Optional floor filter." }
                },
                "required": ["hk_id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "list_invoices",
            "List incoming invoices (рахунки).",
            pagination_schema(),
        ),
        tool(
            "list_support_requests",
            "List your support requests (заявки) with their status and SLA.",
            pagination_schema(),
        ),
        tool(
            "get_support_request_comments",
            "Get the comments on one support request.",
            json!({
                "type": "object",
                "properties": { "id": { "type": "integer", "description": "Support request id." } },
                "required": ["id"],
                "additionalProperties": false
            }),
        ),
        tool(
            "list_messages",
            "List internal messages / announcements.",
            json!({
                "type": "object",
                "properties": {
                    "page": { "type": "integer", "default": 1 },
                    "per_page": { "type": "integer", "default": 25 },
                    "role_owner_id": { "type": "integer", "description": "Required by the endpoint; from list_housekeepers." },
                    "role_id": { "type": "integer", "description": "Required by the endpoint." }
                },
                "additionalProperties": false
            }),
        ),
        tool(
            "list_votings",
            "List building votes (голосування).",
            pagination_schema(),
        ),
    ]
}

fn write_tools() -> Vec<Value> {
    vec![
        tool(
            "create_support_request",
            "Create a new support request (заявка). WRITE action. Requires the full set of \
             fields the dashboard sends (see docs/API.md: saveSupportRequest) — notably \
             user_id, author_user_id, property_id, property_type, support_id (hk id), category, \
             sub_category, and description (1–999 chars). Confirm details with the user first.",
            json!({
                "type": "object",
                "properties": {
                    "user_id": { "type": "integer" },
                    "author_user_id": { "type": "integer" },
                    "property_id": { "type": "integer" },
                    "property_type": { "type": "integer" },
                    "support_id": { "type": "integer", "description": "Housekeeper id." },
                    "category": { "type": "integer" },
                    "sub_category": { "type": "integer" },
                    "description": { "type": "string", "maxLength": 999 },
                    "request_entity_type": { "type": "integer" },
                    "subscriber_id": { "type": "integer" }
                },
                "required": ["user_id", "property_id", "support_id", "category", "description"],
                "additionalProperties": true
            }),
        ),
        tool(
            "comment_on_support_request",
            "Add a comment to an existing support request. WRITE action.",
            json!({
                "type": "object",
                "properties": {
                    "request_id": { "type": "integer", "description": "Support request id." },
                    "comment_text": { "type": "string" }
                },
                "required": ["request_id", "comment_text"],
                "additionalProperties": false
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({ "name": name, "description": description, "inputSchema": input_schema })
}

fn pagination_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "page": { "type": "integer", "default": 1 },
            "per_page": { "type": "integer", "default": 25 },
            "filters": { "type": "object", "description": "Optional column filters." }
        },
        "additionalProperties": false
    })
}

fn pagination(args: &Value) -> Value {
    let page = args.get("page").and_then(Value::as_i64).unwrap_or(1);
    let per_page = args.get("per_page").and_then(Value::as_i64).unwrap_or(25);
    json!({
        "pagination": { "page": page, "perPage": per_page, "orderBy": { "param": "id", "dir": "desc" } },
        "filters": args.get("filters").cloned().unwrap_or_else(|| json!({}))
    })
}

fn opt(args: &Value, key: &str) -> Value {
    args.get(key).cloned().unwrap_or(Value::Null)
}

fn req_i64(args: &Value, key: &str) -> Result<i64> {
    args.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| CheckDomError::Unexpected(format!("missing or non-integer argument: {key}")))
}

fn req_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CheckDomError::Unexpected(format!("missing or non-string argument: {key}")))
}

fn as_object(args: Value) -> Result<Value> {
    match args {
        Value::Object(_) => Ok(args),
        _ => Err(CheckDomError::Unexpected("expected an object of arguments".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_tools_present_write_tools_gated() {
        let read_only = list(false);
        let both = list(true);
        let names = |v: &Value| -> Vec<String> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert!(names(&read_only).contains(&"list_housekeepers".to_string()));
        assert!(!names(&read_only).contains(&"create_support_request".to_string()));
        assert!(names(&both).contains(&"create_support_request".to_string()));
    }

    #[test]
    fn pagination_defaults() {
        let p = pagination(&json!({}));
        assert_eq!(p["pagination"]["page"], 1);
        assert_eq!(p["pagination"]["perPage"], 25);
    }
}
