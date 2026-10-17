//! Document events for add-ins (plugin protocol 1.2): `events/documentOpened`,
//! `events/documentSaved` and `events/documentClosed`, sent to every loaded, enabled add-in whose
//! manifest declares the event, **after** the event happened.
//!
//! The commands that open, save and close (`file.open`, `file.adopt`, `file.save`, `file.saveAs`,
//! `file.saveTemplate`, `file.close`) queue the event; once the outermost command has finished,
//! the queue goes to one background thread that sends the events in order. So an event never
//! blocks or fails the save, the open or the close, and is never part of an undo step. Autosave
//! (`file.recovery.save`) sends none. The host collects what the add-ins answered;
//! [`Session::take_event_outcomes`] hands it to the UI, which shows it in the status bar.
//!
//! During an event an add-in may use `host/log` and `host/message`; `host/sync` and
//! `host/vbaProject/*` are refused with `AccessDenied` (reads too): events are notifications, not
//! edits, as in GridCraft and WordCraft.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use deckcraft_plugin::HostError;
use serde_json::{Value as Json, json};

use crate::Session;

/// `events/documentOpened {path, name}`.
pub const DOCUMENT_OPENED: &str = "documentOpened";
/// `events/documentSaved {path, name, format}`.
pub const DOCUMENT_SAVED: &str = "documentSaved";
/// `events/documentClosed {path, name}`.
pub const DOCUMENT_CLOSED: &str = "documentClosed";

/// Events waiting to be delivered before new ones are dropped (an add-in that hangs on every event
/// must not grow the queue without bound).
const MAX_QUEUED: usize = 256;

/// What one add-in answered to one event.
#[derive(Clone, Debug, PartialEq)]
pub struct EventOutcome {
    pub plugin: String,
    pub event: String,
    /// `Ok(message)`: the answer's `message`, if any; `Err`: the error (shown as an error notice).
    pub result: Result<Option<String>, String>,
    /// What it said through `host/message` meanwhile.
    pub notes: Vec<String>,
}

struct Job {
    event: &'static str,
    params: Json,
    listeners: Vec<String>,
}

#[derive(Default)]
struct Shared {
    outcomes: Mutex<Vec<EventOutcome>>,
    in_flight: AtomicUsize,
}

/// The session's event queue and the thread that delivers it.
#[derive(Default)]
pub(crate) struct Events {
    queued: Vec<Job>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<std::sync::mpsc::Sender<Job>>,
    shared: Arc<Shared>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// The file name of `path` (the whole path when it has none).
pub fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

/// Counts a job as delivered however its delivery ends (a panic included).
struct Delivered(Arc<Shared>);

impl Drop for Delivered {
    fn drop(&mut self) {
        let _ = self.0.in_flight.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| Some(n.saturating_sub(1)));
    }
}

impl Session {
    /// Queues `event` about the file at `path` for the add-ins that hear it (nothing when none
    /// does). `format` is `documentSaved`'s. Sent once the outermost command has finished.
    pub(crate) fn queue_event(&mut self, event: &'static str, path: &str, format: Option<&str>) {
        let listeners = deckcraft_plugin::listeners(event);
        if listeners.is_empty() {
            return;
        }
        if self.events.queued.len() + self.events.shared.in_flight.load(Ordering::SeqCst) >= MAX_QUEUED {
            log::warn!("add-in events: too many waiting, {event} for {path} dropped");
            return;
        }
        let mut params = json!({"path": path, "name": file_name(path)});
        if let Some(f) = format {
            params["format"] = json!(f);
        }
        self.events.queued.push(Job { event, params, listeners });
    }

