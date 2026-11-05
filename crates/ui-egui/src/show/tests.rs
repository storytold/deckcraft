#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

/// #1005: shapes stacked above a playing video must be drawn over its frame; the video, what
/// is below it and other videos stay out of that overlay.
#[test]
fn shapes_above_a_playing_video_are_the_overlay() {
    let mut session = deckcraft_engine::Session::with_new();
    for _ in 0..3 {
        session.execute("shape.insert", &json!({"preset": "rect", "rect": [0.0, 0.0, 100.0, 100.0]})).unwrap();
    }
    let mut doc = (*session.active().unwrap().doc).clone();
    // Keep only the three inserted rects (a new slide may start with placeholders).
    let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
    let keep = slide.shapes.len() - 3;
    slide.shapes.drain(..keep);
    let ids: Vec<ShapeId> = slide.shapes.iter().map(|s| s.id).collect();
    slide.shapes[1].kind = deckcraft_model::ShapeKind::Media(deckcraft_model::MediaClip { video: true, ..Default::default() });
    // Video in the middle: the shape above it is drawn over it.
    assert_eq!(below_playing_video(&doc, 0, &[ids[1]]), vec![ids[0], ids[1]]);
    // Video on top: nothing above it, the frame is painted last as before.
    std::sync::Arc::make_mut(&mut doc.slides[0]).shapes.swap(1, 2);
    assert!(below_playing_video(&doc, 0, &[ids[1]]).is_empty());
    // No video playing.
    assert!(below_playing_video(&doc, 0, &[]).is_empty());
}

/// #968: over a shape a click acts on (a clip with its play trigger, a link) the pointing hand
/// never gave way to the idle hide, so a still pointer stayed on the playing video.
#[test]
fn a_still_cursor_hides_over_a_clickable_shape() {
    let mut session = deckcraft_engine::Session::with_new();
    let size = session.active().unwrap().doc.slide_size;
    let r = session.execute("shape.insert", &json!({"preset": "rect", "rect": [0.0, 0.0, size.width, size.height]})).unwrap();
    session.execute("insert.hyperlink", &json!({"url": "https://example.org", "ids": [r["id"]]})).unwrap();
    let app = SlideApp::new(session, crate::Services::default());
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).with_step_dt(0.25).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    harness.state_mut().start_show(0, false);
    harness.run_steps(2);
    harness.hover_at(pos2(500.0, 350.0));
    harness.run_steps(2);
    assert_eq!(harness.output().platform_output.cursor_icon, egui::CursorIcon::PointingHand);
    harness.run_steps(12);
    assert_eq!(harness.output().platform_output.cursor_icon, egui::CursorIcon::None, "still for 3 s: hidden");
}

/// #1445: Keep mouse cursor visible leaves a still pointer showing over a link, so a branching
/// deck's choices stay targetable; the setting outlives the show and the link still follows.
#[test]
fn keep_cursor_visible_leaves_a_still_cursor_over_a_link() {
    let mut session = deckcraft_engine::Session::with_new();
    let size = session.active().unwrap().doc.slide_size;
    session.execute("slide.new", &json!({})).unwrap();
    session.execute("slide.go", &json!({"index": 0})).unwrap();
    let r = session.execute("shape.insert", &json!({"preset": "rect", "rect": [0.0, 0.0, size.width, size.height]})).unwrap();
    session.execute("insert.hyperlink", &json!({"slide": 1, "ids": [r["id"]]})).unwrap();
    let mut app = SlideApp::new(session, crate::Services::default());
    app.ui.presenter_view = false;
    app.run("show.keepCursor", json!({"on": true})).unwrap();
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).with_step_dt(0.25).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    harness.state_mut().start_show(0, false);
    harness.run_steps(2);
    harness.hover_at(pos2(500.0, 350.0));
    harness.run_steps(14);
    assert_eq!(harness.output().platform_output.cursor_icon, egui::CursorIcon::PointingHand, "still for 3 s: kept");
    click_at(&mut harness, pos2(500.0, 350.0));
    harness.run_steps(2);
    assert_eq!(harness.state().show.as_ref().map(|s| s.state.slide), Some(1), "the link still follows");
    assert!(harness.state().ui.show_keep_cursor, "the setting is kept for the next show");
}

/// #1445: Set Up Slide Show turns Keep mouse cursor visible on and off.
#[test]
fn set_up_slide_show_keeps_the_cursor_visible() {
    use egui_kittest::kittest::Queryable;

    let app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    for on in [true, false] {
        harness.state_mut().run("app.dialog", json!({"id": "setupShow"})).unwrap();
        harness.run_steps(6);
        harness.get_by_label("Keep mouse cursor visible").click();
        harness.run_steps(3);
        harness.get_by_label("OK").click();
        harness.run_steps(2);
        assert!(harness.state().dialog.is_none());
        assert_eq!(harness.state().ui.show_keep_cursor, on);
    }
}

/// #1445: the idle hide follows the pointer's last move, Reading View and Keep mouse cursor visible.
#[test]
fn the_idle_cursor_hides_only_when_asked() {
    let session = deckcraft_engine::Session::with_new();
    let doc = session.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, false);
    show.last_move = 10.0;
    assert!(!show.hides_idle_cursor(12.0), "moved 2 s ago");
    assert!(show.hides_idle_cursor(13.0), "still for 3 s");
    show.keep_cursor = true;
    assert!(!show.hides_idle_cursor(13.0), "kept visible");
    show.keep_cursor = false;
    show.reading = true;
    assert!(!show.hides_idle_cursor(13.0), "Reading View never hides it");
}

/// #1130: the Presenter View's All slides button opens the grid in that window, not on the audience screen.
#[test]
fn presenter_all_slides_does_not_cover_the_audience_screen() {
    use egui_kittest::kittest::Queryable;

    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    let doc = session.active().unwrap().doc.clone();
    let show = Show::new(&doc, 0, false, true);
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1100.0, 700.0)).build_ui_state(
        |ui, (app, doc, show, keep): &mut (SlideApp, std::sync::Arc<Presentation>, Show, bool)| {
            let now = ui.input(|i| i.time);
            presenter_window(app, ui.ctx(), show, doc, now, keep);
        },
        (SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default()), doc, show, true),
    );
    harness.run_steps(2);
    harness.get_by_label("All slides").click();
    harness.run_steps(2);
    assert!(harness.state().2.presenter_grid, "the grid opens in the Presenter View");
    assert!(!harness.state().2.grid, "the audience screen keeps showing the slide");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(!harness.state().2.presenter_grid, "Esc closes the Presenter View grid");
    assert!(harness.state().3, "and does not end the show");
    harness.key_press(egui::Key::G);
    harness.run_steps(2);
    assert!(harness.state().2.presenter_grid, "G in Presenter View opens its own grid");
    assert!(!harness.state().2.grid, "G in Presenter View leaves the audience screen alone");
    harness.key_press(egui::Key::G);
    harness.run_steps(2);
    assert!(!harness.state().2.presenter_grid, "G again closes it");
}

/// A silent mono 8 kHz WAV, so the clip has a real duration to seek in.
fn silent_wav(secs: u32) -> Vec<u8> {
    let rate = 8_000u32;
    let data = vec![0u8; (rate * secs * 2) as usize];
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    b
}

/// A running show on a slide with one 10 s clip. Returns the harness, the clip, a point on the
/// bar's seek track, and what `pointer_on_media_bar` said on the last frame.
fn show_with_clip() -> (egui_kittest::Harness<'static, SlideApp>, ShapeId, Pos2, std::rc::Rc<std::cell::Cell<bool>>) {
    let size = vec2(1000.0, 700.0);
    let mut session = deckcraft_engine::Session::with_new();
    let id = ShapeId(4000);
    session
        .edit(|doc, _| {
            let media = doc.add_media("clip.wav", "audio/wav", silent_wav(10)).unwrap();
            let (w, h) = (doc.slide_size.width, doc.slide_size.height);
            let clip = deckcraft_model::MediaClip { media, video: true, volume: 1.0, duration_ms: 10_000, ..Default::default() };
            let xfrm = deckcraft_geom::Xfrm::new(w * 0.2, h * 0.2, w * 0.6, h * 0.6);
            let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
            slide.shapes.push(deckcraft_model::Shape { id, xfrm: Some(xfrm), kind: deckcraft_model::ShapeKind::Media(clip), ..Default::default() });
            Ok(())
        })
        .unwrap();
    let on_bar = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = on_bar.clone();
    let slide = std::rc::Rc::new(std::cell::Cell::new(Rect::NOTHING));
    let slide_seen = slide.clone();
    // Presenter view opens an embedded window over the whole screen here, which takes the clicks.
    let mut app = SlideApp::new(session, crate::Services::default());
    app.ui.presenter_view = false;
    let mut harness = egui_kittest::Harness::builder().with_size(size).with_step_dt(0.25).build_ui_state(
        move |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            if let (Some(show), Some(d)) = (app.show.as_ref(), app.session.active()) {
                let now = ui.input(|i| i.time);
                seen.set(pointer_on_media_bar(ui.ctx(), ui, &d.doc, show, now));
                slide_seen.set(shown_slide_rect(ui, show, &d.doc));
            }
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    harness.state_mut().start_show(0, false);
    harness.run_steps(2);
    let doc = harness.state().session.active().unwrap().doc.clone();
    let bar = slide_media_bars(slide.get(), &doc, 0)[0].3;
    let track = pos2(bar.min.x + bar.width() * 0.8, bar.center().y);
    (harness, id, track, on_bar)
}

/// Press and release the primary button at `p`.
fn click_at(harness: &mut egui_kittest::Harness<'static, SlideApp>, p: Pos2) {
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        harness.step();
    }
}

