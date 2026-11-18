//! The cabinet control plane: how a commit becomes a new file version.
//!
//! The server builds the `.pptx` from the authoritative document and posts it to the cabinet
//! (`POST <callback URL>`), because the cabinet owns versions, audit and antivirus. The file
//! never travels through a browser: a closed tab cannot lose the save. The body is the shared
//! contract of `INTEGRATION.md` §5.2:
//!
//! ```json
//! { "fileId", "name"?, "bytes": "<base64>", "sha256", "version", "expectedVersion"? }
//! ```
//!
//! The cabinet answers `{ "version", "etag" }`. `409` means the file changed outside the
//! session (WebDAV, another client): the commit is refused and the user is offered a copy,
//! never an overwrite (ADR-0009 §4). The `etag` the cabinet returns is what the next commit
//! expects; a cabinet that answers without one is compared by the committed bytes' digest.
//!
//! Only `http://` and `https://` are accepted; the request names no credentials beyond the
//! bearer token, and neither the token nor the file content is ever logged.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use base64::Engine as _;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

/// The process-wide TLS client configuration, built from the platform's root store on first use.
/// `None` when no roots could be loaded; an `https://` callback then fails with a clear message.
static TLS: LazyLock<Option<Arc<ClientConfig>>> = LazyLock::new(|| {
    let mut roots = RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().certs {
        let _ = roots.add(cert);
    }
    if roots.is_empty() {
        return None;
    }
    Some(Arc::new(ClientConfig::builder().with_root_certificates(roots).with_no_client_auth()))
});

/// Where and how the cabinet takes a committed file.
#[derive(Clone, Debug)]
pub struct CabinetConfig {
    /// The cabinet's commit endpoint. `{sessionId}` and `{fileId}` are replaced with the room id.
    pub url: String,
    /// The service token the callback is authenticated with. Never logged.
    pub token: String,
    /// Deadline for the whole request.
    pub timeout: Duration,
}

/// The cabinet's answer for one accepted file: the version it stores and the tag the next
/// commit must expect.
#[derive(Clone, Debug)]
pub struct Published {
    pub etag: String,
    /// The cabinet's file version number, when it names one.
    pub version: Option<u64>,
}

/// Why the cabinet did not take the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishError {
    /// `409`: the cabinet holds another version of the file. The commit must not overwrite it.
    Conflict(String),
    /// Anything else: the file was built but is not a cabinet version yet.
    Failed(String),
}

/// The cabinet client. One instance per server process; `publish` opens its own connection so a
/// slow cabinet blocks one commit, not the room.
pub struct Cabinet {
    endpoint: String,
    token: String,
    timeout: Duration,
}

impl Cabinet {
    pub fn new(config: CabinetConfig) -> Self {
        Cabinet { endpoint: config.url, token: config.token, timeout: config.timeout }
    }

    /// The endpoint for one room, with `{sessionId}`/`{fileId}` substituted.
    fn endpoint_for(&self, file_id: &str) -> String {
        self.endpoint.replace("{sessionId}", file_id).replace("{fileId}", file_id)
    }

    /// Post one committed file. `expected` is the version the session was opened from (or last
    /// wrote); the cabinet refuses the write when it moved on.
    pub fn publish(
        &self,
        file_id: &str,
        name: Option<&str>,
        document_version: u64,
        expected: Option<&str>,
        bytes: &[u8],
        sha256: &str,
    ) -> Result<Published, PublishError> {
        let mut body = serde_json::Map::new();
        body.insert("fileId".into(), file_id.into());
        if let Some(name) = name {
            body.insert("name".into(), name.into());
        }
        body.insert("bytes".into(), base64::engine::general_purpose::STANDARD.encode(bytes).into());
        body.insert("sha256".into(), sha256.into());
        body.insert("version".into(), document_version.into());
        if let Some(expected) = expected {
            body.insert("expectedVersion".into(), expected.into());
        }
        let body = serde_json::Value::Object(body).to_string();

        let url = self.endpoint_for(file_id);
        let (status, answer) = self.post(&url, body.as_bytes()).map_err(PublishError::Failed)?;
        match status {
            200..=299 => {
                let value: serde_json::Value = serde_json::from_slice(&answer).unwrap_or(serde_json::Value::Null);
                let etag = value.get("etag").and_then(|etag| etag.as_str()).map(str::to_string).unwrap_or_else(|| format!("sha256:{sha256}"));
                let version = value.get("version").and_then(|version| version.as_u64());
                Ok(Published { etag, version })
            }
            409 => Err(PublishError::Conflict("the cabinet holds another version of this file".into())),
            status => Err(PublishError::Failed(format!("the cabinet answered HTTP {status}"))),
        }
    }

