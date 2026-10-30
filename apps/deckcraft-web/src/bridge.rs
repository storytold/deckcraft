//! The host bridge's decisions, kept free of `web_sys` so they are tested natively: which parent
//! origin the bridge may talk to, and which saves the parent has confirmed.

use std::collections::BTreeMap;

/// How long the parent has to answer `deckcraft:save` with `deckcraft:saved` (milliseconds).
pub const SAVE_ACK_TIMEOUT_MS: f64 = 30_000.0;

/// The origin the bridge posts to and hears from, or `None` for no bridge (saves download).
///
/// - `own`: this page's origin.
/// - `parent`: the parent window's origin, `None` when reading it threw (a cross-origin parent).
/// - `allowed`: the `?host_origin=` query value, an origin the embedding page vouches for.
///
/// A same-origin parent is accepted. A cross-origin parent is accepted only when `allowed` is a
/// valid URL origin (`scheme://host[:port]`, nothing else); messages then go to that exact origin,
/// so the browser drops them if the parent is anything else. Opaque origins (`"null"`) never match.
pub fn target_origin(own: &str, parent: Option<&str>, allowed: Option<&str>) -> Option<String> {
    if let Some(parent) = parent
        && parent == own
        && is_origin(own)
    {
        return Some(own.to_string());
    }
    canonical_origin(allowed?.trim())
}

/// `s` as a browser serialises an origin (lowercase scheme and host, no default port), or `None`
/// when it isn't one [`is_origin`] accepts. `MessageEvent.origin` is compared with this string
/// exactly, so `https://Files.Example:443` must become `https://files.example` to ever match.
pub fn canonical_origin(s: &str) -> Option<String> {
    if !is_origin(s) {
        return None;
    }
    let (scheme, rest) = s.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let (host, port) = match rest.split_once(':') {
        Some((h, p)) => (h, Some(p.parse::<u16>().ok()?)),
        None => (rest, None),
    };
    let host = host.to_ascii_lowercase();
    match (scheme.as_str(), port) {
        ("https", Some(443)) | ("http", Some(80)) | (_, None) => Some(format!("{scheme}://{host}")),
        (_, Some(port)) => Some(format!("{scheme}://{host}:{port}")),
    }
}

/// True for a URL origin: `http(s)://host[:port]` with no path, query, fragment or credentials,
/// and only in forms whose browser serialisation [`canonical_origin`] can produce: an ASCII host
/// (an IDN must be given in punycode, `xn--…`), and a numeric host only as a dotted-decimal IPv4.
pub fn is_origin(s: &str) -> bool {
    let Some((scheme, rest)) = s.split_once("://") else { return false };
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) || rest.is_empty() {
        return false;
    }
    // IPv6 literals aren't accepted; name the host instead.
    if rest.contains(['/', '?', '#', '@', ' ', '\\', '[', ']']) {
        return false;
    }
    let (host, port) = match rest.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (rest, None),
    };
    if host.is_empty() || !host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_')) {
        return false;
    }
    // Browsers rewrite numeric hosts (`0x7f.1`, `127.1`, `010.0.0.1`) to dotted decimal; accept
    // only the form they would produce.
    let last = host.trim_end_matches('.').rsplit('.').next().unwrap_or("");
    if last.bytes().all(|b| b.is_ascii_digit()) || last.to_ascii_lowercase().starts_with("0x") {
        let parts: Vec<&str> = host.split('.').collect();
        let decimal = |p: &&str| !p.is_empty() && (*p == "0" || !p.starts_with('0')) && p.parse::<u8>().is_ok();
        if parts.len() != 4 || !parts.iter().all(decimal) {
            return false;
        }
    }
    match port {
        Some(p) => p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok(),
        None => true,
    }
}

/// Saves posted to the parent and not yet confirmed, each carrying `T` (for a presentation
/// save: which presentation, as it was when posted).
#[derive(Debug)]
pub struct SaveAcks<T> {
    next_id: u64,
    /// Request id → (file name, deadline in ms, payload).
    pending: BTreeMap<u64, (String, f64, Option<T>)>,
}

impl<T> Default for SaveAcks<T> {
    fn default() -> Self {
        Self { next_id: 0, pending: BTreeMap::new() }
    }
}

impl<T> SaveAcks<T> {
    /// Record a save posted at `now`; returns its request id.
    pub fn posted(&mut self, name: &str, now: f64) -> u64 {
        self.next_id = self.next_id.wrapping_add(1);
        self.pending.insert(self.next_id, (name.to_string(), now + SAVE_ACK_TIMEOUT_MS, None));
        self.next_id
    }

    /// Attach what a confirmation of `id` applies to.
    pub fn attach(&mut self, id: u64, payload: T) {
        if let Some(entry) = self.pending.get_mut(&id) {
            entry.2 = Some(payload);
        }
    }

