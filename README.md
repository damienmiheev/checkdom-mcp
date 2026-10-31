# checkdom-mcp

An unofficial [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server for the
**CheckDom** resident dashboard ([checkdom.in.ua](https://checkdom.in.ua)) — the building /
property-management ("домоуправитель" / ОСББ) portal used by residents in Ukraine.

It lets an MCP-capable AI assistant (Claude Desktop, Claude Code, etc.) read your own
account — balances, invoices, support requests, messages, votes — and, when you explicitly
enable it, file a support request or comment on one, **on your behalf and only against your
own login**.

> [!WARNING]
> **Unofficial and unaffiliated.** This project is not made, endorsed, or supported by
> CheckDom. It talks to the dashboard's private, undocumented web API (reverse-engineered
> from the public frontend — see [`docs/API.md`](docs/API.md)), which can change or break at
> any time. Use it with your own account, at your own risk. No warranty. See
> [LICENSE-APACHE](LICENSE-APACHE) / [LICENSE-MIT](LICENSE-MIT).

## Design goals

- **Local-first.** Runs on your machine over stdio. Your credentials and session token never
  leave your computer and are never sent to any third party.
- **Read-first and least-privilege.** Read tools are always available. Tools that change
  anything are **off by default** and must be turned on explicitly.
- **Never moves money by itself.** There is no tool that completes a payment. Payment-related
  flows can at most produce a pay link for *you* to open; the server never charges a card.
- **Auditable.** Small, typed tool surface — no raw "call any endpoint" passthrough. Every
  exposed action is a vetted function with a known shape.

## Status

Early **v0.1**, not yet released: build from source (below). Runs on macOS, Linux and
Windows; CI builds and tests all three.

Checked against a real account on 2026-10-31: `login`, `status`, `list_housekeepers`,
`list_invoices`, `list_support_requests`, `get_support_request_comments` and `list_votings`
work. Known issues, being fixed next: `get_debt_status` and `list_messages` fail with an HTTP
500 (wrong parameters), and `list_houses` returns an empty list. The write tools have not been
tried against a real account yet. The API surface is mapped in [`docs/API.md`](docs/API.md).

## Install

Requires a [Rust toolchain](https://rustup.rs); `rustup` installs the pinned version from
`rust-toolchain.toml` (currently 1.99.1) on first build.

```bash
git clone https://github.com/damienmiheev/checkdom-mcp
cd checkdom-mcp
cargo build --release --locked
```

The binary is `target/release/checkdom-mcp` (`target\release\checkdom-mcp.exe` on Windows).
Copy it anywhere you like; the examples below call it `checkdom-mcp`.
`checkdom-mcp --help` lists the commands and `checkdom-mcp --version` prints the version.

## Log in (once)

Authentication is a one-time interactive step, so your password never passes through the AI
model or the MCP protocol:

```bash
checkdom-mcp login
# prompts for your CIF-or-email and password (the same ones you use on
# checkdom.in.ua), logs in, checks the new session works, and stores ONLY
# the resulting session token locally
```

When the session expires, tools answer "session expired; run `checkdom-mcp login` again".

- `checkdom-mcp logout` removes the stored token.
- `checkdom-mcp status` shows whether a valid session is stored.

The token is written to `session-token` in your config directory:

| OS | Path | Protection |
|---|---|---|
| macOS | `~/Library/Application Support/checkdom-mcp/session-token` | file `0600`, directory `0700` |
| Linux | `$XDG_CONFIG_HOME/checkdom-mcp/session-token` (default `~/.config/…`) | file `0600`, directory `0700` |
| Windows | `%APPDATA%\checkdom-mcp\session-token` | your user profile's default permissions |

It is a session token, not your password — the password is used only during `login` and is
never stored.

## Connect it to Claude

Log in first (above); the server uses the stored session.

**Claude Code:**

```bash
claude mcp add checkdom -- /absolute/path/to/checkdom-mcp serve
```

**Claude Desktop:** add the server to `claude_desktop_config.json`
(macOS: `~/Library/Application Support/Claude/`, Windows: `%APPDATA%\Claude\`) and restart
Claude Desktop:

```json
{
  "mcpServers": {
    "checkdom": {
      "command": "/absolute/path/to/checkdom-mcp",
      "args": ["serve"]
    }
  }
}
```

On Windows, use the `.exe` path with doubled backslashes, e.g.
`"C:\\Users\\you\\bin\\checkdom-mcp.exe"`.

Other MCP clients: run `checkdom-mcp serve` as a stdio server.

To allow the write tools (file / comment on support requests), add the opt-in flag:

```json
      "args": ["serve", "--allow-writes"]
```

## Tools

**Read (always on):**

| Tool | What it returns |
|---|---|
| `list_housekeepers` | Your building managers (домоуправителі) and their ids |
| `list_houses` | Your properties / houses |
| `get_debt_status` | Balance / debt state for a building |
| `list_invoices` | Incoming invoices (rahunky) |
| `list_support_requests` | Your support requests (заявки) with status |
| `get_support_request_comments` | Comments on one request |
| `list_messages` | Internal messages / announcements |
| `list_votings` | Building votes (голосування) |

**Write (only with `--allow-writes`):**

| Tool | Effect |
|---|---|
| `create_support_request` | Files a new support request |
| `comment_on_support_request` | Adds a comment to an existing request |

Without `--allow-writes`, the write tools are not registered and any attempt returns a clear
"writes are disabled" error. Every tool rejects arguments it does not document. Tools carry
MCP annotations (`readOnlyHint`), so clients can ask for stricter approval on writes.

> [!CAUTION]
> Messages and request comments are written by other people, and the model reads them next to
> your private data. With `--allow-writes`, a crafted message could try to make the model post
> that data in a comment. Keep per-call approval on for write tools in your MCP client, and
> read what the model is about to send.

## Security

See [SECURITY.md](SECURITY.md) for the security model and how to report a vulnerability in
**this project** (please do not open public issues for security bugs). Issues in the CheckDom
service itself are not this project's to disclose.

## Contributing

Issues and PRs welcome; `main` is protected and changes land through pull requests. For
code changes, CI runs `cargo fmt`, `cargo clippy -D warnings`, unit and end-to-end tests and
a release build on Linux, macOS and Windows, plus `cargo deny` and CodeQL; docs-only changes
skip the build. Please keep the tool surface typed and least-privilege, add tests with every
change, and never add a tool that completes a payment or stores a password.
Rules for AI coding agents are in [AGENTS.md](AGENTS.md).

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your
option.
