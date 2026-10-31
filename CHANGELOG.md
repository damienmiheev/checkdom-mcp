# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- Releases take three clicks: *Prepare release* opens the version and CHANGELOG pull request,
  merging it tags and builds the release automatically, and approving the signing publishes it.

## 0.1.0 (2026-10-31)

### Added
- Release pipeline (RELEASE.md): signed archives for macOS, Linux and Windows (arm64 and
  x86_64) with licence notices and SBOMs, `SHA256SUMS` signed with minisign after the owner's
  approval, and build provenance attestations.
- GETTING_STARTED.md: setup for people and AI agents, with a copy-paste setup prompt.
- API drift check: `scripts/api-inventory.py` inventories the dashboard's API calls into
  `docs/api-inventory.txt`; a weekly workflow opens an issue when they change, and the
  `checkdom-api-drift` project skill describes how to bring the server back in line.
- 33 more read tools, all checked against a real account: balance, properties, amount to pay,
  debt radar, reporting charts (settlements, cash flow), bank accounts, services, invoices,
  payments and payment codes, support request lookups (recipients, categories,
  sub-categories, properties, visit slots), inbox and archive, unread count, service messages,
  news, poll invitations and polls, passes, access keys, cars, documents and statements.
- Write tools: `read_message`, `close_support_request`, `vote`, `cancel_temporary_pass`,
  `create_statement`, and `get_pay_link` (a LiqPay or monobank page for you to pay on;
  nothing is charged by the tool).
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
- Tables send the dashboard's exact shape (`filters` inside `pagination`, `additionalData`).
- Your user id is read from the dashboard page once per session and filled in by the server.
- `list_houses` is replaced by `list_properties`; the dashboard's internal `payToken` is
  removed from every response.
- CI skips the build only when every changed file is documentation (any other file, including
  new kinds, runs everything); a README-only change runs just the README test; scheduled and
  manual runs check everything. The `CI result` / `CodeQL result` gate jobs are the required
  checks.
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
- `get_debt_status` and `list_messages` failed with HTTP 500; `list_houses` (now
  `list_properties`) returned nothing. Requests now match the dashboard's own.
- List tools send the same filters as the dashboard pages; support request rows are also
  checked against your user id.
- `create_support_request` and `comment_on_support_request` now send the dashboard's exact
  fields (string `property_id`, `files`, `user_id`, …).
- Login uses the dashboard's `userLogin` RPC (verified live). Posting the HTML login form
  never worked: `POST /login` is 301-redirected and loses its body.

### Security
- Every id argument is checked against your own data before the request is sent; read and
  write tools refuse other people's buildings, properties, accounts, invoices, requests,
  passes, cars, polls and messages.
- `get_pay_link` accepts only monobank's or LiqPay's own payment pages, and amounts in whole
  kopecks from 0.01 UAH.
- Irreversible writes (`vote`, `close_support_request`, `cancel_temporary_pass`) carry the MCP
  `destructiveHint`.
- Redirects are followed only to checkdom.in.ua, so a 307/308 can't replay the password.
- The token file is written atomically and never through a symlink.
- Tool arguments can no longer override the RPC `action` (they were merged after it).
- `create_support_request` accepts only documented `saveSupportRequest` fields.
- The token file is created `0600` (directory `0700`) instead of being chmod-ed after writing.
- HTTPS-only client with connect and request timeouts; `per_page` capped at 100.
- `login` verifies the new session before storing it.