/// #967: hovering a clip shows its bar and a click on the bar seeks. After 3 s without moving
/// the bar is gone, and a click where it was acts on the clip itself (play), not on the bar.
#[test]
fn media_controls_hide_when_pointer_is_idle() {
    // Awake: the bar is there and a click on its timeline seeks, leaving the clip paused.
    let (mut harness, id, track, on_bar) = show_with_clip();
    harness.hover_at(track);
    harness.run_steps(2);
    assert!(harness.state().show.as_ref().unwrap().pointer_awake(harness.ctx.input(|i| i.time)));
    assert!(on_bar.get(), "the bar is under a pointer that just moved");
    click_at(&mut harness, track);
    harness.run_steps(2);
    let status = harness.state().media.status(id).expect("the click on the bar cued the clip");
    assert!(!harness.state().media.is_playing(id), "a bar click seeks, it does not play");
    assert!(status.position > 1.0, "seeked along the timeline, at {}", status.position);

    // Idle: same spot, no movement for 3 s.
    let (mut harness, id, track, on_bar) = show_with_clip();
    harness.hover_at(track);
    harness.run_steps(2);
    assert!(on_bar.get());
    harness.run_steps(12);
    assert!(!harness.state().show.as_ref().unwrap().pointer_awake(harness.ctx.input(|i| i.time)));
    assert!(!on_bar.get(), "the bar is hidden once the pointer is idle");
    click_at(&mut harness, track);
    harness.run_steps(2);
    assert!(harness.state().media.is_playing(id), "the click reached the clip");
    let at = harness.state().media.status(id).map_or(0.0, |s| s.position);
    assert!(at < 5.0, "the click did not seek along the timeline, position {at}");
}

/// Two 1×1 frames (black, then white), 100 ms each.
fn two_frame_gif() -> Vec<u8> {
    let mut b = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff".to_vec();
    for pixel in [b"\x44\x01", b"\x4c\x01"] {
        b.extend_from_slice(b"\x21\xf9\x04\x00\x0a\x00\x00\x00");
        b.extend_from_slice(b"\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02");
        b.extend_from_slice(pixel);
        b.push(0);
    }
    b.push(0x3b);
    b
}

/// A running show on the first of two slides; slide 1 holds a large animated GIF. Returns the
/// harness, the GIF, and its play/pause button's centre.
fn show_with_gif() -> (egui_kittest::Harness<'static, SlideApp>, ShapeId, Pos2) {
    let mut session = deckcraft_engine::Session::with_new();
    // The GIF is on slide 1, the slide the editor has selected, with a slide after it.
    session.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    session.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    session.execute("slide.go", &json!({"index": 1})).unwrap();
    let id = ShapeId(4100);
    session
        .edit(|doc, _| {
            let media = doc.add_media("a.gif", "image/gif", two_frame_gif()).unwrap();
            let (w, h) = (doc.slide_size.width, doc.slide_size.height);
            let slide = std::sync::Arc::make_mut(&mut doc.slides[1]);
            slide.shapes.push(deckcraft_model::Shape {
                id,
                xfrm: Some(deckcraft_geom::Xfrm::new(w * 0.2, h * 0.2, w * 0.6, h * 0.6)),
                kind: deckcraft_model::ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { media, ..Default::default() } },
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    let slide = std::rc::Rc::new(std::cell::Cell::new(Rect::NOTHING));
    let slide_seen = slide.clone();
    let mut app = SlideApp::new(session, crate::Services::default());
    app.ui.presenter_view = false;
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).with_step_dt(0.25).build_ui_state(
        move |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            if let (Some(show), Some(d)) = (app.show.as_ref(), app.session.active()) {
                slide_seen.set(shown_slide_rect(ui, show, &d.doc));
            }
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    harness.state_mut().start_show(1, false);
    harness.run_steps(2);
    let doc = harness.state().session.active().unwrap().doc.clone();
    let srect = slide.get();
    let k = srect.width() / doc.slide_size.width.max(1.0) as f32;
    let rect = doc.slides[1].shapes.last().unwrap().xfrm.unwrap();
    let button = crate::media::gif_button_rect(on_screen(srect, k, &rect)).center();
    (harness, id, button)
}

/// The GIF's play/pause button is drawn only while the pointer is awake, so a click on its
/// spot after 2.5 s without movement advances the show instead of pausing the GIF.
#[test]
fn gif_button_click_needs_an_awake_pointer() {
    let (mut harness, id, button) = show_with_gif();
    harness.hover_at(button);
    harness.run_steps(2);
    click_at(&mut harness, button);
    harness.run_steps(2);
    assert!(harness.state().session.gif_paused.contains(&id), "an awake pointer's click pauses the GIF");
    assert_eq!(harness.state().show.as_ref().unwrap().state.slide, 1);

    let (mut harness, id, button) = show_with_gif();
    harness.hover_at(button);
    harness.run_steps(2);
    harness.run_steps(12);
    assert!(!harness.state().show.as_ref().unwrap().pointer_awake(harness.ctx.input(|i| i.time)));
    click_at(&mut harness, button);
    harness.run_steps(2);
    assert!(!harness.state().session.gif_paused.contains(&id), "the hidden button is not clickable");
    assert_eq!(harness.state().show.as_ref().unwrap().state.slide, 2, "the click advanced the show");
}

/// Send real egui key events to the presenter UI, without the audience window's handler.
#[test]
fn presenter_navigation_uses_its_own_keyboard_events() {
    use egui_kittest::kittest::Queryable;

    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    session.execute("slide.new", &json!({})).unwrap();
    let doc = session.active().unwrap().doc.clone();
    let show = Show::new(&doc, 0, false, true);
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1100.0, 700.0)).build_ui_state(
        |ui, (app, doc, show, keep): &mut (SlideApp, std::sync::Arc<Presentation>, Show, bool)| {
            let now = ui.input(|i| i.time);
            presenter_window(app, ui.ctx(), show, doc, now, keep);
        },
        (SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default()), doc, show, true),
    );
    harness.run_steps(2);
    for (key, slide) in [
        (egui::Key::ArrowRight, 1),
        (egui::Key::ArrowLeft, 0),
        (egui::Key::Space, 1),
        (egui::Key::PageDown, 2),
        (egui::Key::PageUp, 1),
        (egui::Key::Home, 0),
        (egui::Key::End, 2),
    ] {
        harness.key_press(key);
        harness.run_steps(2);
        assert_eq!(harness.state().2.state.slide, slide, "presenter key {key:?}");
    }
    harness.key_press(egui::Key::ArrowRight);
    harness.run_steps(2);
    assert!(harness.state().2.ended);
    harness.key_press(egui::Key::ArrowLeft);
    harness.run_steps(2);
    assert!(!harness.state().2.ended);
    assert_eq!(harness.state().2.state.slide, 2, "back from the end screen restores the last slide");
    harness.key_press(egui::Key::ArrowRight);
    harness.run_steps(2);
    assert!(harness.state().2.ended);
    harness.get_by_label("◀ Previous").click();
    harness.run_steps(2);
    assert!(!harness.state().2.ended, "the presenter Previous button also leaves the end screen");
    harness.key_press(egui::Key::Home);
    harness.run_steps(2);
    harness.get_by_label("Next ▶").click();
    harness.run_steps(2);
    assert_eq!(harness.state().2.state.slide, 1);
    harness.key_press(egui::Key::Space);
    harness.run_steps(2);
    assert_eq!(harness.state().2.state.slide, 2);
    assert!(!harness.state().2.ended, "Space must not also activate the focused Next button");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(!harness.state().3, "Escape from Presenter View ends the show");
}

#[test]
fn slideshow_navigation_advances_once_with_presenter_view() {
    use egui_kittest::kittest::Queryable;

    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    session.execute("slide.new", &json!({})).unwrap();
    let mut app = SlideApp::new(session, crate::Services::default());
    app.ui.presenter_view = true;
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    harness.state_mut().start_show(0, false);
    harness.run_steps(2);
    harness.key_press(egui::Key::ArrowRight);
    harness.run_steps(2);
    assert_eq!(harness.state().show.as_ref().unwrap().state.slide, 1, "one key advances one slide");
    harness.key_press(egui::Key::End);
    harness.run_steps(2);
    harness.key_press(egui::Key::ArrowRight);
    harness.run_steps(2);
    assert!(harness.state().show.as_ref().unwrap().ended);
    assert!(harness.query_by_label("End Show").is_some(), "the presenter remains available at the end screen");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(harness.state().show.is_none());
}

#[test]
fn the_prefetched_frame_is_the_frame_shown_on_entry() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({"title": "Second"})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let size = (96, 54);
    let p = start_prefetch(&doc, 1, size).expect("prefetch starts");
    assert_eq!(p.key, Key { slide: 1, step: 0, size, doc: std::sync::Arc::as_ptr(&doc) as usize, hidden: vec![], gif: 0, plays: vec![], ink: 0 });
    let got = p.rx.recv().expect("the frame arrives");
    let want = render_frame(&doc, 1, ShowState::timeline(&doc, 1), 0, 0.0, size, vec![], &[], 0, &[], &[], None, None);
    assert_eq!(got.size, want.size);
    assert_eq!(got.pixels, want.pixels);
}

/// A glowing full-slide picture in a layout made every animation frame re-render (and re-blur)
/// the backdrop: 2–3 frames a second. Frames now draw the slide's shapes over the kept
/// backdrop, and must look the same as a whole-slide render.
#[test]
fn an_animation_frame_over_the_kept_backdrop_matches_a_full_render() {
    let mut s = deckcraft_engine::Session::with_new();
    let r = s.execute("shape.insert", &json!({"preset": "star5", "rect": [100.0, 100.0, 300.0, 200.0]})).unwrap();
    s.execute("animation.set", &json!({"effect": "fade", "ids": [r["id"]]})).unwrap();
    let mut doc = (*s.active().unwrap().doc).clone();
    // A glowing master graphic under the animated star.
    let mut deco = doc.slides[0].shapes.last().unwrap().clone();
    deco.id = ShapeId(9000);
    deco.xfrm = deco.xfrm.map(|x| deckcraft_geom::Xfrm { x: x.x - 60.0, y: x.y + 40.0, ..x });
    deco.effects = Some(deckcraft_model::Effects {
        glow: Some(deckcraft_model::style::Glow {
            color: deckcraft_model::style::ColorRef::rgb(deckcraft_color::Rgba::rgb(0, 120, 255)),
            radius: 12.0,
        }),
        ..Default::default()
    });
    std::sync::Arc::make_mut(&mut doc.masters[0]).shapes.push(deco);
    let doc = std::sync::Arc::new(doc);
    let size = (480, 270);
    let key = BackKey { slide: 0, size, doc: std::sync::Arc::as_ptr(&doc) as usize };
    let mut show = Show::new(&doc, 0, false, false);
    show.prefetch_backdrop(&doc, key);
    let back = show.backdrop(&doc, key);
    let tl = ShowState::timeline(&doc, 0);
    assert!(!tl.is_empty());
    // Half-way through the fade, the star half see-through over the glow.
    let fast = render_frame(&doc, 0, tl.clone(), 0, 0.25, size, vec![], &[], 0, &[], &[], Some(&back), None);
    let full = render_frame(&doc, 0, tl, 0, 0.25, size, vec![], &[], 0, &[], &[], None, None);
    assert_eq!(fast.size, full.size);
    let worst =
        fast.pixels.iter().zip(&full.pixels).map(|(a, b)| a.to_array().iter().zip(b.to_array()).map(|(x, y)| x.abs_diff(y)).max().unwrap_or(0)).max();
    assert!(worst.is_some_and(|d| d <= 2), "frames differ by up to {worst:?}");
    // The backdrop is kept, not rendered again.
    assert!(std::sync::Arc::ptr_eq(&back, &show.backdrop(&doc, key)));
    // Another document under the same key (an edit whose copy reused the old address) gets
    // its own backdrop.
    let edited = std::sync::Arc::new((*doc).clone());
    assert!(!std::sync::Arc::ptr_eq(&back, &show.backdrop(&edited, key)));
}

