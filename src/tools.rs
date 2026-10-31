//! MCP tool definitions and dispatch.
//!
//! Every tool is one row of [`TOOLS`]: a fixed controller path and action,
//! whether it reads or writes, and its typed arguments. The tool list, the
//! input schemas, the write gate and argument validation are all derived from
//! that table, so they cannot drift apart. Read tools are always available;
//! write tools are only listed and callable with `--allow-writes`.

use crate::client::CheckDomClient;
use crate::error::{CheckDomError, Result};
use serde_json::{Map, Value, json};

// RPC controller paths (see docs/API.md).
const IDX: &str = "/modules/private/api/apiIndexController.php";
const PROP: &str = "/modules/dash/api/apiProperty.php";
const DEBT: &str = "/modules/houseKeeper/api/apiDebtStatusController.php";
const INV: &str = "/modules/private/api/apiInvoicesController.php";
const SUP: &str = "/modules/supportRequests/api/apiSupportRequest.php";
const MSG: &str = "/modules/internalMessages/api/apiInternalMessages.php";
const VOTE: &str = "/modules/voting/api/apiVotingController.php";

/// Largest page a list tool asks for, so one call can't pull an unbounded result.
const MAX_PER_PAGE: i64 = 100;
const DEFAULT_PER_PAGE: i64 = 25;

/// Whether a tool only reads, or changes state on CheckDom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}

/// The type of one tool argument, checked before any request is made.
#[derive(Clone, Copy, Debug)]
enum Ty {
    /// A JSON integer (ids, enum codes). `-1` is a valid category id ("Інше").
    Int,
    /// A non-blank string of at most `max` characters.
    Str { max: usize },
    /// `{"start_datetime": str, "end_datetime": str}` or `null`.
    TimeSlot,
}

#[derive(Debug)]
struct Arg {
    name: &'static str,
    ty: Ty,
    required: bool,
    description: &'static str,
}

const fn req(name: &'static str, ty: Ty, description: &'static str) -> Arg {
    Arg {
        name,
        ty,
        required: true,
        description,
    }
}

const fn opt(name: &'static str, ty: Ty, description: &'static str) -> Arg {
    Arg {
        name,
        ty,
        required: false,
        description,
    }
}

/// How the request body is built from the validated arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// The arguments as they are.
    Plain,
    /// The arguments plus a `pagination` object built from `page` / `per_page`.
    Paginated,
}

#[derive(Debug)]
struct Tool {
    name: &'static str,
    description: &'static str,
    access: Access,
    path: &'static str,
    action: &'static str,
    shape: Shape,
    args: &'static [Arg],
    /// Constant fields the dashboard always sends with this action.
    fixed: &'static [(&'static str, i64)],
}

const HK_ID: &str = "Housekeeper (building manager) id from list_housekeepers.";
const ROLE_ID: &str = "Your active role id (from your CheckDom session).";
const ROLE_OWNER_ID: &str = "The owner id of your active role, e.g. the housekeeper id.";

