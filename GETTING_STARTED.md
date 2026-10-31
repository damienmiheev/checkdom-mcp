# Getting started

checkdom-mcp lets an AI assistant read your **CheckDom** resident account (checkdom.in.ua):
balances, invoices, payments, reports, support requests, messages, news, polls, passes and
documents. It runs on your computer and talks only to checkdom.in.ua. It is unofficial and
unaffiliated with CheckDom.

- **People:** follow [For people](#for-people). About 5 minutes.
- **AI agents** asked to set this up: follow [For AI agents](#for-ai-agents).
- **Want your assistant to do it?** Copy the [setup prompt](#setup-prompt-to-copy) into it.

## For people

### 1. Download

Open the [latest release](https://github.com/damienmiheev/checkdom-mcp/releases/latest) and
download the archive for your computer, plus `SHA256SUMS` and `SHA256SUMS.minisig`:

| Computer | Archive |
|---|---|
| Mac with Apple silicon (M1 and later) | `checkdom-mcp-<version>-aarch64-apple-darwin.tar.gz` |
| Mac with Intel | `checkdom-mcp-<version>-x86_64-apple-darwin.tar.gz` |
| Windows (most PCs) | `checkdom-mcp-<version>-x86_64-pc-windows-msvc.zip` |
| Windows on Arm | `checkdom-mcp-<version>-aarch64-pc-windows-msvc.zip` |
| Linux x86_64 / arm64 (glibc 2.35+) | `checkdom-mcp-<version>-{x86_64,aarch64}-unknown-linux-gnu.tar.gz` |

### 2. Check the download (recommended)

```bash
sha256sum --check --ignore-missing SHA256SUMS      # macOS: shasum -a 256 -c --ignore-missing SHA256SUMS
minisign -Vm SHA256SUMS -P "<the key from minisign.pub or the release notes>"
```

Both must say OK. On Windows (PowerShell): `Get-FileHash <archive>` and compare the hash with
the line for that file in `SHA256SUMS`.

### 3. Install

Unpack the archive and put the `checkdom-mcp` binary (`checkdom-mcp.exe` on Windows) somewhere
permanent, e.g. `~/bin/` or `C:\Users\<you>\bin\`. Note its **absolute path**; you need it below.

- **macOS:** the binary is not notarized. Allow it once: `xattr -d com.apple.quarantine ~/bin/checkdom-mcp`.
- **Windows:** SmartScreen may warn the first time; choose *More info → Run anyway*.

Prefer to build it yourself? See the [README](README.md#install) (needs Rust).

### 4. Log in (once)

In a terminal, run the binary with `login` and type your CheckDom CIF or email and password —
the same ones you use on checkdom.in.ua:

```bash
~/bin/checkdom-mcp login
~/bin/checkdom-mcp status        # "Session is valid."
```

Your password is used once and never stored; only a session token is saved, readable only by
you. When it expires, tools say so: run `login` again.

### 5. Connect your assistant

**Claude Desktop:** open the config file (macOS
`~/Library/Application Support/Claude/claude_desktop_config.json`, Windows
`%APPDATA%\Claude\claude_desktop_config.json`), add the server, and restart Claude Desktop:

```json
{
  "mcpServers": {
    "checkdom": { "command": "/Users/you/bin/checkdom-mcp", "args": ["serve"] }
  }
}
```

On Windows use the `.exe` path with doubled backslashes:
`"C:\\Users\\you\\bin\\checkdom-mcp.exe"`.

**Claude Code:**

```bash
claude mcp add checkdom -- /Users/you/bin/checkdom-mcp serve
```

**Other assistants:** any MCP client that can start a local (stdio) server works: command
= the binary's absolute path, arguments = `serve`.

### 6. Try it

Ask your assistant, for example:

- "What do I owe on CheckDom right now, and for which services?"
- "Show my last 5 invoices and my payments this year."
- "What's the status of my support requests?"
- "Show billed vs paid for my building over the last 12 months."
- "Do I have unread messages?"

### Allowing changes (optional)

By default the assistant can only **read**. To let it file or comment on support requests,
vote, cancel a pass or prepare a pay link, add `--allow-writes`:
`"args": ["serve", "--allow-writes"]`. Keep your client's per-call approval on and read what it
is about to send: messages from other people could try to trick it. A pay link opens monobank's
or LiqPay's own page; nothing is paid unless you pay there yourself.

## For AI agents

You are setting up checkdom-mcp for the user on their own computer. Rules:

- **Never ask for, read, type or store the user's CheckDom password.** The user runs
  `checkdom-mcp login` themselves in their own terminal.
- **Verify the download** against `SHA256SUMS`, and with minisign when available. Stop if a
  check fails.
- **Read-only by default.** Do not add `--allow-writes` unless the user asks for it.
- Change nothing else on the system without asking.

Steps:

1. Detect the platform (OS and CPU) and pick the archive from the table in
   [Download](#1-download). Get the latest release's file list:
   `https://api.github.com/repos/damienmiheev/checkdom-mcp/releases/latest`.
2. Download the archive, `SHA256SUMS` and `SHA256SUMS.minisig`, and verify (see
   [Check the download](#2-check-the-download-recommended)). The minisign key is in
   `minisign.pub` in the repository and in the release notes.
3. Unpack and move the binary to a user-owned directory (`~/bin/` or
   `%USERPROFILE%\bin\`); on macOS remove the quarantine attribute. Run `<binary> --version`.
4. Ask the user to run `<binary> login` in their own terminal and tell you when
   `<binary> status` says "Session is valid."
5. Register the server with the user's MCP client (see [Connect](#5-connect-your-assistant)),
   using the absolute path and `serve`. Restart the client if it needs it.
6. Check it works: call `list_housekeepers` (read-only) and tell the user which building
   manager(s) it found.

Updating later: repeat steps 1–3 with the new release; the login stays valid until it expires.

## Setup prompt to copy

Paste this into Claude Code, Claude Desktop (with a terminal tool), ChatGPT, or another agent
that can run commands on your computer:

```text
Please set up checkdom-mcp for me: an open-source, read-only-by-default MCP server for my
CheckDom resident account (https://github.com/damienmiheev/checkdom-mcp).

Follow the "For AI agents" section of
https://github.com/damienmiheev/checkdom-mcp/blob/main/GETTING_STARTED.md exactly:
1. Pick the latest release archive for my OS and CPU, download it with SHA256SUMS and
   SHA256SUMS.minisig, and verify it. Stop and tell me if any check fails.
2. Install the binary in a folder in my home directory and check `--version`.
3. Do NOT ask for my CheckDom password. Tell me the exact `login` command to run in my own
   terminal, and wait until I confirm `status` says "Session is valid."
4. Add it to my MCP client as a read-only server (no --allow-writes) and tell me if I need
   to restart the client.
5. Test it with list_housekeepers and show me the result.
Explain each step briefly before you do it, and don't change anything else on my computer.
```
