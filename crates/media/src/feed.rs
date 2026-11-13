//! Video frames for a playback clock: decoding runs ahead on a worker thread (inline on wasm), and
//! [`VideoFeed::frame_at`] hands out the frame due at the clock's time, seeking when the clock
//! jumps (seek, loop).

use std::sync::Arc;

use crate::video::{Frame, VideoDecoder};
use crate::{Bytes, Result};

/// Frames decoded ahead of the clock.
#[cfg(not(target_arch = "wasm32"))]
const AHEAD: usize = 6;

/// A jump forward larger than this (seconds) seeks instead of decoding through.
const MAX_CATCH_UP: f64 = 1.5;

#[cfg(not(target_arch = "wasm32"))]
enum Cmd {
    Seek(f64, u64),
}

pub struct VideoFeed {
    size: (u32, u32),
    current: Option<Arc<Frame>>,
    next: Option<Arc<Frame>>,
    generation: u64,
    ended: bool,
    #[cfg(not(target_arch = "wasm32"))]
    tx: std::sync::mpsc::Sender<Cmd>,
    #[cfg(not(target_arch = "wasm32"))]
    rx: std::sync::mpsc::Receiver<(u64, Option<Frame>)>,
    #[cfg(target_arch = "wasm32")]
    dec: VideoDecoder,
}

