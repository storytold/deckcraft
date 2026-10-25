//! Bounded screenshot completion for the control channel.
//!
//! egui 0.37 replaced the `UserData` token and `Event::Screenshot` with a one-shot callback.
//! The callback belongs to the viewport output and may run on a different thread; it must not
//! write files or send replies. This app-owned registry is the strong owner of each admitted
//! request and its completion slot, while the callback only holds a [`std::sync::Weak`] handle.
//!
//! Rules enforced here:
//! - at most [`MAX_REQUESTS`] admitted requests;
//! - at most [`MAX_IMAGE_BYTES`] for one `ColorImage` and [`MAX_TOTAL_BYTES`] for the peak
//!   memory of all in-flight/completed-but-undrained captures, including the RGBA copy, the
//!   renderer's straight-alpha copy, the PNG and (for an inline reply) its base64 text;
//! - a request is issued only after the UI has settled for [`SETTLE`] and two real UI passes;
//! - the geometry (logical size, DPI/zoom, viewport and epoch) is recorded in a real UI pass,
//!   never from stale `run_logic` input;
//! - a completion is correlated by token and rejected before saving when its geometry epoch,
//!   image size or document generation no longer matches;
//! - a late completion after cancellation/timeout is released without a file or reply.
//!
//! The one-shot callback wakes only the viewport that issued the screenshot, after the slot lock
//! is released. While queued/settling requests are still waiting to be issued, logic asks a hidden
//! viewport for the bounded paint passes they need; an issued request relies on the screenshot
//! command and the callback wake, and a deadline wake keeps the normal timeout reachable. There is
//! no per-frame 16 ms repaint timer.
//!
//! Web qualification note: the shared eframe fork used here has no `RequestPaintWhileHidden`
//! support. A hidden browser tab may therefore not service `request_repaint_after`; hidden web
//! capture is explicitly unqualified until that fork fix lands. Native hidden/occluded windows
//! are the supported path.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use egui::{ColorImage, Context, ScreenshotCallback, Vec2, ViewportCommand, ViewportId};
use serde_json::json;

use crate::control::ControlResponse;

/// Admitted screenshot requests at once.
pub const MAX_REQUESTS: usize = 4;
/// Bytes for one `ColorImage` (`width * height * 4`). Encoding copies are bounded by the
/// aggregate budget below.
pub const MAX_IMAGE_BYTES: usize = 128 << 20;
/// Peak bytes reserved for all in-flight and completed-but-undrained captures, including the
/// image, its RGBA/encoder copies, the PNG and an inline base64 reply.
pub const MAX_TOTAL_BYTES: usize = 256 << 20;
/// The window must have been visible and unchanged for this long before capture.
pub const SETTLE: Duration = Duration::from_millis(120);
/// Completed UI passes required after admission before capture.
pub const SETTLED_PASSES: u32 = 2;
/// A screenshot that has no completion by then fails, as before 0.37.
pub const DEADLINE: Duration = Duration::from_secs(8);

/// The aggregate reservation after replacing an old reservation with a new one, or `None` when
/// it would exceed [`MAX_TOTAL_BYTES`]. The limit itself is inclusive.
fn budget_total(reserved: usize, replacing: usize, bytes: usize) -> Option<usize> {
    let total = reserved.checked_sub(replacing)?.checked_add(bytes)?;
    (total <= MAX_TOTAL_BYTES).then_some(total)
}

/// Every allocation a screenshot can hold at its peak.
///
/// `save_screenshot` makes one RGBA copy from the `ColorImage`, `Image::to_png` clones that copy
/// again while straightening alpha, and the PNG encoder writes its own buffer. An inline reply
/// (`path` absent) additionally base64-encodes the PNG. Reserving the sum, not just the raw
/// pixels, keeps the 256 MiB aggregate honest during encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryPlan {
    /// The callback's `ColorImage` (`width * height * 4`).
    pub color_image: usize,
    /// The RGBA `Vec<u8>` handed to `deckcraft_render::Image`.
    pub rgba: usize,
    /// `Image::to_png`'s straight-alpha copy.
    pub encoder_copy: usize,
    /// Conservative PNG output bound (raw bytes plus deflate/header slack).
    pub png: usize,
    /// Inline base64 text; zero when the request writes a path instead.
    pub inline: usize,
    /// Peak bytes held at once: all of the above.
    pub peak: usize,
}

impl MemoryPlan {
    /// Checked plan for an exact pixel size. `None` for zero, non-`u32`, or overflowing images.
    pub fn for_image(width: usize, height: usize, inline_response: bool) -> Option<Self> {
        if u32::try_from(width).is_err() || u32::try_from(height).is_err() {
            return None;
        }
        let color_image = width.checked_mul(height)?.checked_mul(std::mem::size_of::<egui::Color32>())?;
        if color_image == 0 {
            return None;
        }
        let rgba = color_image;
        let encoder_copy = color_image;
        // The image crate's PNG encoder cannot expand past the raw scanlines, one filter byte
        // per row, zlib block headers and chunk framing. 1% plus that row overhead and 1 KiB is
        // comfortably above the worst case, including a one-pixel-wide very tall image.
        let png = color_image.checked_add(height)?.checked_add(color_image / 100)?.checked_add(1024)?;
        let mut peak = color_image.checked_mul(3)?.checked_add(png)?;
        let inline = if inline_response {
            let base64 = png.checked_add(2)?.checked_div(3)?.checked_mul(4)?;
            peak = peak.checked_add(base64)?;
            base64
        } else {
            0
        };
        Some(Self { color_image, rgba, encoder_copy, png, inline, peak })
    }

    /// Checked plan for a reserved viewport geometry.
    pub fn for_geometry(geometry: Geometry, inline_response: bool) -> Option<Self> {
        let [width, height] = geometry.pixel_size()?;
        Self::for_image(width, height, inline_response)
    }
}

