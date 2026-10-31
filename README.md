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
  reads can produce a pay link for *you* to open; the server never charges a card.
- **Auditable.** Small, typed tool surface — no raw "call any endpoint" passthrough. Every
  exposed action is a vetted function with a known shape.

## Status

Early **v0.1**. Core read tools and the two support-request write flows are implemented. The
API surface is mapped in [`docs/API.md`](docs/API.md); more tools are easy to add.

## Install

Requires a [Rust toolchain](https://rustup.rs); `rustup` installs the pinned version from
`rust-toolchain.toml` (currently 1.99.1) on first build.

```bash
git clone https://github.com/damienmiheev/checkdom-mcp
cd checkdom-mcp
cargo build --release --locked
# binary at ./target/release/checkdom-mcp
```

## Log in (once)

Authentication is a one-time interactive step, so your password never passes through the AI
model or the MCP protocol:

```bash
checkdom-mcp login
# prompts for your CIF-or-email and password, performs the dashboard login,
# checks the new session works, and stores ONLY the resulting session token
# locally (0600, in your config dir)
```

- `checkdom-mcp logout` removes the stored token.
- `checkdom-mcp status` shows whether a valid session is stored.

The token is written with owner-only permissions (`0600`, directory `0700`) to
`credentials.toml` in your config directory:

| OS | Path |
|---|---|
| macOS | `~/Library/Application Support/checkdom-mcp/credentials.toml` |
| Linux | `$XDG_CONFIG_HOME/checkdom-mcp/credentials.toml` (default `~/.config/…`) |
| Windows | `%APPDATA%\checkdom-mcp\credentials.toml` |

 It is a session token, not your password — the
password is used only during `login` and is never stored.

## Connect it to Claude

Add the server to your MCP client config. For Claude Desktop
(`claude_desktop_config.json`):

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
"writes are disabled" error. `create_support_request` accepts only the documented
`saveSupportRequest` fields and rejects anything else.

## Security

See [SECURITY.md](SECURITY.md) for the security model and how to report a vulnerability in
**this project** (please do not open public issues for security bugs). Issues in the CheckDom
service itself are not this project's to disclose.

## Contributing

Issues and PRs welcome; `main` is protected and changes land through pull requests. CI runs
`cargo fmt`, `cargo clippy -D warnings`, unit and end-to-end tests, a release build,
`cargo deny`, and CodeQL. Please keep the tool surface typed and least-privilege, add tests
with every change, and never add a tool that completes a payment or stores a password.
Rules for AI coding agents are in [AGENTS.md](AGENTS.md).

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your
option.
