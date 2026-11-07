//! `window.deckcraft`: the page command API (`craft-web-api`, storytold/craft-libs#413) over the
//! app's own control channel, the one the desktop control server drives (`docs/control-protocol.md`).
//!
//! The crate owns the JavaScript side (`request`, `execute`, `commands`, `docInspect`, `uiInspect`,
//! `screenshot`, as promises with a timeout). This module is DeckCraft's part: a [`Request`] that
//! queues a [`ControlRequest`] for the app, which answers it on its next frame, and waits for the
//! reply. A call the page gave up on is released ([`Pending`]), so the app skips it if it has not started it, instead of
//! applying a command the page was told had failed.

use std::sync::Arc;
use std::sync::atomic::AtomicU8;
use std::sync::mpsc::{Receiver, TryRecvError};

use deckcraft_ui_egui::ControlRequest;
use serde_json::{Value, json};

/// How often a waiting call looks for the app's reply, in milliseconds (about a frame).
#[cfg(target_arch = "wasm32")]
const POLL_MS: i32 = 16;

/// A queued control request whose reply has not arrived. Dropping it before the reply (the page's
/// timeout fired, or the call was abandoned) releases the request: the app skips it if it has not started it (one it has taken still finishes).
struct Pending {
    reply: Receiver<Value>,
    state: Arc<AtomicU8>,
    answered: bool,
}

impl Pending {
    /// `Some` once the app has answered or gone away; `None` while the request is still waiting.
    fn poll(&mut self) -> Option<Value> {
        match self.reply.try_recv() {
            Ok(reply) => {
                self.answered = true;
                Some(reply)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.answered = true;
                Some(json!({"ok": false, "error": "the app dropped the request"}))
            }
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.answered {
            ControlRequest::abandon(&self.state);
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::install;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::rc::Rc;
    use std::sync::mpsc::channel;

    use craft_web_api::methods::Methods;
    use craft_web_api::page::{PageApi, Request};
    use deckcraft_ui_egui::{ControlRequest, SlideApp};
    use serde_json::json;
    use wasm_bindgen_futures::JsFuture;

    use super::{POLL_MS, Pending};

    /// The app id in `window.<app>`.
    const APP: &str = "deckcraft";

    /// Give the app its control queue and put `window.deckcraft` on the page. An existing
    /// `window.deckcraft` keeps the functions it already has.
    pub fn install(app: &mut SlideApp, ctx: egui::Context) {
        let (queue, requests) = channel::<ControlRequest>();
        app.control_rx = Some(requests);
        let request: Request = Rc::new(move |method, params| {
            let (control, reply) = ControlRequest::new(method, params);
            let mut pending = Pending { reply, state: control.state.clone(), answered: false };
            if queue.send(control).is_err() {
                pending.answered = true;
                return Box::pin(async { json!({"ok": false, "error": "the app is not running"}) });
            }
            ctx.request_repaint();
            Box::pin(async move {
                loop {
                    if let Some(reply) = pending.poll() {
                        return reply;
                    }
                    sleep(POLL_MS).await;
                }
            })
        });
        if let Err(e) = Rc::new(PageApi::new(APP, Methods::default(), request)).install() {
            log::warn!("couldn't install window.{APP}: {e:?}");
        }
    }

    async fn sleep(ms: i32) {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            if let Some(window) = web_sys::window() {
                let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
            }
        });
        let _ = JsFuture::from(promise).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    /// `ControlRequest`'s `state` values (`deckcraft_ui_egui::control`): the app has started it, or its
    /// sender gave up.
    const TAKEN: u8 = 1;
    const ABANDONED: u8 = 2;

    fn pending() -> (Pending, ControlRequest) {
        let (control, reply) = ControlRequest::new("document.inspect", json!({}));
        (Pending { reply, state: control.state.clone(), answered: false }, control)
    }

    #[test]
    fn a_call_the_page_gave_up_on_is_released_so_the_app_skips_it() {
        let (waiting, control) = pending();
        drop(waiting);
        assert_eq!(control.state.load(Ordering::SeqCst), ABANDONED);
        assert!(!control.take(), "the app must not run a request the page was told had failed");
    }

    #[test]
    fn an_answered_call_is_not_released() {
        let (mut waiting, control) = pending();
        assert!(control.take());
        control.reply.send(json!({"ok": true, "result": 1})).unwrap_or_default();
        assert_eq!(waiting.poll(), Some(json!({"ok": true, "result": 1})));
        drop(waiting);
        assert_eq!(control.state.load(Ordering::SeqCst), TAKEN);
    }

    #[test]
    fn a_request_the_app_never_runs_waits_until_it_is_dropped() {
        let (mut waiting, control) = pending();
        assert_eq!(waiting.poll(), None);
        drop(control);
        assert_eq!(waiting.poll().and_then(|r| r["ok"].as_bool()), Some(false), "the app went away");
    }
}
