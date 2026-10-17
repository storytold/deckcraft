//! Authentication for the loopback control channel (`--control`).
//!
//! The channel binds to `127.0.0.1`, which keeps it off the network but says nothing about the
//! other processes on the same machine. Left open, any of them could connect and drive the app,
//! quit it with unsaved work, or make it read and write files at a path of their choosing
//! (`app.open`, `app.save`, `app.export`). So a connection has to present the app's token before
//! it may send a method: [`HELLO`] is the only method an unauthenticated connection may send, and
//! a wrong token closes the connection.
//!
//! The app picks the token when it starts — `--control-token`, else [`ENV`], else a fresh random
//! one, which [`store`] writes to [`token_file`] so a client running as the same user finds it
//! without the operator copying it by hand. A client takes `--token`, else [`ENV`], else the file.
//!
//! Both sides live in this module so the file's name and the handshake cannot drift: the server is
//! `apps/deckcraft/src/control_server.rs`, the client is `crates/mcp/src/backend.rs`.

use std::path::PathBuf;

/// The only method an unauthenticated connection may send; `{"token": "…"}` in its params.
pub const HELLO: &str = "control.hello";

/// Environment variable carrying the token, read by the app and by clients.
pub const ENV: &str = "DECKCRAFT_CONTROL_TOKEN";

/// Bytes of entropy in a generated token, as two 64-bit words.
const TOKEN_WORDS: usize = 2;

/// Where the app keeps its settings: logs, preferences and this token file. The desktop app's
/// `config_dir` is this function, so the two cannot name the directory differently.
pub fn settings_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/DeckCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DeckCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join("deckcraft"))
    }
}

/// The file the app writes `port`'s token to. One file per port: two apps on two ports are two
/// different sessions, and a client that knows the port knows which token belongs to it.
pub fn token_file(port: u16) -> Option<PathBuf> {
    Some(settings_dir()?.join(format!("control-{port}.token")))
}

/// A fresh token: [`TOKEN_WORDS`] words of entropy as hex.
///
/// The words come from the keys behind a fresh [`RandomState`], which the standard library seeds
/// from the operating system. That is enough for this: the token only has to be unguessable by
/// the other processes on this machine, and it guards nothing else. It is not a CSPRNG, and on
/// `wasm32-unknown-unknown` (where the channel does not run) the seed is weaker still — so do not
/// reuse this for a token that leaves the machine.
pub fn generate() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut token = String::with_capacity(TOKEN_WORDS * 16);
    for i in 0..TOKEN_WORDS as u64 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(0x6465_636b_6372_6166 ^ i);
        token.push_str(&format!("{:016x}", hasher.finish()));
    }
    token
}

/// Write `port`'s token where [`load`] can read it back, owner-readable only where the platform
/// has such a mode. The parent directory is created; a failure is the caller's to report.
pub fn store(port: u16, token: &str) -> std::io::Result<Option<PathBuf>> {
    let Some(path) = token_file(port) else { return Ok(None) };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, token)?;
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(&path) {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode() & !0o077;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode));
    }
    Ok(Some(path))
}

/// The token the app stored for `port`, if it left one and it is not empty.
pub fn load(port: u16) -> Option<String> {
    let token = std::fs::read_to_string(token_file(port)?).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// The port in `addr`, which may be a bare port or a `host:port` pair.
pub fn port_of(addr: &str) -> Option<u16> {
    let tail = addr.rsplit(':').next()?;
    tail.parse().ok()
}

/// Whether `given` is the expected token, without saying where it first differs. A client that can
/// time the rejection learns how much of its guess was right, so neither the length nor the bytes
/// may end the comparison early.
pub fn matches(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_differ_and_are_hex() {
        let a = generate();
        let b = generate();
        assert_ne!(a, b, "two generated tokens must not collide");
        assert_eq!(a.len(), TOKEN_WORDS * 16, "a token is {TOKEN_WORDS} words of hex");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a} is not hex");
    }

    #[test]
    fn matches_compares_the_whole_token() {
        let token = generate();
        assert!(matches(&token, &token));
        assert!(!matches(&token, ""));
        assert!(!matches(&token, &format!("{token}0")));
        assert!(!matches(&token, &token[..token.len() - 1]));
        assert!(!matches(&token, &"0".repeat(token.len())), "a same-length wrong token is still wrong");
        // Equal length, differing only at the end: the difference must not be masked by the XOR.
        let mut other = token.clone();
        other.pop();
        other.push(if token.ends_with('0') { '1' } else { '0' });
        assert!(!matches(&token, &other));
    }

    #[test]
    fn port_of_reads_bare_ports_and_host_pairs() {
        assert_eq!(port_of("7979"), Some(7979));
        assert_eq!(port_of("127.0.0.1:7979"), Some(7979));
        assert_eq!(port_of("localhost:4242"), Some(4242));
        assert_eq!(port_of("[::1]:7979"), Some(7979));
        assert_eq!(port_of("127.0.0.1"), None);
        assert_eq!(port_of(""), None);
    }
}
