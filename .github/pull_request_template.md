## What and why

<!-- One or two sentences. Link the issue if there is one. -->

## Checklist

- [ ] Tests added or updated (unit test for the logic; `tests/stdio.rs` if CLI/MCP behaviour changed)
- [ ] `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, `cargo deny check` pass locally
- [ ] No payment-execution path; new write tools are gated behind `--allow-writes`
- [ ] No passthrough of free-form actions, paths, or request bodies
- [ ] No token, password, or raw HTTP body in logs or errors
- [ ] New dependency justified (why, size, maintenance, license) — or none added
- [ ] CHANGELOG.md updated for user-visible changes