    fn post(&self, url: &str, body: &[u8]) -> Result<(u16, Vec<u8>), String> {
        let endpoint = Endpoint::parse(url)?;
        let mut stream = endpoint.connect(self.timeout)?;
        let head = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            endpoint.path,
            endpoint.host_header,
            self.token,
            body.len()
        );
        stream.write_all(head.as_bytes()).map_err(|e| format!("the cabinet callback could not be sent: {e}"))?;
        stream.write_all(body).map_err(|e| format!("the cabinet callback could not be sent: {e}"))?;
        stream.flush().map_err(|e| format!("the cabinet callback could not be sent: {e}"))?;

        let mut response = Vec::new();
        if let Err(e) = stream.read_to_end(&mut response) {
            // A peer that closes without `close_notify` is common for `Connection: close`
            // responses; the bytes that arrived are still the answer.
            if response.is_empty() || e.kind() != std::io::ErrorKind::UnexpectedEof {
                return Err(format!("the cabinet callback got no answer: {e}"));
            }
        }
        split_response(&response)
    }
}

fn split_response(response: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4).ok_or("the cabinet sent no HTTP headers")?;
    let head = String::from_utf8_lossy(&response[..split]);
    let status = head.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()).ok_or("the cabinet sent no status line")?;
    let chunked = head.lines().any(|line| {
        let mut parts = line.splitn(2, ':');
        parts.next().is_some_and(|name| name.eq_ignore_ascii_case("transfer-encoding"))
            && parts.next().is_some_and(|value| value.to_ascii_lowercase().contains("chunked"))
    });
    let body = &response[split..];
    if chunked { Ok((status, decode_chunked(body)?)) } else { Ok((status, body.to_vec())) }
}

