//! The desktop WebSocket link: one background thread, two channels, and a read timeout so the
//! UI thread can push messages without waiting for the socket.
//!
//! The link moves bytes only. It reconnects and resends the handshake, but the session state
//! (queue, versions, resync) stays in [`crate::ClientSession`]; a dropped socket is reported as
//! `ServerMsg::Error { code: "offline" }`, and the application decides when to retry.

use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;
use std::time::Duration;

use deckcraft_collab_proto::{ClientMsg, ServerMsg, decode_server, encode_client};

use crate::Transport;
use tungstenite::error::Error as WsError;
use tungstenite::stream::MaybeTlsStream;

/// A desktop WebSocket client with a background reader/writer.
pub struct WsTransport {
    out: Sender<ClientMsg>,
    incoming: Receiver<ServerMsg>,
    open: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl WsTransport {
    /// Connect to `url` and send `hello` first; the thread reconnects with the same handshake and
    /// keeps unread messages in the channel while the UI is busy.
    pub fn start(url: impl Into<String>, hello: ClientMsg) -> Self {
        let (out_tx, out_rx) = channel::<ClientMsg>();
        let (in_tx, in_rx) = channel::<ServerMsg>();
        let open = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_url = url.into();
        let thread_open = open.clone();
        let thread_stop = stop.clone();
        let join = thread::spawn(move || run(&thread_url, hello, out_rx, in_tx, thread_open, thread_stop));
        WsTransport { out: out_tx, incoming: in_rx, open, stop, join: Some(join) }
    }
}

impl Transport for WsTransport {
    fn send(&mut self, msg: ClientMsg) -> Result<(), String> {
        self.out.send(msg).map_err(|e| e.to_string())
    }

    fn next(&mut self) -> Option<ServerMsg> {
        self.incoming.try_recv().ok()
    }

    fn is_open(&self) -> bool {
        self.open.load(Ordering::Relaxed)
    }
}

impl Drop for WsTransport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            // The reader loop wakes on its 50 ms timeout; don't block the UI if it is mid-connect.
            let _ = join.join();
        }
    }
}

fn run(url: &str, hello: ClientMsg, out: Receiver<ClientMsg>, incoming: Sender<ServerMsg>, open: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    let mut backoff = Duration::from_millis(200);
    while !stop.load(Ordering::Relaxed) {
        match tungstenite::connect(url) {
            Ok((mut ws, _)) => {
                if set_timeout(&mut ws, Duration::from_millis(50)).is_err() {
                    let _ = incoming.send(offline("cannot set a socket timeout"));
                    break;
                }
                if send_json(&mut ws, &hello).is_err() {
                    let _ = incoming.send(offline("the handshake could not be sent"));
                    thread::sleep(backoff);
                    continue;
                }
                open.store(true, Ordering::Relaxed);
                backoff = Duration::from_millis(200);
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    // Messages from the UI first, so a command is not starved by an idle socket.
                    loop {
                        match out.try_recv() {
                            Ok(msg) => {
                                if send_json(&mut ws, &msg).is_err() {
                                    break;
                                }
                            }
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => {
                                stop.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                    match ws.read() {
                        Ok(tungstenite::Message::Text(text)) => {
                            if let Ok(msg) = decode_server(text.as_str()) {
                                let _ = incoming.send(msg);
                            }
                        }
                        Ok(tungstenite::Message::Ping(payload)) => {
                            let _ = ws.send(tungstenite::Message::Pong(payload));
                        }
                        Ok(tungstenite::Message::Close(_)) => break,
                        Ok(_) => {}
                        Err(WsError::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                        Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => break,
                        Err(e) => {
                            log::debug!("collab ws: {e}");
                            break;
                        }
                    }
                }
                open.store(false, Ordering::Relaxed);
                let _ = incoming.send(offline("the connection was lost"));
            }
            Err(e) => {
                open.store(false, Ordering::Relaxed);
                let _ = incoming.send(offline(&format!("cannot connect: {e}")));
            }
        }
        let mut slept = Duration::ZERO;
        while slept < backoff && !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));
            slept += Duration::from_millis(50);
        }
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
    open.store(false, Ordering::Relaxed);
}

fn send_json(ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>, msg: &ClientMsg) -> Result<(), String> {
    let text = encode_client(msg).map_err(|e| e.to_string())?;
    ws.send(tungstenite::Message::Text(text.into())).map_err(|e| e.to_string())
}

fn set_timeout(ws: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>, timeout: Duration) -> std::io::Result<()> {
    match ws.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(timeout)),
        MaybeTlsStream::Rustls(stream) => stream.get_ref().set_read_timeout(Some(timeout)),
        _ => Err(std::io::Error::other("unsupported stream")),
    }
}

fn offline(message: &str) -> ServerMsg {
    ServerMsg::Error(deckcraft_collab_proto::ServerError { code: "offline".into(), message: message.into(), retryable: true })
}