/// Geometry recorded from a real UI pass. `epoch` advances on every size, DPI/zoom or viewport
/// change, including a change back, so a stale image cannot be relabelled with fresh bounds.
///
/// eframe 0.37 exposes no surface-generation identity: a same-size surface recreation cannot be
/// distinguished from an unchanged surface. That remains an explicit native qualification gap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub size: Vec2,
    pub pixels_per_point: f32,
    pub viewport: ViewportId,
    pub epoch: u64,
}

impl Geometry {
    /// Physical pixels the viewport should return, or `None` for a zero/NaN/hostile size.
    pub fn pixel_size(self) -> Option<[usize; 2]> {
        if !self.size.x.is_finite() || !self.size.y.is_finite() || !self.pixels_per_point.is_finite() {
            return None;
        }
        let w = (self.size.x.max(0.0) as f64 * self.pixels_per_point as f64).round();
        let h = (self.size.y.max(0.0) as f64 * self.pixels_per_point as f64).round();
        if w < 1.0 || h < 1.0 || w > usize::MAX as f64 || h > usize::MAX as f64 {
            return None;
        }
        Some([w as usize, h as usize])
    }

    /// Checked bytes for a reserved image.
    pub fn image_bytes(self) -> Option<usize> {
        let [w, h] = self.pixel_size()?;
        w.checked_mul(h)?.checked_mul(std::mem::size_of::<egui::Color32>())
    }
}

/// Bounds a callback image must satisfy before it is stored. `max_image_bytes` is the
/// `ColorImage` share of the request's reservation for the geometry its screenshot was issued
/// against; the aggregate reservation also covers the copies and encoding in [`MemoryPlan`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureBounds {
    pub max_image_bytes: usize,
}