impl VideoFeed {
    pub fn new(bytes: Bytes) -> Result<VideoFeed> {
        let dec = VideoDecoder::open(bytes)?;
        let size = dec.size();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, cmd_rx) = std::sync::mpsc::channel::<Cmd>();
            let (frame_tx, rx) = std::sync::mpsc::sync_channel::<(u64, Option<Frame>)>(AHEAD);
            let worker = move || worker(dec, cmd_rx, frame_tx);
            if let Err(e) = std::thread::Builder::new().name("deckcraft-video-decode".into()).spawn(worker) {
                return Err(crate::MediaError::Corrupt(format!("couldn't start the decoder: {e}")));
            }
            Ok(VideoFeed { size, current: None, next: None, generation: 0, ended: false, tx, rx })
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(VideoFeed { size, current: None, next: None, generation: 0, ended: false, dec })
        }
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Restart decoding at `t`.
    pub fn seek(&mut self, t: f64) {
        self.generation += 1;
        self.current = None;
        self.next = None;
        self.ended = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = self.tx.send(Cmd::Seek(t, self.generation));
            // pull() discards stale generations and unblocks the worker. Draining here could
            // also discard the new generation's frames or EOF if the worker handles seek fast.
        }
        #[cfg(target_arch = "wasm32")]
        self.dec.seek(t);
    }

    /// The next decoded frame, if one is ready (`Err(())` at the end of the stream).
    fn pull(&mut self) -> std::result::Result<Option<Arc<Frame>>, ()> {
        #[cfg(not(target_arch = "wasm32"))]
        loop {
            match self.rx.try_recv() {
                Ok((g, _)) if g != self.generation => continue,
                Ok((_, Some(f))) => return Ok(Some(Arc::new(f))),
                Ok((_, None)) => return Err(()),
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(None),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return Err(()),
            }
        }
        #[cfg(target_arch = "wasm32")]
        match self.dec.next_frame() {
            Ok(Some(f)) => Ok(Some(Arc::new(f))),
            _ => Err(()),
        }
    }

    /// The frame on screen at media time `t` (seconds): the last one whose time is ≤ `t`, or the
    /// first one while nothing earlier exists. `None` until the first frame has decoded.
    pub fn frame_at(&mut self, t: f64) -> Option<Arc<Frame>> {
        let behind = self.current.as_ref().is_some_and(|c| t + 0.02 < c.time);
        let far = self.next.as_ref().or(self.current.as_ref()).is_some_and(|c| t > c.time + MAX_CATCH_UP);
        if behind || far {
            self.seek(t);
        }
        loop {
            if self.next.is_none() && !self.ended {
                match self.pull() {
                    Ok(Some(f)) => self.next = Some(f),
                    Ok(None) => break,
                    Err(()) => self.ended = true,
                }
            }
            match &self.next {
                Some(n) if n.time <= t + 1e-3 || self.current.is_none() => {
                    self.current = self.next.take();
                }
                _ => break,
            }
        }
        self.current.clone()
    }

    /// The decoder has delivered its last frame.
    pub fn ended(&self) -> bool {
        self.ended && self.next.is_none()
    }

    /// Wait (up to `timeout`) for the frame at `t` — for tests and offline rendering.
    pub fn frame_at_blocking(&mut self, t: f64, timeout: std::time::Duration) -> Option<Arc<Frame>> {
        let mut waited = std::time::Duration::ZERO;
        loop {
            let f = self.frame_at(t);
            let done = self.ended || self.next.is_some();
            if done || waited >= timeout || !crate::THREADS {
                return f;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
            waited += std::time::Duration::from_millis(2);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn worker(mut dec: VideoDecoder, cmds: std::sync::mpsc::Receiver<Cmd>, frames: std::sync::mpsc::SyncSender<(u64, Option<Frame>)>) {
    let mut generation = 0u64;
    let mut at_end = false;
    loop {
        // Commands first; block on them once the stream has ended.
        let cmd = if at_end {
            match cmds.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        } else {
            match cmds.try_recv() {
                Ok(c) => Some(c),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        };
        if let Some(Cmd::Seek(t, g)) = cmd {
            // Only the latest seek matters.
            let (mut t, mut g) = (t, g);
            while let Ok(Cmd::Seek(t2, g2)) = cmds.try_recv() {
                (t, g) = (t2, g2);
            }
            dec.seek(t);
            generation = g;
        }
        let frame = match dec.next_frame() {
            Ok(f) => f,
            Err(e) => {
                log::debug!("video decode stopped: {e}");
                None
            }
        };
        at_end = frame.is_none();
        if frames.send((generation, frame)).is_err() {
            return;
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn seek_keeps_new_generation_frames_and_end_of_stream() {
        let (tx, _commands) = std::sync::mpsc::channel();
        let (frames, rx) = std::sync::mpsc::sync_channel(AHEAD);
        let frame = |time| Frame { time, width: 1, height: 1, rgba: vec![0, 0, 0, 255] };
        frames.send((0, Some(frame(0.3)))).expect("stale frame");
        frames.send((0, None)).expect("stale end of stream");
        // Model a fast worker's seek response already waiting when seek drains the queue.
        frames.send((1, Some(frame(0.0)))).expect("new frame");
        frames.send((1, None)).expect("new end of stream");
        let mut feed = VideoFeed { size: (1, 1), current: None, next: None, generation: 0, ended: false, tx, rx };

        feed.seek(0.05);
        let shown = feed.frame_at(0.05).expect("frame from the new generation");
        assert_eq!(shown.time, 0.0);
        assert!(feed.ended());

        frames.send((2, Some(frame(0.3)))).expect("superseded seek frame");
        frames.send((2, None)).expect("superseded seek end of stream");
        frames.send((3, Some(frame(0.0)))).expect("latest seek frame");
        frames.send((3, None)).expect("latest seek end of stream");
        feed.seek(0.35);
        feed.seek(0.05);
        assert_eq!(feed.frame_at(0.05).expect("latest seek wins").time, 0.0);
        assert!(feed.ended());
    }

    #[test]
    fn seek_unblocks_worker_when_stale_queue_is_full() {
        let (tx, commands) = std::sync::mpsc::channel();
        let (frames, rx) = std::sync::mpsc::sync_channel(AHEAD);
        let frame = || Frame { time: 0.0, width: 1, height: 1, rgba: vec![0, 0, 0, 255] };
        for _ in 0..AHEAD {
            frames.send((0, Some(frame()))).expect("fill stale queue");
        }
        let worker = std::thread::spawn(move || {
            let Cmd::Seek(_, generation) = commands.recv().expect("seek command");
            frames.send((generation, Some(frame()))).expect("seek frame");
            frames.send((generation, None)).expect("seek end of stream");
        });
        let mut feed = VideoFeed { size: (1, 1), current: None, next: None, generation: 0, ended: false, tx, rx };

        feed.seek(0.05);
        let shown = feed.frame_at_blocking(0.05, std::time::Duration::from_secs(5)).expect("worker unblocked");
        assert_eq!(shown.time, 0.0);
        assert!(feed.ended());
        worker.join().expect("worker completed");
    }
}
