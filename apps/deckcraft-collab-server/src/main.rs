//! DeckCraft collaboration server process.
//!
//! Environment (all secrets come from the environment, never from a file in the repository):
//!
//! - `COLLAB_BIND` — `host:port`, default `0.0.0.0:8080`;
//! - `COLLAB_SERVICE_TOKEN` — the cabinet's service token for `/v1/sessions` (required);
//! - `COLLAB_JWT_SECRET` — HS256 secret for local runs, or
//! - `COLLAB_JWT_PEM` — the cabinet's public key (ES256/RS256/Ed25519);
//! - `COLLAB_AUDIENCE` — token audience, default `office-sync`;
//! - `COLLAB_MAX_PARTICIPANTS` (16), `COLLAB_MAX_MESSAGE_BYTES` (262144),
//!   `COLLAB_MAX_OPS_PER_SEC` (50);
//! - `COLLAB_STORE_DIR` — persist rooms here (native base + journal); absent = memory only;
//! - `COLLAB_SNAPSHOT_EVERY` — operations between base snapshots (200);
//! - `COLLAB_CABINET_URL` — the cabinet's commit endpoint (`{sessionId}` is substituted); when
//!   set, a commit becomes a file version there instead of staying in this process;
//! - `COLLAB_CABINET_TOKEN` — the service token the callback is authenticated with (required
//!   with the URL, never logged);
//! - `COLLAB_CABINET_TIMEOUT_MS` — deadline for one callback (10000);
//! - `DECKCRAFT_CORS_ORIGINS` — comma-separated origins a browser page may call this server
//!   from (`https://cabinet.example`); empty or absent = no CORS headers, as before. The browser
//!   link fetches media with `Authorization`, so the origin, not a wildcard, is what is echoed.
//!
//! TLS is terminated by the ingress: this process speaks plain HTTP/WS on a private network.
//! Logs name sessions, versions and counts; slide content, text and tokens are never logged.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use deckcraft_collab_server::cabinet::CabinetConfig;
use deckcraft_collab_server::{Config, JwtConfig, serve};

fn main() -> ExitCode {
    let Ok(bind) = std::env::var("COLLAB_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into()).parse::<SocketAddr>() else {
        eprintln!("COLLAB_BIND is not a host:port address");
        return ExitCode::from(2);
    };
    let service_token = std::env::var("COLLAB_SERVICE_TOKEN").ok().filter(|t| !t.is_empty());
    if service_token.is_none() {
        eprintln!("COLLAB_SERVICE_TOKEN is required: session control fails closed without it");
        return ExitCode::from(2);
    }
    let jwt = match (std::env::var("COLLAB_JWT_PEM"), std::env::var("COLLAB_JWT_SECRET")) {
        (Ok(pem), _) if !pem.is_empty() => JwtConfig::Pem(pem.into_bytes()),
        (_, Ok(secret)) if !secret.is_empty() => JwtConfig::Secret(secret.into_bytes()),
        _ => {
            eprintln!("COLLAB_JWT_PEM or COLLAB_JWT_SECRET is required");
            return ExitCode::from(2);
        }
    };
    let number = |name: &str, default: usize| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
    let cabinet_url = std::env::var("COLLAB_CABINET_URL").ok().filter(|url| !url.is_empty());
    let cabinet_token = std::env::var("COLLAB_CABINET_TOKEN").ok().filter(|token| !token.is_empty());
    let cabinet = match (cabinet_url, cabinet_token) {
        (Some(url), Some(token)) => {
            Some(CabinetConfig { url, token, timeout: Duration::from_millis(number("COLLAB_CABINET_TIMEOUT_MS", 10_000) as u64) })
        }
        (Some(_), None) => {
            eprintln!("COLLAB_CABINET_TOKEN is required with COLLAB_CABINET_URL");
            return ExitCode::from(2);
        }
        _ => None,
    };
    let config = Config {
        service_token,
        jwt,
        audience: std::env::var("COLLAB_AUDIENCE").unwrap_or_else(|_| "office-sync".into()),
        max_participants: number("COLLAB_MAX_PARTICIPANTS", 16),
        max_message_bytes: number("COLLAB_MAX_MESSAGE_BYTES", 256 * 1024),
        max_ops_per_sec: number("COLLAB_MAX_OPS_PER_SEC", 50),
        store_dir: std::env::var("COLLAB_STORE_DIR").ok().filter(|d| !d.is_empty()).map(PathBuf::from),
        snapshot_every: number("COLLAB_SNAPSHOT_EVERY", 200) as u64,
        max_base_inline: number("COLLAB_MAX_BASE_INLINE", 8 << 20),
        cabinet,
        cors_origins: deckcraft_collab_server::parse_cors_origins(&std::env::var("DECKCRAFT_CORS_ORIGINS").unwrap_or_default()),
    };
    match serve(bind, config) {
        Ok(handle) => {
            eprintln!("deckcraft-collab-server listening on {} (proto {})", handle.addr, deckcraft_collab_server::PROTOCOL);
            // Park the main thread; the accept loop is on its own thread and runs until exit.
            loop {
                std::thread::park();
            }
        }
        Err(e) => {
            eprintln!("cannot listen on {bind}: {e}");
            ExitCode::from(1)
        }
    }
}