#[derive(Debug)]
enum SlotState {
    Pending,
    Ready(Arc<ColorImage>),
    /// A frame outside the reserved capture geometry. It is not stored; drain retries it until
    /// the request's normal deadline, so an oversized image is never retained.
    Retry(&'static str),
    Rejected(&'static str),
    Cancelled,
}

/// Completion slot shared by the registry (strong) and the callback (weak).
#[derive(Debug)]
pub struct CaptureSlot {
    token: u64,
    state: Mutex<SlotState>,
}

impl CaptureSlot {
    /// Create a pending completion slot bound to one screenshot request token.
    fn new(token: u64) -> Self {
        Self { token, state: Mutex::new(SlotState::Pending) }
    }

    /// Called only by the one-shot egui callback. Never writes files or sends replies.
    ///
    /// The image's byte size is checked against the reservation for the issued geometry *before*
    /// the pixels are stored. A larger image is a retryable resize, not a permanent user error.
    /// A smaller image is kept so [`Capture::drain`] can compare the actual size exactly and
    /// retry instead of saving a frame from an older surface. Returns true when this completion
    /// changed the slot, so the callback can wake the viewport after the lock is released.
    pub fn complete(&self, token: u64, bounds: CaptureBounds, image: Arc<ColorImage>) -> bool {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if token != self.token || !matches!(*state, SlotState::Pending) {
            return false;
        }
        let [width, height] = image.size;
        if width == 0 || height == 0 {
            *state = SlotState::Rejected("the screenshot was empty");
            return true;
        }
        let bytes = width.checked_mul(height).and_then(|pixels| pixels.checked_mul(std::mem::size_of::<egui::Color32>()));
        match bytes {
            Some(bytes) if bytes <= bounds.max_image_bytes => *state = SlotState::Ready(image),
            _ => *state = SlotState::Retry("the screenshot was larger than the reserved window"),
        }
        true
    }

    /// Atomically replace the slot with `Pending` and return its previous state.
    fn take(&self) -> SlotResult {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match std::mem::replace(&mut *state, SlotState::Pending) {
            SlotState::Pending => SlotResult::Pending,
            SlotState::Ready(image) => SlotResult::Ready(image),
            SlotState::Retry(why) => SlotResult::Retry(why),
            SlotState::Rejected(why) => SlotResult::Rejected(why),
            SlotState::Cancelled => SlotResult::Cancelled,
        }
    }

    /// Mark the slot cancelled so a later completion or drain cannot deliver an image.
    fn cancel(&self) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *state = SlotState::Cancelled;
    }
}

#[derive(Debug)]
enum SlotResult {
    Pending,
    Ready(Arc<ColorImage>),
    Retry(&'static str),
    Rejected(&'static str),
    Cancelled,
}

/// The one-shot callback wakes only the viewport that issued this screenshot. `complete` has
/// already released the slot lock when this runs, so no lock is held across the wake.
fn completion_callback(ctx: Context, viewport: ViewportId, weak: Weak<CaptureSlot>, token: u64, bounds: CaptureBounds) -> ScreenshotCallback {
    ScreenshotCallback::new(move |image| {
        let Some(slot) = weak.upgrade() else { return };
        if slot.complete(token, bounds, image) {
            ctx.request_repaint_of(viewport);
        }
    })
}

#[derive(Clone, Copy, Debug)]
struct Issued {
    geometry: Geometry,
}

struct Request {
    token: u64,
    path: Option<String>,
    reply: std::sync::mpsc::Sender<ControlResponse>,
    slot: Arc<CaptureSlot>,
    admitted: web_time::Instant,
    deadline: web_time::Instant,
    ui_passes_at_admission: u64,
    doc_generation: Option<u64>,
    issued: Option<Issued>,
    /// Peak bytes reserved for this request. A queued request is reserved from the geometry
    /// known at admission and rebooked when the window resizes; an issued request keeps the
    /// reservation of the geometry whose callback is in flight.
    reservation: usize,
}

/// One completed screenshot handed to the app thread for saving and replying.
pub struct Completed {
    pub token: u64,
    pub path: Option<String>,
    pub reply: std::sync::mpsc::Sender<ControlResponse>,
    pub image: Arc<ColorImage>,
    pub geometry: Geometry,
    /// Still reserved while the app encodes/writes it; release after saving.
    pub reserved: usize,
}

#[derive(Default)]
pub struct Capture {
    next_token: u64,
    requests: Vec<Request>,
    geometry: Option<Geometry>,
    settled_passes: u32,
    ui_passes: u64,
    reserved: usize,
}

impl Capture {
    /// Report whether the maximum number of screenshot requests is already admitted.
    pub fn is_full(&self) -> bool {
        self.requests.len() >= MAX_REQUESTS
    }

    /// Return the number of currently admitted screenshot requests.
    pub fn pending(&self) -> usize {
        self.requests.len()
    }

    /// Peak bytes reserved for in-flight and completed-but-undrained screenshots.
    pub fn reserved_bytes(&self) -> usize {
        self.reserved
    }

    /// Admit one control request, reserving its peak encode memory when the UI geometry is
    /// already known. The caller replies with the returned error when admission is refused.
    pub fn admit(
        &mut self,
        path: Option<String>,
        reply: std::sync::mpsc::Sender<ControlResponse>,
        doc_generation: Option<u64>,
        now: web_time::Instant,
    ) -> Result<(), &'static str> {
        if self.is_full() {
            return Err("busy: at most 4 screenshots may be pending");
        }
        let reservation = match self.geometry {
            Some(geometry) => {
                let plan = MemoryPlan::for_geometry(geometry, path.is_none()).ok_or("the window has no usable surface size")?;
                if plan.color_image > MAX_IMAGE_BYTES {
                    return Err("the screenshot would exceed 128 MiB; use ui.render");
                }
                let total = budget_total(self.reserved, 0, plan.peak)
                    .ok_or("screenshot budget exhausted (4 requests / 256 MiB including encode); wait for one to finish")?;
                self.reserved = total;
                plan.peak
            }
            // No UI pass yet: the first settled pass books the real geometry in `issue`.
            None => 0,
        };
        self.next_token = self.next_token.wrapping_add(1).max(1);
        let token = self.next_token;
        self.requests.push(Request {
            token,
            path,
            reply,
            slot: Arc::new(CaptureSlot::new(token)),
            admitted: now,
            deadline: now + DEADLINE,
            ui_passes_at_admission: self.ui_passes,
            doc_generation,
            issued: None,
            reservation,
        });
        Ok(())
    }

    /// Record the geometry of the pass that just laid out the real UI. A change resets the
    /// settle counter, advances the epoch, and rebooks queued (not in-flight) captures at the
    /// new peak size so the aggregate remains accurate during a resize.
    pub fn observe_ui(&mut self, ctx: &Context) {
        // The screenshot callback returns the whole framebuffer, not the safe-area content
        // rect: on web/iOS `content_rect` excludes status-bar/notch insets and would make every
        // completion look like a resize.
        let size = ctx.viewport_rect().size();
        let ppp = ctx.pixels_per_point();
        let viewport = ctx.viewport_id();
        let changed = self.geometry.is_none_or(|old| old.size != size || old.pixels_per_point != ppp || old.viewport != viewport);
        if changed {
            let epoch = self.geometry.map_or(1, |g| g.epoch.wrapping_add(1));
            self.geometry = Some(Geometry { size, pixels_per_point: ppp, viewport, epoch });
            self.settled_passes = 1;
            self.rebook_unissued();
        } else {
            self.settled_passes = self.settled_passes.saturating_add(1);
        }
        self.ui_passes = self.ui_passes.saturating_add(1);
    }

    /// Update reservations of requests whose screenshot has not been requested yet.
    fn rebook_unissued(&mut self) {
        let geometry = self.geometry;
        let mut i = 0;
        while i < self.requests.len() {
            if self.requests[i].issued.is_some() {
                i += 1;
                continue;
            }
            let desired = match geometry {
                Some(geometry) => match MemoryPlan::for_geometry(geometry, self.requests[i].path.is_none()) {
                    Some(plan) if plan.color_image <= MAX_IMAGE_BYTES => plan.peak,
                    _ => {
                        self.fail(i, "the window has no usable surface size");
                        continue;
                    }
                },
                None => 0,
            };
            let old = self.requests[i].reservation;
            match budget_total(self.reserved, old, desired) {
                Some(total) => {
                    self.reserved = total;
                    self.requests[i].reservation = desired;
                    i += 1;
                }
                None => self.fail(i, "screenshot budget exhausted (4 requests / 256 MiB including encode); wait for one to finish"),
            }
        }
    }

    /// Issue due requests from the end of a real, settled UI pass.
    ///
    /// A request is failed here (with a single JSON reply) when the document changed, the
    /// window has no usable surface, or the bounded memory budget would be exceeded.
    pub fn issue(&mut self, ctx: &Context, doc_generation: Option<u64>, now: web_time::Instant) {
        let Some(geometry) = self.geometry else { return };
        if self.settled_passes < SETTLED_PASSES {
            return;
        }
        let mut failures: Vec<(usize, String)> = vec![];
        for (i, req) in self.requests.iter_mut().enumerate() {
            if req.issued.is_some()
                || now.saturating_duration_since(req.admitted) < SETTLE
                || self.ui_passes.saturating_sub(req.ui_passes_at_admission) < u64::from(SETTLED_PASSES)
                || self.settled_passes < SETTLED_PASSES
            {
                continue;
            }
            if req.doc_generation != doc_generation {
                failures.push((i, "the presentation changed while the screenshot was waiting".into()));
                continue;
            }
            let Some(plan) = MemoryPlan::for_geometry(geometry, req.path.is_none()) else {
                failures.push((i, "the window has no usable surface size".into()));
                continue;
            };
            if plan.color_image > MAX_IMAGE_BYTES {
                failures.push((i, "the screenshot would exceed 128 MiB; use ui.render".into()));
                continue;
            }
            let Some(new_total) = budget_total(self.reserved, req.reservation, plan.peak) else {
                failures.push((i, "screenshot budget exhausted (4 requests / 256 MiB including encode); wait for one to finish".into()));
                continue;
            };
            let weak: Weak<CaptureSlot> = Arc::downgrade(&req.slot);
            let callback = completion_callback(ctx.clone(), geometry.viewport, weak, req.token, CaptureBounds { max_image_bytes: plan.color_image });
            ctx.send_viewport_cmd_to(geometry.viewport, ViewportCommand::Screenshot(callback));
            self.reserved = new_total;
            req.reservation = plan.peak;
            req.issued = Some(Issued { geometry });
        }
        for (i, why) in failures.into_iter().rev() {
            self.fail(i, &why);
        }
    }

    /// Take completed results and expired/cancelled requests. A completed image keeps its
    /// reservation until [`Self::release`] is called after the app has encoded or written it.
    pub fn drain(&mut self, now: web_time::Instant, doc_generation: Option<u64>) -> Vec<Completed> {
        let mut done = vec![];
        let current_epoch = self.geometry.map(|g| g.epoch);
        let mut i = 0;
        while i < self.requests.len() {
            // A completion that arrived just before the deadline is still valid: check the slot
            // first and only fail on the deadline when nothing is ready. Otherwise the deadline
            // branch below would drop a frame that is already in hand.
            let result = if self.requests[i].issued.is_some() { self.requests[i].slot.take() } else { SlotResult::Pending };
            match result {
                SlotResult::Pending => {
                    if now >= self.requests[i].deadline {
                        self.fail(i, "no frame was presented (screen locked or window hidden); use ui.render");
                    } else {
                        i += 1;
                    }
                }
                SlotResult::Retry(why) => {
                    // The callback refused to retain an image outside the reservation. Retry it
                    // against the current geometry until the normal deadline, exactly like a
                    // size mismatch; an expired request fails with the same reason.
                    let Some(_issued) = self.requests[i].issued.take() else {
                        i += 1;
                        continue;
                    };
                    self.release_request(i);
                    if now >= self.requests[i].deadline {
                        self.fail(i, why);
                    } else {
                        self.requests[i].slot = Arc::new(CaptureSlot::new(self.requests[i].token));
                        self.requests[i].ui_passes_at_admission = self.ui_passes;
                        self.requests[i].admitted = now;
                        i += 1;
                    }
                }
                SlotResult::Cancelled => self.fail(i, "screenshot was cancelled"),
                SlotResult::Rejected(why) => self.fail(i, why),
                SlotResult::Ready(image) => {
                    let Some(issued) = self.requests[i].issued.take() else {
                        i += 1;
                        continue;
                    };
                    let same_geometry = current_epoch == Some(issued.geometry.epoch)
                        && self.geometry.is_some_and(|g| {
                            g.size == issued.geometry.size
                                && g.pixels_per_point == issued.geometry.pixels_per_point
                                && g.viewport == issued.geometry.viewport
                        });
                    let exact_size = issued.geometry.pixel_size() == Some(image.size);
                    if !same_geometry || !exact_size {
                        // The window changed (or the image came from an earlier size) while
                        // capturing: retry with fresh geometry and the same deadline. This is not
                        // a document change and writes no file.
                        self.release_request(i);
                        if now >= self.requests[i].deadline {
                            self.fail(i, "the window kept resizing while the screenshot was being captured");
                        } else {
                            self.requests[i].slot = Arc::new(CaptureSlot::new(self.requests[i].token));
                            self.requests[i].ui_passes_at_admission = self.ui_passes;
                            self.requests[i].admitted = now;
                            i += 1;
                        }
                    } else if self.requests[i].doc_generation != doc_generation {
                        self.fail(i, "the presentation changed while the screenshot was being captured");
                    } else {
                        let reservation = self.requests[i].reservation;
                        self.requests[i].reservation = 0;
                        let req = self.requests.remove(i);
                        done.push(Completed {
                            token: req.token,
                            path: req.path,
                            reply: req.reply,
                            image,
                            geometry: issued.geometry,
                            reserved: reservation,
                        });
                    }
                }
            }
        }
        done
    }

    /// Release the reservation for a completed screenshot after the app has encoded or written
    /// it and replied. The aggregate stays charged through the encode to keep its peak honest.
    pub fn release(&mut self, bytes: usize) {
        self.reserved = self.reserved.saturating_sub(bytes);
    }

    /// Drop one request's reservation but keep it queued (used by a retry).
    fn release_request(&mut self, i: usize) {
        let bytes = self.requests[i].reservation;
        self.requests[i].reservation = 0;
        self.release(bytes);
    }

    /// Remove a request at `i`, release its reservation and reply exactly once.
    fn fail(&mut self, i: usize, why: &str) {
        self.release_request(i);
        let req = self.requests.remove(i);
        req.slot.cancel();
        let _ = req.reply.send(json!({"ok": false, "error": why}));
    }

    /// Test-only: make the settle deadline already pass without sleeping.
    #[cfg(test)]
    pub fn force_settle(&mut self) {
        let now = web_time::Instant::now();
        for req in &mut self.requests {
            req.admitted = now - SETTLE - Duration::from_millis(1);
            req.ui_passes_at_admission = 0;
        }
    }

    /// Ask the issuing viewport for the bounded work a pending screenshot still needs.
    ///
    /// Queued/settling requests need real hidden paints (`RequestPaintWhileHidden`) until they
    /// are issued. Once issued, the screenshot command itself requests the capture paint and the
    /// completion callback wakes the viewport; only a deadline timer is scheduled here, so a
    /// missed hidden frame still fails through the normal request deadline instead of a 16 ms
    /// repaint loop.
    pub fn request_paint_if_needed(&self, ctx: &Context, now: web_time::Instant) {
        if self.requests.is_empty() {
            return;
        }
        let viewport = self.geometry.map_or_else(|| ctx.viewport_id(), |g| g.viewport);
        if self.requests.iter().any(|req| req.issued.is_none()) {
            ctx.send_viewport_cmd_to(viewport, ViewportCommand::RequestPaintWhileHidden);
        }
        if let Some(deadline) = self.requests.iter().map(|req| req.deadline).min() {
            let wait = deadline.saturating_duration_since(now).max(Duration::from_millis(1));
            ctx.request_repaint_after_for(wait, viewport);
        }
    }
}

impl Drop for Capture {
    /// Cancel every outstanding completion when the capture registry is dropped.
    fn drop(&mut self) {
        for req in &self.requests {
            req.slot.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// Test helper: build a solid red `ColorImage` of the requested size.
    fn image(w: usize, h: usize) -> Arc<ColorImage> {
        Arc::new(ColorImage::new([w, h], vec![egui::Color32::RED; w * h]))
    }

    /// Test helper: admit one `ui.screenshot` request and return its token and reply receiver.
    fn setup(cap: &mut Capture, now: web_time::Instant, path: Option<&str>) -> (u64, std::sync::mpsc::Receiver<ControlResponse>) {
        let (req, rx) = crate::ControlRequest::new("ui.screenshot", json!({}));
        cap.admit(path.map(str::to_string), req.reply.clone(), Some(7), now).unwrap();
        (cap.requests.last().unwrap().token, rx)
    }

    /// Filling the registry to `MAX_REQUESTS` reports full; cancelling after a late completion
    /// leaves no image.
    #[test]
    fn admission_is_bounded_and_late_cancel_keeps_no_image() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        for _ in 0..MAX_REQUESTS {
            setup(&mut cap, now, None);
        }
        assert!(cap.is_full(), "four admitted requests fill the registry");
        let slot = Arc::clone(&cap.requests[0].slot);
        slot.complete(cap.requests[0].token, CaptureBounds { max_image_bytes: 16 }, image(2, 2));
        slot.cancel();
        assert!(matches!(slot.take(), SlotResult::Cancelled), "a completion after cancel retains nothing");
    }

    /// A completion carrying a different request token cannot complete the tracked slot.
    #[test]
    fn one_shot_token_correlation_ignores_a_foreign_completion() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        let (token, _rx) = setup(&mut cap, now, Some("first.png"));
        let slot = Arc::clone(&cap.requests[0].slot);
        slot.complete(token + 1, CaptureBounds { max_image_bytes: 16 }, image(2, 2));
        assert!(matches!(slot.take(), SlotResult::Pending), "another token cannot complete this slot");
        slot.complete(token, CaptureBounds { max_image_bytes: 16 }, image(2, 2));
        assert!(matches!(slot.take(), SlotResult::Ready(_)));
    }

