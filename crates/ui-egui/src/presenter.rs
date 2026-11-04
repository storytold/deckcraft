//! Bounded presenter-window messages. The audience owns the show; the remote window
//! receives previews and sends input, never a second independently advancing show.
use serde::{Deserialize, Serialize};

pub const MAX_PREVIEW_SIDE: usize = 640;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}
impl Preview {
    pub fn from_image(image: &egui::ColorImage) -> Option<Self> {
        let [w, h] = image.size;
        if w == 0 || h == 0 || w.checked_mul(h)? != image.pixels.len() {
            return None;
        }
        let scale = (MAX_PREVIEW_SIDE as f64 / w.max(h) as f64).min(1.0);
        let width = ((w as f64 * scale) as usize).max(1);
        let height = ((h as f64 * scale) as usize).max(1);
        let mut rgba = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let pixel = image.pixels.get((y * h / height) * w + x * w / width)?;
                rgba.extend_from_slice(&pixel.to_srgba_unmultiplied());
            }
        }
        Some(Self { width, height, rgba })
    }
    pub fn image(&self) -> Option<egui::ColorImage> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_PREVIEW_SIDE
            || self.height > MAX_PREVIEW_SIDE
            || self.width.checked_mul(self.height)?.checked_mul(4)? != self.rgba.len()
        {
            return None;
        }
        Some(egui::ColorImage::from_rgba_unmultiplied([self.width, self.height], &self.rgba))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub slide: usize,
    pub total: usize,
    pub ended: bool,
    pub notes: String,
    pub elapsed: String,
    pub paused: bool,
    pub notes_px: f32,
    pub blank: Option<String>,
    pub grid: bool,
    pub zoom_armed: bool,
    pub current: Option<Preview>,
    pub next: Option<Preview>,
}

