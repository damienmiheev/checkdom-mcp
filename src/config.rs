//! Local storage of the session token.
//!
//! The token is written to `<config dir>/checkdom-mcp/session-token` with
//! owner-only permissions on Unix. The password is never stored here.

use anyhow::{Context, Result, bail};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "checkdom-mcp";
const TOKEN_FILE: &str = "session-token";

/// Path to the token file (`<config dir>/checkdom-mcp/session-token`).
pub fn token_path() -> Result<PathBuf> {
    let dir = config_dir(|key| std::env::var_os(key))
        .context("could not determine the user config directory (is HOME set?)")?;
    Ok(dir.join(APP_DIR).join(TOKEN_FILE))
}

/// The per-user config directory, from the environment `env`:
/// macOS `~/Library/Application Support`, Windows `%APPDATA%`, other Unix
/// `$XDG_CONFIG_HOME` (if absolute) or `~/.config`.
fn config_dir(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let path = |key: &str| {
        env(key)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if cfg!(target_os = "macos") {
        path("HOME").map(|h| h.join("Library").join("Application Support"))
    } else if cfg!(windows) {
        path("APPDATA")
    } else {
        path("XDG_CONFIG_HOME").or_else(|| path("HOME").map(|h| h.join(".config")))
    }
}

/// Write the session token: directory `0700`, file `0600` on Unix.
///
/// The file is created with owner-only permissions, so the token is never
/// readable by other users, not even briefly.
pub fn save_token(token: &str) -> Result<()> {
    if !is_valid_token(token) {
        bail!("CheckDom returned a session token in an unexpected format; not storing it");
    }
    write_private(&token_path()?, format!("{token}\n").as_bytes())
}

/// Load the stored session token, if any.
pub fn load_token() -> Result<Option<String>> {
    let path = token_path()?;
    match std::fs::read_to_string(&path) {
        Ok(body) => parse_token(&body).map(Some).with_context(|| {
            format!(
                "{} is malformed; run `checkdom-mcp login` again",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Remove the stored token. Returns `true` if a file was removed.
pub fn delete_token() -> Result<bool> {
    let path = token_path()?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

fn parse_token(body: &str) -> Result<String> {
    let token = body.trim();
    if !is_valid_token(token) {
        bail!("not a session token");
    }
    Ok(token.to_string())
}

/// A token goes into a `Cookie: session=<token>` header, so it must be a
/// non-empty RFC 6265 cookie value: printable ASCII without space, `"`, `,`,
/// `;` or `\`. This also rules out header injection via CR/LF.
fn is_valid_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .bytes()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b'"' | b',' | b';' | b'\\'))
}

/// Write `body` to `path` atomically with owner-only permissions.
///
/// The data goes to a fresh temporary file in the same (`0700`) directory,
/// created exclusively, so a planted symlink is never followed; it is then
/// renamed over `path`, which replaces a symlink rather than writing through
/// it. Readers see the old token or the new one, never a partial file.
fn write_private(path: &Path, body: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("token path has no parent directory")?;
    create_private_dir(parent).with_context(|| format!("creating {}", parent.display()))?;

    let tmp = parent.join(format!(".{TOKEN_FILE}.{}.tmp", std::process::id()));
    // A leftover from a crashed write; removing a symlink removes only the link.
    let _ = std::fs::remove_file(&tmp);
    let result = (|| {
        let mut file = private_open_options().open(&tmp)?;
        file.write_all(body)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.with_context(|| format!("writing {}", path.display()))
}

#[cfg(unix)]
fn private_open_options() -> std::fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true).mode(0o600);
    opts
}

#[cfg(not(unix))]
fn private_open_options() -> std::fs::OpenOptions {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |key| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn config_dir_on_macos() {
        assert_eq!(
            config_dir(env(&[("HOME", "/Users/a"), ("XDG_CONFIG_HOME", "/x")])),
            Some(PathBuf::from("/Users/a/Library/Application Support"))
        );
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn config_dir_on_linux_prefers_absolute_xdg() {
        let home = ("HOME", "/home/a");
        assert_eq!(
            config_dir(env(&[home, ("XDG_CONFIG_HOME", "/xdg")])),
            Some(PathBuf::from("/xdg"))
        );
        // Relative or empty XDG_CONFIG_HOME is ignored, per the XDG spec.
        for xdg in ["relative/dir", ""] {
            assert_eq!(
                config_dir(env(&[home, ("XDG_CONFIG_HOME", xdg)])),
                Some(PathBuf::from("/home/a/.config"))
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn config_dir_on_windows_is_appdata() {
        assert_eq!(
            config_dir(env(&[
                ("APPDATA", r"C:\Users\a\AppData\Roaming"),
                ("HOME", r"C:\x")
            ])),
            Some(PathBuf::from(r"C:\Users\a\AppData\Roaming"))
        );
        assert_eq!(config_dir(env(&[("APPDATA", "relative")])), None);
        assert_eq!(config_dir(env(&[])), None);
    }

    #[test]
    #[cfg(unix)]
    fn config_dir_needs_an_absolute_home() {
        assert_eq!(config_dir(env(&[])), None);
        assert_eq!(config_dir(env(&[("HOME", "")])), None);
        assert_eq!(config_dir(env(&[("HOME", "not/absolute")])), None);
    }

    #[test]
    fn token_validation() {
        assert!(is_valid_token("abc.DEF-123_~%3D=="));
        for bad in [
            "",
            "a b",
            "a;b",
            "a\r\nX-Evil: 1",
            "a\"b",
            "a,b",
            "a\\b",
            "токен",
        ] {
            assert!(!is_valid_token(bad), "{bad:?}");
        }
        assert_eq!(parse_token("  tok123\n").unwrap(), "tok123");
        assert!(parse_token("\n").is_err());
        assert!(parse_token("session = \"old toml\"").is_err());
    }

    #[test]
    #[cfg(unix)]
    fn token_file_and_dir_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("checkdom-mcp-test-{}", std::process::id()));
        let path = root.join(APP_DIR).join(TOKEN_FILE);
        write_private(&path, b"t\n").unwrap();

        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);

        // Rewriting replaces an existing world-readable file with an owner-only one.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, b"u\n").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "u\n");

        // A symlink planted at the token path is replaced, not written through.
        let victim = root.join("victim");
        std::fs::write(&victim, "keep").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&victim, &path).unwrap();
        write_private(&path, b"v\n").unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
        assert!(
            !std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(mode(&path), 0o600);

        // No temporary files are left behind.
        let leftovers = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1);

        std::fs::remove_dir_all(&root).unwrap();
    }
}
