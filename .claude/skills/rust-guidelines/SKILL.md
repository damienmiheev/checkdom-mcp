---
name: rust-guidelines
description: Microsoft's Pragmatic Rust Guidelines. Use when writing, reviewing, or refactoring Rust code in this repo — error design, panics vs errors, parsers over HTTP responses, async, docs, and lints.
license: MIT
---

# Pragmatic Rust Guidelines (Microsoft)

The full guidelines are in `guidelines.md` next to this file (~3,500 lines). **Don't read it all.** Find the relevant guidelines by heading and read only those sections:

```bash
grep -n "^## " .claude/skills/rust-guidelines/guidelines.md
```

Each guideline has an ID like `M-PANIC-ON-BUG`; cite the ID when it drives a decision or review comment.

## Where to look first

| Task | Guideline IDs |
|---|---|
| Error types (`CheckDomError` via `thiserror`, `anyhow` in the CLI) | M-APP-ERROR, M-PANIC-ON-BUG, M-PANIC-IS-STOP |
| Parsing CheckDom responses and HTML (keep them pure, test them) | M-IMPL-IO ("sans IO"), M-DOCUMENTED-MAGIC, M-TAUTOLOGICAL-TESTS |
| Async (tokio, reqwest) | M-YIELD-POINTS |
| Docs and modules | M-CANONICAL-DOCS, M-FIRST-DOC-SENTENCE, M-MODULE-DOCS |
| Cargo, MSRV | M-MSRV |
| Logging | M-LOG-STRUCTURED |
| Lints | M-STATIC-VERIFICATION, M-LINT-OVERRIDE-EXPECT |

## Precedence

`AGENTS.md` wins over these guidelines. Known differences:

- **M-UNSAFE and the FFI guidelines** — not applicable: `unsafe_code` is forbidden.
- **M-MIMALLOC-APPS, M-TARGET-CPU** — not adopted: a small I/O-bound CLI gains nothing, and
  `target-cpu=native` would break distributed binaries.

If another guideline seems to conflict with project rules, follow the project rule and mention the conflict.

## Source

`guidelines.md` is an unmodified copy of <https://microsoft.github.io/rust-guidelines/agents/all.txt>
(repo: <https://github.com/microsoft/rust-guidelines>, MIT License, © Microsoft Corporation), downloaded 2026-10-23.
To update: re-download the file and review the diff before committing.
