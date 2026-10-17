//! File › Settings › AI (#732): bring your own model. Any server that speaks the OpenAI chat
//! completions API (Ollama, LM Studio, LocalAI, vLLM, OpenAI itself…) at an address the user
//! picks, with their own key and model. DeckCraft only builds the request and reads the reply;
//! the host sends it (`Services::ai_send`), so the web build and the engine stay offline.
//!
//! Draft slides from a prompt asks the model for a Markdown outline and builds it with
//! `slide.fromOutline`, so the slides are ordinary, undoable slides.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// The address, model and key size limits: longer is a paste gone wrong.
const MAX_FIELD: usize = 2048;
/// The longest prompt `ai.draftSlides` sends.
pub const MAX_PROMPT: usize = 8000;
/// The most slides one draft adds: a model that runs away doesn't bury the presentation.
pub const MAX_SLIDES: usize = 50;

/// Where requests go, saved with the other settings. The key is not here: it is kept apart from
/// the settings and their backups (`SlideApp::ai_key`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AiSettings {
    /// The API's base address, up to `/v1` (`/chat/completions` is added).
    pub endpoint: String,
    pub model: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        // Ollama on this computer: free, private, and needs no key.
        AiSettings { endpoint: PRESETS[0].1.into(), model: PRESETS[0].2.into() }
    }
}

/// Ready-made servers for the AI tab: (name, address, a model to start with).
pub const PRESETS: [(&str, &str, &str); 3] = [
    ("Ollama", "http://localhost:11434/v1", "llama3.2"),
    ("LM Studio", "http://localhost:1234/v1", ""),
    ("OpenAI", "https://api.openai.com/v1", "gpt-4o-mini"),
];

/// The API key and the server it was typed for (`origin`: scheme, host and port). DeckCraft sends
/// it to that server only, so changing the address never hands the key to another one.
#[derive(Clone, Default, PartialEq)]
pub struct ApiKey {
    pub origin: String,
    pub secret: String,
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKey").field("origin", &self.origin).field("secret", &if self.secret.is_empty() { "" } else { "<hidden>" }).finish()
    }
}

impl ApiKey {
    /// A key typed for `endpoint`; none when either is empty or the address isn't one.
    pub fn new(endpoint: &str, secret: &str) -> ApiKey {
        match origin(endpoint) {
            Some(origin) if !secret.is_empty() => ApiKey { origin, secret: secret.to_string() },
            _ => ApiKey::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.secret.is_empty()
    }

    /// The key a request to `endpoint` carries: this one for the server it was typed for, none
    /// for any other.
    pub fn for_endpoint(&self, endpoint: &str) -> &str {
        if !self.secret.is_empty() && origin(endpoint).is_some_and(|o| o == self.origin) { &self.secret } else { "" }
    }

    /// Where the key goes, for messages: the origin without its scheme.
    pub fn host(&self) -> &str {
        self.origin.split_once("://").map_or(self.origin.as_str(), |(_, h)| h)
    }
}

/// The origin of an address: scheme, host and port (the default one filled in), in lower case.
/// Anything before an `@` is a user name, not the host.
pub fn origin(endpoint: &str) -> Option<String> {
    let e = endpoint.trim().to_ascii_lowercase();
    let (scheme, rest, default) = match e.strip_prefix("https://") {
        Some(r) => ("https", r, 443),
        None => ("http", e.strip_prefix("http://")?, 80),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if host.is_empty() {
        return None;
    }
    let has_port = if host.starts_with('[') { host.contains("]:") } else { host.contains(':') };
    Some(if has_port { format!("{scheme}://{host}") } else { format!("{scheme}://{host}:{default}") })
}

/// A request for the host to send: POST `body` to `url`, with `key` as a bearer token when
/// there is one. The reply is the response's JSON, or a sentence saying what went wrong.
#[derive(Clone, PartialEq)]
pub struct AiCall {
    pub url: String,
    pub key: String,
    pub body: Value,
}

// Debug output (logs, test failures) never shows the key.
impl std::fmt::Debug for AiCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = if self.key.is_empty() { "" } else { "<hidden>" };
        f.debug_struct("AiCall").field("url", &self.url).field("key", &key).field("body", &self.body).finish()
    }
}

/// What a request in flight is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Test,
    Draft,
}

/// AI requests for the Settings tab: the one in flight, the last outcome and the prompt box.
#[derive(Default)]
pub struct State {
    pub pending: Option<(Job, std::sync::mpsc::Receiver<Result<Value, String>>)>,
    /// The presentation (uid) a draft's slides are for.
    pub target: Option<u64>,
    /// The last result to show under the buttons: `Ok` for good news, `Err` for a problem.
    pub outcome: Option<Result<String, String>>,
    pub prompt: String,
}

