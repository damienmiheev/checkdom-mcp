# Security

## Reporting a vulnerability in checkdom-mcp

Report privately via **GitHub Security Advisories** on this repository
(Security → Report a vulnerability). Please do not open public issues for security bugs.

This policy covers **this software only** — the MCP server. Vulnerabilities in the CheckDom
service itself should go to CheckDom through their own support channels, not here.

## Security model

checkdom-mcp is a local tool that acts as *you* against *your own* CheckDom account.

### Credentials

- Your password is entered only during `checkdom-mcp login`, used once to authenticate, and
  **never stored** and **never sent** anywhere except the CheckDom login endpoint over HTTPS.
- Only the resulting **session token** is persisted, to
  `~/.config/checkdom-mcp/credentials.toml` (or the platform-equivalent config dir), written
  with owner-only (`0600`) permissions.
- The password and token are never exposed to the AI model or sent over the MCP protocol —
  login is a separate, human-run CLI step.
- `checkdom-mcp logout` deletes the stored token.

> A future release may move the token into the OS keychain behind a feature flag; v0.1 uses
> the `0600` file described above.

### Least privilege

- **Read tools are always available; write tools are off by default** and require the
  explicit `--allow-writes` flag at launch. Without it, the write tools are not registered.
- There is **no tool that completes a payment** and **no raw "call any endpoint"
  passthrough.** Every tool is a typed, vetted function.
- Agent/model input is untrusted: tool arguments are validated before any request is made.

### Network and privacy

- The server talks only to `https://checkdom.in.ua` and sends no telemetry.
- Everything runs locally over stdio; nothing about your account is sent to any third party.

## Supply chain

- Third-party GitHub Actions are pinned to commit SHAs; Dependabot proposes grouped updates
  weekly, which CI re-checks.
- `cargo deny check` audits advisories, licenses, and sources (currently a soft CI gate; to
  become a hard gate before 1.0).
- Built only on permissive/`rustls` dependencies — no OpenSSL system linkage.
- `unsafe` code is forbidden at the crate level (`#![forbid(unsafe_code)]` via
  `[lints.rust] unsafe_code = "forbid"`).

## Scope and disclaimer

This is unofficial software that depends on an undocumented API; it may break at any time and
provides no warranty. It is intended for use with your own account only.