#[test]
fn a_transition_keeps_the_old_slide_in_its_own_texture() {
    // #33: the old slide's handle was cloned, so the new slide was rendered into the same
    // texture and every transition blended the new slide with itself.
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("transition.set", &json!({"kind": "fade", "index": 1})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let ctx = egui::Context::default();
    let red = crate::textures::upload(&ctx, "show", egui::ColorImage::filled([1, 1], Color32::RED));
    let mut show = Show::new(&doc, 0, false, false);
    show.tex = Some((Key { slide: 0, step: 0, size: (1, 1), doc: 0, hidden: vec![], gif: 0, plays: vec![], ink: 0 }, red.clone()));
    assert!(show.apply(ShowAction::Slide(1), &doc, 0.0));
    let tr = show.trans.as_ref().expect("the fade plays");
    assert_eq!(tr.old.as_ref().map(TextureHandle::id), Some(red.id()));
    assert_eq!(tr.from, Some(0));
    assert!(show.tex.is_none(), "the new slide gets a texture of its own instead of overwriting the old one");
}

#[test]
fn going_back_plays_the_left_slides_transition_reversed() {
    // #1531: Back from B to A played A's own entrance transition. PowerPoint plays the one B came
    // in with, backwards.
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("transition.set", &json!({"kind": "fade", "index": 0})).unwrap();
    s.execute("transition.set", &json!({"kind": "push", "index": 1})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let ctx = egui::Context::default();
    let tex = |slide| {
        let t = crate::textures::upload(&ctx, "show", egui::ColorImage::filled([1, 1], Color32::RED));
        Some((Key { slide, step: 0, size: (1, 1), doc: 0, hidden: vec![], gif: 0, plays: vec![], ink: 0 }, t))
    };
    let mut show = Show::new(&doc, 0, false, false);
    show.tex = tex(0);
    let a = show.state.next(&doc);
    assert_eq!(a, ShowAction::Slide(1));
    assert!(show.apply(a, &doc, 0.0));
    let tr = show.trans.as_ref().expect("B's push plays");
    assert_eq!((tr.kind.as_str(), tr.reverse), ("push", false));

    show.tex = tex(1);
    let a = show.state.prev(&doc);
    assert_eq!(a, ShowAction::Slide(0));
    assert!(show.apply(a, &doc, 1.0));
    let tr = show.trans.as_ref().expect("going back plays a transition");
    assert_eq!((tr.kind.as_str(), tr.reverse, tr.from), ("push", true, Some(1)), "B's push, reversed, not A's fade");

    // Forward again: B's own transition, forward.
    show.tex = tex(0);
    let a = show.state.next(&doc);
    assert!(show.apply(a, &doc, 2.0));
    let tr = show.trans.as_ref().expect("B's push plays");
    assert_eq!((tr.kind.as_str(), tr.reverse), ("push", false));

    // A jump to an earlier slide is not Back: the slide's own transition plays.
    show.tex = tex(1);
    let a = show.state.goto(&doc, 0);
    assert!(show.apply(a, &doc, 3.0));
    let tr = show.trans.as_ref().expect("A's fade plays");
    assert_eq!((tr.kind.as_str(), tr.reverse), ("fade", false));
}

#[test]
fn a_reversed_transition_starts_on_the_slide_left_and_ends_on_the_one_come_back_to() {
    // #1531: running the frames from the end, with the old and new slides trading places.
    for kind in ["fade", "push", "wipe", "cover"] {
        let start = transition_layers(kind, "", true, 0.0, 16.0 / 9.0);
        let top = start.last().expect("a frame");
        assert_eq!((top.source, top.alpha), (Source::Old, 1.0), "{kind}: the slide left is shown first");
        let end = transition_layers(kind, "", true, 1.0, 16.0 / 9.0);
        assert!(end.iter().all(|l| l.source == Source::New), "{kind}: only the slide come back to is left: {:?}", end);
    }
}

#[test]
fn media_in_the_click_sequence_plays_with_its_step() {
    // #116: media set to play in the click sequence never started; only a click on it did.
    let s = deckcraft_engine::Session::with_new();
    let mut doc = s.active().unwrap().doc.clone();
    let id = doc.slides[0].shapes[0].id;
    let play = |delay_ms| deckcraft_model::Animation {
        shape: id,
        class: deckcraft_model::AnimClass::Media,
        effect: "play".into(),
        duration_ms: 5000,
        delay_ms,
        ..Default::default()
    };
    std::sync::Arc::make_mut(&mut std::sync::Arc::make_mut(&mut doc).slides[0]).animations = vec![play(0), play(2000)];
    let mut show = Show::new(&doc, 0, false, false);
    assert!(show.apply(ShowAction::Slide(0), &doc, 0.0));
    // Next: the first step plays and sets off its Play once.
    let a = show.state.next(&doc);
    assert_eq!(a, ShowAction::PlayStep(0));
    assert!(show.apply(a, &doc, 1.0));
    show.cue_media(&doc, 0, 0.0);
    show.cue_media(&doc, 0, 0.5);
    assert_eq!(std::mem::take(&mut show.media_cues), [(id, "play".to_string())]);
    // The step doesn't wait for the video: the step clock ends it at once.
    assert_eq!(show.timeline(&doc).step_duration(0), 0.0);
    let a = show.state.step_done(&doc);
    assert!(show.apply(a, &doc, 1.0));
    assert!(show.media_cues.is_empty(), "set off once");
    // Next again, before the second Play's delay: cutting the step short still sets it off.
    let a = show.state.next(&doc);
    assert_eq!(a, ShowAction::PlayStep(1));
    assert!(show.apply(a, &doc, 2.0));
    show.cue_media(&doc, 1, 0.5);
    assert!(show.media_cues.is_empty());
    let a = show.state.step_done(&doc);
    assert!(show.apply(a, &doc, 2.6));
    assert_eq!(std::mem::take(&mut show.media_cues), [(id, "play".to_string())]);
    // Going back doesn't set anything off.
    let a = show.state.prev(&doc);
    assert!(show.apply(a, &doc, 3.0));
    show.cue_media(&doc, 1, 9.0);
    assert!(show.media_cues.is_empty());
}

#[test]
fn the_laser_and_the_pen_are_not_on_together() {
    let doc = deckcraft_model::Presentation::default();
    let mut show = Show::new(&doc, 0, false, false);
    show.set_laser(true);
    assert!(show.laser && !show.pen);
    show.set_pen(true);
    assert!(show.pen && !show.laser);
    show.set_laser(true);
    show.arrow();
    assert!(!show.laser && !show.pen);
    show.set_laser(false);
    assert!(!show.laser);
}

fn deck_with_first_animation(start: deckcraft_model::anim::AnimStart) -> Presentation {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let mut doc = Presentation::clone(&s.active().unwrap().doc);
    let slide = std::sync::Arc::make_mut(&mut doc.slides[1]);
    let shape = slide.shapes[0].id;
    slide.animations = vec![deckcraft_model::anim::Animation { shape, start, duration_ms: 3000, ..Default::default() }];
    doc
}

#[test]
fn a_first_animation_after_or_with_previous_plays_when_the_slide_appears() {
    // #631: it was started twice and the second start finished it, so the slide appeared
    // fully built.
    use deckcraft_model::anim::AnimStart;
    for start in [AnimStart::AfterPrevious, AnimStart::WithPrevious] {
        let doc = deck_with_first_animation(start);
        // Starting the show on the slide.
        let mut show = Show::new(&doc, 1, false, false);
        show.after_enter(&doc, 5.0);
        assert_eq!((show.state.step, show.state.playing), (0, true), "{start:?} from the start");
        assert_eq!(show.step_start, 5.0);
        // Arriving from the slide before.
        let mut show = Show::new(&doc, 0, false, false);
        let a = show.state.next(&doc);
        assert!(matches!(a, ShowAction::Slide(1)));
        assert!(show.apply(a, &doc, 7.0));
        assert_eq!((show.state.step, show.state.playing), (0, true), "{start:?} on arrival");
    }
    // On Click still waits for the click.
    let doc = deck_with_first_animation(AnimStart::OnClick);
    let mut show = Show::new(&doc, 1, false, false);
    show.after_enter(&doc, 5.0);
    assert_eq!((show.state.step, show.state.playing), (0, false));
    assert!(matches!(show.state.next(&doc), ShowAction::PlayStep(0)));
}

#[test]
fn zoom_stays_on_the_slide_and_the_grid_hits_a_thumbnail() {
    let uv = zoom_uv(0.5, 0.5, 2.0);
    assert!((uv.min.x - 0.25).abs() < 1e-4 && (uv.min.y - 0.25).abs() < 1e-4);
    assert!((uv.width() - 0.5).abs() < 1e-4);
    let corner = zoom_uv(0.0, 0.0, 2.0);
    assert_eq!(corner.min, pos2(0.0, 0.0));
    assert!(corner.max.x <= 1.0 && corner.max.y <= 1.0);
    let full = Rect::from_min_size(Pos2::ZERO, vec2(1728.0, 1084.0));
    let cells = see_all_cells(full, 7, 16.0 / 9.0);
    assert_eq!(cells.len(), 7);
    for (i, a) in cells.iter().enumerate() {
        assert!(full.contains_rect(*a), "cell {i} leaves the window");
        for b in cells.iter().skip(i + 1) {
            let overlap = a.intersect(*b);
            assert!(overlap.width() < 1.0 || overlap.height() < 1.0, "thumbnails overlap {a:?} {b:?}");
        }
    }
    let hit = see_all_hit(full, 7, 16.0 / 9.0, cells[3].center());
    assert_eq!(hit, Some(3));
    assert_eq!(see_all_hit(full, 7, 16.0 / 9.0, pos2(2.0, 2.0)), None);
    let srect = Rect::from_min_size(pos2(10.0, 20.0), vec2(200.0, 100.0));
    assert_eq!(norm_on_slide(srect, unit_uv(), pos2(110.0, 70.0)), Some((0.5, 0.5)));
    let zoomed = zoom_uv(0.5, 0.5, 2.0);
    let back = norm_on_slide(srect, zoomed, pos2(110.0, 70.0)).expect("on the slide");
    assert!((back.0 - 0.5).abs() < 1e-3 && (back.1 - 0.5).abs() < 1e-3);
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

/// #1286: Ctrl+wheel zooms around the pointer, four notches per doubling, up to 4× (the most
/// PowerPoint's magnifier goes); zooming back to the whole slide leaves the zoom.
#[test]
fn wheel_zoom_keeps_the_point_under_the_pointer_and_stops_at_4x() {
    let doc = deckcraft_model::Presentation::default();
    let mut show = Show::new(&doc, 0, false, false);
    // A 16:9 slide, not at the origin, and a pointer off its centre.
    let srect = Rect::from_min_size(pos2(40.0, 30.0), vec2(320.0, 180.0));
    let p = pos2(40.0 + 320.0 * 0.2, 30.0 + 180.0 * 0.3);
    show.wheel_zoom(srect, p, 4.0);
    let (_, _, scale) = show.zoom_view().expect("zoomed in");
    assert!(near(scale, 2.0), "four notches double, got {scale}");
    // The window keeps the slide's shape: half the width and half the height of the slide.
    let uv = show.zoom_uv();
    assert!(near(uv.width(), 0.5) && near(uv.height(), 0.5), "{uv:?}");
    assert!(near(uv.min.x, 0.2 - 0.5 * 0.2) && near(uv.min.y, 0.3 - 0.5 * 0.3), "{uv:?}");
    let under = norm_on_slide(srect, uv, p).expect("on the slide");
    assert!(near(under.0, 0.2) && near(under.1, 0.3), "the point under the pointer moved to {under:?}");

    // Again from the zoomed view: the same slide point stays under the pointer.
    show.wheel_zoom(srect, p, 2.0);
    let (_, _, scale) = show.zoom_view().expect("zoomed in");
    assert!(near(scale, 2f32.powf(1.5)), "{scale}");
    let under = norm_on_slide(srect, show.zoom_uv(), p).expect("on the slide");
    assert!(near(under.0, 0.2) && near(under.1, 0.3), "{under:?}");

    show.wheel_zoom(srect, p, 40.0);
    let (_, _, scale) = show.zoom_view().expect("zoomed in");
    assert!(near(scale, 4.0), "capped at 4×, got {scale}");
    assert!(near(show.zoom_uv().width(), 0.25));

    show.wheel_zoom(srect, p, -7.0);
    assert!(show.zoom_view().is_some(), "one notch short of the whole slide is still zoomed");
    show.wheel_zoom(srect, p, -1.0);
    assert!(show.zoom_view().is_none(), "back to the whole slide leaves the zoom");
    assert_eq!(show.zoom_uv(), unit_uv());
    show.wheel_zoom(srect, p, -3.0);
    assert!(show.zoom_view().is_none(), "zooming out of the whole slide does nothing");

    // Off the slide (the letterbox) the wheel does not zoom.
    show.wheel_zoom(srect, pos2(10.0, 10.0), 4.0);
    assert!(show.zoom_view().is_none());
    // An armed click-zoom gives way to the wheel.
    show.zoom_arm = true;
    show.wheel_zoom(srect, p, 4.0);
    assert!(!show.zoom_arm && show.zoom_view().is_some());
}

#[test]
fn wheel_zoom_reads_only_the_wheel_with_ctrl_or_cmd() {
    let wheel = |unit, y: f32, modifiers| egui::Event::MouseWheel {
        unit,
        delta: vec2(0.0, y),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers,
    };
    use egui::MouseWheelUnit::{Line, Point};
    assert_eq!(wheel_zoom_lines(&[wheel(Line, 1.0, egui::Modifiers::CTRL)], 40.0), Some(1.0));
    assert_eq!(wheel_zoom_lines(&[wheel(Line, -2.0, egui::Modifiers::COMMAND)], 40.0), Some(-2.0));
    assert_eq!(wheel_zoom_lines(&[wheel(Point, 20.0, egui::Modifiers::CTRL), wheel(Point, 20.0, egui::Modifiers::CTRL)], 40.0), Some(1.0));
    assert_eq!(wheel_zoom_lines(&[wheel(Line, 1.0, egui::Modifiers::NONE)], 40.0), None, "a plain wheel turns the page");
    assert_eq!(wheel_zoom_lines(&[wheel(Line, 1.0, egui::Modifiers::SHIFT)], 40.0), None);
    assert_eq!(wheel_zoom_lines(&[egui::Event::Zoom(2.0)], 40.0), None, "a pinch is not a show gesture");
    assert_eq!(wheel_zoom_lines(&[wheel(Line, f32::NAN, egui::Modifiers::CTRL)], 40.0), Some(0.0));
}

/// A running show on the first of three slides, with presenter view off.
fn show_of_three() -> egui_kittest::Harness<'static, SlideApp> {
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    session.execute("slide.new", &json!({})).unwrap();
    let mut app = SlideApp::new(session, crate::Services::default());
    app.ui.presenter_view = false;
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    harness.state_mut().start_show(0, false);
    harness.run_steps(2);
    harness
}

fn show_wheel(harness: &mut egui_kittest::Harness<'static, SlideApp>, at: Pos2, y: f32, modifiers: egui::Modifiers) {
    harness.input_mut().events.push(egui::Event::PointerMoved(at));
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: vec2(0.0, y),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers,
    });
    harness.run_steps(40);
}