    /// The parent confirmed `id`: its payload, or `None` when no such save is waiting (an
    /// unknown id, or one that already timed out).
    pub fn saved(&mut self, id: u64) -> Option<Option<T>> {
        self.pending.remove(&id).map(|(_, _, payload)| payload)
    }

    /// Forget `id` (its post failed); returns its name.
    pub fn failed(&mut self, id: u64) -> Option<String> {
        self.pending.remove(&id).map(|(name, _, _)| name)
    }

    /// Saves past their deadline at `now`, removed and returned as `(id, name)`.
    pub fn expire(&mut self, now: f64) -> Vec<(u64, String)> {
        let late: Vec<u64> = self.pending.iter().filter(|(_, (_, deadline, _))| now >= *deadline).map(|(id, _)| *id).collect();
        late.into_iter().filter_map(|id| self.pending.remove(&id).map(|(name, _, _)| (id, name))).collect()
    }

    /// Whether any save is still waiting for the parent.
    pub fn waiting(&self) -> bool {
        !self.pending.is_empty()
    }
}

/// A hash of saved bytes, to tell whether a snapshot taken later is the one that was posted.
pub fn bytes_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: &str = "https://app.example";

    #[test]
    fn same_origin_parent_enables_the_bridge() {
        assert_eq!(target_origin(OWN, Some(OWN), None).as_deref(), Some(OWN));
    }

    #[test]
    fn cross_origin_parent_disables_the_bridge() {
        // Reading a cross-origin parent's origin throws (None) or names another origin.
        assert_eq!(target_origin(OWN, None, None), None);
        assert_eq!(target_origin(OWN, Some("https://evil.example"), None), None);
        assert_eq!(target_origin(OWN, Some("null"), None), None);
        assert_eq!(target_origin("null", Some("null"), None), None);
    }

    #[test]
    fn host_origin_parameter_must_be_an_exact_origin() {
        let files = "https://files.example:8443";
        assert_eq!(target_origin(OWN, None, Some(files)).as_deref(), Some(files));
        for bad in
            ["*", "null", "files.example", "https://files.example/", "https://a@files.example", "javascript://x", "https://", "https://x:99999"]
        {
            assert_eq!(target_origin(OWN, None, Some(bad)), None, "{bad}");
        }
    }

    #[test]
    fn host_origin_is_stored_as_the_browser_serialises_it() {
        // `MessageEvent.origin` is lowercase with no default port; the stored origin must match it.
        for (given, stored) in [
            ("https://Files.Example", "https://files.example"),
            ("HTTPS://FILES.EXAMPLE", "https://files.example"),
            ("https://files.example:443", "https://files.example"),
            ("http://files.example:80", "http://files.example"),
            ("http://files.example:443", "http://files.example:443"),
            ("https://files.example:80", "https://files.example:80"),
            ("https://files.example:08443", "https://files.example:8443"),
            ("https://xn--bcher-kva.example", "https://xn--bcher-kva.example"),
            ("http://127.0.0.1:8080", "http://127.0.0.1:8080"),
            ("  https://Files.Example:443  ", "https://files.example"),
        ] {
            assert_eq!(target_origin(OWN, None, Some(given)).as_deref(), Some(stored), "{given}");
        }
    }

    #[test]
    fn host_origin_a_browser_would_serialise_differently_is_refused() {
        for bad in [
            "https://b\u{fc}cher.example",
            "https://files.example:",
            "https://0x7f.0.0.1",
            "https://127.1",
            "https://010.0.0.1",
            "https://256.0.0.1",
            "https://files%2eexample",
            "https://[::1]",
        ] {
            assert_eq!(target_origin(OWN, None, Some(bad)), None, "{bad}");
        }
    }

    #[test]
    fn a_save_waits_for_its_own_confirmation() {
        let mut acks = SaveAcks::<&str>::default();
        let id = acks.posted("Deck.deckcraft", 0.0);
        acks.attach(id, "snapshot");
        assert_eq!(acks.saved(id + 1), None, "an unknown id confirms nothing");
        assert!(acks.waiting());
        assert_eq!(acks.saved(id), Some(Some("snapshot")));
        assert!(!acks.waiting());
        assert_eq!(acks.saved(id), None, "a confirmation counts once");
    }

    #[test]
    fn an_unanswered_save_times_out() {
        let mut acks = SaveAcks::<()>::default();
        let id = acks.posted("Deck.deckcraft", 1_000.0);
        assert!(acks.expire(1_000.0 + SAVE_ACK_TIMEOUT_MS - 1.0).is_empty());
        assert_eq!(acks.expire(1_000.0 + SAVE_ACK_TIMEOUT_MS), vec![(id, "Deck.deckcraft".to_string())]);
        assert_eq!(acks.saved(id), None, "a late confirmation is ignored: the save was reported failed");
    }

    #[test]
    fn bytes_hash_tells_saves_apart() {
        assert_eq!(bytes_hash(b"deck"), bytes_hash(b"deck"));
        assert_ne!(bytes_hash(b"deck"), bytes_hash(b"deck!"));
    }
}
