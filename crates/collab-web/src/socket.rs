//! The browser's `WebSocket` and `fetch`, driven from the UI frame.
//!
//! There are no threads on wasm32: the socket's callbacks push into a queue that
//! [`Transport::poll`] drains once per frame, and every message wakes the app so an idle page does
//! not sit on an unread frame. A close schedules a reconnect with the same backoff as the desktop
//! link (250 ms doubling to 10 s); operations the server never confirmed are resent by
//! `CollabDoc` after the next welcome, so a dropped socket loses nothing.
//!
//! The token is never part of a URL: the socket's handshake frame and the `Authorization` header of
//! a media request are the only two places it appears (ADR-0008).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{CloseEvent, Event, MessageEvent, Request, RequestInit, Response, WebSocket};

use crate::media::MediaUrls;
use crate::{Cache, cache_put};

/// What the frame loop hears from the socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Incoming {
    /// The socket is open: send the handshake.
    Opened,
    /// One text frame from the server.
    Text(String),
    /// The socket closed; a reconnect is scheduled.
    Closed,
}

/// The socket and the messages it produced since the last frame.
pub struct Transport {
    inner: Rc<RefCell<Inner>>,
}

struct Inner {
    url: String,
    socket: Option<WebSocket>,
    incoming: VecDeque<Incoming>,
    attempt: u32,
    live: bool,
    closers: Closers,
    wake: Rc<dyn Fn()>,
}

#[derive(Default)]
#[allow(dead_code, reason = "the closures are kept alive by being stored; they are never called from Rust")]
struct Closers {
    open: Option<Closure<dyn FnMut(Event)>>,
    message: Option<Closure<dyn FnMut(MessageEvent)>>,
    close: Option<Closure<dyn FnMut(CloseEvent)>>,
}

impl Transport {
    /// Open `url`; the token is not part of it, and `wake` asks the page for a frame.
    pub fn open(url: &str, wake: Rc<dyn Fn()>) -> Result<Transport, String> {
        let inner = Rc::new(RefCell::new(Inner {
            url: url.to_string(),
            socket: None,
            incoming: VecDeque::new(),
            attempt: 0,
            live: true,
            closers: Closers::default(),
            wake,
        }));
        dial(&inner)?;
        Ok(Transport { inner })
    }

    /// Drain what arrived since the previous frame.
    pub fn poll(&self) -> Vec<Incoming> {
        let mut inner = self.inner.borrow_mut();
        inner.incoming.drain(..).collect()
    }

    /// Send one text frame; an error means the socket is not open (the caller keeps the operation
    /// for the resend after the reconnect).
    pub fn send(&self, text: &str) -> Result<(), String> {
        let inner = self.inner.borrow();
        let Some(socket) = &inner.socket else { return Err("the collaboration socket is not open".into()) };
        if socket.ready_state() != WebSocket::OPEN {
            return Err("the collaboration socket is not open".into());
        }
        socket.send_with_str(text).map_err(|err| format!("cannot send a collaboration frame: {err:?}"))
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.live = false;
        if let Some(socket) = inner.socket.take() {
            let _ = socket.close();
        }
    }
}

/// Make one socket and wire its callbacks.
fn dial(inner: &Rc<RefCell<Inner>>) -> Result<(), String> {
    let url = inner.borrow().url.clone();
    // Drop the previous socket's callbacks first: they point at closures this call replaces.
    if let Some(old) = inner.borrow_mut().socket.take() {
        old.set_onopen(None);
        old.set_onmessage(None);
        old.set_onclose(None);
        let _ = old.close();
    }
    let socket = WebSocket::new(&url).map_err(|err| format!("cannot open a collaboration socket: {err:?}"))?;
    socket.set_binary_type(web_sys::BinaryType::Arraybuffer);
    let weak = Rc::downgrade(inner);

    let on_open = Closure::<dyn FnMut(Event)>::new(move |_event: Event| {
        let Some(inner) = weak.upgrade() else { return };
        let wake = {
            let mut inner = inner.borrow_mut();
            inner.attempt = 0;
            inner.incoming.push_back(Incoming::Opened);
            inner.wake.clone()
        };
        wake();
    });
    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));

    let weak = Rc::downgrade(inner);
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let Some(inner) = weak.upgrade() else { return };
        let Some(text) = event.data().as_string() else { return };
        let wake = {
            let mut inner = inner.borrow_mut();
            inner.incoming.push_back(Incoming::Text(text));
            inner.wake.clone()
        };
        wake();
    });
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

    let weak = Rc::downgrade(inner);
    let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_event: CloseEvent| {
        let Some(inner) = weak.upgrade() else { return };
        {
            let mut inner = inner.borrow_mut();
            if !inner.live {
                return;
            }
            inner.socket = None;
            inner.incoming.push_back(Incoming::Closed);
        }
        schedule(&inner);
    });
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));

    let mut inner = inner.borrow_mut();
    inner.socket = Some(socket);
    inner.closers = Closers { open: Some(on_open), message: Some(on_message), close: Some(on_close) };
    Ok(())
}

