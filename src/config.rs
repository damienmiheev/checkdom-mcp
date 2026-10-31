//! Local storage of the session token.
//!
//! The token is written to `<config dir>/checkdom-mcp/credentials.toml` with
//! owner-only permissions on Unix. The password is never stored here.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct Credentials {
    session: String,
}

/// Path to the credentials file (`<config dir>/checkdom-mcp/credentials.toml`).
pub fn credentials_path() -> Result<PathBuf> {
    let dir = dirs::config_dir().context("could not determine the user config directory")?;
    Ok(dir.join("checkdom-mcp").join("credentials.toml"))
}

/// Write the session token, creating the directory and setting `0600` on Unix.
pub fn save_token(token: &str) -> Result<()> {
    let path = credentials_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let creds = Credentials { session: token.to_string() };
    let body = toml::to_string(&creds).context("serializing credentials")?;
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    set_owner_only(&path)?;
    Ok(())
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