/// Check and tidy a field typed by the user.
pub fn field(name: &str, v: &str) -> Result<String, String> {
    let v = v.trim();
    if v.len() > MAX_FIELD {
        return Err(format!("`{name}` is too long"));
    }
    if v.chars().any(char::is_control) {
        return Err(format!("`{name}` can't contain line breaks or control characters"));
    }
    Ok(v.to_string())
}

/// The chat completions address for a base address. Accepts the base (`…/v1`) or the full
/// address someone pasted (`…/v1/chat/completions`).
pub fn chat_url(endpoint: &str) -> Result<String, String> {
    let e = endpoint.trim().trim_end_matches('/');
    // The scheme in any case (HTTPS:// too).
    let scheme = ["http://", "https://"].into_iter().find(|s| e.get(..s.len()).is_some_and(|p| p.eq_ignore_ascii_case(s)));
    let Some(rest) = scheme.and_then(|s| e.get(s.len()..)).filter(|r| !r.is_empty()) else {
        return Err("The AI address must start with http:// or https://, like http://localhost:11434/v1".into());
    };
    if e.chars().any(char::is_whitespace) {
        return Err("The AI address can't contain spaces.".into());
    }
    // The port, when there is one, is a number up to 65535 (`[::1]:port` for IPv6).
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // `name@host` would send `name` as a login and hide which server this really is.
    if authority.contains('@') {
        return Err("The AI address can't contain @: give the server's own address, like https://api.openai.com/v1".into());
    }
    let port = if authority.starts_with('[') { authority.rsplit_once("]:").map(|(_, p)| p) } else { authority.split_once(':').map(|(_, p)| p) };
    if let Some(port) = port
        && !port.parse::<u16>().is_ok_and(|p| p != 0)
    {
        return Err(format!("`{port}` isn't a port: after the colon comes a number from 1 to 65535, like :11434."));
    }
    if e.ends_with("/chat/completions") { Ok(e.to_string()) } else { Ok(format!("{e}/chat/completions")) }
}

/// Is the key about to travel unencrypted to another computer? (http:// to anything but this one.)
pub fn key_in_the_clear(endpoint: &str, key: &str) -> bool {
    let e = endpoint.trim().to_ascii_lowercase();
    let Some(rest) = e.strip_prefix("http://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split(']').next().map(|h| h.trim_start_matches('[')).unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or(host)
    };
    let local = host == "localhost" || host == "::1" || host.starts_with("127.");
    !key.trim().is_empty() && !local
}

const SYSTEM: &str = "You write presentation outlines in Markdown. Reply with the outline only: no introduction, \
no closing remarks and no code fences. Each slide is a level-2 heading (## Slide title) followed by \
3 to 6 short bullet points (- point); indent a bullet by two spaces to make it a sub-point. Keep \
bullets under 12 words. Write in the language of the request.";

fn request(settings: &AiSettings, key: &ApiKey, messages: Value, max_tokens: u32) -> Result<AiCall, String> {
    let url = chat_url(&settings.endpoint)?;
    let model = settings.model.trim();
    if model.is_empty() {
        return Err("Pick a model first: type its name in File › Settings › AI.".into());
    }
    // Only the server the key was typed for gets it, and never over plain http to another computer.
    let key = key.for_endpoint(&settings.endpoint);
    if key_in_the_clear(&settings.endpoint, key) {
        return Err("DeckCraft won't send your API key unencrypted to another computer: use an https:// address for it.".into());
    }
    Ok(AiCall { url, key: key.to_string(), body: json!({"model": model, "messages": messages, "max_tokens": max_tokens, "stream": false}) })
}

/// A tiny request that shows whether the server, key and model work.
pub fn test_call(settings: &AiSettings, key: &ApiKey) -> Result<AiCall, String> {
    request(settings, key, json!([{"role": "user", "content": "Reply with the word OK."}]), 16)
}

/// The request for slides about `prompt`.
pub fn draft_call(settings: &AiSettings, key: &ApiKey, prompt: &str) -> Result<AiCall, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("Say what the slides should be about.".into());
    }
    if prompt.len() > MAX_PROMPT {
        return Err(format!("That request is too long: keep it under {MAX_PROMPT} characters."));
    }
    let messages = json!([{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}]);
    request(settings, key, messages, 2048)
}

/// The text of a chat completions reply, or the server's own error message.
pub fn reply_text(reply: &Value) -> Result<String, String> {
    if let Some(e) = reply.get("error") {
        let msg = e.get("message").and_then(Value::as_str).or(e.as_str()).unwrap_or("the server reported an error");
        return Err(format!("The AI server said: {msg}"));
    }
    let content = reply.pointer("/choices/0/message/content").and_then(Value::as_str);
    content.map(str::to_string).ok_or_else(|| "The AI server's reply has no text in it. Is the address an OpenAI-compatible API?".into())
}