/// Every tool this server offers. Adding a tool means adding a row here.
const TOOLS: &[Tool] = &[
    Tool {
        name: "list_housekeepers",
        description: "List your building managers (домоуправителі / ОСББ) and their ids. Many \
                      other tools take an `hk_id` from here.",
        access: Access::Read,
        path: IDX,
        action: "getUserHousekeepers",
        shape: Shape::Plain,
        args: &[],
        fixed: &[],
    },
    Tool {
        name: "list_houses",
        description: "List your properties / houses registered on CheckDom.",
        access: Access::Read,
        path: PROP,
        action: "getUserHouses",
        shape: Shape::Plain,
        args: &[],
        fixed: &[],
    },
    Tool {
        name: "get_debt_status",
        description: "Get the balance / debt state (платіжний радар) for a building.",
        access: Access::Read,
        path: DEBT,
        action: "getDebtStatus",
        shape: Shape::Plain,
        args: &[
            req("hk_id", Ty::Int, HK_ID),
            opt("house_id", Ty::Int, "Optional house filter."),
            opt("section", Ty::Str { max: 64 }, "Optional section filter."),
            opt("floor", Ty::Str { max: 64 }, "Optional floor filter."),
        ],
        fixed: &[],
    },
    Tool {
        name: "list_invoices",
        description: "List incoming invoices (рахунки), newest first.",
        access: Access::Read,
        path: INV,
        action: "getIncomingInvoicesPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[],
    },
    Tool {
        name: "list_support_requests",
        description: "List your support requests (заявки) with their status and SLA, newest first.",
        access: Access::Read,
        path: SUP,
        action: "getSupportRequestsPagination",
        shape: Shape::Paginated,
        args: &[],
        fixed: &[],
    },
    Tool {
        name: "get_support_request_comments",
        description: "Get the comments on one support request.",
        access: Access::Read,
        path: SUP,
        action: "getCommentsByRequestId",
        shape: Shape::Plain,
        args: &[req("id", Ty::Int, "Support request id.")],
        fixed: &[],
    },
    Tool {
        name: "list_messages",
        description: "List internal messages / announcements, newest first.",
        access: Access::Read,
        path: MSG,
        action: "getMessagesPagination",
        shape: Shape::Paginated,
        args: &[
            req("role_owner_id", Ty::Int, ROLE_OWNER_ID),
            req("role_id", Ty::Int, ROLE_ID),
        ],
        fixed: &[],
    },
    Tool {
        name: "list_votings",
        description: "List building votes (голосування), newest first.",
        access: Access::Read,
        path: VOTE,
        action: "getVotingsPagination",
        shape: Shape::Paginated,
        args: &[
            opt("role_owner_id", Ty::Int, ROLE_OWNER_ID),
            opt("role_id", Ty::Int, ROLE_ID),
        ],
        fixed: &[],
    },
    Tool {
        name: "create_support_request",
        description: "Create a new support request (заявка). WRITE action: confirm the details \
                      with the user first. Category and sub-category ids come from the \
                      dashboard's category lists (`-1` = \"Інше\" / other).",
        access: Access::Write,
        path: SUP,
        action: "saveSupportRequest",
        shape: Shape::Plain,
        args: &[
            req("user_id", Ty::Int, "Your user id."),
            req("property_id", Ty::Int, "The property the request is about."),
            req("support_id", Ty::Int, HK_ID),
            req("category", Ty::Int, "Category id."),
            req(
                "description",
                Ty::Str { max: 999 },
                "What is wrong (1–999 characters).",
            ),
            opt(
                "author_user_id",
                Ty::Int,
                "Author user id (usually your user id).",
            ),
            opt("property_type", Ty::Int, "Property type code."),
            opt("type", Ty::Int, "Request type code."),
            opt("sub_category", Ty::Int, "Sub-category id."),
            opt("status", Ty::Int, "Initial status code."),
            opt("user_full_name", Ty::Str { max: 255 }, "Your full name."),
            opt(
                "request_entity_type",
                Ty::Int,
                "Entity type (1 = individual).",
            ),
            opt("request_entity_id", Ty::Int, "Entity id."),
            opt("subscriber_id", Ty::Int, "Subscriber (account) id."),
            opt(
                "schedule",
                Ty::TimeSlot,
                "Time slot for sub-categories that need one, or null.",
            ),
        ],
        fixed: &[("comment_type", 0)],
    },
    Tool {
        name: "comment_on_support_request",
        description: "Add a comment to an existing support request. WRITE action: the comment \
                      is visible to building staff; confirm the text with the user first.",
        access: Access::Write,
        path: SUP,
        action: "saveComment",
        shape: Shape::Plain,
        args: &[
            req("request_id", Ty::Int, "Support request id."),
            req("comment_text", Ty::Str { max: 2000 }, "The comment."),
            opt("user_id", Ty::Int, "Your user id."),
        ],
        fixed: &[("comment_type", 0)],
    },
];

fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// Whether `name` is a tool this server knows, read or write.
pub fn exists(name: &str) -> bool {
    find(name).is_some()
}

/// The tool list advertised over `tools/list`.
pub fn list(allow_writes: bool) -> Value {
    TOOLS
        .iter()
        .filter(|t| allow_writes || t.access == Access::Read)
        .map(describe)
        .collect()
}

fn describe(tool: &Tool) -> Value {
    let read = tool.access == Access::Read;
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": input_schema(tool),
        // Hints let clients ask for stricter approval on writes.
        "annotations": {
            "readOnlyHint": read,
            "destructiveHint": false,
            "idempotentHint": read,
            "openWorldHint": true
        }
    })
}

fn input_schema(tool: &Tool) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    if tool.shape == Shape::Paginated {
        props.insert(
            "page".into(),
            json!({ "type": "integer", "minimum": 1, "default": 1 }),
        );
        props.insert(
            "per_page".into(),
            json!({ "type": "integer", "minimum": 1, "maximum": MAX_PER_PAGE, "default": DEFAULT_PER_PAGE }),
        );
    }
    for arg in tool.args {
        let mut schema = match arg.ty {
            Ty::Int => json!({ "type": "integer" }),
            Ty::Str { max } => json!({ "type": "string", "minLength": 1, "maxLength": max }),
            Ty::TimeSlot => json!({
                "type": ["object", "null"],
                "properties": {
                    "start_datetime": { "type": "string" },
                    "end_datetime": { "type": "string" }
                },
                "required": ["start_datetime", "end_datetime"],
                "additionalProperties": false
            }),
        };
        schema["description"] = json!(arg.description);
        props.insert(arg.name.into(), schema);
        if arg.required {
            required.push(arg.name);
        }
    }
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false
    })
}

