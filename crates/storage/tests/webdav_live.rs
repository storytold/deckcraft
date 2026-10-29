//! Live oCIS round-trip through [`WebDavBackend`](deckcraft_storage::webdav::WebDavBackend).
//!
//! Hermetic by default: this test does nothing unless both `CRAFT_DAV_URL` (the WebDAV
//! collection, e.g. `http://ocis.lan:9200/remote.php/dav/files/USER/`)
//! and `CRAFT_DAV_TOKEN` (a fresh OIDC Bearer token) are set — and it is `#[ignore]`d so
//! plain `cargo test` never touches the network. Run it explicitly:
//!
//! ```sh
//! CRAFT_DAV_URL=http://ocis.lan:9200/remote.php/dav/files/alice/ \
//! CRAFT_DAV_TOKEN="$(cat token.txt)" \
//!   cargo test -p deckcraft-storage --test webdav_live -- --ignored --nocapture
//! ```
//!
//! The token never appears in output: only HTTP shapes (statuses, sizes, match flags).

use deckcraft_storage::webdav::WebDavBackend;
use deckcraft_storage::{StorageBackend, WriteOptions};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("live test needs ${name}"))
}

#[test]
#[ignore = "needs a live oCIS (CRAFT_DAV_URL + CRAFT_DAV_TOKEN); see module docs"]
fn bearer_roundtrip_against_ocis() {
    let url = env("CRAFT_DAV_URL");
    let token = env("CRAFT_DAV_TOKEN");
    // The tester names the server, so the network is theirs to trust (plain http, no TLS).
    let store = WebDavBackend::with_bearer(&url, &token).unwrap().allow_cleartext_credentials();

    let name = format!("deckcraft-live-{}.bin", std::process::id());
    let payload = b"PK\x03\x04deckcraft-live-rust-backend".to_vec();

    let probe = store.probe().unwrap();
    assert!(!probe.dav_classes.is_empty(), "server must advertise DAV classes");

    let meta = store.write(&name, &payload, &WriteOptions::default()).unwrap();
    assert_eq!(meta.size, payload.len() as u64);

    let back = store.read(&name).unwrap();
    assert_eq!(back, payload);

    let stat = store.stat(&name).unwrap();
    assert_eq!(stat.size, payload.len() as u64);

    let names: Vec<String> = store.list("").unwrap().into_iter().map(|e| e.path).collect();
    assert!(names.iter().any(|n| n == &name), "list must contain the written file");

    store.remove(&name).unwrap();
    assert!(store.read(&name).is_err(), "deleted file must be gone");
    store.remove(&name).unwrap();
}
