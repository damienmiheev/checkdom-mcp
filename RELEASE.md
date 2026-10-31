# Releasing checkdom-mcp

GitHub Actions builds every release from a tagged commit on `main`, makes a draft release,
and, once the owner approves, signs it with a key that lives only in the protected `release`
environment. The owner then publishes. No release file is built on a laptop.

## What a release contains

| File | Built by | Checked by |
|---|---|---|
| `checkdom-mcp-<version>-<target>.tar.gz` for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | Release build | `SHA256SUMS` |
| `checkdom-mcp-<version>-<target>.zip` for `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc` | Release build | `SHA256SUMS` |
| `SHA256SUMS` (all six archives) | Release build | `SHA256SUMS.minisig` |
| `SHA256SUMS.minisig` | Release sign, after the owner's approval | `minisign.pub` in this repository |

Each archive holds the binary, README.md, SECURITY.md, CHANGELOG.md, both licences,
`THIRD_PARTY_NOTICES.txt` (every crate compiled in and its licence, from cargo-about) and
`checkdom-mcp.cdx.json` (a CycloneDX SBOM for that target, from cargo-cyclonedx).

Every archive also has a GitHub **build provenance attestation** (Sigstore-signed with
`actions/attest`): `gh attestation verify <archive> --repo damienmiheev/checkdom-mcp` shows
which workflow run and commit built it.

Platform notes: Linux binaries need glibc 2.35 or newer (built on Ubuntu 22.04). macOS binaries
are not notarized, so Gatekeeper blocks them until `xattr -d com.apple.quarantine checkdom-mcp`.
Windows binaries are not Authenticode-signed; SmartScreen may warn on first run.

## Verifying a download

```bash
minisign -Vm SHA256SUMS -P "$(sed -n 2p minisign.pub)"   # or paste the key from the release notes
sha256sum --check --ignore-missing SHA256SUMS            # macOS: shasum -a 256 -c --ignore-missing
gh attestation verify checkdom-mcp-<version>-<target>.tar.gz --repo damienmiheev/checkdom-mcp
```

## One-time setup (owner)

1. **Signing key.** On your own machine (never in CI or a chat):

   ```bash
   brew install minisign                     # or your package manager
   mkdir -m 700 ~/.checkdom-mcp-release
   minisign -G -p minisign.pub -s ~/.checkdom-mcp-release/minisign.key
   ```

   It asks for a password. Keep the secret key and password somewhere safe (a password
   manager); losing them means a new key, and users have to trust the new `minisign.pub`.
   Commit `minisign.pub` to the repository root through a pull request.

2. **The `release` environment** (Settings → Environments). It must have you as the required
   reviewer, allow deployments from `main` only, and not let administrators bypass it. Then
   add its two secrets, which `gh` reads without echoing:

   ```bash
   gh secret set MINISIGN_KEY --env release --repo damienmiheev/checkdom-mcp < ~/.checkdom-mcp-release/minisign.key
   gh secret set MINISIGN_PASSWORD --env release --repo damienmiheev/checkdom-mcp
   ```

   Only `.github/workflows/release-sign.yml` as merged on `main` can read them, and only after
   you press **Approve**. That job runs no repository code and no third-party action.

## Releasing

1. **Prepare** (pull request): set `version` in `Cargo.toml` (then `cargo check` so
   `Cargo.lock` follows), and rename the CHANGELOG's `## [Unreleased]` section to
   `## <version> (<YYYY-MM-DD>)`, with a fresh empty `## [Unreleased]` above it. Merge once CI
   is green.
2. **Tag:** Actions → *Tag release* → Run workflow on `main`. It checks that the tag is new,
   the CHANGELOG section exists, `minisign.pub` has a key and CI passed on `main`'s head, then
   tags `v<version>` and starts *Release build*.
3. **Build:** *Release build* builds the six archives, the notices and SBOMs, writes
   `SHA256SUMS`, attests provenance, creates the **draft** release with the CHANGELOG section
   as notes, and starts *Release sign*.
4. **Sign:** *Release sign* checks that the draft's files are exactly that build's, shows the
   checksums in its summary, and waits. Review them, then **Approve** the `sign` job (works
   from the GitHub mobile app). It signs `SHA256SUMS` and uploads `SHA256SUMS.minisig`.
5. **Publish:** open the draft, check the notes and files, and press **Publish release**.

A dry run of the build (no tag, no draft, no attestations): Actions → *Release build* → Run
workflow on any branch.

If something fails before publishing, fix it, delete the draft and the tag (the `release tags`
ruleset lets the owner do that), and start again from step 2. A published release is never
changed: release a new patch version instead.