/// Dispatch a `tools/call`. Returns the pretty-printed JSON payload as text.
pub async fn call(name: &str, args: Value, allow_writes: bool) -> Result<String> {
    let tool = find(name).ok_or_else(|| invalid(format!("unknown tool: {name}")))?;
    // Gate writes, then validate, before reading the token or touching the network.
    if tool.access == Access::Write && !allow_writes {
        return Err(invalid(
            "write tools are disabled; restart the server with --allow-writes to enable them",
        ));
    }
    let params = build_params(tool, args)?;
    let client = CheckDomClient::from_stored()?;
    let data = client.rpc(tool.path, tool.action, params).await?;
    serde_json::to_string_pretty(&data).map_err(CheckDomError::from)
}

fn invalid(msg: impl Into<String>) -> CheckDomError {
    CheckDomError::Unexpected(msg.into())
}

/// Validate `args` against the tool's declared arguments and build the RPC params.
///
/// Unknown keys are rejected, so a tool sends only the fields it documents.
fn build_params(tool: &Tool, args: Value) -> Result<Value> {
    let mut args = match args {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        _ => return Err(invalid("arguments must be an object")),
    };

    let mut params = Map::new();
    if tool.shape == Shape::Paginated {
        let page = take_int_in(&mut args, "page", 1, 1, i64::MAX)?;
        let per_page = take_int_in(&mut args, "per_page", DEFAULT_PER_PAGE, 1, MAX_PER_PAGE)?;
        params.insert(
            "pagination".into(),
            json!({ "page": page, "perPage": per_page, "orderBy": { "param": "id", "dir": "desc" } }),
        );
        params.insert("filters".into(), json!({}));
    }

    for arg in tool.args {
        match args.remove(arg.name) {
            // An explicit `null` time slot means "no slot" and is sent as such.
            Some(Value::Null) if matches!(arg.ty, Ty::TimeSlot) => {
                params.insert(arg.name.into(), Value::Null);
            }
            None | Some(Value::Null) => {
                if arg.required {
                    return Err(invalid(format!("missing argument: {}", arg.name)));
                }
            }
            Some(value) => {
                check_type(arg, &value)?;
                params.insert(arg.name.into(), value);
            }
        }
    }

    if let Some(key) = args.keys().next() {
        return Err(invalid(format!(
            "unsupported argument for {}: {key}",
            tool.name
        )));
    }
    for (key, value) in tool.fixed {
        params.insert((*key).into(), json!(value));
    }
    Ok(Value::Object(params))
}

fn take_int_in(
    args: &mut Map<String, Value>,
    key: &str,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64> {
    let Some(value) = args.remove(key) else {
        return Ok(default);
    };
    value
        .as_i64()
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| invalid(format!("{key} must be an integer from {min} to {max}")))
}

