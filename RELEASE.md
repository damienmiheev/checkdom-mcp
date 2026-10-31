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

Three clicks; everything else is automatic.

1. **Prepare:** Actions → *Prepare release* → Run workflow → pick `patch`, `minor` or `major`
   (also from the GitHub mobile app). It bumps the version in `Cargo.toml` / `Cargo.lock`,
   turns the CHANGELOG's `## [Unreleased]` section into `## <version> (<date>)`, opens the
   **Release v<version>** pull request with those notes, and starts its CI. It refuses when
   `[Unreleased]` has no entries, so keep the CHANGELOG up to date as changes land.
2. **Merge** the release pull request once its checks are green.
3. **Approve:** when CI has passed on `main`, *Tag release* tags `v<version>` by itself and
   starts *Release build*, which builds the six archives, the notices and SBOMs, writes
   `SHA256SUMS`, attests provenance, creates a draft release and starts *Release sign*. That
   checks the draft's files are exactly that build's, shows the checksums in its summary, and
   waits for you: open the run (GitHub also notifies you), click **Review deployments → release
   → Approve and deploy**. It signs `SHA256SUMS`, uploads `SHA256SUMS.minisig` and **publishes**
   the release.

Why not fully automatic: a pull request merged with the workflow token would start no further
workflows, so the merge stays a click; the approval is the security gate in front of the
signing key.

By hand, if needed: *Tag release* can be run on `main` (it tags main's head if its version is
new), and *Release sign* can be run with a tag. A dry run of the build (no tag, no draft, no
attestations): *Release build* → Run workflow on any branch; it also runs on pull requests that
change the release machinery.

If something fails before publishing, fix it, delete the draft and the tag (the `release tags`
ruleset lets the owner do that), and re-run *Tag release* on `main`. A published release is
never changed: release a new patch version instead.