    /// Empty images are rejected; nonzero images are retained with their size.
    #[test]
    fn empty_images_are_rejected_and_other_sizes_are_kept_for_the_drain() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        let (token, _rx) = setup(&mut cap, now, None);
        let slot = Arc::clone(&cap.requests[0].slot);
        slot.complete(token, CaptureBounds { max_image_bytes: 36 }, image(0, 2));
        assert!(matches!(slot.take(), SlotResult::Rejected(_)), "zero width");
        slot.complete(token, CaptureBounds { max_image_bytes: 36 }, image(3, 3));
        let taken = slot.take();
        assert!(matches!(&taken, SlotResult::Ready(image) if image.size == [3, 3]), "the actual size reaches drain");
    }

    /// A smaller image from a resize that has not been observed yet must not be saved just
    /// because it fits the byte bound; drain compares the exact pixel size and retries.
    #[test]
    fn an_image_of_a_different_size_retries_instead_of_saving() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        let geometry = Geometry { size: Vec2::new(4.0, 4.0), pixels_per_point: 1.0, viewport: ViewportId::ROOT, epoch: 1 };
        cap.geometry = Some(geometry);
        cap.settled_passes = SETTLED_PASSES;
        cap.ui_passes = 10;
        let (_token, rx) = setup(&mut cap, now, Some("resized.png"));
        cap.requests[0].issued = Some(Issued { geometry });
        cap.requests[0].reservation = 64;
        cap.reserved = 64;
        let slot = Arc::clone(&cap.requests[0].slot);
        slot.complete(cap.requests[0].token, CaptureBounds { max_image_bytes: 64 }, image(2, 2));
        let done = cap.drain(now, Some(7));
        assert!(done.is_empty(), "a stale-size image is not written");
        assert_eq!(cap.pending(), 1, "the request is retried");
        assert_eq!(cap.reserved_bytes(), 0, "the old reservation is released");
        assert!(rx.try_recv().is_err(), "no premature error reply");
    }

