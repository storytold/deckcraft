//! Control-channel authentication, shared by the desktop app (which publishes a token) and
//! [`crate::Remote`] (which presents it).
//!
//! The control channel listens on loopback, but any local process (and, through a cross-protocol
//! request, a web page in the user's browser) can reach a loopback port. So every connection must
//! first send `{"method": "auth", "params": {"token": …}}` with the app's token; anything else
//! closes the connection.
//!
//! The token is `DECKCRAFT_CONTROL_TOKEN` when set, else 32 random bytes in hex. The app writes it
//! to [`token_path`] (readable by the user only), where `deckcraft-cli` finds it.

use std::io::Write as _;
use std::path::PathBuf;

/// Environment variable holding the token: chosen by whoever starts the app, and read by clients
/// (required for a control port on another host, where there is no token file to read).
pub const TOKEN_ENV: &str = "DECKCRAFT_CONTROL_TOKEN";

/// Where the app publishes the token for `port`: `$XDG_RUNTIME_DIR` when set, else the per-user
/// temporary folder.
pub fn token_path(port: u16) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join(format!("deckcraft-control-{port}.token"))
}

/// The token from [`TOKEN_ENV`], when set and non-empty.
pub fn env_token() -> Option<String> {
    std::env::var(TOKEN_ENV).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// A fresh random token: 32 bytes from the OS, in hex.
pub fn new_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Write `token` to [`token_path`]`(port)`, replacing an earlier file. The new file is created
/// exclusively (never through a planted symlink) and, on Unix, readable by the user only.
pub fn publish_token(port: u16, token: &str) -> std::io::Result<PathBuf> {
    let path = token_path(port);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(token.as_bytes())?;
    Ok(path)
}

/// The token a client should present to `addr` (`host:port`): [`TOKEN_ENV`] when set, else the
/// token file the app published for that port.
pub fn token_for(addr: &str) -> Option<String> {
    if let Some(t) = env_token() {
        return Some(t);
    }
    let port: u16 = addr.rsplit_once(':').and_then(|(_, p)| p.parse().ok())?;
    std::fs::read_to_string(token_path(port)).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// Compare two tokens in time that depends only on their lengths.
pub fn tokens_match(given: &str, expected: &str) -> bool {
    let (a, b) = (given.as_bytes(), expected.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_hex() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn tokens_match_only_exactly() {
        assert!(tokens_match("abc", "abc"));
        assert!(!tokens_match("abd", "abc"));
        assert!(!tokens_match("ab", "abc"));
        assert!(!tokens_match("", "abc"));
    }

    #[test]
    fn published_token_is_read_back_and_private() {
        // A port no other test uses.
        let port = 1;
        let path = publish_token(port, "first").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
        // Publishing again replaces the file.
        publish_token(port, "second").unwrap();
        if env_token().is_none() {
            assert_eq!(token_for("127.0.0.1:1").as_deref(), Some("second"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_file(path).unwrap();
        assert_eq!(token_for("not-an-address"), env_token());
    }
}