/// At most `MAX_SLIDES` slides of an outline (a heading starts each): the outline cut before the
/// heading after the last one kept, and how many slides the whole outline had.
pub fn cap_slides(outline: &str) -> (String, usize) {
    let mut kept = vec![];
    let mut slides = 0;
    for line in outline.lines() {
        if line.trim_start().starts_with('#') {
            slides += 1;
        }
        if slides <= MAX_SLIDES {
            kept.push(line);
        }
    }
    (
        kept.join(
            "
",
        ),
        slides,
    )
}

/// The Markdown outline in a model's reply: thinking (`<think>…</think>`) and code fences
/// around the outline are dropped.
pub fn outline(reply: &str) -> String {
    let mut text = reply.to_string();
    while let Some(start) = text.find("<think>") {
        match text[start..].find("</think>") {
            Some(end) => text.replace_range(start..start + end + "</think>".len(), ""),
            // Thinking that never ended: there is no outline after it.
            None => text.truncate(start),
        }
    }
    text.lines().filter(|l| !l.trim_start().starts_with("```")).collect::<Vec<_>>().join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ollama() -> AiSettings {
        AiSettings::default()
    }

    #[test]
    fn the_chat_address_is_built_from_the_base_or_kept_when_whole() {
        assert_eq!(chat_url("http://localhost:11434/v1").unwrap(), "http://localhost:11434/v1/chat/completions");
        assert_eq!(chat_url(" https://api.openai.com/v1/ ").unwrap(), "https://api.openai.com/v1/chat/completions");
        assert_eq!(chat_url("http://h/v1/chat/completions").unwrap(), "http://h/v1/chat/completions");
        assert!(chat_url("localhost:11434").is_err());
        assert!(chat_url("ftp://x").is_err());
        assert!(chat_url("https://").is_err());
        assert!(chat_url("http://127.0.0.1:8811/v 1").unwrap_err().contains("spaces"));
        assert!(chat_url("http://127.0.0.1:99999/v1").unwrap_err().contains("99999"));
        assert!(chat_url("http://127.0.0.1:/v1").is_err());
        assert!(chat_url("http://127.0.0.1:0/v1").is_err());
        assert!(chat_url("http://h:abc/v1").is_err());
        assert!(chat_url("http://[::1]:1234/v1").is_ok());
        assert!(chat_url("http://[::1]/v1").is_ok());
        assert!(chat_url("http://user:pw@host/v1").unwrap_err().contains("@"));
        assert!(chat_url("http://localhost@evil.example/v1").is_err());
    }

    #[test]
    fn a_runaway_outline_is_cut_to_the_most_slides_one_draft_adds() {
        let long: String = (0..5000)
            .map(|i| {
                format!(
                    "## Slide {i}
- point
"
                )
            })
            .collect();
        let (kept, total) = cap_slides(&long);
        assert_eq!(total, 5000);
        assert_eq!(kept.lines().filter(|l| l.starts_with("##")).count(), MAX_SLIDES);
        assert!(kept.ends_with("- point"), "the last slide kept keeps its bullets");
        assert_eq!(
            cap_slides(
                "## A
- b"
            ),
            (
                "## A
- b"
                .to_string(),
                1
            )
        );
    }

    #[test]
    fn a_key_sent_over_http_to_another_computer_is_flagged() {
        assert!(!key_in_the_clear("http://localhost:11434/v1", "sk-1"));
        assert!(!key_in_the_clear("http://127.0.0.1:1234/v1", "sk-1"));
        assert!(!key_in_the_clear("http://[::1]:1234/v1", "sk-1"));
        assert!(!key_in_the_clear("https://api.openai.com/v1", "sk-1"));
        assert!(!key_in_the_clear("http://192.168.1.5/v1", ""));
        assert!(key_in_the_clear("http://192.168.1.5:8000/v1", "sk-1"));
        assert!(key_in_the_clear("HTTP://ai.example.com/v1", "sk-1"));
        assert!(key_in_the_clear("http://localhost@evil.example/v1", "sk-1"));
    }

    #[test]
    fn a_draft_asks_the_model_for_a_markdown_outline() {
        let key = ApiKey::new("http://localhost:11434/v1", "sk-1");
        let call = draft_call(&ollama(), &key, "  The solar system  ").unwrap();
        assert_eq!(call.url, "http://localhost:11434/v1/chat/completions");
        assert_eq!(call.key, "sk-1");
        assert_eq!(call.body["model"], "llama3.2");
        assert_eq!(call.body["stream"], false);
        assert_eq!(call.body["messages"][0]["role"], "system");
        assert_eq!(call.body["messages"][1]["content"], "The solar system");
    }

    #[test]
    fn a_draft_needs_a_prompt_a_model_and_a_good_address() {
        let none = ApiKey::default();
        assert!(draft_call(&ollama(), &none, "  ").is_err());
        assert!(draft_call(&ollama(), &none, &"x".repeat(MAX_PROMPT + 1)).is_err());
        let no_model = AiSettings { model: " ".into(), ..ollama() };
        assert!(draft_call(&no_model, &none, "Bees").unwrap_err().contains("model"));
        let bad = AiSettings { endpoint: "localhost".into(), ..ollama() };
        assert!(test_call(&bad, &none).unwrap_err().contains("http://"));
    }

    #[test]
    fn the_key_goes_only_to_the_server_it_was_typed_for() {
        let openai = AiSettings { endpoint: "https://api.openai.com/v1".into(), model: "m".into() };
        let key = ApiKey::new("https://api.openai.com/v1", "sk-live");
        assert_eq!(test_call(&openai, &key).unwrap().key, "sk-live");
        // The same server written differently still gets it.
        let same = AiSettings { endpoint: "HTTPS://API.OPENAI.COM:443/v1/chat/completions".into(), ..openai.clone() };
        assert_eq!(test_call(&same, &key).unwrap().key, "sk-live");
        // Any other server, port or scheme doesn't.
        for other in [
            "http://localhost:11434/v1",
            "https://api.openai.com.evil.example/v1",
            "https://api.openai.com:8443/v1",
            "https://api.openai.com@evil.example/v1",
            "https://evil.example/api.openai.com/v1",
        ] {
            let s = AiSettings { endpoint: other.into(), ..openai.clone() };
            assert_eq!(test_call(&s, &key).map(|c| c.key).unwrap_or_default(), "", "{other}");
        }
        assert_eq!(key.host(), "api.openai.com:443");
        assert!(ApiKey::new("not an address", "sk").is_empty());
    }

    #[test]
    fn a_key_is_never_sent_unencrypted_to_another_computer() {
        let lan = AiSettings { endpoint: "http://192.168.1.5:8000/v1".into(), model: "m".into() };
        let e = test_call(&lan, &ApiKey::new("http://192.168.1.5:8000/v1", "sk-1")).unwrap_err();
        assert!(e.contains("https://"), "{e}");
        // Without a key the same server is fine, and a key over http to this computer is too.
        assert!(test_call(&lan, &ApiKey::default()).is_ok());
        let local = AiSettings { endpoint: "http://127.0.0.1:1234/v1".into(), model: "m".into() };
        assert_eq!(test_call(&local, &ApiKey::new("http://127.0.0.1:1234/v1", "sk-1")).unwrap().key, "sk-1");
    }

    #[test]
    fn debug_output_never_shows_the_key() {
        let key = ApiKey::new("https://api.openai.com/v1", "sk-secret-123");
        let call = AiCall { url: "https://api.openai.com/v1/chat/completions".into(), key: "sk-secret-123".into(), body: json!({}) };
        for shown in [format!("{key:?}"), format!("{call:?}"), format!("{:?}", Some(&call))] {
            assert!(!shown.contains("sk-secret-123") && shown.contains("<hidden>"), "{shown}");
        }
    }

    #[test]
    fn fields_are_trimmed_and_must_be_one_short_line() {
        assert_eq!(field("model", "  llama3  ").unwrap(), "llama3");
        assert!(field("model", "a\nb").is_err());
        assert!(field("key", &"k".repeat(MAX_FIELD + 1)).is_err());
    }

    #[test]
    fn the_reply_text_or_the_servers_error_is_read() {
        let ok = json!({"choices": [{"message": {"role": "assistant", "content": "## A\n- b"}}]});
        assert_eq!(reply_text(&ok).unwrap(), "## A\n- b");
        let err = json!({"error": {"message": "Incorrect API key provided"}});
        assert!(reply_text(&err).unwrap_err().contains("Incorrect API key"));
        assert!(reply_text(&json!({"error": "model not found"})).unwrap_err().contains("model not found"));
        assert!(reply_text(&json!({"choices": []})).is_err());
        assert!(reply_text(&json!("hello")).is_err());
    }

    #[test]
    fn thinking_and_code_fences_are_left_out_of_the_outline() {
        let reply = "<think>The user wants planets.</think>\n```markdown\n## Planets\n- Eight of them\n```\n";
        assert_eq!(outline(reply), "## Planets\n- Eight of them");
        assert_eq!(outline("<think>never ends"), "");
        assert_eq!(outline("<think>a</think>## One\n<think>b</think>- two"), "## One\n- two");
    }
}