/// #1286: in the show, Ctrl+wheel (Cmd+wheel on a Mac) zooms the slide and does not turn the
/// page; the plain wheel still turns it.
#[test]
fn ctrl_wheel_zooms_the_show_and_the_plain_wheel_turns_the_page() {
    for held in [egui::Modifiers::CTRL, egui::Modifiers::COMMAND] {
        let mut h = show_of_three();
        let centre = pos2(500.0, 350.0);
        show_wheel(&mut h, centre, 4.0, held);
        let show = h.state().show.as_ref().expect("still showing");
        assert_eq!(show.state.slide, 0, "{held:?}: the zoom wheel does not turn the page");
        let (_, _, scale) = show.zoom_view().expect("zoomed in");
        assert!(near(scale, 2.0), "{held:?}: {scale}");

        show_wheel(&mut h, centre, -4.0, held);
        let show = h.state().show.as_ref().expect("still showing");
        assert_eq!(show.state.slide, 0, "{held:?}: zooming out does not go back a page either");
        assert!(show.zoom_view().is_none(), "{held:?}: back to the whole slide");

        show_wheel(&mut h, centre, -3.0, egui::Modifiers::NONE);
        assert_eq!(h.state().show.as_ref().expect("still showing").state.slide, 1, "the plain wheel turns the page");
    }
}

#[test]
fn escape_only_closes_the_grid_of_its_own_window() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, true);
    show.presenter_grid = true;
    assert!(!show.back_out(false), "audience Esc leaves the show, the Presenter View grid stays");
    assert!(show.presenter_grid);
    assert!(show.back_out(true));
    assert!(!show.presenter_grid);
    show.grid = true;
    assert!(!show.back_out(true), "Presenter View Esc does not close the audience grid");
    assert!(show.grid);
    assert!(show.back_out(false));
    assert!(!show.grid);
}

#[test]
fn escape_leaves_the_grid_then_the_zoom_and_blank_toggles() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, true);
    show.grid = true;
    show.zoom = Some(Zoom::at(0.2, 0.3));
    assert!(show.back_out(false));
    assert!(!show.grid);
    assert!(show.zoom.is_some(), "closing the grid leaves the zoom");
    assert!(show.back_out(false));
    assert!(show.zoom.is_none());
    assert!(!show.back_out(false));
    show.zoom_key();
    assert!(show.zoom_arm);
    show.zoom_key();
    assert!(!show.zoom_arm);
    toggle_blank(&mut show, Color32::BLACK);
    assert_eq!(show.blank_name(), Some("black"));
    toggle_blank(&mut show, Color32::WHITE);
    assert_eq!(show.blank_name(), Some("white"));
    toggle_blank(&mut show, Color32::WHITE);
    assert_eq!(show.blank_name(), None);
    show.zoom = Some(Zoom::at(0.4, 0.6));
    assert!(show.apply(ShowAction::Slide(1), &doc, 1.0));
    assert!(show.zoom.is_none(), "a new slide is shown whole");
}

/// #1128: the presenter drew the notes as plain text, so bullets, numbers and bold were lost
/// although the notes pane shows them; the notes stay selectable.
#[test]
fn presenter_notes_keep_bullets_numbers_and_bold() {
    use deckcraft_model::text::{Bullet, ParaProps, Paragraph, Run, RunProps, TextBody};
    let point = |text: &str, bullet: Bullet| Paragraph {
        level: 1,
        props: ParaProps { bullet: Some(bullet), ..Default::default() },
        runs: vec![Run::new(text)],
        ..Default::default()
    };
    let num = || Bullet::AutoNum { scheme: "arabicPeriod".into(), start_at: 1 };
    let notes = TextBody {
        paragraphs: vec![
            Paragraph { runs: vec![Run::with("Opening", RunProps { bold: Some(true), ..Default::default() })], ..Default::default() },
            point("Welcome", Bullet::Char { char: "•".into() }),
            point("Agenda", num()),
            point("Demo", num()),
        ],
        ..Default::default()
    };
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let mut sense = None;
    let mut out = ctx.run_ui(Default::default(), |ui| sense = Some(presenter_notes(ui, &notes, 24.0).sense));
    out.textures_delta.clear();
    assert!(sense.unwrap().senses_drag(), "the notes can be selected, as the plain label could");
    let mut texts = vec![];
    let mut stack: Vec<egui::Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    while let Some(shape) = stack.pop() {
        match shape {
            egui::Shape::Vec(v) => stack.extend(v),
            egui::Shape::Text(t) => texts.push(t.galley),
            _ => {}
        }
    }
    let painted: Vec<&str> = texts.iter().map(|g| g.job.text.as_str()).collect();
    for marker in ["•", "1.", "2."] {
        assert!(painted.contains(&marker), "{marker} is drawn: {painted:?}");
    }
    let notes_galley = texts.iter().find(|g| g.job.text.contains("Agenda")).expect("the notes are drawn");
    let font_of = |s: &str| {
        let b = notes_galley.job.text.find(s).unwrap();
        notes_galley.job.sections.iter().find(|x| (x.byte_range.start.0..x.byte_range.end.0).contains(&b)).unwrap().format.font_id.clone()
    };
    assert_eq!(font_of("Opening"), theme::bold(24.0), "bold runs stay bold at the pane's size");
    assert_eq!(font_of("Agenda"), theme::font(24.0));
}

