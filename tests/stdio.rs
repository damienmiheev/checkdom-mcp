//! End-to-end tests of the built binary: the MCP stdio protocol and the CLI.
//!
//! Each test runs with an empty temporary HOME, so it never reads the real
//! stored session and never reaches the network.

// Test helpers panic on failure by design; clippy.toml only covers `#[test]` fns.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_checkdom-mcp");

/// A fresh, empty home directory for one test.
fn empty_home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("checkdom-mcp-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn command(home: &PathBuf, args: &[&str]) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("APPDATA", home)
        .env("RUST_LOG", "warn");
    cmd
}

/// Send `requests` (one JSON value per line) to `serve` and return the replies.
fn serve(name: &str, extra_args: &[&str], requests: &[Value]) -> Vec<Value> {
    let home = empty_home(name);
    let mut args = vec!["serve"];
    args.extend_from_slice(extra_args);
    let mut child = command(&home, &args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    for r in requests {
        writeln!(stdin, "{r}").unwrap();
    }
    drop(stdin); // EOF: the server shuts down after answering.

    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&home);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("every stdout line is JSON"))
        .collect()
}

fn tool_names(reply: &Value) -> Vec<String> {
    reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn handshake_and_read_only_tool_list() {
    let replies = serve(
        "handshake",
        &[],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        ],
    );
    // The notification gets no reply.
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-06-18");

    let names = tool_names(&replies[1]);
    assert_eq!(names.len(), 8);
    assert!(!names.iter().any(|n| n == "create_support_request"));
    assert!(!names.iter().any(|n| n.to_lowercase().contains("pay")));
}

#[test]
fn allow_writes_adds_exactly_the_two_write_tools() {
    let replies = serve(
        "writes",
        &["--allow-writes"],
        &[json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})],
    );
    let names = tool_names(&replies[0]);
    assert_eq!(names.len(), 10);
    assert!(names.iter().any(|n| n == "create_support_request"));
    assert!(names.iter().any(|n| n == "comment_on_support_request"));
}

#[test]
fn tool_call_without_login_is_a_tool_error() {
    let replies = serve(
        "nologin",
        &[],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_housekeepers","arguments":{}}}),
        ],
    );
    assert_eq!(replies[0]["result"]["isError"], true);
    let text = replies[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("not logged in"), "{text}");
}

#[test]
fn write_tool_is_refused_without_flag() {
    let replies = serve(
        "nowrite",
        &[],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_support_request","arguments":{}}}),
        ],
    );
    assert_eq!(replies[0]["result"]["isError"], true);
    let text = replies[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("--allow-writes"), "{text}");
}

#[test]
fn garbage_input_keeps_the_server_alive() {
    let home = empty_home("garbage");
    let mut child = command(&home, &["serve"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        writeln!(stdin, "this is not json").unwrap();
        writeln!(stdin, "{}", json!({"jsonrpc":"2.0","id":9,"method":"ping"})).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    let replies: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[1]["id"], 9);
    assert_eq!(replies[1]["result"], json!({}));
}

fn run(name: &str, args: &[&str]) -> Output {
    let home = empty_home(name);
    let out = command(&home, args).output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    out
}

#[test]
fn status_and_logout_without_a_stored_token() {
    let status = run("status", &["status"]);
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("Not logged in"));

    let logout = run("logout", &["logout"]);
    assert!(logout.status.success());
    assert!(String::from_utf8_lossy(&logout.stdout).contains("No stored token"));
}