/// Minimal chunked transfer decoding for the callback's JSON answer.
fn decode_chunked(mut body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = body.windows(2).position(|w| w == b"\r\n").ok_or("a chunk has no size line")?;
        let size = std::str::from_utf8(&body[..line_end])
            .ok()
            .and_then(|line| usize::from_str_radix(line.trim(), 16).ok())
            .ok_or("a chunk size is not hex")?;
        body = &body[line_end + 2..];
        if size == 0 {
            break;
        }
        // The size comes from the cabinet's text: `size + 2` must be checked, or a huge size
        // panics in debug and wraps in release (and then `&body[..size]` panics too).
        let end = size.checked_add(2).ok_or("a chunk size is too large")?;
        if body.len() < end {
            return Err("a chunk is shorter than its size".into());
        }
        out.extend_from_slice(&body[..size]);
        body = &body[end..];
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scheme {
    Plain,
    Tls,
}

impl Scheme {
    fn default_port(self) -> u16 {
        match self {
            Scheme::Plain => 80,
            Scheme::Tls => 443,
        }
    }
}

struct Endpoint {
    scheme: Scheme,
    /// Host name for the TLS handshake.
    host: String,
    /// `host:port` for `TcpStream::connect`.
    authority: String,
    /// Value of the `Host` header (the port is omitted when it is the scheme's default).
    host_header: String,
    path: String,
}

impl Endpoint {
    fn parse(url: &str) -> Result<Self, String> {
        let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
            (Scheme::Tls, rest)
        } else if let Some(rest) = url.strip_prefix("http://") {
            (Scheme::Plain, rest)
        } else {
            return Err("the cabinet callback URL must start with http:// or https://".into());
        };
        let (authority, path) = match rest.split_once('/') {
            Some((authority, path)) => (authority, format!("/{path}")),
            None => (rest, "/".to_string()),
        };
        if authority.is_empty() || authority.contains('@') {
            return Err("the cabinet callback URL names no host and takes no credentials".into());
        }
        let (host, port) = split_host_port(authority)?;
        let port = port.unwrap_or(scheme.default_port());
        let authority = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
        let host_header =
            if port == scheme.default_port() { if host.contains(':') { format!("[{host}]") } else { host.clone() } } else { authority.clone() };
        Ok(Endpoint { scheme, host, authority, host_header, path })
    }

    fn connect(&self, timeout: Duration) -> Result<Wire, String> {
        let socket = TcpStream::connect(&self.authority).map_err(|e| format!("the cabinet callback could not connect: {e}"))?;
        socket.set_read_timeout(Some(timeout)).ok();
        socket.set_write_timeout(Some(timeout)).ok();
        match self.scheme {
            Scheme::Plain => Ok(Wire::Plain(socket)),
            Scheme::Tls => {
                let config = TLS.as_ref().cloned().ok_or("the cabinet callback has no TLS root certificates")?;
                let name = ServerName::try_from(self.host.clone()).map_err(|e| format!("the cabinet callback host: {e}"))?;
                let connection = ClientConnection::new(config, name).map_err(|e| format!("the cabinet callback TLS: {e}"))?;
                Ok(Wire::Tls(Box::new(StreamOwned::new(connection, socket))))
            }
        }
    }
}

fn split_host_port(authority: &str) -> Result<(String, Option<u16>), String> {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, rest) = rest.split_once(']').ok_or("the cabinet callback URL has an unclosed IPv6 address")?;
        let port = match rest.strip_prefix(':') {
            Some(port) => Some(port.parse::<u16>().map_err(|_| "the cabinet callback port is not a number".to_string())?),
            None if rest.is_empty() => None,
            None => return Err("the cabinet callback URL has trailing text after the IPv6 address".into()),
        };
        (host, port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() => {
                (host, Some(port.parse::<u16>().map_err(|_| "the cabinet callback port is not a number".to_string())?))
            }
            _ => (authority, None),
        }
    };
    if host.is_empty() {
        return Err("the cabinet callback URL names no host".into());
    }
    Ok((host.to_string(), port))
}

