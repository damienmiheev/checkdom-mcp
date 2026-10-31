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
- Only the resulting **session token** is persisted, to `checkdom-mcp/credentials.toml` in
  the platform config dir (see README). On Unix the file is *created* `0600` inside a `0700`
  directory, so it is never readable by other users, even briefly; an older file with wider
  permissions is tightened on the next login.
- `login` keeps the token only after an authenticated call confirms the session works.
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
  Each tool calls one fixed controller and action; arguments can never change which action is
  called, and `create_support_request` passes only an allowlist of documented fields.
- List tools cap `per_page` at 100.

### Network and privacy

- The server talks only to `https://checkdom.in.ua` (HTTPS only; plain-HTTP redirects are
  refused) and sends no telemetry. Requests time out after 30 s.
- Error messages returned to the model never include raw HTTP response bodies.
- Everything runs locally over stdio; nothing about your account is sent to any third party.

## Supply chain

- Third-party GitHub Actions are pinned to commit SHAs, checkouts don't persist the token,
  and workflows run with read-only `contents` permission. Dependabot proposes grouped updates
  weekly (Cargo and Actions), which CI re-checks.
- `Cargo.lock` is committed and CI builds with `--locked`.
- `cargo deny check` audits advisories, licenses, bans and sources. It is a required check on
  every push and pull request and also runs weekly, so a new RustSec advisory surfaces without
  a code change.
- CodeQL scans the Rust code and the workflows on every push, pull request and weekly.
- GitHub secret scanning with push protection, Dependabot alerts and security updates are on.
- `main` is protected by a ruleset: changes land through pull requests with passing CI, and
  force-pushes and branch deletion are blocked.
- Built only on permissive/`rustls` dependencies — no OpenSSL system linkage.
- `unsafe` code is forbidden at the crate level (`#![forbid(unsafe_code)]` via
  `[lints.rust] unsafe_code = "forbid"`).

## Scope and disclaimer

This is unofficial software that depends on an undocumented API; it may break at any time and
provides no warranty. It is intended for use with your own account only.