/// Images acknowledged by a successful transport send, never merely attempted images.
#[derive(Default)]
pub struct DeltaCache {
    current: Option<Preview>,
    next: Option<Preview>,
    token: String,
}
impl DeltaCache {
    pub fn send<E>(
        &mut self,
        mut snapshot: Snapshot,
        token: &str,
        refresh: bool,
        send: impl FnOnce(Snapshot, bool, bool) -> Result<(), E>,
    ) -> Result<(), E> {
        let fresh = refresh || self.token != token;
        let current_changed = fresh || self.current != snapshot.current;
        let next_changed = fresh || self.next != snapshot.next;
        let current = snapshot.current.clone();
        let next = snapshot.next.clone();
        if !current_changed {
            snapshot.current = None;
        }
        if !next_changed {
            snapshot.next = None;
        }
        send(snapshot, current_changed, next_changed)?;
        self.current = current;
        self.next = next;
        self.token = token.to_owned();
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Command {
    Key { name: String, command: bool, shift: bool, alt: bool, ctrl: bool },
    Zoom { x: f32, y: f32 },
    End,
}
/// Implemented by the browser host. Native hosts retain the existing egui viewport.
pub trait Host {
    fn open(&mut self) -> Result<(), String>;
    fn is_open(&self) -> bool;
    fn commands(&mut self) -> Vec<Command>;
    fn update(&mut self, snapshot: Snapshot);
    fn close(&mut self);
}

/// The popup's independent egui renderer. It never advances a local show clock.
#[derive(Default)]
pub struct View {
    fonts: crate::uifonts::UiFonts,
    styled: bool,
    current: Option<egui::TextureHandle>,
    next: Option<egui::TextureHandle>,
    previous_current: Option<Preview>,
    previous_next: Option<Preview>,
}
impl View {
    /// Install the same interface and dynamic fallback fonts as SlideApp before layout.
    /// Font definitions take effect on the next pass; never forward input on a discarded pass.
    pub fn prepare(&mut self, ctx: &egui::Context, notes: &str) -> bool {
        if !self.styled {
            crate::theme::install_fonts_with(ctx, &self.fonts.faces);
            crate::theme::apply(ctx, &crate::theme::Tokens::dark());
            self.styled = true;
            ctx.request_discard("install presenter interface fonts before layout");
            return false;
        }
        if ctx.will_discard() {
            return false;
        }
        self.fonts.note(ctx, notes);
        if self.fonts.apply(ctx) {
            ctx.request_discard("install presenter note fallback fonts before layout");
            return false;
        }
        true
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) -> Vec<Command> {
        if !self.prepare(ui.ctx(), &snapshot.notes) {
            return Vec::new();
        }
        let mut commands = Vec::new();
        let keys = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|event| if let egui::Event::Key { key, pressed: true, modifiers, .. } = event { Some((*key, *modifiers)) } else { None })
                .collect::<Vec<_>>()
        });
        for (key, modifiers) in keys {
            // Consume once before widgets see Space/Enter, avoiding a second button activation.
            ui.input_mut(|i| i.consume_key(modifiers, key));
            commands.push(Command::Key {
                name: key.name().into(),
                command: modifiers.command,
                shift: modifiers.shift,
                alt: modifiers.alt,
                ctrl: modifiers.ctrl,
            });
        }
        fn update(
            ctx: &egui::Context,
            name: &str,
            previous: &mut Option<Preview>,
            texture: &mut Option<egui::TextureHandle>,
            image: &Option<Preview>,
        ) {
            if previous == image {
                return;
            }
            *previous = image.clone();
            *texture = image.as_ref().and_then(Preview::image).map(|image| ctx.load_texture(name, image, egui::TextureOptions::LINEAR));
        }
        update(ui.ctx(), "remote-current", &mut self.previous_current, &mut self.current, &snapshot.current);
        update(ui.ctx(), "remote-next", &mut self.previous_next, &mut self.next, &snapshot.next);
        let key = |name: &str| Command::Key { name: name.into(), command: false, shift: false, alt: false, ctrl: false };
        ui.horizontal_wrapped(|ui| {
            ui.heading(&snapshot.elapsed);
            ui.label(format!("Slide {} of {}", snapshot.slide.saturating_add(1), snapshot.total));
            for (label, name) in [("◀ Previous", "ArrowLeft"), ("Next ▶", "ArrowRight")] {
                if ui.button(label).clicked() {
                    commands.push(key(name));
                }
            }
            if ui.button("End Show").clicked() {
                commands.push(Command::End);
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (label, name) in [
                (if snapshot.paused { "Resume" } else { "Pause" }, "T"),
                ("Reset", "R"),
                ("Black", "B"),
                ("White", "W"),
                ("All slides", "G"),
                ("Zoom", "Z"),
            ] {
                if ui.button(label).clicked() {
                    commands.push(key(name));
                }
            }
        });
        ui.separator();
        ui.columns(2, |columns| {
            let left = &mut columns[0];
            if snapshot.ended {
                left.heading("End of slide show");
            } else if let Some(texture) = &self.current {
                let response = left.add(
                    egui::Image::new(texture)
                        .fit_to_exact_size(fit_preview(texture.size_vec2(), left.available_width(), left.available_height() * 0.8))
                        .sense(egui::Sense::click()),
                );
                if let Some(blank) = &snapshot.blank {
                    let color = if blank == "white" { egui::Color32::WHITE } else { egui::Color32::BLACK };
                    left.painter().rect_filled(response.rect, 0.0, color);
                }
                if snapshot.zoom_armed
                    && response.clicked()
                    && let Some(point) = response.interact_pointer_pos()
                {
                    commands.push(Command::Zoom {
                        x: (point.x - response.rect.left()) / response.rect.width().max(1.0),
                        y: (point.y - response.rect.top()) / response.rect.height().max(1.0),
                    });
                }
            }
            if snapshot.zoom_armed {
                left.label("Click the preview to zoom.");
            }
            if snapshot.grid {
                left.label("Choose a slide in the audience window, or press G to return.");
            }
            let right = &mut columns[1];
            right.label("Next");
            if let Some(texture) = &self.next {
                right.add(egui::Image::new(texture).fit_to_exact_size(fit_preview(
                    texture.size_vec2(),
                    right.available_width(),
                    right.available_height() * 0.45,
                )));
            } else {
                right.label("End of slide show");
            }
            right.horizontal(|ui| {
                ui.label("Notes");
                if ui.button("A−").clicked() {
                    commands.push(key(egui::Key::OpenBracket.name()));
                }
                if ui.button("A+").clicked() {
                    commands.push(key(egui::Key::CloseBracket.name()));
                }
            });
            egui::ScrollArea::vertical().show(right, |ui| {
                ui.label(
                    egui::RichText::new(if snapshot.notes.is_empty() { "No notes." } else { &snapshot.notes })
                        .size(snapshot.notes_px.clamp(10.0, 72.0)),
                );
            });
        });
        commands
    }
}
fn fit_preview(size: egui::Vec2, width: f32, height: f32) -> egui::Vec2 {
    size * (width.max(1.0) / size.x.max(1.0)).min(height.max(1.0) / size.y.max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot {
            slide: 0,
            total: 2,
            ended: false,
            notes: String::new(),
            elapsed: "00:00:00".into(),
            paused: false,
            notes_px: 18.0,
            blank: None,
            grid: false,
            zoom_armed: false,
            current: Some(Preview { width: 1, height: 1, rgba: vec![255; 4] }),
            next: Some(Preview { width: 1, height: 1, rgba: vec![0; 4] }),
        }
    }

    #[test]
    fn failed_transport_retries_images_and_only_success_deduplicates() {
        let mut cache = DeltaCache::default();
        let first = snapshot();
        let failed = cache.send(first.clone(), "session", false, |delta, current, next| {
            assert!(current && next);
            assert_eq!(delta.current, first.current);
            assert_eq!(delta.next, first.next);
            Err("serialization or postMessage failed")
        });
        assert!(failed.is_err());
        let sent = cache.send(first.clone(), "session", false, |delta, current, next| {
            assert!(current && next, "a failed initial send must not suppress either image");
            assert_eq!(delta.current, first.current);
            assert_eq!(delta.next, first.next);
            Ok::<_, ()>(())
        });
        assert!(sent.is_ok());
        let sent = cache.send(first.clone(), "session", false, |delta, current, next| {
            assert!(!current && !next);
            assert!(delta.current.is_none() && delta.next.is_none());
            Ok::<_, ()>(())
        });
        assert!(sent.is_ok());
        let mut changed = first.clone();
        changed.current.as_mut().unwrap().rgba = vec![42; 4];
        let failed = cache.send(changed.clone(), "session", false, |_, current, next| {
            assert!(current && !next);
            Err(())
        });
        assert!(failed.is_err());
        let sent = cache.send(changed.clone(), "session", false, |delta, current, next| {
            assert!(current && !next, "unchanged retry after a failed changed-image send");
            assert_eq!(delta.current, changed.current);
            Ok::<_, ()>(())
        });
        assert!(sent.is_ok());
    }

    #[test]
    fn transport_refreshes_new_sessions_reloads_and_clears_previews() {
        let mut cache = DeltaCache::default();
        for (token, refresh) in [("first", false), ("second", false), ("second", true)] {
            let sent = cache.send(snapshot(), token, refresh, |delta, current, next| {
                assert!(current && next);
                assert!(delta.current.is_some() && delta.next.is_some());
                Ok::<_, ()>(())
            });
            assert!(sent.is_ok());
        }
        let failed = cache.send(snapshot(), "second", true, |_, current, next| {
            assert!(current && next);
            Err(())
        });
        // BrowserHost retains refresh_images after failure, even when the token is unchanged.
        let refresh_images = failed.is_err();
        let sent = cache.send(snapshot(), "second", refresh_images, |_, current, next| {
            assert!(current && next, "reload refresh must survive a failed send");
            Ok::<_, ()>(())
        });
        assert!(sent.is_ok());
        let mut cleared = snapshot();
        cleared.current = None;
        cleared.next = None;
        let failed = cache.send(cleared.clone(), "second", false, |_, current, next| {
            assert!(current && next);
            Err(())
        });
        assert!(failed.is_err());
        let sent = cache.send(cleared.clone(), "second", false, |delta, current, next| {
            assert!(current && next, "clearing images is retried too");
            assert!(delta.current.is_none() && delta.next.is_none());
            Ok::<_, ()>(())
        });
        assert!(sent.is_ok());
        let sent = cache.send(cleared, "second", false, |_, current, next| {
            assert!(!current && !next);
            Ok::<_, ()>(())
        });
        assert!(sent.is_ok());
    }

    #[test]
    fn popup_installs_interface_fonts_before_non_latin_note_layout() {
        let ctx = egui::Context::default();
        let mut view = View::default();
        let mut data = snapshot();
        let arabic = "\u{645}\u{631}\u{62d}\u{628}\u{627}";
        let japanese = "\u{65e5}\u{672c}\u{8a9e}";
        let korean = "\u{d55c}\u{ae00}";
        data.notes = format!("{arabic} {japanese} {korean}");
        let mut installing = 0;
        let mut layouts = 0;
        for _ in 0..4 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                if !view.prepare(ui.ctx(), &data.notes) {
                    installing += 1;
                    assert!(view.ui(ui, &data).is_empty(), "no commands during a discarded font-install pass");
                    return;
                }
                assert!(!ui.ctx().will_discard());
                layouts += 1;
                for (family, text) in [("Noto Sans Arabic", arabic), ("BIZ UDPGothic", japanese), ("NanumGothic", korean)] {
                    if deckcraft_fonts::CRAFT_FONTS.iter().any(|face| face.family == family) {
                        ui.ctx().fonts_mut(|fonts| {
                            for c in text.chars() {
                                assert!(fonts.has_glyph(&egui::FontId::proportional(18.0), c), "missing bundled {family} glyph {c:?}");
                            }
                        });
                    }
                }
                let _ = view.ui(ui, &data);
            });
            output.textures_delta.clear();
        }
        assert!(installing > 0 && layouts > 0, "font installation must precede an actual notes layout");
    }
    #[test]
    fn popup_input_is_forwarded_once_without_advancing_its_own_state() {
        use egui_kittest::kittest::Queryable;
        let snapshot = Snapshot {
            slide: 0,
            total: 3,
            ended: false,
            notes: "Synthetic notes".into(),
            elapsed: "00:00:00".into(),
            paused: false,
            notes_px: 18.0,
            blank: None,
            grid: false,
            zoom_armed: false,
            current: None,
            next: None,
        };
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1000.0, 700.0)).build_ui_state(
            |ui, (view, snapshot, sent): &mut (View, Snapshot, Vec<Command>)| {
                sent.extend(view.ui(ui, snapshot));
            },
            (View::default(), snapshot, Vec::new()),
        );
        h.run_steps(2);
        h.get_by_label("Next ▶").click();
        h.run_steps(2);
        assert_eq!(h.state().2.len(), 1);
        h.key_press(egui::Key::Space);
        h.run_steps(2);
        assert_eq!(h.state().2.len(), 2, "Space must not also activate the focused Next button");
        assert_eq!(h.state().0.previous_current, None);
        assert_eq!(h.state().1.slide, 0, "only an audience snapshot changes the displayed position");
        assert!(matches!(h.state().2.last(), Some(Command::Key { name, .. }) if name == egui::Key::Space.name()));
    }

    #[test]
    fn previews_are_bounded_and_bad_dimensions_are_rejected() {
        let image = egui::ColorImage::filled([1280, 720], egui::Color32::RED);
        let p = Preview::from_image(&image).unwrap();
        assert_eq!((p.width, p.height), (640, 360));
        assert_eq!(p.image().unwrap().pixels[0], egui::Color32::RED);
        for (width, height, rgba) in [(usize::MAX, 1, vec![]), (0, 1, vec![]), (1, 1, vec![0; 3]), (641, 1, vec![0; 2564])] {
            assert!(Preview { width, height, rgba }.image().is_none());
        }
    }
}
