//! Local storage of the session token.
//!
//! The token is written to `<config dir>/checkdom-mcp/credentials.toml` with
//! owner-only permissions on Unix. The password is never stored here.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

// No `Debug`: the struct holds the session token and must never end up in a log.
#[derive(Serialize, Deserialize)]
struct Credentials {
    session: String,
}

/// Path to the credentials file (`<config dir>/checkdom-mcp/credentials.toml`).
pub fn credentials_path() -> Result<PathBuf> {
    let dir = dirs::config_dir().context("could not determine the user config directory")?;
    Ok(dir.join("checkdom-mcp").join("credentials.toml"))
}

/// Write the session token: directory `0700`, file `0600` on Unix.
///
/// The file is created with owner-only permissions, so the token is never
/// readable by other users, not even briefly.
pub fn save_token(token: &str) -> Result<()> {
    let path = credentials_path()?;
    let body = toml::to_string(&Credentials {
        session: token.to_string(),
    })
    .context("serializing credentials")?;
    write_private(&path, body.as_bytes())
}

fn write_private(path: &Path, body: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut file = private_open_options()
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    // A file left over from an older version may have wider permissions.
    set_owner_only(path)?;
    file.write_all(body)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("writing {}", path.display()))
}

/// Load the stored session token, if any.
pub fn load_token() -> Result<Option<String>> {
    let path = credentials_path()?;
    match std::fs::read_to_string(&path) {
        Ok(body) => {
            let creds: Credentials =
                toml::from_str(&body).with_context(|| format!("parsing {}", path.display()))?;
            Ok(Some(creds.session))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Remove the stored token. Returns `true` if a file was removed.
pub fn delete_token() -> Result<bool> {
    let path = credentials_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

#[cfg(unix)]
fn private_open_options() -> std::fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true).mode(0o600);
    opts
}

#[cfg(not(unix))]
fn private_open_options() -> std::fs::OpenOptions {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    opts
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    // `mode` applies only to directories this call created; tighten an existing one too.
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("setting permissions on {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn token_file_and_dir_are_owner_only() {
        let root = std::env::temp_dir().join(format!("checkdom-mcp-test-{}", std::process::id()));
        let path = root.join("checkdom-mcp").join("credentials.toml");
        write_private(&path, b"session = \"t\"\n").unwrap();

        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);

        // Rewriting an existing world-readable file tightens it.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, b"session = \"u\"\n").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "session = \"u\"\n");

        std::fs::remove_dir_all(&root).unwrap();
    }
}
