//! Audio Format and Video Format: the picture of a selected clip. An audio clip's picture is its
//! icon (Change Picture, Reset Picture); a video's is its poster frame. Playback options are on
//! the Playback tab.

use super::{Icon, ShapeKind, SlideApp, Ui, big, big_button, group, json, rows, run, small};

/// The first selected media clip, if any.
fn selected_clip(app: &SlideApp) -> Option<deckcraft_model::MediaClip> {
    app.session
        .active()
        .and_then(|d| d.selected_shapes().into_iter().find_map(|s| if let ShapeKind::Media(m) = &s.kind { Some(m.clone()) } else { None }))
}

pub(super) fn audio_format(app: &mut SlideApp, ui: &mut Ui) {
    // Adjust
    group(ui, |ui| {
        rows(ui, |ui| {
            small(app, ui, Icon::ChangePicture, "Change Picture", "app.changePictureDialog", json!({}));
            small(app, ui, Icon::ResetPicture, "Reset Picture", "picture.reset", json!({}));
        });
    });
}

pub(super) fn video_format(app: &mut SlideApp, ui: &mut Ui) {
    let clip = selected_clip(app);
    // Preview
    group(ui, |ui| {
        let playing = app.session.active().and_then(|d| d.selection.shapes.first().copied()).is_some_and(|id| app.media.is_playing(id));
        if big_button(ui, if playing { Icon::Pause } else { Icon::Play }, if playing { "Pause" } else { "Play" }, clip.is_some()).clicked() {
            run(app, "media.toggle", json!({}));
        }
    });
    // Adjust
    group(ui, |ui| {
        let r = big_button(ui, Icon::Video, "Poster Frame", clip.is_some());
        egui::Popup::menu(&r).show(|ui| {
            if ui.button("Current Frame").clicked() {
                run(app, "media.posterFrame", json!({}));
            }
            if ui.button("Image From File…").clicked() {
                run(app, "app.changePictureDialog", json!({}));
            }
            let has_poster = clip.as_ref().is_some_and(|c| c.poster.is_some());
            if ui.add_enabled(has_poster, egui::Button::new("Reset")).clicked() {
                run(app, "media.posterFrame", json!({"reset": true}));
            }
        });
        big(app, ui, Icon::ResetPicture, "Reset Design", "picture.reset", json!({}));
    });
}
