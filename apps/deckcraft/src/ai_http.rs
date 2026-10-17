//! File › Settings › AI (#732): sends the AI requests `deckcraft_ui_egui::ai` builds, on a thread
//! of their own, and keeps the API key in a file apart from the other settings.

use std::path::Path;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use deckcraft_ui_egui::ai::{AiCall, ApiKey};
use serde_json::Value;

/// The key's file in the settings directory. Not `ui.json`, so settings backups never hold it.
pub const KEY_FILE: &str = "ai-key";
/// How long a model may take to answer: one on this computer can be slow to start.
const TIMEOUT: Duration = Duration::from_secs(300);
/// The largest reply read: an outline is a few kilobytes.
const MAX_REPLY: u64 = 4 << 20;

/// Send `call` in the background; the reply (or what went wrong) arrives on the receiver.
pub fn send(call: AiCall) -> Receiver<Result<Value, String>> {
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("ai-request".into()).spawn(move || {
        let _ = tx.send(post(&call));
    });
    if let Err(e) = spawned {
        // The sender went with the closure, so the UI hears "stopped without an answer".
        log::warn!("AI request thread: {e}");
    }
    rx
}

fn post(call: &AiCall) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        // Servers explain a refused key or a missing model in the body: read it.
        .http_status_as_error(false)
        // A redirect could carry the key to another server: never follow one.
        .max_redirects(0)
        .build()
        .into();
    let mut req = agent.post(&call.url).header("Content-Type", "application/json");
    if !call.key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {}", call.key));
    }
    let body = serde_json::to_string(&call.body).map_err(|e| e.to_string())?;
    let mut resp = req.send(body).map_err(|e| reach_error(&call.url, &e))?;
    let status = resp.status();
    if status.is_redirection() {
        let to = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("another address");
        return Err(format!(
            "The AI server sends requests on to {to}. DeckCraft doesn't follow, so your key stays put: put that address in File › Settings › AI if you trust it."
        ));
    }
    let bytes = match resp.body_mut().with_config().limit(MAX_REPLY).read_to_vec() {
        Ok(b) => b,
        Err(ureq::Error::BodyExceedsLimit(_)) => return Err("The AI server's answer is too big to be an outline.".into()),
        Err(e) => return Err(format!("Couldn't read the AI server's answer: {e}")),
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(v) if status.is_success() || v.get("error").is_some() => Ok(v),
        _ if status.as_u16() == 401 || status.as_u16() == 403 => Err("The AI server refused the API key.".into()),
        _ if status.as_u16() == 404 => Err("The AI server has nothing at that address, or doesn't know that model.".into()),
        _ if !status.is_success() => Err(format!("The AI server answered with an error ({status}).")),
        _ => Err("The AI server's answer isn't JSON. Is the address an OpenAI-compatible API?".into()),
    }
}

fn reach_error(url: &str, e: &ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "The AI server took too long to answer.".into(),
        ureq::Error::ConnectionFailed | ureq::Error::HostNotFound | ureq::Error::Io(_) => {
            // A server on this computer rarely speaks https.
            let local = ["https://localhost", "https://127.", "https://[::1]"].iter().any(|p| url.starts_with(p));
            let hint = if local { " Servers on this computer usually use http://, not https://." } else { "" };
            format!("Couldn't reach the AI server at {url}. Is it running?{hint}")
        }
        e => format!("Couldn't reach the AI server: {e}"),
    }
}

/// The saved key and the server it is for, or none. A file that isn't one gives none.
pub fn load_key(dir: &Path) -> ApiKey {
    let Ok(text) = std::fs::read_to_string(dir.join(KEY_FILE)) else { return ApiKey::default() };
    let v: Value = serde_json::from_str(&text).unwrap_or_default();
    let field = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let (origin, secret) = (field("for"), field("key"));
    // An origin as DeckCraft writes it, and a key it would have accepted.
    if deckcraft_ui_egui::ai::origin(&origin).as_deref() != Some(origin.as_str()) || deckcraft_ui_egui::ai::field("key", &secret).is_err() {
        return ApiKey::default();
    }
    ApiKey { origin, secret }
}

