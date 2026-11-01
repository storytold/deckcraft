//! Authentication of the control channel (#777).
//!
//! Anything on the machine, and a web page that POSTs `text/plain` to `127.0.0.1:<port>`, can
//! reach a loopback port. So the first line of every connection must be
//! `{"id":…,"method":"auth","params":{"token":"…"}}`, with the secret the app made at start. The
//! app keeps it in memory and writes it to a file only the user can read (or the user chooses it
//! with `DECKCRAFT_CONTROL_TOKEN`); [`Remote`](crate::Remote) reads it from the same places.

use std::path::PathBuf;

use serde_json::{Value, json};

/// Environment variable holding a token chosen by the user. The app uses it instead of making
/// one (and writes no file); clients use it instead of reading the file.
pub const TOKEN_ENV: &str = "DECKCRAFT_CONTROL_TOKEN";
/// The method of the handshake line.
pub const AUTH_METHOD: &str = "auth";
/// Shortest token the app accepts from the user (a made one has 256 bits).
pub const MIN_TOKEN_LEN: usize = 16;
/// Longest first line the app reads from a peer that hasn't authenticated yet.
pub const MAX_AUTH_LINE: usize = 4096;

/// Compare two secrets without stopping at the first difference. Unequal lengths are unequal
/// at once (the length of a token isn't secret).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    // Keep the optimiser from turning the loop into an early exit.
    std::hint::black_box(diff) == 0
}

/// A made token as lowercase hex (two characters per byte).
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len().saturating_mul(2));
    for b in bytes {
        s.push(char::from(DIGITS[usize::from(b >> 4)]));
        s.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    s
}

/// The handshake line a client sends first.
pub fn auth_line(id: u64, token: &str) -> String {
    json!({"id": id, "method": AUTH_METHOD, "params": {"token": token}}).to_string()
}

/// Check a connection's first line. `Ok` is the reply to send before serving further requests;
/// `Err` is the reply for a refused connection, which the caller sends and then closes. The
/// refusal is the same whatever was wrong, so it tells a stranger nothing.
pub fn check_first_line(line: &str, token: &str) -> Result<Value, Value> {
    let refused = || json!({"ok": false, "error": "authentication required"});
    let msg: Value = serde_json::from_str(line.trim()).map_err(|_| refused())?;
    if msg.get("method").and_then(Value::as_str) != Some(AUTH_METHOD) {
        return Err(refused());
    }
    let given = msg.get("params").and_then(|p| p.get("token")).and_then(Value::as_str).unwrap_or("");
    // Always compare, so a missing token costs the same as a wrong one.
    if constant_time_eq(given.as_bytes(), token.as_bytes()) && !token.is_empty() {
        Ok(json!({"id": msg.get("id").cloned().unwrap_or(Value::Null), "ok": true, "result": {"authenticated": true}}))
    } else {
        Err(refused())
    }
}

/// Where the app writes the token it made for `port`, and where clients look for it: a folder
/// only the user can open (`$XDG_RUNTIME_DIR/deckcraft`, else `~/.cache/deckcraft`; on Windows
/// `%USERPROFILE%\.deckcraft`), file `control-<port>.token`. `None` when no such folder is known.
pub fn token_path(port: u16) -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("USERPROFILE").filter(|v| !v.is_empty())?).join(".deckcraft")
    } else if let Some(run) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        PathBuf::from(run).join("deckcraft")
    } else {
        PathBuf::from(std::env::var_os("HOME").filter(|v| !v.is_empty())?).join(".cache").join("deckcraft")
    };
    Some(dir.join(format!("control-{port}.token")))
}

/// The port of a control address (`127.0.0.1:7990` → 7990).
fn port_of(addr: &str) -> Option<u16> {
    addr.rsplit(':').next()?.parse().ok()
}

/// The token a client uses for `addr`: [`TOKEN_ENV`] if set, else the file the app wrote.
pub fn client_token(addr: &str) -> Result<String, String> {
    if let Ok(t) = std::env::var(TOKEN_ENV)
        && !t.trim().is_empty()
    {
        return Ok(t.trim().to_string());
    }
    let path = port_of(addr).and_then(token_path).ok_or_else(|| format!("no control token: set {TOKEN_ENV}"))?;
    match std::fs::read_to_string(&path) {
        Ok(t) if !t.trim().is_empty() => Ok(t.trim().to_string()),
        _ => Err(format!("no control token: set {TOKEN_ENV}, or start the app on this machine and user (it writes {})", path.display())),
    }
}

/// Write `token` to [`token_path`] for the app: the folder is made private (mode 0700 on Unix),
/// an older file is replaced, and the new one is created readable by the user only (mode 0600).
/// Returns the path.
pub fn write_token_file(port: u16, token: &str) -> std::io::Result<PathBuf> {
    use std::io::Write;
    let path = token_path(port).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no user folder for the control token"))?;
    let dir = path.parent().ok_or_else(|| std::io::Error::other("token path has no folder"))?;
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Only our own folder: leave a shared one (`$XDG_RUNTIME_DIR` itself) as it is.
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    // Replace rather than open: a leftover file may have looser permissions, or be a link.
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path)?;
    f.write_all(token.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(path)
}