    /// Hands the queued events to the delivery thread (after the outermost command).
    pub(crate) fn flush_events(&mut self) {
        if self.events.queued.is_empty() {
            return;
        }
        let jobs = std::mem::take(&mut self.events.queued);
        #[cfg(target_arch = "wasm32")]
        drop(jobs);
        #[cfg(not(target_arch = "wasm32"))]
        for job in jobs {
            let Some(tx) = self.event_worker() else {
                log::warn!("add-in events: no delivery thread, {} dropped", job.event);
                continue;
            };
            self.events.shared.in_flight.fetch_add(1, Ordering::SeqCst);
            if let Err(std::sync::mpsc::SendError(job)) = tx.send(job) {
                // The thread is gone: start a new one next time.
                self.events.worker = None;
                drop(Delivered(self.events.shared.clone()));
                log::warn!("add-in events: the delivery thread stopped, {} dropped", job.event);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn event_worker(&mut self) -> Option<std::sync::mpsc::Sender<Job>> {
        if let Some(tx) = &self.events.worker {
            return Some(tx.clone());
        }
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let shared = self.events.shared.clone();
        std::thread::Builder::new()
            .name("deckcraft-addin-events".into())
            .spawn(move || {
                for job in rx {
                    let _done = Delivered(shared.clone());
                    let outcomes = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| deliver(&job)));
                    match outcomes {
                        Ok(list) => lock(&shared.outcomes).extend(list),
                        Err(_) => log::error!("add-in events: delivering {} panicked", job.event),
                    }
                }
            })
            .ok()?;
        self.events.worker = Some(tx.clone());
        Some(tx)
    }

    /// What the add-ins answered to events since the last call (the UI shows it).
    pub fn take_event_outcomes(&mut self) -> Vec<EventOutcome> {
        std::mem::take(&mut *lock(&self.events.shared.outcomes))
    }

    /// Whether events are still being delivered (or answers wait to be taken).
    pub fn events_pending(&self) -> bool {
        self.events.shared.in_flight.load(Ordering::SeqCst) > 0 || !lock(&self.events.shared.outcomes).is_empty()
    }

    /// Waits at most `timeout` for the events sent so far to be delivered (quitting, the CLI,
    /// tests). True when they all were.
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::disallowed_methods)] // off wasm only: the clock works here
    pub fn wait_events(&mut self, timeout: std::time::Duration) -> bool {
        self.flush_events();
        let end = std::time::Instant::now() + timeout;
        while self.events.shared.in_flight.load(Ordering::SeqCst) > 0 {
            if std::time::Instant::now() >= end {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        true
    }

    /// The app is quitting: `documentClosed` for every open presentation that has a file, then a
    /// wait of at most [`deckcraft_plugin::CLOSE_EVENT_TIMEOUT`] (best effort: the app doesn't
    /// wait longer). Returns whether everything was delivered.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn quit_events(&mut self) -> bool {
        let open: Vec<String> = self.docs.iter().filter_map(|d| d.path.clone()).collect();
        for path in open {
            self.queue_event(DOCUMENT_CLOSED, &path, None);
        }
        self.wait_events(deckcraft_plugin::CLOSE_EVENT_TIMEOUT)
    }
}

/// Sends one event to each add-in that hears it, in order.
fn deliver(job: &Job) -> Vec<EventOutcome> {
    let mut out = Vec::with_capacity(job.listeners.len());
    for id in &job.listeners {
        let mut notes = Vec::new();
        let result = deckcraft_plugin::send_event(id, job.event, job.params.clone(), &mut |m, p| event_host_call(job.event, id, m, p, &mut notes));
        if let Err(e) = &result {
            log::warn!("add-in {id}: events/{}: {e}", job.event);
        }
        out.push(EventOutcome { plugin: id.clone(), event: job.event.to_string(), result, notes });
    }
    out
}

/// The `host/*` requests an add-in may make during an event: log and message. `host/sync` and the
/// VBA project are refused, reads included (`AccessDenied`): an event is a notification, and the
/// presentation may have changed since it happened.
fn event_host_call(event: &str, plugin: &str, method: &str, params: &Json, notes: &mut Vec<String>) -> std::result::Result<Json, HostError> {
    match method {
        "host/log" => {
            let text: String = params.get("text").and_then(Json::as_str).unwrap_or("").chars().take(4000).collect();
            log::info!("add-in {plugin}: {text}");
            Ok(json!({}))
        }
        "host/message" => {
            let text: String = params.get("text").and_then(Json::as_str).unwrap_or("").chars().take(4000).collect();
            if notes.len() < 16 {
                notes.push(text);
            }
            Ok(json!({"result": 1}))
        }
        "host/sync" | "host/vbaProject/get" | "host/vbaProject/set" => Err(HostError {
            code: -32000,
            message: format!("events/{event} is a notification: the presentation can't be read or changed during it"),
            data: json!({"code": "AccessDenied"}),
        }),
        _ => Err(HostError::unknown()),
    }
}
