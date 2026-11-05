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
