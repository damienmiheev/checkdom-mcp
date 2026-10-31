# checkdom-mcp

An unofficial [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server for the
**CheckDom** resident dashboard ([checkdom.in.ua](https://checkdom.in.ua)) — the building /
property-management ("домоуправитель" / ОСББ) portal used by residents in Ukraine.

It lets an MCP-capable AI assistant (Claude Desktop, Claude Code, etc.) read your own
account — balances, invoices, payments, reports, support requests, messages, news, polls,
passes and documents — and, when you explicitly enable it, file or comment on support
requests, vote, cancel a pass or prepare a pay link, **on your behalf and only against your
own login**. Every id a tool takes (building, property, account, request, pass, poll, …) is
checked against your own data before the request is sent.

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
- **Never moves money by itself.** There is no tool that completes a payment. `get_pay_link`
  only returns a LiqPay or monobank page for *you* to open and pay on; the server never charges
  a card or uses saved cards.
- **Auditable.** Small, typed tool surface — no raw "call any endpoint" passthrough. Every
  exposed action is a vetted function with a known shape.

## Status

Early **v0.1**, not yet released: build from source (below). Runs on macOS, Linux and
Windows; CI builds and tests all three.

Checked against a real account on 2026-10-31: `login`, `status`, and every read tool for which
the account had data (`get_voting` and `get_car` had nothing to open). The write tools are
built from the dashboard's own code and unit-tested, but have not been run against a real
account yet; try them with care.
The API surface is mapped in [`docs/API.md`](docs/API.md).

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

To allow the write tools (support requests, votes, pay links, …), add the opt-in flag:

```json
      "args": ["serve", "--allow-writes"]
```

## Tools

**Read (always on):**

| Tool | What it returns |
|---|---|
| **Buildings and balance** | |
| `list_housekeepers` | Your building managers (домоуправителі / ОСББ) and their ids |
| `list_properties` | Your properties (owned or rented) |
| `get_property` | One property: areas, owners and shares, personal accounts |
| `get_amount_to_pay` | What is due for one property, with the tariffs |
| `get_balance` | Balance summary: to pay, debt, fines, overpayment, last payments |
| `get_debt_filters` | Houses, sections and floors of a building |
| `get_debt_status` | The building's debt radar (платіжний радар) |
| **Reports (Звітність)** | |
| `list_services` | Services a building manager bills |
| `list_buildings` | A building manager's houses |
| `get_settlements_chart` | Billed vs paid by month, and by service |
| `list_bank_accounts` | A building manager's bank accounts |
| `get_cash_flow_chart` | Cash flow: money in and out, income and expenses by category |
| **Invoices and payments** | |
| `list_invoices` | Incoming invoices (рахунки) |
| `get_invoice` | One invoice with its lines and the provider's details |
| `list_payments` | Your payments |
| `get_payment_stats` | Totals of your payments |
| `list_payment_codes` | Payment codes (IDS/IDSP/IDSA) for paying through other banks |
| `list_payment_providers` | Online payment providers available to you |
| **Support requests (заявки)** | |
| `list_support_requests` | Your support requests with status and SLA |
| `get_support_request_comments` | Comments on one request |
| `list_support_recipients` | Building managers you can send a request to |
| `list_support_categories` | Request categories of a building manager |
| `list_support_subcategories` | Sub-categories of a category |
| `list_support_properties` | Your properties known to a building manager |
| `get_support_schedule` | Booked visit slots of a sub-category |
| **Messages and news** | |
| `list_messages` | Your inbox |
| `list_archived_messages` | Your message archive |
| `count_unread_messages` | Number of unread messages |
| `list_service_messages` | CheckDom service notices |
| `list_news` | News from your building managers and CheckDom |
| **Polls and votes** | |
| `list_voting_invitations` | Polls and votes you are invited to |
| `list_votings` | Polls you created or manage |
| `get_voting` | One poll: questions, your votes, results where visible |
| **Passes, keys and cars** | |
| `list_temporary_passes` | Your guest, taxi and delivery passes |
| `get_temporary_pass` | One pass, with its share link |
| `list_pass_addresses` | Addresses where you can issue passes |
| `list_access_keys` | Your access key orders (fobs, QR codes) |
| `list_cars` | Your registered cars |
| `get_car` | One car |
| **Documents** | |
| `list_documents` | Documents shared with you |
| `list_statements` | Statements (довідки) you generated |

**Write (only with `--allow-writes`):**

| Tool | Effect |
|---|---|
| `read_message` | Opens a message (CheckDom marks it read) |
| `create_support_request` | Files a new support request |
| `comment_on_support_request` | Adds a comment; can move a request waiting for you back to in progress |
| `close_support_request` | Closes your request as cancelled by you |
| `vote` | Casts your vote on one question of a poll |
| `get_pay_link` | Creates a pending order and returns a LiqPay or monobank page for **you** to pay on; nothing is charged by the tool |
| `create_statement` | Generates a statement of settlements (довідка) |
| `cancel_temporary_pass` | Cancels one of your passes |

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

Because the CheckDom API is unofficial, a weekly workflow compares the dashboard's public scripts
with [`docs/api-inventory.txt`](docs/api-inventory.txt) and opens an issue when they differ;
`python3 scripts/api-inventory.py --check` runs the same check locally.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your
option.