/// Hidden logic keeps playing from wall time even though `run_logic` leaves egui's input
/// time at the last visible frame. A visible pass re-anchors instead of shifting a pause.
#[test]
fn hidden_logic_advances_the_show_and_visible_frames_reanchor() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, true);
    show.started = true;
    show.step_start = 10.0;
    show.slide_start = 10.0;
    show.step_end = 10.0;
    show.show_start = 10.0;
    show.last_move = 10.0;
    show.timer = PresenterTimer::start(10.0);
    let t0 = web_time::Instant::now();
    show.observe_visible_frame(10.0, t0);
    assert_eq!(show.hidden_logic_time(10.0, t0), 10.0, "the first hidden tick starts at the last visible time");
    let hidden = show.hidden_logic_time(10.0, t0 + std::time::Duration::from_secs(2));
    assert!((hidden - 12.0).abs() < 1e-9, "hidden logic advances the show time: {hidden}");
    assert!((show.presenter_elapsed(hidden) - 2.0).abs() < 1e-9, "presenter time keeps running");
    assert!((show.clock_snapshot().step_start - 10.0).abs() < 1e-9, "the clock origin is not shifted");
    let later = show.hidden_logic_time(10.0, t0 + std::time::Duration::from_secs(5));
    assert!((later - 15.0).abs() < 1e-9, "repeated hidden ticks do not stack a pause: {later}");
    show.observe_visible_frame(20.0, t0 + std::time::Duration::from_secs(10));
    assert_eq!(show.hidden_logic_time(20.0, t0 + std::time::Duration::from_secs(10)), 20.0, "a visible frame re-anchors");
}

#[test]
fn presenter_timer_pauses_resets_and_notes_size_clamps() {
    let mut t = PresenterTimer::start(10.0);
    assert!((t.elapsed(12.5) - 2.5).abs() < 1e-9);
    t.toggle(15.0);
    assert!(t.paused());
    assert!((t.elapsed(100.0) - 5.0).abs() < 1e-9);
    t.reset(100.0);
    assert!(t.paused());
    assert_eq!(t.elapsed(200.0), 0.0);
    t.toggle(200.0);
    assert!(!t.paused());
    assert!((t.elapsed(203.0) - 3.0).abs() < 1e-9);
    t.reset(203.0);
    assert!(!t.paused());
    assert!((t.elapsed(204.0) - 1.0).abs() < 1e-9);
    let bad = PresenterTimer::start(f64::NAN);
    assert_eq!(bad.elapsed(f64::INFINITY), 0.0);
    assert_eq!(format_elapsed(3661.4), "01:01:01");
    assert_eq!(format_elapsed(-1.0), "00:00:00");
    assert_eq!(format_elapsed(f64::NAN), "00:00:00");
    let clock = wall_clock();
    assert!(clock == "--:--" || (clock.len() == 5 && clock.as_bytes().get(2) == Some(&b':')));

    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, true);
    assert_eq!(show.notes_px, NOTES_DEFAULT);
    show.nudge_notes(NOTES_STEP);
    assert_eq!(show.notes_px, 22.0);
    show.nudge_notes(1000.0);
    assert_eq!(show.notes_px, NOTES_MAX);
    show.nudge_notes(-1000.0);
    assert_eq!(show.notes_px, NOTES_MIN);
    show.nudge_notes(f32::NAN);
    assert_eq!(show.notes_px, NOTES_MIN);
    let v = show.showing_value(1, 16.0 / 9.0, Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0)), 0.0);
    assert_eq!(v["presenterPaused"], json!(false));
    assert_eq!(v["notesPx"], json!(NOTES_MIN));
}

#[test]
fn a_typed_slide_number_jumps_and_then_expires() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, false);
    show.push_slide_number('1', 10.0);
    show.push_slide_number('2', 10.2);
    assert_eq!(show.goto, "12");
    assert_eq!(show.take_goto(), Some(11));
    assert!(show.goto.is_empty());
    show.push_slide_number('0', 1.0);
    assert_eq!(show.take_goto(), None, "slide 0 is not a slide");
    show.push_slide_number('4', 5.0);
    show.expire_goto(7.0);
    assert_eq!(show.goto, "4");
    show.expire_goto(8.1);
    assert!(show.goto.is_empty());
    show.push_slide_number('9', 0.0);
    assert!(show.clear_goto());
    assert!(!show.clear_goto());
    for _ in 0..6 {
        show.push_slide_number('9', 1.0);
    }
    assert_eq!(show.goto.len(), GOTO_DIGITS);
    show.push_slide_number('a', 1.0);
    assert!(show.goto.chars().all(|c| c.is_ascii_digit()));
}

#[test]
fn browsed_by_an_individual_stays_in_a_window() {
    assert!(!show_fills_screen(false, "browsed"));
    assert!(!show_fills_screen(true, "speaker"));
    assert!(show_fills_screen(false, "speaker"));
    assert!(show_fills_screen(false, "kiosk"));
}

#[test]
fn the_next_preview_fits_its_column_whatever_the_slide_shape() {
    let right = Rect::from_min_size(pos2(700.0, 100.0), vec2(400.0, 600.0));
    let mut s = deckcraft_engine::Session::with_new();
    for (w, h) in [(72, 4032), (4032, 72), (960, 540)] {
        s.execute("design.slideSize", &json!({"w": w, "h": h})).unwrap();
        let doc = s.active().unwrap().doc.clone();
        let r = next_rect(right, &doc);
        assert!(right.contains_rect(r), "{w}x{h}: {r:?}");
        let aspect = r.height() / r.width();
        assert!((aspect - h as f32 / w as f32).abs() / (h as f32 / w as f32) < 0.01, "{w}x{h}: {r:?}");
    }
    // 16:9 still takes the column's full width.
    let doc = s.active().unwrap().doc.clone();
    assert_eq!(next_rect(right, &doc).width(), 400.0);
}

#[test]
fn back_leaves_a_slide_whose_first_animation_is_with_previous() {
    // #844: Back on such a slide undid its first step, and the next frame played that step
    // again by itself, so Back never reached the slide before.
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    let id = session.active().unwrap().doc.slides[1].shapes[0].id;
    session
        .execute("animation.add", &json!({"effect": "fade", "class": "entrance", "ids": [id.0], "start": "withPrevious", "duration": 500}))
        .unwrap();
    let app = SlideApp::new(session, crate::Services::default());
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    harness.state_mut().start_show(0, false);
    harness.run_steps(2);
    harness.key_press(egui::Key::ArrowRight);
    // The first step plays by itself and ends (0.5 s; a frame is 0.25 s).
    harness.run_steps(8);
    let st = &harness.state().show.as_ref().unwrap().state;
    assert_eq!((st.slide, st.step, st.playing), (1, 1, false), "on slide 2, fully built");
    for _ in 0..5 {
        harness.key_press(egui::Key::ArrowLeft);
        harness.run_steps(2);
    }
    let st = &harness.state().show.as_ref().unwrap().state;
    assert_eq!(st.slide, 0, "five Backs leave slide 2 (step {}, playing {})", st.step, st.playing);
}

/// Click-target state follows show instance, reading/presenter modes, grid, and other layout
/// changes instead of retaining a stale target.
#[test]
fn input_state_tracks_the_modes_that_move_show_click_targets() {
    let session = deckcraft_engine::Session::with_new();
    let doc = &session.active().unwrap().doc;
    let mut show = Show::new(doc, 0, false, false);
    let base = show.input_state();
    assert_eq!(show.input_state(), base);

    show.instance += 1;
    assert_ne!(show.input_state(), base, "a new show instance is a new input target");
    show.instance = base.instance;

    show.reading = true;
    assert_ne!(show.input_state(), base, "the reading layout moves the slide");
    show.reading = false;

    show.presenter = true;
    assert_ne!(show.input_state(), base, "presenter view changes the available area");
    show.presenter = false;

    show.grid = true;
    assert_ne!(show.input_state(), base, "See All Slides replaces the slide with thumbnails");
    show.grid = false;

    show.pen = true;
    assert_ne!(show.input_state(), base, "the pen changes what a drag means");
    show.pen = false;

    show.laser = true;
    assert_ne!(show.input_state(), base, "the laser changes what a drag means");
    show.laser = false;

    show.zoom_arm = true;
    assert_ne!(show.input_state(), base, "armed zoom changes what a click means");
    show.zoom_arm = false;

    show.zoom = Some(Zoom::at(0.25, 0.75));
    assert_ne!(show.input_state(), base, "a zoomed slide has a different click mapping");
    show.zoom = None;

    show.blank = Some(Color32::BLACK);
    assert_ne!(show.input_state(), base, "a blank screen has no slide click targets");
}

