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

/// #1130: the Presenter View's All slides button opens the grid in that window, not on the audience screen.
#[test]
fn presenter_all_slides_does_not_cover_the_audience_screen() {
    use egui_kittest::kittest::Queryable;

    let mut session = deckcraft_engine::Session::with_new();
    session.execute("slide.new", &json!({})).unwrap();
    let doc = session.active().unwrap().doc.clone();
    let show = Show::new(&doc, 0, false, true);
    let mut harness = egui_kittest::Harness::builder().with_size(vec2(1100.0, 700.0)).build_ui_state(
        |ui, (doc, show, keep): &mut (std::sync::Arc<Presentation>, Show, bool)| {
            let now = ui.input(|i| i.time);
            presenter_window(ui.ctx(), show, doc, now, keep);
        },
        (doc, show, true),
    );
    harness.run_steps(2);
    harness.get_by_label("All slides").click();
    harness.run_steps(2);
    assert!(harness.state().1.presenter_grid, "the grid opens in the Presenter View");
    assert!(!harness.state().1.grid, "the audience screen keeps showing the slide");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(!harness.state().1.presenter_grid, "Esc closes the Presenter View grid");
    assert!(harness.state().2, "and does not end the show");
    harness.key_press(egui::Key::G);
    harness.run_steps(2);
    assert!(harness.state().1.presenter_grid, "G in Presenter View opens its own grid");
    assert!(!harness.state().1.grid, "G in Presenter View leaves the audience screen alone");
    harness.key_press(egui::Key::G);
    harness.run_steps(2);
    assert!(!harness.state().1.presenter_grid, "G again closes it");
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
        |ui, (doc, show, keep): &mut (std::sync::Arc<Presentation>, Show, bool)| {
            let now = ui.input(|i| i.time);
            presenter_window(ui.ctx(), show, doc, now, keep);
        },
        (doc, show, true),
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
        assert_eq!(harness.state().1.state.slide, slide, "presenter key {key:?}");
    }
    harness.key_press(egui::Key::ArrowRight);
    harness.run_steps(2);
    assert!(harness.state().1.ended);
    harness.key_press(egui::Key::ArrowLeft);
    harness.run_steps(2);
    assert!(!harness.state().1.ended);
    assert_eq!(harness.state().1.state.slide, 2, "back from the end screen restores the last slide");
    harness.key_press(egui::Key::ArrowRight);
    harness.run_steps(2);
    assert!(harness.state().1.ended);
    harness.get_by_label("◀ Previous").click();
    harness.run_steps(2);
    assert!(!harness.state().1.ended, "the presenter Previous button also leaves the end screen");
    harness.key_press(egui::Key::Home);
    harness.run_steps(2);
    harness.get_by_label("Next ▶").click();
    harness.run_steps(2);
    assert_eq!(harness.state().1.state.slide, 1);
    harness.key_press(egui::Key::Space);
    harness.run_steps(2);
    assert_eq!(harness.state().1.state.slide, 2);
    assert!(!harness.state().1.ended, "Space must not also activate the focused Next button");
    harness.key_press(egui::Key::Escape);
    harness.run_steps(2);
    assert!(!harness.state().2, "Escape from Presenter View ends the show");
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
    assert_eq!(p.key, Key { slide: 1, step: 0, size, doc: std::sync::Arc::as_ptr(&doc) as usize, hidden: vec![], gif: 0, plays: vec![] });
    let got = p.rx.recv().expect("the frame arrives");
    let want = render_frame(&doc, 1, ShowState::timeline(&doc, 1), 0, 0.0, size, vec![], &[], 0, &[], &[], None);
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
    let fast = render_frame(&doc, 0, tl.clone(), 0, 0.25, size, vec![], &[], 0, &[], &[], Some(&back));
    let full = render_frame(&doc, 0, tl, 0, 0.25, size, vec![], &[], 0, &[], &[], None);
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
    show.tex = Some((Key { slide: 0, step: 0, size: (1, 1), doc: 0, hidden: vec![], gif: 0, plays: vec![] }, red.clone()));
    assert!(show.apply(ShowAction::Slide(1), &doc, 0.0));
    let tr = show.trans.as_ref().expect("the fade plays");
    assert_eq!(tr.old.as_ref().map(TextureHandle::id), Some(red.id()));
    assert_eq!(tr.from, Some(0));
    assert!(show.tex.is_none(), "the new slide gets a texture of its own instead of overwriting the old one");
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