    /// A completion that is ready when the deadline has just passed is still a valid frame.
    #[test]
    fn a_ready_image_wins_over_the_deadline() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        let geometry = Geometry { size: Vec2::new(2.0, 2.0), pixels_per_point: 1.0, viewport: ViewportId::ROOT, epoch: 1 };
        cap.geometry = Some(geometry);
        cap.settled_passes = SETTLED_PASSES;
        let (_token, _rx) = setup(&mut cap, now, Some("late.png"));
        cap.requests[0].issued = Some(Issued { geometry });
        cap.requests[0].reservation = 16;
        cap.reserved = 16;
        let slot = Arc::clone(&cap.requests[0].slot);
        slot.complete(cap.requests[0].token, CaptureBounds { max_image_bytes: 64 }, image(2, 2));
        let done = cap.drain(now + DEADLINE + Duration::from_millis(1), Some(7));
        assert_eq!(done.len(), 1, "the completed frame is delivered");
        assert_eq!(done[0].path.as_deref(), Some("late.png"));
        assert_eq!(done[0].reserved, 16);
        assert_eq!(cap.pending(), 0);
        assert_eq!(cap.reserved_bytes(), 16, "the encode reservation is still held");
        cap.release(done[0].reserved);
        assert_eq!(cap.reserved_bytes(), 0);
    }

    /// Fractional DPI rounds the framebuffer size once, on the reserved geometry only.
    #[test]
    fn fractional_dpi_rounds_to_the_reserved_pixel_size() {
        let g = Geometry { size: Vec2::new(101.0, 51.0), pixels_per_point: 1.5, viewport: ViewportId::ROOT, epoch: 1 };
        assert_eq!(g.pixel_size(), Some([152, 77]));
        let accepted = |image: [usize; 2]| g.pixel_size() == Some(image);
        assert!(accepted([152, 77]));
        assert!(!accepted([151, 76]), "a different framebuffer size is a resize, not a fit");
    }

    /// Pixel-memory budgets use checked geometry and reject zero, NaN, or overflowing sizes.
    #[test]
    fn budgets_use_checked_geometry_and_reject_zero_or_nan_sizes() {
        let g = Geometry { size: Vec2::new(100.0, 50.0), pixels_per_point: 2.0, viewport: ViewportId::ROOT, epoch: 1 };
        assert_eq!(g.pixel_size(), Some([200, 100]));
        assert_eq!(g.image_bytes(), Some(200 * 100 * 4));
        assert_eq!(Geometry { size: Vec2::new(0.0, 50.0), ..g }.image_bytes(), None);
        assert_eq!(Geometry { size: Vec2::new(f32::NAN, 50.0), ..g }.image_bytes(), None);
        assert_eq!(Geometry { size: Vec2::new(f32::MAX, f32::MAX), pixels_per_point: f32::MAX, ..g }.image_bytes(), None);
    }

    /// A changed geometry epoch retries instead of saving a stale image.
    #[test]
    fn a_geometry_epoch_change_retries_instead_of_saving() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        // Fake a settled UI pass and issue the request against it.
        let geometry = Geometry { size: Vec2::new(2.0, 2.0), pixels_per_point: 1.0, viewport: ViewportId::ROOT, epoch: 4 };
        cap.geometry = Some(geometry);
        cap.settled_passes = SETTLED_PASSES;
        cap.ui_passes = 10;
        let (_token, _rx) = setup(&mut cap, now, Some("retry.png"));
        cap.requests[0].issued = Some(Issued { geometry });
        cap.requests[0].reservation = 16;
        cap.reserved = 16;
        // The geometry changed before the completion arrived.
        cap.geometry = Some(Geometry { epoch: 5, ..geometry });
        let slot = Arc::clone(&cap.requests[0].slot);
        slot.complete(cap.requests[0].token, CaptureBounds { max_image_bytes: 64 }, image(2, 2));
        let done = cap.drain(now, Some(7));
        assert!(done.is_empty(), "stale geometry is not written");
        assert_eq!(cap.pending(), 1, "the request is retried");
        assert_eq!(cap.reserved_bytes(), 0, "the old reservation is released");
        assert!(cap.requests[0].issued.is_none());
    }

    /// A document change after admission fails the request before any file is written.
    #[test]
    fn a_document_change_fails_before_writing() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        let geometry = Geometry { size: Vec2::new(2.0, 2.0), pixels_per_point: 1.0, viewport: ViewportId::ROOT, epoch: 1 };
        cap.geometry = Some(geometry);
        cap.settled_passes = SETTLED_PASSES;
        let (_token, rx) = setup(&mut cap, now, Some("doc.png"));
        cap.requests[0].issued = Some(Issued { geometry });
        cap.requests[0].reservation = 16;
        cap.reserved = 16;
        let slot = Arc::clone(&cap.requests[0].slot);
        slot.complete(cap.requests[0].token, CaptureBounds { max_image_bytes: 64 }, image(2, 2));
        let done = cap.drain(now, Some(8));
        assert!(done.is_empty());
        assert_eq!(cap.pending(), 0);
        let reply = rx.recv().unwrap();
        assert_eq!(reply["ok"], false);
    }

    /// A timed-out capture replies with failure and releases its reservation.
    #[test]
    fn timeout_replies_once_and_releases_the_reservation() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        let geometry = Geometry { size: Vec2::new(2.0, 2.0), pixels_per_point: 1.0, viewport: ViewportId::ROOT, epoch: 1 };
        cap.geometry = Some(geometry);
        cap.settled_passes = SETTLED_PASSES;
        let (_token, rx) = setup(&mut cap, now, None);
        cap.requests[0].issued = Some(Issued { geometry });
        cap.requests[0].reservation = 16;
        cap.reserved = 16;
        let done = cap.drain(now + DEADLINE, Some(7));
        assert!(done.is_empty());
        assert_eq!(cap.pending(), 0);
        assert_eq!(cap.reserved_bytes(), 0);
        assert_eq!(rx.recv().unwrap()["ok"], false);
    }

    /// An out-of-order completion is written to the path belonging to its own token.
    #[test]
    fn distinct_tokens_keep_distinct_paths_even_when_completed_out_of_order() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        let geometry = Geometry { size: Vec2::new(2.0, 2.0), pixels_per_point: 1.0, viewport: ViewportId::ROOT, epoch: 1 };
        cap.geometry = Some(geometry);
        cap.settled_passes = SETTLED_PASSES;
        setup(&mut cap, now, Some("a.png"));
        setup(&mut cap, now, Some("b.png"));
        let plan = MemoryPlan::for_image(2, 2, false).expect("path capture plan");
        for req in &mut cap.requests {
            req.issued = Some(Issued { geometry });
            req.reservation = plan.peak;
        }
        cap.reserved = 2 * plan.peak;
        let bounds = CaptureBounds { max_image_bytes: plan.color_image };
        let second = Arc::clone(&cap.requests[1].slot);
        second.complete(cap.requests[1].token, bounds, image(2, 2));
        let done = cap.drain(now, Some(7));
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].path.as_deref(), Some("b.png"));
        assert_eq!(cap.pending(), 1, "the unfinished first request continues");
        assert_eq!(cap.reserved_bytes(), 2 * plan.peak, "completed images stay reserved through encoding");
        cap.release(done[0].reserved);
        assert_eq!(cap.reserved_bytes(), plan.peak, "only the saved second capture is released");
        let first = Arc::clone(&cap.requests[0].slot);
        first.complete(cap.requests[0].token, bounds, image(2, 2));
        let done = cap.drain(now, Some(7));
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].path.as_deref(), Some("a.png"));
        cap.release(done[0].reserved);
        assert_eq!(cap.reserved_bytes(), 0);
    }

    /// The reservation counts the callback image, the RGBA copy, the encoder's straight-alpha
    /// copy, the PNG bound (raw + row filters) and (only for an inline reply) its base64 text.
    #[test]
    fn memory_plan_counts_image_rgba_png_and_inline_base64() {
        let path = MemoryPlan::for_image(2, 2, false).expect("path plan");
        assert_eq!(path.color_image, 16);
        assert_eq!(path.rgba, 16);
        assert_eq!(path.encoder_copy, 16);
        assert_eq!(path.png, 16 + 2 + 1024, "raw, one filter byte per row and encoder slack");
        assert_eq!(path.inline, 0, "a path reply does not hold base64");
        assert_eq!(path.peak, 3 * 16 + path.png);

        let inline = MemoryPlan::for_image(2, 2, true).expect("inline plan");
        assert_eq!(inline.png, path.png);
        assert_eq!(inline.inline, path.png.div_ceil(3) * 4);
        assert_eq!(inline.peak, path.peak + inline.inline);

        assert_eq!(MemoryPlan::for_image(0, 2, false), None, "zero width");
        assert_eq!(MemoryPlan::for_image(usize::MAX, 2, false), None, "overflowing width");
        assert_eq!(MemoryPlan::for_image(usize::MAX / 4, 4, false), None, "pixel-byte overflow");
    }

    /// Admission reserves the peak before the first UI pass can issue the screenshot, and a
    /// second request that would cross the aggregate is refused without changing the total.
    #[test]
    fn admission_reserves_the_peak_and_refuses_the_one_that_would_exceed_it() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        let geometry = Geometry { size: Vec2::new(100.0, 100.0), pixels_per_point: 1.0, viewport: ViewportId::ROOT, epoch: 1 };
        cap.geometry = Some(geometry);
        let plan = MemoryPlan::for_geometry(geometry, true).expect("plan");
        cap.reserved = MAX_TOTAL_BYTES - plan.peak;
        setup(&mut cap, now, None);
        assert_eq!(cap.reserved_bytes(), MAX_TOTAL_BYTES, "the exact limit is admitted");
        let (req, rx) = crate::ControlRequest::new("ui.screenshot", json!({}));
        assert!(cap.admit(None, req.reply, Some(7), now).is_err(), "one more peak does not fit");
        assert_eq!(cap.reserved_bytes(), MAX_TOTAL_BYTES, "a refused admission changes nothing");
        assert!(rx.try_recv().is_err(), "the caller sends the refusal reply");
    }

    /// A resize rebooks a queued (not in-flight) request at the new peak; the old reservation
    /// is released first, so replacement never double-counts it.
    #[test]
    fn rebooking_at_a_new_geometry_replaces_the_old_reservation() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        let geometry = Geometry { size: Vec2::new(100.0, 100.0), pixels_per_point: 1.0, viewport: ViewportId::ROOT, epoch: 1 };
        cap.geometry = Some(geometry);
        setup(&mut cap, now, Some("resize.png"));
        let small = MemoryPlan::for_geometry(geometry, false).expect("small plan");
        assert_eq!(cap.reserved_bytes(), small.peak);
        let bigger = Geometry { size: Vec2::new(200.0, 200.0), ..geometry };
        cap.geometry = Some(bigger);
        cap.rebook_unissued();
        let large = MemoryPlan::for_geometry(bigger, false).expect("large plan");
        assert_eq!(cap.reserved_bytes(), large.peak, "the new peak replaces the old one");
        assert_eq!(cap.requests[0].reservation, large.peak);
    }

    /// The aggregate limit is inclusive: exactly 256 MiB is admitted; one byte more is refused.
    /// Replacing a request's own reservation does not count it twice.
    #[test]
    fn aggregate_budget_accepts_the_limit_and_refuses_one_byte_over() {
        assert_eq!(budget_total(MAX_TOTAL_BYTES - 16, 0, 16), Some(MAX_TOTAL_BYTES), "equality is allowed");
        assert_eq!(budget_total(MAX_TOTAL_BYTES - 15, 0, 16), None, "one byte over is refused");
        assert_eq!(budget_total(MAX_TOTAL_BYTES, 16, 16), Some(MAX_TOTAL_BYTES), "a replaced reservation is released first");
    }

    /// The callback checks the image against the reservation for the issued geometry before it
    /// stores anything. An oversized frame is not retained and drain retries it until the
    /// normal deadline, where it fails once.
    #[test]
    fn a_callback_image_over_the_reserved_geometry_is_not_retained() {
        let now = web_time::Instant::now();
        let mut cap = Capture::default();
        let geometry = Geometry { size: Vec2::new(2.0, 2.0), pixels_per_point: 1.0, viewport: ViewportId::ROOT, epoch: 1 };
        cap.geometry = Some(geometry);
        cap.settled_passes = SETTLED_PASSES;
        cap.ui_passes = 10;
        let (_token, rx) = setup(&mut cap, now, Some("huge.png"));
        cap.requests[0].issued = Some(Issued { geometry });
        cap.requests[0].reservation = 16;
        cap.reserved = 16;
        let slot = Arc::clone(&cap.requests[0].slot);
        let oversized = image(3, 3);
        let weak = Arc::downgrade(&oversized);
        assert!(slot.complete(cap.requests[0].token, CaptureBounds { max_image_bytes: 16 }, oversized.clone()));
        drop(oversized);
        assert!(weak.upgrade().is_none(), "the rejected pixels are released before the next pass");
        assert!(matches!(slot.take(), SlotResult::Retry(_)), "the oversized frame is retryable, not stored");

        // Put the retry result back into the request and retry once without writing a file.
        let slot = Arc::clone(&cap.requests[0].slot);
        slot.complete(cap.requests[0].token, CaptureBounds { max_image_bytes: 16 }, image(3, 3));
        let done = cap.drain(now, Some(7));
        assert!(done.is_empty(), "an oversized image is never written");
        assert_eq!(cap.pending(), 1, "the request retries against fresh geometry");
        assert_eq!(cap.reserved_bytes(), 0, "the old reservation is released");
        assert!(rx.try_recv().is_err(), "no premature error reply");

        let done = cap.drain(now + DEADLINE, Some(7));
        assert!(done.is_empty());
        assert_eq!(cap.pending(), 0);
        assert_eq!(cap.reserved_bytes(), 0);
        assert_eq!(rx.recv().unwrap()["ok"], false, "the normal deadline still fails the request once");
    }

    /// A completion wakes the viewport that issued it, once, and only after the slot lock has
    /// been released. A second clone of the one-shot callback cannot wake it again.
    #[test]
    fn a_completion_wakes_only_its_own_viewport_once() {
        let ctx = Context::default();
        let viewport = ViewportId::from_hash_of("capture-wake");
        let slot = Arc::new(CaptureSlot::new(1));
        let weak = Arc::downgrade(&slot);
        let (tx, rx) = std::sync::mpsc::channel();
        // A new Context starts with pending root paints. Establish the idle state first.
        for _ in 0..3 {
            ctx.run_ui(Default::default(), |_| {}).drop_without_applying_deltas();
        }
        assert!(!ctx.has_requested_repaint_for(&ViewportId::ROOT));
        ctx.set_repaint_observer(move |info| {
            let _ = tx.send(info.viewport_id);
        });
        let callback = completion_callback(ctx.clone(), viewport, weak, 1, CaptureBounds { max_image_bytes: 16 });
        let second = callback.clone();
        callback.complete(image(2, 2));
        second.complete(image(2, 2));
        assert_eq!(rx.try_recv().ok(), Some(viewport), "the issuing viewport is woken");
        assert!(rx.try_recv().is_err(), "a one-shot completion wakes once");
        assert!(ctx.has_requested_repaint_for(&viewport));
        assert!(!ctx.has_requested_repaint_for(&ViewportId::ROOT), "another viewport is not woken");
    }

    /// A queued request needs a bounded hidden paint to reach issue. Once issued, the screenshot
    /// command/callback own the wake and only the deadline timer remains, so a hidden window does
    /// not repaint every 16 ms.
    #[test]
    fn hidden_paints_are_bounded_to_queued_requests_and_the_deadline() {
        let now = web_time::Instant::now();
        let ctx = Context::default();
        let mut cap = Capture::default();
        setup(&mut cap, now, None);

        let output = ctx.run_ui(Default::default(), |ui| cap.request_paint_if_needed(ui.ctx(), now));
        let queued = output.viewport_output.get(&ViewportId::ROOT).expect("root output");
        assert!(
            queued.commands.iter().any(|c| matches!(c, ViewportCommand::RequestPaintWhileHidden)),
            "a queued hidden request asks for a real bounded paint: {:?}",
            queued.commands
        );
        output.drop_without_applying_deltas();

        let geometry = Geometry { size: Vec2::new(2.0, 2.0), pixels_per_point: 1.0, viewport: ViewportId::ROOT, epoch: 1 };
        cap.geometry = Some(geometry);
        cap.requests[0].issued = Some(Issued { geometry });
        cap.requests[0].reservation = 16;
        // Drain egui's initial/previous repaint passes before inspecting the future deadline.
        for _ in 0..3 {
            let output = ctx.run_ui(Default::default(), |ui| cap.request_paint_if_needed(ui.ctx(), now));
            assert!(!output.viewport_output[&ViewportId::ROOT].commands.iter().any(|c| matches!(c, ViewportCommand::RequestPaintWhileHidden)));
            output.drop_without_applying_deltas();
        }
        let output = ctx.run_ui(Default::default(), |ui| cap.request_paint_if_needed(ui.ctx(), now));
        let issued = output.viewport_output.get(&ViewportId::ROOT).expect("root output");
        assert!(
            !issued.commands.iter().any(|c| matches!(c, ViewportCommand::RequestPaintWhileHidden)),
            "an issued screenshot does not force hidden paints: {:?}",
            issued.commands
        );
        assert!(
            issued.repaint_delay > Duration::from_secs(1) && issued.repaint_delay <= DEADLINE,
            "only the normal deadline wake remains: {:?}",
            issued.repaint_delay
        );
        output.drop_without_applying_deltas();
    }
}