#[test]
fn remote_presenter_uses_the_audience_state_and_closes_with_the_show() {
    use crate::presenter::{Command, Host, Snapshot};
    use std::{cell::RefCell, rc::Rc};
    #[derive(Default)]
    struct Bridge {
        opens: usize,
        closes: usize,
        commands: Vec<Command>,
        last: Option<Snapshot>,
        gone: bool,
    }
    struct Mock(Rc<RefCell<Bridge>>);
    impl Host for Mock {
        fn open(&mut self) -> Result<(), String> {
            self.0.borrow_mut().opens += 1;
            Ok(())
        }
        fn is_open(&self) -> bool {
            !self.0.borrow().gone
        }
        fn commands(&mut self) -> Vec<Command> {
            std::mem::take(&mut self.0.borrow_mut().commands)
        }
        fn update(&mut self, snapshot: Snapshot) {
            self.0.borrow_mut().last = Some(snapshot);
        }
        fn close(&mut self) {
            self.0.borrow_mut().closes += 1;
        }
    }
    let bridge = Rc::new(RefCell::new(Bridge::default()));
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    session.execute("slide.new", &json!({})).unwrap();
    let services = crate::Services { presenter: Some(Box::new(Mock(bridge.clone()))), ..Default::default() };
    let mut app = SlideApp::new(session, services);
    app.ui.presenter_view = true;
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(3);
    h.state_mut().start_show(0, false);
    h.run_steps(2);
    assert_eq!(bridge.borrow().opens, 1);
    for (key, slide) in [(egui::Key::ArrowRight, 1), (egui::Key::ArrowLeft, 0), (egui::Key::End, 2), (egui::Key::Home, 0)] {
        bridge.borrow_mut().commands.push(Command::Key { name: key.name().into(), command: false, shift: false, alt: false, ctrl: false });
        h.run_steps(2);
        assert_eq!(h.state().show.as_ref().unwrap().state.slide, slide);
    }
    // #1864: the web popup's Previous/Next buttons send the DOM key names.
    for (name, slide) in [("ArrowRight", 1), ("ArrowLeft", 0)] {
        bridge.borrow_mut().commands.push(Command::Key { name: name.into(), command: false, shift: false, alt: false, ctrl: false });
        h.run_steps(2);
        assert_eq!(h.state().show.as_ref().unwrap().state.slide, slide, "{name}");
    }
    // #1866: All slides opens in the web Presenter View, as on desktop; the audience keeps its slide.
    bridge.borrow_mut().commands.push(Command::Key { name: "G".into(), command: false, shift: false, alt: false, ctrl: false });
    h.run_steps(2);
    let show = h.state().show.as_ref().unwrap();
    assert!(show.presenter_grid && !show.grid, "G from the popup opens its own grid, not the audience's");
    for _ in 0..20 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    let last = bridge.borrow().last.clone().unwrap();
    assert!(last.grid);
    assert_eq!(last.slides.as_ref().map(Vec::len), Some(3), "the popup gets a thumbnail per slide");
    assert!(last.slides.iter().flatten().all(|p| p.width.max(p.height) <= 96), "thumbnails stay small enough for one message");
    assert!(last.current.is_none() && last.next.is_none(), "the grid replaces both previews");
    bridge.borrow_mut().commands.push(Command::Goto { slide: 2 });
    h.run_steps(2);
    let show = h.state().show.as_ref().unwrap();
    assert!(!show.presenter_grid && !show.grid);
    assert_eq!(show.state.slide, 2);
    // Closing the popup with its grid open must not hold auto-advance for the rest of the show.
    bridge.borrow_mut().commands.push(Command::Key { name: "G".into(), command: false, shift: false, alt: false, ctrl: false });
    h.run_steps(2);
    assert!(h.state().show.as_ref().unwrap().presenter_grid);
    bridge.borrow_mut().gone = true;
    h.run_steps(2);
    assert!(!h.state().show.as_ref().unwrap().presenter_grid);
    bridge.borrow_mut().gone = false;
    bridge.borrow_mut().commands.push(Command::End);
    h.run_steps(2);
    assert!(h.state().show.is_none());
    assert_eq!(bridge.borrow().closes, 1);
    h.state_mut().start_show(1, false);
    h.run_steps(2);
    assert_eq!(bridge.borrow().opens, 2);
    assert_eq!(h.state().show.as_ref().unwrap().state.slide, 1);
}

/// #964: the next-slide preview renders on another thread; the presenter frame that asks for
/// it doesn't wait, and the preview arrives on a later frame.
#[test]
fn the_presenter_next_preview_arrives_without_blocking_the_frame() {
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    let doc = session.active().unwrap().doc.clone();
    // One frame on its own (a harness runs frames until the preview lands): it asks for the
    // preview and returns without it.
    let mut show = Show::new(&doc, 0, false, true);
    let mut keep = true;
    let mut app = SlideApp::new(session, crate::Services::default());
    let ctx = egui::Context::default();
    ctx.run_ui(egui::RawInput::default(), |ui| presenter_window(&mut app, ui.ctx(), &mut show, &doc, 0.0, &mut keep)).textures_delta.clear();
    assert!(show.next_job.is_some() && show.next_tex.is_none(), "the first frame leaves the preview pending");
    assert_eq!(show.preview_spawns, 1);
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1100.0, 700.0)).build_ui_state(
        |ui, (app, doc, show, keep): &mut (SlideApp, std::sync::Arc<Presentation>, Show, bool)| {
            let now = ui.input(|i| i.time);
            presenter_window(app, ui.ctx(), show, doc, now, keep);
        },
        (SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default()), doc, show, true),
    );
    for _ in 0..200 {
        if harness.state().2.next_tex.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        harness.step();
    }
    assert_eq!(harness.state().2.next_tex.as_ref().map(|(_, n, _)| *n), Some(1));
    assert!(harness.state().2.next_job.is_none());
    assert_eq!(harness.state().2.preview_spawns, 1, "the pending render landed; none started over");
}

/// #964: paging quickly keeps one preview render running, and the preview that lands is the
/// newest one asked for.
#[test]
fn paging_quickly_renders_one_preview_at_a_time_and_lands_on_the_newest() {
    let mut session = deckcraft_engine::Session::with_new();
    for _ in 0..7 {
        session.execute("slide.new", &json!({})).unwrap();
    }
    let doc = session.active().unwrap().doc.clone();
    let show = Show::new(&doc, 0, false, true);
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1100.0, 700.0)).build_ui_state(
        |ui, (app, doc, show, keep): &mut (SlideApp, std::sync::Arc<Presentation>, Show, bool)| {
            let now = ui.input(|i| i.time);
            presenter_window(app, ui.ctx(), show, doc, now, keep);
        },
        (SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default()), doc, show, true),
    );
    let mut turns = 0;
    for _ in 0..6 {
        let doc = harness.state().1.clone();
        let a = harness.state_mut().2.state.next(&doc);
        harness.state_mut().2.apply(a, &doc, 0.0);
        harness.step();
        turns += 1;
    }
    let want = harness.state().2.state.peek_next(&harness.state().1);
    for _ in 0..400 {
        if harness.state().2.next_tex.as_ref().map(|(_, k, _)| Some(*k)) == Some(want) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        harness.step();
    }
    let st = &harness.state().2;
    assert_eq!(st.next_tex.as_ref().map(|(_, k, _)| *k), want, "the newest preview lands");
    assert!(st.next_job.is_none());
    // A render starts only once the one before it has finished: never more than one per turn.
    assert!(st.preview_spawns <= turns + 1, "{} renders for {turns} turns", st.preview_spawns);

    // While a render is pending, turning the page starts no other.
    let (_tx, rx) = std::sync::mpsc::channel();
    let doc = harness.state().1.clone();
    let spawns = harness.state().2.preview_spawns;
    harness.state_mut().2.next_tex = None;
    harness.state_mut().2.next_job = Some((doc.clone(), 0, rx));
    let a = harness.state_mut().2.state.prev(&doc);
    harness.state_mut().2.apply(a, &doc, 0.0);
    harness.step();
    let st = &harness.state().2;
    assert_eq!(st.preview_spawns, spawns, "no render starts while one is pending");
    assert!(st.next_job.is_some());
}

/// A preview rendered from a presentation that has since been edited (replaced) is dropped,
/// and the preview is rendered again from the new one.
#[test]
fn a_preview_of_an_edited_presentation_is_rendered_again() {
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    let doc = session.active().unwrap().doc.clone();
    let show = Show::new(&doc, 0, false, true);
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1100.0, 700.0)).build_ui_state(
        |ui, (app, doc, show, keep): &mut (SlideApp, std::sync::Arc<Presentation>, Show, bool)| {
            let now = ui.input(|i| i.time);
            presenter_window(app, ui.ctx(), show, doc, now, keep);
        },
        (SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default()), doc, show, true),
    );
    harness.step();
    let edited = std::sync::Arc::new((*harness.state().1).clone());
    harness.state_mut().1 = edited.clone();
    for _ in 0..400 {
        if harness.state().2.next_tex.as_ref().is_some_and(|(d, _, _)| std::sync::Arc::ptr_eq(d, &edited)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        harness.step();
    }
    let tex = harness.state().2.next_tex.as_ref().map(|(d, k, _)| (std::sync::Arc::ptr_eq(d, &edited), *k));
    assert_eq!(tex, Some((true, 1)), "the preview comes from the edited presentation");
}

/// #1654: Preview on the Transitions ribbon plays the current slide's transition from the
/// previous slide into the current slide, and exits when finished.
#[test]
fn transition_preview_plays_from_the_previous_slide() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("transition.set", &json!({"kind": "push", "index": 1, "duration": 1000})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 1, true, false);
    show.start_transition_preview(&doc, 10.0);
    assert!(show.preview_only);
    let tr = show.trans.as_ref().expect("transition preview is playing");
    assert_eq!(tr.kind, "push");
    assert_eq!(tr.from, Some(0), "enters from slide 0 into slide 1");
    assert_eq!(tr.dur, 1.0);
    assert!(!tr.reverse);
    assert_eq!(tr.start, 10.0);
}

/// #1654: Transition preview for slide 0 plays with from = None (transitioning from black).
#[test]
fn transition_preview_for_first_slide_transitions_from_black() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("transition.set", &json!({"kind": "fade", "index": 0, "duration": 500})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, true, false);
    show.start_transition_preview(&doc, 5.0);
    assert!(show.preview_only);
    let tr = show.trans.as_ref().expect("fade transition preview is playing");
    assert_eq!(tr.kind, "fade");
    assert_eq!(tr.from, None, "slide 0 transitions from black (None)");
    assert_eq!(tr.dur, 0.5);
}

#[test]
fn narration_skips_silence_and_play_narrations_can_turn_it_off() {
    let quiet = vec![0.0f32; 4000];
    assert!(narration_wav(8000, &quiet).is_none());
    assert!(narration_wav(8000, &[0.2; 100]).is_none());
    let tone: Vec<f32> = (0..4000).map(|i| (i as f32 * 0.2).sin() * 0.3).collect();
    assert!(narration_wav(8000, &tone).is_some());
    let narr = deckcraft_model::MediaClip { autoplay: true, narration: true, ..Default::default() };
    assert!(play_on_enter(&narr, false));
    assert!(!play_on_enter(&narr, true));
    let clip = deckcraft_model::MediaClip { autoplay: true, ..Default::default() };
    assert!(play_on_enter(&clip, true));
}