/// Save the key and its server (readable by this user only on Unix), or remove the file when
/// there is none.
pub fn save_key(dir: &Path, key: &ApiKey) {
    let path = dir.join(KEY_FILE);
    if key.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let text = serde_json::json!({"for": key.origin, "key": key.secret}).to_string();
    if let Ok(mut f) = options.open(&path) {
        let _ = std::io::Write::write_all(&mut f, text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use serde_json::json;

    use super::*;

    /// A one-request HTTP server on this computer that answers `status` and `body`, and hands
    /// back the request it got.
    fn serve_once(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        serve_raw(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
    }

    /// A one-request server that answers with a raw `reply` (status line, headers and body) once it
    /// has read the whole request, and hands back the request.
    fn serve_raw(reply: String) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/chat/completions", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            // Read the head, then as much body as Content-Length says.
            loop {
                let n = s.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let len =
                        text.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()));
                    if got.len() >= end + 4 + len.unwrap_or(0) {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            s.write_all(reply.as_bytes()).unwrap();
            String::from_utf8_lossy(&got).to_string()
        });
        (url, handle)
    }

    #[test]
    fn a_request_carries_the_key_and_brings_back_the_reply() {
        let (url, server) = serve_once("200 OK", r###"{"choices":[{"message":{"content":"## Hi"}}]}"###);
        let call = AiCall { url, key: "sk-test".into(), body: json!({"model": "m"}) };
        let reply = send(call).recv().unwrap().unwrap();
        assert_eq!(reply["choices"][0]["message"]["content"], "## Hi");
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions"));
        assert!(request.contains("Bearer sk-test"), "{request}");
        assert!(request.contains(r#""model":"m""#));
    }

    #[test]
    fn a_servers_error_message_comes_through_and_a_refused_key_is_named() {
        let (url, _server) = serve_once("404 Not Found", r#"{"error":{"message":"model 'x' not found"}}"#);
        let reply = send(AiCall { url, key: String::new(), body: json!({}) }).recv().unwrap().unwrap();
        assert_eq!(reply["error"]["message"], "model 'x' not found");
        let (url, _server) = serve_once("401 Unauthorized", "nope");
        let err = send(AiCall { url, key: "bad".into(), body: json!({}) }).recv().unwrap().unwrap_err();
        assert!(err.contains("refused the API key"), "{err}");
    }

    #[test]
    fn a_redirect_is_not_followed_so_the_key_stays_put() {
        let (url, _server) =
            serve_raw("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/steal\r\nContent-Length: 0\r\n\r\n".to_string());
        let err = send(AiCall { url, key: "secret".into(), body: json!({}) }).recv().unwrap().unwrap_err();
        assert!(err.contains("http://127.0.0.1:1/steal") && err.contains("doesn't follow"), "{err}");
    }

    #[test]
    fn an_answer_that_isnt_json_says_so_in_words() {
        let (url, _server) = serve_raw("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\n\u{0}\u{1}PNG\n".to_string());
        let err = send(AiCall { url, key: String::new(), body: json!({}) }).recv().unwrap().unwrap_err();
        assert!(err.contains("isn't JSON"), "{err}");
    }

    #[test]
    fn a_server_that_isnt_running_is_reported() {
        // Bind and drop a listener: nothing answers on that port now.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
        let err = send(AiCall { url, key: String::new(), body: json!({}) }).recv().unwrap().unwrap_err();
        assert!(err.contains("Couldn't reach"), "{err}");
    }

    #[test]
    fn the_key_file_is_written_read_and_removed() {
        let dir = std::env::temp_dir().join(format!("deckcraft-ai-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = ApiKey::new("https://api.openai.com/v1", "sk-123");
        save_key(&dir, &key);
        assert_eq!(load_key(&dir), key);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join(KEY_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // A hand-edited file that points the key somewhere odd, or isn't JSON, loads no key.
        for bad in [r#"{"for":"https://evil.example","key":"a\nb"}"#, r#"{"for":"evil.example","key":"sk"}"#, "sk-plain-text"] {
            std::fs::write(dir.join(KEY_FILE), bad).unwrap();
            assert!(load_key(&dir).is_empty(), "{bad}");
        }
        save_key(&dir, &ApiKey::default());
        assert!(!dir.join(KEY_FILE).exists());
        assert!(load_key(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
