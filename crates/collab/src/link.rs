//! The one method set a UI needs from a transport. Desktops implement it with a WebSocket
//! thread, the browser with `web-sys`, tests with an in-process pipe.

use deckcraft_collab_proto::{ClientMsg, ServerMsg};

pub trait Transport: Send {
    /// Queue one message. The link may hold it until the socket is live.
    fn send(&mut self, msg: ClientMsg) -> Result<(), String>;
    /// Take the next message, if one has arrived.
    fn next(&mut self) -> Option<ServerMsg>;
    /// Whether the socket is connected right now (not whether the session is live).
    fn is_open(&self) -> bool;
    /// Upload picture bytes to the media channel. A transport without a media side channel
    /// refuses: a picture inserted over it would be rejected by the server, never silently lost.
    fn put_media(&mut self, _sha: &str, _bytes: &[u8]) -> Result<(), String> {
        Err("this transport has no media channel".into())
    }
}

/// An in-process transport: tests (and a browser bridge that is not built yet) can drive the full
/// message loop without a socket.
pub struct PipeTransport {
    out: std::sync::mpsc::Sender<ClientMsg>,
    incoming: std::sync::mpsc::Receiver<ServerMsg>,
    open: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl PipeTransport {
    /// One end for [`crate::CollabDoc`], the other for the test server: `(ui, server_receives,
    /// server_sends)`.
    pub fn pair() -> (Self, std::sync::mpsc::Receiver<ClientMsg>, std::sync::mpsc::Sender<ServerMsg>) {
        let (out, server_in) = std::sync::mpsc::channel();
        let (server_out, incoming) = std::sync::mpsc::channel();
        (PipeTransport { out, incoming, open: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)) }, server_in, server_out)
    }
}

impl Transport for PipeTransport {
    fn send(&mut self, msg: ClientMsg) -> Result<(), String> {
        self.out.send(msg).map_err(|e| e.to_string())
    }
    fn next(&mut self) -> Option<ServerMsg> {
        self.incoming.try_recv().ok()
    }
    fn is_open(&self) -> bool {
        self.open.load(std::sync::atomic::Ordering::Relaxed)
    }
}