#[test]
fn ctrl_shows_the_laser_without_a_trace_and_the_button_can_leave_it_on() {
    assert!(pointer_is_laser(false, true, false));
    assert!(pointer_is_laser(true, false, false));
    assert!(!pointer_is_laser(false, false, false));
    assert!(!pointer_is_laser(true, true, true));
}

#[test]
fn laser_follows_the_slide_clock_then_goes_out() {
    let pts = vec![deckcraft_model::LaserSample { x: 1.0, y: 2.0, t_ms: 0 }, deckcraft_model::LaserSample { x: 9.0, y: 8.0, t_ms: 400 }];
    assert_eq!(laser_pos(&pts, 100), Some((1.0, 2.0)));
    assert_eq!(laser_pos(&pts, 400), Some((9.0, 8.0)));
    assert_eq!(laser_pos(&pts, 700), Some((9.0, 8.0)));
    assert_eq!(laser_pos(&pts, 701), None);
    assert_eq!(saved_status(true, false, true), "Recording saved: slide timings, narration and laser.");
    assert_eq!(saved_status(false, false, false), "Recording saved: slide timings.");
}

/// #1812: a Record preview follows the recorded timings and narration even when the deck turns
/// Use Timings and Play Narrations off, because it shows what was recorded.
#[test]
fn a_record_preview_forces_timings_and_narration() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("show.setup", &json!({"useTimings": false, "noNarration": true})).unwrap();
    s.execute("transition.timing", &json!({"index": 0, "after": 1000})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, false);
    assert!(!show.state.auto_advance_due(&doc, 2.0), "the deck does not use timings");
    assert!(show.without_narration(&doc), "the deck turns narration off");

    show.recording_preview = true;
    assert!(show.auto_advance_due(&doc, 2.0), "the preview follows the recorded timing");
    assert!(!show.auto_advance_due(&doc, 0.5), "not before its time");
    assert!(!show.without_narration(&doc), "the preview plays the narration");
    // Hidden playback keeps waking for the preview's timings: the wake-up is asked for while the
    // timing is still pending, before the turn is queued.
    show.started = true;
    assert!(show.hidden_time_driven(&doc));
    let mut actions = Vec::new();
    show.queue_clock_actions(&doc, 2.0, &mut actions);
    assert!(matches!(actions.as_slice(), [ShowAction::Slide(1)]), "the slide turns by itself: {actions:?}");
}

/// #1812: a Record preview writes nothing, even if the show was flagged to record: no timings,
/// ink, laser trace or narration end up in the recording buffers.
#[test]
fn a_record_preview_keeps_no_recorded_material() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    show.rehearse = true;
    show.recording_preview = true;
    show.begin_recording(0.0);
    assert!(!show.keeps_recording());
    show.strokes.push(InkStroke {
        points: vec![pos2(0.1, 0.1), pos2(0.2, 0.2)],
        mode: InkMode::Pen,
        color: deckcraft_model::Rgba::BLACK,
        width: 2.25,
        in_take: true,
    });
    show.laser_live.push((0.5, 0.5, 10));
    let keep = show.keeps_recording();
    assert!(!keep);
    show.stash_ink(&doc, 0, keep, 1.0);
    show.stash_laser(0, keep);
    show.apply(ShowAction::Slide(1), &doc, 1.0);
    assert!(show.recorded.is_empty(), "no ink");
    assert!(show.laser_saved.is_empty(), "no laser trace");
    assert!(show.timings.is_empty(), "no slide timing");
    assert!(show.narration.is_empty(), "no narration");
}

/// While the Record view is up its command chords belong to the top bar: the show must not flip
/// the pen with Ctrl+Shift+P (the take's Pause) nor erase the slide's ink with Ctrl+E (Export).
#[test]
fn the_record_view_keeps_its_command_chords() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    show.strokes.push(InkStroke {
        points: vec![pos2(0.1, 0.1), pos2(0.2, 0.2)],
        mode: InkMode::Pen,
        color: deckcraft_model::Rgba::BLACK,
        width: 2.25,
        in_take: true,
    });
    let command = egui::Modifiers { command: true, ..Default::default() };
    let command_shift = egui::Modifiers { command: true, shift: true, ..Default::default() };
    let mut mic: crate::show::MicSlot = None;
    let (_, consumed) = super::keys::handle_keys(&mut mic, &mut show, &doc, 0.0, vec![(egui::Key::P, command_shift), (egui::Key::E, command)], false);
    assert!(consumed.is_empty(), "the top bar reads these: {consumed:?}");
    assert!(!show.pen, "Ctrl+Shift+P is Pause, not the pen");
    assert_eq!(show.strokes.len(), 1, "Ctrl+E is Export, not erase-all");
}

/// #1812: an open Record view that is not capturing keeps nothing. Browsing the deck before
/// Record, or after Stop, must not write timings, ink or laser for the browsed slides.
#[test]
fn an_idle_record_view_keeps_no_recorded_material() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    assert!(!show.keeps_recording(), "the Record view opens idle");
    let stroke = || InkStroke {
        points: vec![pos2(0.1, 0.1), pos2(0.2, 0.2)],
        mode: InkMode::Pen,
        color: deckcraft_model::Rgba::BLACK,
        width: 2.25,
        in_take: true,
    };
    show.strokes.push(stroke());
    show.laser_live.push((0.5, 0.5, 10));
    show.apply(ShowAction::Slide(1), &doc, 1.0);
    assert!(show.recorded.is_empty(), "no ink before the take");
    assert!(show.laser_saved.is_empty(), "no laser before the take");
    assert!(show.timings.is_empty(), "no slide timing before the take");
    // A take starts: the show records again.
    show.begin_recording(1.0);
    assert!(show.keeps_recording());
    show.strokes.push(stroke());
    show.laser_live.push((0.5, 0.5, 10));
    show.apply(ShowAction::Slide(0), &doc, 2.0);
    assert_eq!(show.recorded.len(), 1, "the take keeps its ink");
    assert_eq!(show.laser_saved.len(), 1, "and its laser");
    assert_eq!(show.timings.len(), 1, "and its timing");
    // Stop keeps what the take captured and records nothing new.
    show.end_recording();
    assert!(!show.keeps_recording());
    show.strokes.push(stroke());
    show.apply(ShowAction::Slide(1), &doc, 3.0);
    assert_eq!(show.recorded.len(), 1, "a stopped take records nothing more");
    assert_eq!(show.timings.len(), 1);
}

/// A microphone that hands over one take of a steady tone, so narration_wav accepts it.
struct ToneMic {
    rate: u32,
    samples: Vec<f32>,
}

impl deckcraft_media::MicIn for ToneMic {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn take(&mut self) -> (u32, Vec<f32>) {
        (self.rate, std::mem::take(&mut self.samples))
    }
    fn stop(&mut self) {}
}

/// The take a slide change ends belongs to the slide being left, not to the one that opened:
/// navigation has already entered the destination when the shell hands the audio over.
#[test]
fn a_transition_take_belongs_to_the_slide_being_left() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    show.begin_recording(0.0);
    show.mic_ok = true;
    // Cache the slide 0 timeline the way a frame does, then enter slide 1.
    let _ = show.timeline(&doc);
    show.state.slide = 1;
    let rate = 8_000;
    let tone: Vec<f32> = (0..rate / 2).map(|i| (i as f32 * 0.5).sin() * 0.4).collect();
    let mut mic: crate::show::MicSlot = Some(Box::new(ToneMic { rate, samples: tone }));
    super::before_slide_change_with(&mut mic, &mut show, &ShowAction::Slide(1));
    assert_eq!(show.narration.len(), 1, "the take is kept");
    assert_eq!(show.narration[0].0, 0, "under the slide it was spoken on");
}

/// A microphone that opens and then fails, as a refused permission or a device unplugged mid-take
/// does after `start` returned `Ok`.
struct FailingMic {
    rate: u32,
    samples: Vec<f32>,
    failure: Option<String>,
}

impl deckcraft_media::MicIn for FailingMic {
    fn start(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn take(&mut self) -> (u32, Vec<f32>) {
        (self.rate, std::mem::take(&mut self.samples))
    }
    fn failure(&self) -> Option<String> {
        self.failure.clone()
    }
    fn stop(&mut self) {}
}

/// The device opens asynchronously, so a refused microphone shows up only through `failure()`:
/// the take keeps what was captured, stops the microphone and says why.
#[test]
fn a_microphone_that_fails_mid_take_keeps_what_it_captured_and_says_so() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    show.begin_recording(0.0);
    show.mic_ok = true;
    let rate = 8_000;
    let tone: Vec<f32> = (0..rate / 2).map(|i| (i as f32 * 0.5).sin() * 0.4).collect();
    let mic = FailingMic { rate, samples: tone, failure: Some("no input device".into()) };
    let mut app = crate::SlideApp::new(s, crate::Services { mic: Some(Box::new(mic)), ..Default::default() });
    assert!(super::ui::mic_failure(&mut app, &mut show));
    assert_eq!(show.narration.len(), 1, "the samples captured before the failure are kept");
    assert_eq!(show.narration[0].0, 0, "under the slide they were spoken on");
    assert!(!show.mic_ok, "the toolbar stops claiming a live microphone");
    assert_eq!(show.mic_error.as_deref(), Some("no input device"));
    assert!(show.record_view.devices.mic.failed);
    assert_eq!(app.status.as_ref().map(|s| s.0.as_str()), Some("Microphone: no input device"));
    // Nothing more to report: the next frame does not repeat the message or the handover.
    assert!(!super::ui::mic_failure(&mut app, &mut show));
    assert_eq!(show.narration.len(), 1);
}

/// The buffer stops at the WAV sample cap, and the ten-minute allowance only narrows it further.
#[test]
fn the_narration_buffer_never_grows_past_the_wav_cap() {
    let cap = super::narration_sample_cap();
    assert_eq!(super::narration_room(8_000, 0), 8_000 * 600, "a short clip is limited by ten minutes");
    assert_eq!(super::narration_room(192_000, 0), cap, "a long clip is limited by the WAV cap");
    assert_eq!(super::narration_room(192_000, cap - 10), 10, "what the clip already holds is subtracted");
    assert_eq!(super::narration_room(192_000, cap + 5), 0, "and never wraps");
}