/// Reconnect after a backoff: 250 ms, doubling, at most 10 s.
fn schedule(inner: &Rc<RefCell<Inner>>) {
    let (delay, wake) = {
        let mut inner = inner.borrow_mut();
        if !inner.live {
            return;
        }
        let delay = 250u32.saturating_mul(1u32 << inner.attempt.min(6));
        inner.attempt = inner.attempt.saturating_add(1);
        (delay.min(10_000), inner.wake.clone())
    };
    let weak = Rc::downgrade(inner);
    let timer = Closure::<dyn FnMut()>::once(move || {
        if let Some(inner) = weak.upgrade()
            && inner.borrow().live
            && let Err(_error) = dial(&inner)
        {
            schedule(&inner);
        }
    });
    if let Some(window) = web_sys::window()
        && window.set_timeout_with_callback_and_timeout_and_arguments_0(timer.as_ref().unchecked_ref(), delay as i32).is_ok()
    {
        // A closure that fires once; keeping it alive for the page would leak one small allocation
        // per reconnect, dropping it would free code that is still running.
        timer.forget();
    }
    wake();
}

/// One HTTP request with the session's bearer token. The body is returned as it arrived.
async fn request(urls: &MediaUrls, target: &str, method: &str, body: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(web_sys::RequestMode::Cors);
    let headers = web_sys::Headers::new().map_err(|err| format!("media: {err:?}"))?;
    headers.set("Authorization", &urls.bearer()).map_err(|err| format!("media: {err:?}"))?;
    if let Some(bytes) = &body {
        headers.set("Content-Type", "application/octet-stream").map_err(|err| format!("media: {err:?}"))?;
        init.set_body(&js_sys::Uint8Array::from(bytes.as_slice()));
    }
    init.set_headers(&headers);
    let request = Request::new_with_str_and_init(target, &init).map_err(|err| format!("media: {err:?}"))?;
    let response = JsFuture::from(window.fetch_with_request(&request)).await.map_err(|err| format!("media: {err:?}"))?;
    let response: Response = response.dyn_into().map_err(|_| "media: the response is not a Response".to_string())?;
    if !(200..300).contains(&response.status()) {
        return Err(format!("media: the server answered {}", response.status()));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(|err| format!("media: {err:?}"))?).await.map_err(|err| format!("media: {err:?}"))?;
    let bytes = js_sys::Uint8Array::new(&buffer);
    Ok(bytes.to_vec())
}

/// Fetch one object into the replica's cache and wake the page. A failure is logged: the frame that
/// needs it is released after the wait and the adapter then reports the picture it cannot place.
pub fn fetch(urls: &MediaUrls, sha: &str, cache: Cache, wake: Rc<dyn Fn()>) {
    let target = urls.fetch(sha);
    let urls = urls.clone();
    let sha = sha.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        match request(&urls, &target, "GET", None).await {
            Ok(bytes) => {
                cache_put(&cache, &sha, bytes);
                wake();
            }
            Err(error) => log::warn!("collab media {sha}: {error}"),
        }
    });
}

/// Upload one object (a picture, a clip) before its operation reaches the server.
pub fn upload(urls: &MediaUrls, sha: &str, bytes: &[u8], wake: Rc<dyn Fn()>) {
    let target = urls.upload(sha);
    let urls = urls.clone();
    let sha = sha.to_string();
    let body = bytes.to_vec();
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(error) = request(&urls, &target, "PUT", Some(body)).await {
            log::warn!("collab media upload {sha}: {error}");
        }
        wake();
    });
}