fn check_type(arg: &Arg, value: &Value) -> Result<()> {
    let ok = match arg.ty {
        Ty::Int => value.is_i64(),
        Ty::Str { max } => value
            .as_str()
            .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= max),
        Ty::TimeSlot => match value {
            Value::Null => true,
            Value::Object(m) => {
                m.len() == 2
                    && ["start_datetime", "end_datetime"]
                        .iter()
                        .all(|k| m.get(*k).is_some_and(Value::is_string))
            }
            _ => false,
        },
    };
    if ok {
        Ok(())
    } else {
        Err(invalid(format!(
            "invalid argument {}: expected {}",
            arg.name,
            match arg.ty {
                Ty::Int => "an integer".to_string(),
                Ty::Str { max } => format!("a non-empty string of at most {max} characters"),
                Ty::TimeSlot => "{start_datetime, end_datetime} or null".to_string(),
            }
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid arguments for `tool`: every required argument filled in.
    fn minimal_args(tool: &Tool) -> Value {
        let mut m = Map::new();
        for arg in tool.args.iter().filter(|a| a.required) {
            let v = match arg.ty {
                Ty::Int => json!(1),
                Ty::Str { .. } => json!("text"),
                Ty::TimeSlot => Value::Null,
            };
            m.insert(arg.name.into(), v);
        }
        Value::Object(m)
    }

    fn names(v: &Value) -> Vec<&str> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn tool_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        assert!(TOOLS.iter().all(|t| seen.insert(t.name)));
    }

    #[test]
    fn read_only_list_has_exactly_the_read_tools() {
        let reads: Vec<&str> = TOOLS
            .iter()
            .filter(|t| t.access == Access::Read)
            .map(|t| t.name)
            .collect();
        assert_eq!(names(&list(false)), reads);
        assert_eq!(names(&list(true)).len(), TOOLS.len());
    }

    #[tokio::test]
    async fn every_write_tool_is_refused_without_allow_writes() {
        for tool in TOOLS.iter().filter(|t| t.access == Access::Write) {
            let err = call(tool.name, minimal_args(tool), false)
                .await
                .unwrap_err();
            assert!(err.to_string().contains("--allow-writes"), "{}", tool.name);
        }
    }

    #[test]
    fn no_tool_reaches_a_payment_action() {
        // payByClientCard charges a saved card; it must never be reachable.
        for tool in TOOLS {
            assert_ne!(tool.action, "payByClientCard", "{}", tool.name);
        }
    }

    #[test]
    fn annotations_follow_access() {
        for tool in TOOLS {
            let hints = &describe(tool)["annotations"];
            let read = tool.access == Access::Read;
            assert_eq!(hints["readOnlyHint"], read, "{}", tool.name);
            assert_eq!(hints["idempotentHint"], read, "{}", tool.name);
        }
    }

    #[test]
    fn schema_lists_exactly_the_accepted_arguments() {
        for tool in TOOLS {
            let schema = input_schema(tool);
            assert_eq!(schema["additionalProperties"], false);
            let props = schema["properties"].as_object().unwrap();
            let paged = usize::from(tool.shape == Shape::Paginated) * 2;
            assert_eq!(props.len(), tool.args.len() + paged, "{}", tool.name);
            // The minimal arguments the schema requires are accepted.
            build_params(tool, minimal_args(tool)).unwrap();
        }
    }

    #[test]
    fn unknown_arguments_are_rejected_for_every_tool() {
        for tool in TOOLS {
            let mut args = minimal_args(tool);
            args["action"] = json!("changeStatus");
            assert!(build_params(tool, args).is_err(), "{}", tool.name);

            let mut args = minimal_args(tool);
            args["filters"] = json!({ "user_id": 2 });
            assert!(build_params(tool, args).is_err(), "{}", tool.name);
        }
    }

    #[test]
    fn missing_or_mistyped_arguments_are_rejected() {
        let tool = find("get_debt_status").unwrap();
        assert!(build_params(tool, json!({})).is_err());
        assert!(build_params(tool, json!({ "hk_id": "7" })).is_err());
        assert!(build_params(tool, json!({ "hk_id": 7, "house_id": { "$ne": 0 } })).is_err());
        assert!(build_params(tool, json!({ "hk_id": 7, "section": ["a"] })).is_err());
        assert!(build_params(tool, json!([7])).is_err());
        let p = build_params(tool, json!({ "hk_id": 7, "floor": "3" })).unwrap();
        assert_eq!(p, json!({ "hk_id": 7, "floor": "3" }));
    }

    #[test]
    fn pagination_defaults_and_range() {
        let tool = find("list_invoices").unwrap();
        let p = build_params(tool, Value::Null).unwrap();
        assert_eq!(p["pagination"]["page"], 1);
        assert_eq!(p["pagination"]["perPage"], DEFAULT_PER_PAGE);
        assert_eq!(p["filters"], json!({}));

        let p = build_params(tool, json!({ "page": 3, "per_page": 100 })).unwrap();
        assert_eq!(p["pagination"]["page"], 3);
        assert_eq!(p["pagination"]["perPage"], 100);

        for bad in [
            json!({ "page": 0 }),
            json!({ "per_page": 101 }),
            json!({ "page": "2" }),
        ] {
            assert!(build_params(tool, bad).is_err());
        }
    }

    #[test]
    fn support_request_validation() {
        let tool = find("create_support_request").unwrap();
        let ok = json!({
            "user_id": 1, "property_id": 2, "support_id": 3, "category": -1,
            "description": "Лампа в під'їзді не світить",
            "schedule": { "start_datetime": "2026-11-02 10:00", "end_datetime": "2026-11-02 12:00" }
        });
        let p = build_params(tool, ok.clone()).unwrap();
        assert_eq!(p["comment_type"], 0);
        assert_eq!(p["category"], -1);

        let mut v = ok.clone();
        v["description"] = json!("  ");
        assert!(build_params(tool, v).is_err());

        let mut v = ok.clone();
        v["description"] = json!("я".repeat(1000));
        assert!(build_params(tool, v).is_err());

        let mut v = ok.clone();
        v["schedule"] = json!({ "start_datetime": "x" });
        assert!(build_params(tool, v).is_err());

        let mut v = ok;
        v["schedule"] = Value::Null;
        assert!(build_params(tool, v).unwrap()["schedule"].is_null());
    }

    #[test]
    fn fixed_fields_cannot_be_overridden() {
        let tool = find("comment_on_support_request").unwrap();
        let args = json!({ "request_id": 1, "comment_text": "Дякую", "comment_type": 5 });
        assert!(build_params(tool, args).is_err());
    }
}