/// A take that runs past the WAV limit keeps its head and reports the cut, so `save_take` cannot
/// call it complete.
#[test]
fn a_narration_cut_at_the_byte_limit_reports_the_cut() {
    let rate = 8_000;
    let tone: Vec<f32> = (0..rate).map(|i| (i as f32 * 0.5).sin() * 0.4).collect();
    // A 44-byte header plus a quarter second of mono 16-bit samples.
    let limit = 44 + 2 * rate as usize / 4;
    let cut = super::narration_wav_within(rate, &tone, limit).expect("a wav");
    assert!(cut.cut, "the tail past the limit is reported");
    assert!(cut.wav.len() < rate as usize * 2, "only the head is encoded: {} bytes", cut.wav.len());
    let full = super::narration_wav_within(rate, &tone, usize::MAX / 4).expect("a wav");
    assert!(!full.cut, "a take under the limit is complete");
    assert!(full.wav.len() > cut.wav.len());
}

/// Two advances queued before either is applied time each slide the take leaves, not the one the
/// timeline cache happens to name.
#[test]
fn queued_slide_changes_time_the_slide_each_one_leaves() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    show.begin_recording(0.0);
    show.apply(ShowAction::Slide(1), &doc, 2.0);
    show.apply(ShowAction::Slide(2), &doc, 3.0);
    let timed: Vec<usize> = show.timings.iter().map(|(i, _)| *i).collect();
    assert!(timed.contains(&0) && timed.contains(&1), "slide 0 and slide 1 are both timed: {timed:?}");
    assert_ne!(timed, vec![1, 2], "the destination of each advance is not the one that was left");
}

/// A slide opened while the take is held starts its clock at resume: shifting its slide-entry
/// time by the pause would undercount it, or make the timing negative after a long hold.
#[test]
fn a_slide_opened_during_a_pause_is_timed_from_resume() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    show.begin_recording(0.0);
    show.pause_recording(5.0);
    // Held on slide 0, the presenter moves to slide 1 (the shell applies the change itself).
    show.state.slide = 1;
    show.slide_start = 6.0;
    show.resume_recording(20.0);
    assert!((show.slide_start - 20.0).abs() < 1e-9, "slide 1's clock starts at resume, got {}", show.slide_start);
    assert!((show.step_start - 20.0).abs() < 1e-9);
    // Staying on the slide through a pause shifts its clock, so the pause is not counted.
    show.pause_recording(25.0);
    show.resume_recording(30.0);
    assert!((30.0 - show.slide_start - 5.0).abs() < 1e-9, "the pause is not added to the slide's time");
}

/// The red Record button starts a new take: earlier browsing and a previous take's buffers go,
/// so one save never mixes two takes.
#[test]
fn a_new_take_replaces_the_previous_buffers() {
    let doc = deckcraft_model::Presentation::default();
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    show.begin_recording(0.0);
    show.strokes.push(InkStroke {
        points: vec![pos2(0.1, 0.1), pos2(0.2, 0.2)],
        mode: InkMode::Pen,
        color: deckcraft_model::Rgba::BLACK,
        width: 2.25,
        in_take: true,
    });
    show.stash_ink(&doc, 0, true, 1.0);
    show.laser_live.push((0.1, 0.1, 5));
    show.stash_laser(0, true);
    show.timings.push((0, 1.5));
    show.narration.push((0, 8_000, vec![1.0, 2.0, 3.0]));
    assert!(!show.recorded.is_empty() && !show.laser_saved.is_empty());
    show.end_recording();
    show.begin_recording(10.0);
    assert!(show.recorded.is_empty(), "ink from the last take goes");
    assert!(show.laser_saved.is_empty(), "laser from the last take goes");
    assert!(show.timings.is_empty(), "timings from the last take go");
    assert!(show.narration.is_empty(), "narration from the last take goes");
}

/// #1812: `show.preview` reaches the app as a preview that does not record, while `show.record`
/// still starts a recording show.
#[test]
fn the_preview_command_turns_on_the_preview_not_the_recorder() {
    let mut app = SlideApp::new(deckcraft_engine::Session::with_new(), crate::Services::default());
    app.session.execute("slide.new", &json!({})).unwrap();
    app.run("show.preview", json!({"from": 0})).unwrap();
    let show = app.show.as_ref().expect("the preview is up");
    assert!(show.recording_preview);
    assert!(!show.record, "a preview does not record");
    app.run("show.record", json!({"from": 0})).unwrap();
    let show = app.show.as_ref().expect("the recording show is up");
    assert!(show.record);
    assert!(!show.recording_preview, "a recording show is not a preview");
}

/// #1812: Esc still leaves a Record preview, and the editor keeps the slide it was on: a preview
/// never moves the editor's selection.
#[test]
fn escape_leaves_a_record_preview_and_keeps_the_editor_slide() {
    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    session.execute("slide.new", &json!({})).unwrap();
    let mut app = SlideApp::new(session, crate::Services::default());
    app.ui.presenter_view = false;
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1000.0, 700.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(3);
    // The editor's own slide: `slide.new` selects each new slide, so it starts on the last one.
    let editor_slide = harness.state().session.active().expect("document").selection.slide;
    harness.state_mut().start_show(2, false);
    let show = harness.state_mut().show.as_mut().expect("preview is up");
    show.recording_preview = true;
    show.record = false;
    harness.run_steps(2);
    harness.key_press(egui::Key::Escape);
    harness.run_steps(3);
    assert!(harness.state().show.is_none(), "Escape ends the preview");
    assert_eq!(harness.state().session.active().expect("document").selection.slide, editor_slide, "the editor stays on its own slide");
}

/// Ink tools: selecting the pen, highlighter or eraser sets the derived pointer flag and the
/// active state the recording toolbar reads; the arrow turns both off.
#[test]
fn the_ink_tools_drive_the_pen_flag_and_the_active_state() {
    let doc = deckcraft_model::Presentation::default();
    let mut show = Show::new(&doc, 0, false, false);
    assert_eq!(show.ink_mode(), InkMode::Arrow);
    assert!(!show.pen && !show.ink_active(), "the arrow leaves no ink");
    show.set_ink_mode(InkMode::Pen);
    assert!(show.pen && show.ink_active());
    show.set_ink_mode(InkMode::Highlighter);
    assert!(show.pen && show.ink_active(), "the highlighter captures drags like the pen");
    show.set_ink_mode(InkMode::Eraser);
    assert!(!show.pen && !show.ink_active(), "the eraser removes ink instead of drawing");
    show.set_ink_mode(InkMode::Arrow);
    assert!(!show.pen && !show.ink_active());
}

/// Picking an ink colour keeps the selected tool; the toolbar's swatch is active for that colour.
#[test]
fn an_ink_colour_sticks_to_the_selected_tool() {
    let doc = deckcraft_model::Presentation::default();
    let mut show = Show::new(&doc, 0, false, false);
    show.set_ink_mode(InkMode::Highlighter);
    let blue = deckcraft_model::Rgba::rgb(0x2E, 0x6F, 0xD8);
    show.set_ink_color(blue);
    assert_eq!(show.ink_color(), blue);
    assert_eq!(show.ink_mode(), InkMode::Highlighter, "the colour does not switch the tool");
}

/// The eraser removes whole strokes it touches and leaves the rest alone.
#[test]
fn the_eraser_removes_touched_strokes() {
    let doc = deckcraft_model::Presentation::default();
    let mut show = Show::new(&doc, 0, false, false);
    let stroke = |pts: &[(f32, f32)]| InkStroke {
        points: pts.iter().map(|(x, y)| pos2(*x, *y)).collect(),
        mode: InkMode::Pen,
        color: deckcraft_model::Rgba::BLACK,
        width: 2.25,
        in_take: true,
    };
    show.strokes.push(stroke(&[(0.1, 0.1), (0.2, 0.1)]));
    show.strokes.push(stroke(&[(0.8, 0.8), (0.9, 0.8)]));
    assert_eq!(show.stroke_count(), 2);
    assert!(show.has_ink());
    assert_eq!(show.erase_ink_at(pos2(0.15, 0.5), pos2(0.15, 0.11), 0.02), 1, "only the touched stroke goes");
    assert_eq!(show.stroke_count(), 1);
    assert_eq!(show.erase_ink_at(pos2(0.0, 0.0), pos2(1.0, 1.0), 0.02), 1, "the diagonal crosses the remaining stroke");
    assert!(!show.has_ink() && show.stroke_count() == 0);
}

/// A stroke is erased by its segment between two pointer samples, not only by a sample that
/// lands exactly on a vertex.
#[test]
fn the_eraser_catches_a_segment_that_crosses_the_stroke() {
    let doc = deckcraft_model::Presentation::default();
    let mut show = Show::new(&doc, 0, false, false);
    show.strokes.push(InkStroke {
        points: vec![pos2(0.1, 0.5), pos2(0.9, 0.5)],
        mode: InkMode::Pen,
        color: deckcraft_model::Rgba::BLACK,
        width: 2.25,
        in_take: true,
    });
    assert_eq!(show.erase_ink_at(pos2(0.5, 0.4), pos2(0.5, 0.6), 0.02), 1);
    assert!(!show.has_ink());
}

/// A recorded highlighter stroke keeps its tool, colour and width, so the save path writes a
/// translucent highlighter rather than the deck's plain pen colour.
#[test]
fn a_recorded_stroke_keeps_its_tool_colour_and_width() {
    let doc = deckcraft_model::Presentation::default();
    let mut show = Show::new(&doc, 0, false, false);
    show.record = true;
    show.begin_recording(0.0);
    let color = deckcraft_model::Rgba::rgb(0xFF, 0xD4, 0x00);
    show.strokes.push(InkStroke { points: vec![pos2(0.1, 0.1), pos2(0.2, 0.2)], mode: InkMode::Highlighter, color, width: 6.75, in_take: true });
    show.stash_ink(&doc, 2, true, 1.0);
    assert_eq!(show.recorded.len(), 1);
    let (slide, pts, mode, saved, width, at) = &show.recorded[0];
    assert_eq!(*slide, 2);
    assert_eq!(pts.len(), 2);
    assert_eq!(*mode, InkMode::Highlighter);
    assert_eq!(*saved, color);
    assert!((*width - 6.75).abs() < 1e-9);
    assert_eq!(*at, 1000, "the stroke keeps the clock it was drawn on");
}