enum Wire {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Wire::Plain(stream) => stream.read(buf),
            Wire::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for Wire {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Wire::Plain(stream) => stream.write(buf),
            Wire::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Wire::Plain(stream) => stream.flush(),
            Wire::Tls(stream) => stream.flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::sync::mpsc::{Receiver, channel};

    /// A one-request cabinet on loopback: returns its address and the request it read.
    fn one_shot(reply: &'static str) -> std::io::Result<(SocketAddr, Receiver<String>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut request = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let Ok(n) = stream.read(&mut tmp) else { return };
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&tmp[..n]);
                if let Some(split) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&request[..split]).to_ascii_lowercase();
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:").map(|value| value.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    if request.len() >= split + 4 + length {
                        break;
                    }
                }
            }
            let _ = sender.send(String::from_utf8_lossy(&request).to_string());
            let _ = stream.write_all(reply.as_bytes());
        });
        Ok((address, receiver))
    }

    fn cabinet(address: SocketAddr) -> Cabinet {
        Cabinet::new(CabinetConfig {
            url: format!("http://{address}/api/v1/internal/office/sessions/{{sessionId}}/commit"),
            token: "cabinet-service".into(),
            timeout: Duration::from_secs(5),
        })
    }

    #[test]
    fn only_http_and_https_urls_with_a_host_parse() {
        let endpoint = Endpoint::parse("https://cabinet.example/api/commit").expect("a valid URL");
        assert_eq!(endpoint.scheme, Scheme::Tls);
        assert_eq!(endpoint.host, "cabinet.example");
        assert_eq!(endpoint.host_header, "cabinet.example");
        assert_eq!(endpoint.path, "/api/commit");
        assert_eq!(Endpoint::parse("http://127.0.0.1:8788/commit").expect("a port").authority, "127.0.0.1:8788");
        assert_eq!(Endpoint::parse("http://[::1]:9/x").expect("IPv6").host, "::1");
        assert_eq!(Endpoint::parse("http://example.test").expect("no path").path, "/");
        assert!(Endpoint::parse("ftp://example.test/commit").is_err());
        assert!(Endpoint::parse("http://user:pass@example.test/").is_err());
        assert!(Endpoint::parse("http:///nohost").is_err());
    }

    #[test]
    fn a_published_file_carries_the_contract_fields_and_the_answer() {
        let (address, receiver) =
            one_shot("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 29\r\n\r\n{\"version\":7,\"etag\":\"etag-7\"}")
                .expect("cabinet");
        let result = cabinet(address).publish("file-1", Some("talk.pptx"), 3, Some("etag-0"), b"PK\x03\x04deck", &"a".repeat(64)).expect("published");
        assert_eq!(result.etag, "etag-7");
        assert_eq!(result.version, Some(7));
        let request = receiver.recv().expect("request");
        assert!(request.starts_with("POST /api/v1/internal/office/sessions/file-1/commit"), "{request}");
        assert!(request.contains("Authorization: Bearer cabinet-service"), "{request}");
        assert!(request.contains(r#""fileId":"file-1""#), "{request}");
        assert!(request.contains(r#""name":"talk.pptx""#), "{request}");
        assert!(request.contains(r#""expectedVersion":"etag-0""#), "{request}");
        assert!(request.contains(r#""version":3"#), "{request}");
        assert!(request.contains(r#""sha256":"aaaa"#), "{request}");
        let encoded = base64::engine::general_purpose::STANDARD.encode(b"PK\x03\x04deck");
        assert!(request.contains(&format!(r#""bytes":"{encoded}""#)), "the file is base64 in the body: {request}");
    }

    #[test]
    fn a_409_is_a_conflict_and_an_error_status_is_a_failure() {
        let (address, _receiver) = one_shot("HTTP/1.1 409 Conflict\r\nContent-Length: 2\r\n\r\n{}").expect("cabinet");
        let conflict = cabinet(address).publish("file-1", None, 0, Some("etag-0"), b"PK", &"b".repeat(64)).expect_err("refused");
        assert!(matches!(conflict, PublishError::Conflict(_)), "{conflict:?}");

        let (address, _receiver) = one_shot("HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n").expect("cabinet");
        let failed = cabinet(address).publish("file-1", None, 0, None, b"PK", &"b".repeat(64)).expect_err("failed");
        assert_eq!(failed, PublishError::Failed("the cabinet answered HTTP 500".into()));
    }

    #[test]
    fn a_cabinet_without_an_etag_is_compared_by_the_digest() {
        // `PUT` the whole file once: the body is bounded by the deck, not by a message limit.
        let bytes = vec![7u8; 300 * 1024];
        let (address, receiver) = one_shot("HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n").expect("cabinet");
        let result = cabinet(address).publish("file-1", None, 1, None, &bytes, &"c".repeat(64)).expect("published");
        assert_eq!(result.etag, format!("sha256:{}", "c".repeat(64)), "no etag: the digest is the tag");
        let request = receiver.recv().expect("request");
        assert!(request.len() > bytes.len(), "the whole file travelled");
    }

    #[test]
    fn a_chunk_size_that_cannot_hold_its_separator_is_refused() {
        let hostile = format!("{:x}\r\n", usize::MAX);
        assert_eq!(decode_chunked(hostile.as_bytes()), Err("a chunk size is too large".into()));
    }
}
