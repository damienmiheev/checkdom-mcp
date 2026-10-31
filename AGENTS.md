# AGENTS.md — mandatory rules for AI agents

These rules apply to every change in this repo, whichever agent makes it. Details live in the
linked docs; do not duplicate them here.

## Safety (non-negotiable — see SECURITY.md)

- **No payment execution.** Never add a tool or code path that pays or charges a card
  (`payByClientCard`, saved cards, Portmone card flows). The one payment-related write is
  `get_pay_link`: it may call `placeOrder` (a pending order) and `getMonoParams` /
  `getLiqpayWidgetParams`, and returns only a hosted payment page for the human to open.
  Never return the dashboard's `payToken` to the model.
- **Writes stay opt-in.** Every tool is a row of `TOOLS` in `src/tools.rs`; a tool that
  changes CheckDom state has `access: Access::Write`, which hides it from `tools/list` and
  refuses it unless the server runs with `--allow-writes`.
- **No passthrough.** Never add a tool that takes a free-form controller path, action name, or
  raw request body. Each tool maps to one fixed `(controller, action)` and sends only the
  typed arguments it declares; unknown arguments are rejected. No free-form `filters`.
- **Only your own rows.** A list tool sends the same hidden own-rows filters as the dashboard
  page it mirrors (`role_id`, `role_owner_id`, …; see docs/API.md), and where rows name their
  owner, sets `owner` so other people's rows are dropped. Verify a new list tool live: its
  `total` must match what the dashboard shows you.
- **Credentials.** The password is used once by `login` and never stored, logged, or sent to
  the model. Only the session token is persisted, `0600` in a `0700` directory. Never log the
  token or put it in an error message.
- **No raw responses in errors.** Errors returned to the model never include the HTTP body.
- `#![forbid(unsafe_code)]` stays. No `unsafe`, anywhere.
- Treat tool arguments (model output) as untrusted input: validate before any request.
- Never log in to a real CheckDom account, or call the live API, unless the human explicitly
  asked in this session. Tests must not touch the network or the real credential file.

## How to work

1. **Think before coding.** State assumptions; ask when requirements are ambiguous.
2. **Simplicity first.** The minimum code that solves the stated problem. No speculative
   abstractions, features, or dependencies.
3. **Surgical changes.** Touch only what the task needs.
4. **Goal-driven, test-backed.** Every change ships with tests: a unit test for the logic
   (factor pure functions out of I/O code), and a case in `tests/stdio.rs` when CLI or MCP
   protocol behaviour changes. A bug fix starts with a test that fails without it.

## Rust standards

- Edition 2024, toolchain pinned in `rust-toolchain.toml` (keep `rust-version` in
  `Cargo.toml` in step).
- `thiserror` (`CheckDomError`) for the client and tools; `anyhow` only in `main.rs` /
  `config.rs` / the MCP loop.
- No `unwrap()`/`expect()` outside tests; where an invariant makes one safe, use
  `#[expect(clippy::expect_used, reason = "...")]`.
- Keep dependencies few (six direct today). New dependency = justify it in the PR (why,
  size, maintenance, license, what it would take to do without). `cargo deny` must pass.
- Tool names and input schemas are a public contract for MCP clients; changing one is a
  breaking change and goes in CHANGELOG.md.

## Docs are always up to date

Every change that affects behaviour updates, in the same PR: README.md (install, login,
client setup, tool tables, known issues), SECURITY.md (anything security-relevant),
docs/API.md (anything learned about the CheckDom API) and CHANGELOG.md. A test
(`readme_tool_tables_match_the_server`) fails when the README tool tables and the server
disagree; everything else is on you, so re-read the README before claiming done.

## Required checks before claiming done

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
cargo deny check
```

Report actual results. If a check fails or was skipped, say so.

CI skips the build only when every changed file matches the documentation pattern in
`ci.yml` / `codeql.yml` (`.github/scripts/changes.sh`); any other file runs everything, so new
kinds of files are checked by default. Only widen that pattern for files that truly can't
affect the build or tests — and never for anything a test reads (README.md has its own job).

## Where things are

| Topic | Where |
|---|---|
| Install, login, connect to a client, tool list | README.md |
| Security model, supply chain, reporting | SECURITY.md |
| Reverse-engineered CheckDom API | docs/API.md |
| CLI (`login` / `logout` / `status` / `serve`) | src/main.rs |
| MCP stdio loop (JSON-RPC 2.0) | src/mcp.rs |
| Tool definitions, gating, argument validation | src/tools.rs |
| HTTP client, login (`userLogin` RPC), response parsing | src/client.rs |
| Token storage | src/config.rs |
| End-to-end tests of the binary | tests/stdio.rs |
| Release notes | CHANGELOG.md |
