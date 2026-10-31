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

### Notes
- `Cargo.lock` is committed once a local build has been run; until then CI resolves
  dependencies fresh and the `--locked` flag is not used.
