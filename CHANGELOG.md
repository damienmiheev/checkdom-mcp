# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial MCP server over stdio for the CheckDom resident dashboard.
- One-time interactive auth: `login`, `logout`, `status`; session token stored `0600` in the
  user config dir (password never stored).
- Read tools: `list_housekeepers`, `list_houses`, `get_debt_status`, `list_invoices`,
  `list_support_requests`, `get_support_request_comments`, `list_messages`, `list_votings`.
- Write tools behind `--allow-writes`: `create_support_request`,
  `comment_on_support_request`.
- Reverse-engineered API reference in `docs/API.md`.
- rusttv-style project hygiene: dual Apache-2.0/MIT license, SHA-pinned GitHub Actions CI
  (fmt, clippy `-D warnings`, test, release build), Dependabot, `cargo deny`,
  `forbid(unsafe_code)`.

- Unit tests for response parsing, CSRF/cookie extraction, argument validation and token-file
  permissions, plus end-to-end tests that drive the built binary over stdio.
- CodeQL workflow (Rust and Actions); AGENTS.md / CLAUDE.md agent rules and project skills.

### Changed
- Toolchain 1.99.1, edition 2024, `rust-version = "1.99"`; dependencies updated.
- `Cargo.lock` is committed and CI builds with `--locked`; `cargo deny` is a required check.
- MCP: invalid JSON gets a `-32700` parse error, non-object or method-less messages get
  `-32600`, unknown tools get `-32602`, and the protocol version is negotiated.

- reqwest 0.13 (`rustls` with aws-lc-rs and the OS trust store).
- Six direct dependencies instead of thirteen (172 → 134 crates): clap, regex, tracing,
  tracing-subscriber, toml, serde and dirs replaced by small, tested code.
- Tools are one typed table; every tool validates its arguments and rejects unknown ones;
  free-form `filters` removed; tools carry MCP annotations.
- The token is stored as a plain `session-token` file (was `credentials.toml`).
- MCP: JSON-RPC batches, id and version checks; client responses are not answered; invalid
  UTF-8 and lines over 1 MiB get an error instead of stopping the server.

### Fixed
- Login uses the dashboard's `userLogin` RPC (verified live). Posting the HTML login form
  never worked: `POST /login` is 301-redirected and loses its body.

- CI and CodeQL run each job only when files it checks changed; docs-only changes skip the
  build, and the `CI result` / `CodeQL result` gate jobs are the required checks.

### Security
- Redirects are followed only to checkdom.in.ua, so a 307/308 can't replay the password.
- The token file is written atomically and never through a symlink.
- Tool arguments can no longer override the RPC `action` (they were merged after it).
- `create_support_request` accepts only documented `saveSupportRequest` fields.
- The token file is created `0600` (directory `0700`) instead of being chmod-ed after writing.
- HTTPS-only client with connect and request timeouts; `per_page` capped at 100.
- `login` verifies the new session before storing it.
