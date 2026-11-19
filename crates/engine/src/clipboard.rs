//! Versioned, self-contained system clipboard payload. MediaItem's regular JSON deliberately
//! omits bytes; carry those explicitly, without temporary files or paths into the source app.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::Clipboard;
use crate::cmd::{base64_decode, base64_encode};

/// Bound clipboard allocations independently of the much larger document-file budget.
pub const MAX_BYTES: usize = 64 << 20;
/// Another process wrote the offer, so its counts are untrusted too (#1510's caps, sized for
/// selections: far above a real copy, far below what pastes or renders slowly).
const MAX_SLIDES: usize = 10_000;
const MAX_SHAPES: usize = 100_000;
const MAX_MEDIA: usize = 10_000;

#[derive(Serialize, Deserialize)]
struct Payload {
    version: u32,
    clipboard: Clipboard,
    media: Vec<String>,
}

impl Clipboard {
    /// Adopt a system offer, keeping local state when it is the same selection.
    pub fn adopt_system_bytes(&mut self, bytes: &[u8]) -> bool {
        if self.system_bytes.is_none() {
            self.system_bytes = self.to_system_bytes();
        }
        if self.system_bytes.as_deref() == Some(bytes) {
            return true;
        }
        let Some(mut clipboard) = Self::from_system_bytes(bytes) else { return false };
        clipboard.system_bytes = Some(bytes.to_vec());
        *self = clipboard;
        true
    }

    /// Encode only this selection and its dependencies. Paste counters are process-local.
    pub fn to_system_bytes(&self) -> Option<Vec<u8>> {
        if self.shapes.is_empty() && self.slides.is_empty() && self.text.is_none() {
            return None;
        }
        let size = self.media.iter().try_fold(0usize, |n, m| n.checked_add(m.data.len()))?;
        if size > MAX_BYTES / 2 {
            return None;
        }
        let payload = Payload { version: 1, clipboard: self.clone(), media: self.media.iter().map(|m| base64_encode(&m.data)).collect() };
        let bytes = serde_json::to_vec(&payload).ok()?;
        (bytes.len() <= MAX_BYTES).then_some(bytes)
    }

    /// Unknown versions, malformed JSON/base64, incomplete media and oversized offers fall back to
    /// normal paste. What is accepted gets the numeric bounds a loaded file gets.
    pub fn from_system_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_BYTES {
            return None;
        }
        // Read the version alone first: a newer layout is ignored whatever its shape.
        #[derive(Deserialize)]
        struct Version {
            version: u32,
        }
        if serde_json::from_slice::<Version>(bytes).ok()?.version != 1 {
            return None;
        }
        let mut payload: Payload = serde_json::from_slice(bytes).ok()?;
        if payload.version != 1 || payload.media.len() != payload.clipboard.media.len() || !within_counts(&payload.clipboard) {
            return None;
        }
        for (item, data) in payload.clipboard.media.iter_mut().zip(payload.media) {
            item.data = Arc::new(base64_decode(&data)?);
        }
        if payload.clipboard.shapes.is_empty() && payload.clipboard.slides.is_empty() && payload.clipboard.text.is_none() {
            return None;
        }
        sane_numbers(&mut payload.clipboard);
        payload.clipboard.foreign = true;
        payload.clipboard.pastes = 0;
        Some(payload.clipboard)
    }
}

/// Slides, shapes (group members included) and media within the caps above.
fn within_counts(clip: &Clipboard) -> bool {
    let mut shapes = 0usize;
    deckcraft_model::walk(&clip.shapes, &mut |_, _| shapes += 1);
    for slide in &clip.slides {
        deckcraft_model::walk(&slide.shapes, &mut |_, _| shapes += 1);
    }
    clip.slides.len() <= MAX_SLIDES && shapes <= MAX_SHAPES && clip.media.len() <= MAX_MEDIA
}

/// Clamp positions, sizes, line widths, font sizes and effect radii exactly as file loading does,
/// by lending the shapes, slides and text to a scratch deck for `deckcraft_format::sane_numbers`.
fn sane_numbers(clip: &mut Clipboard) {
    let shapes = deckcraft_model::Slide { shapes: std::mem::take(&mut clip.shapes), ..Default::default() };
    let text = deckcraft_model::Shape { text: clip.text.take(), ..Default::default() };
    let text = deckcraft_model::Slide { shapes: vec![text], ..Default::default() };
    let slides = [shapes, text].into_iter().chain(clip.slides.drain(..)).map(Arc::new).collect();
    let mut deck = deckcraft_model::Presentation { slides, ..Default::default() };
    deckcraft_format::sane_numbers(&mut deck);
    let mut slides = deck.slides.into_iter().map(Arc::unwrap_or_clone);
    clip.shapes = slides.next().map(|s| s.shapes).unwrap_or_default();
    clip.text = slides.next().and_then(|s| s.shapes.into_iter().next()).and_then(|s| s.text);
    clip.slides = slides.collect();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::Session;
    use serde_json::json;

    #[test]
    fn system_clipboard_round_trip_with_media_and_collision() {
        let mut src = Session::with_new();
        // A contributor-original one-pixel PNG, generated by the image crate.
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([60, 120, 180, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let png = png.into_inner();
        src.execute("insert.picture", &json!({"name": "pixel.png", "data": base64_encode(&png)})).unwrap();
        src.execute("edit.copy", &json!({})).unwrap();
        let bytes = src.clipboard.to_system_bytes().unwrap();
        let mut dst = Session::with_new();
        let id = src.clipboard.media[0].id;
        dst.edit(|doc, _| {
            doc.media.push(crate::model::MediaItem { id, data: Arc::new(vec![9]), ..Default::default() });
            doc.next_id = doc.next_id.max(id.0);
            Ok(json!({}))
        })
        .unwrap();
        dst.execute("edit.paste", &json!({"clipboard": base64_encode(&bytes)})).unwrap();
        let st = dst.active().unwrap();
        assert!(st.doc.media.iter().any(|m| *m.data == png && m.id != id));
        assert_eq!(st.doc.media(id).unwrap().data.as_ref(), &[9]);
        let pasted = st.selection.shapes.clone();
        assert_eq!(pasted.len(), 1);
        dst.execute("edit.undo", &json!({})).unwrap();
        assert!(!dst.active().unwrap().doc.slides[0].shapes.iter().any(|s| pasted.contains(&s.id)));
        dst.execute("edit.redo", &json!({})).unwrap();
        assert!(dst.active().unwrap().doc.slides[0].shapes.iter().any(|s| pasted.contains(&s.id)));
    }

    #[test]
    fn system_clipboard_colliding_shape_ids_keep_position_including_paste_as_picture() {
        for picture in [false, true] {
            for foreign in [false, true] {
                let mut src = Session::with_new();
                src.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 120, 200, 80]})).unwrap();
                src.execute("edit.copy", &json!({})).unwrap();
                let mut dst = Session::with_new();
                dst.execute("shape.insert", &json!({"preset": "rect", "rect": [300, 320, 40, 40]})).unwrap();
                let source_id = src.clipboard.shapes[0].id;
                assert!(dst.doc().unwrap().shape(source_id).is_some());
                let bytes = src.clipboard.to_system_bytes().unwrap();
                dst.clipboard = if foreign { Clipboard::from_system_bytes(&bytes).unwrap() } else { src.clipboard.clone() };
                let off = if foreign { 0.0 } else { dst.prefs.duplicate_offset };
                if picture {
                    dst.execute("edit.pasteSpecial", &json!({"as": "picture"})).unwrap();
                } else {
                    let params = if foreign { json!({"clipboard": base64_encode(&bytes)}) } else { json!({}) };
                    dst.execute("edit.paste", &params).unwrap();
                }
                let st = dst.doc().unwrap();
                let pasted = st.shape(st.selection.shapes[0]).unwrap();
                assert_ne!(pasted.id, source_id);
                let x = pasted.xfrm.unwrap();
                // Picture rendering includes an 8 pt margin around the original shape.
                let margin = if picture { 8.0 } else { 0.0 };
                assert!((x.x - (100.0 - margin + off)).abs() < 0.01, "picture={picture}, foreign={foreign}: {x:?}");
                assert!((x.y - (120.0 - margin + off)).abs() < 0.01, "picture={picture}, foreign={foreign}: {x:?}");
            }
        }
    }

    #[test]
    fn system_clipboard_slides_reuse_destination_layout_without_importing_masters() {
        let mut src = Session::with_new();
        src.execute("edit.copy", &json!({"scope": "slides"})).unwrap();
        let bytes = src.clipboard.to_system_bytes().unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(payload["clipboard"].get("masters").is_none());
        let (name, kind) = src.clipboard.slide_layouts[0].clone();
        let mut dst = Session::with_new();
        // Put a name match after a kind match, with a different id and kind from the source.
        let target = dst
            .edit(|doc, _| {
                let id = deckcraft_model::LayoutId(doc.alloc_id().unwrap());
                let master = Arc::make_mut(&mut doc.masters[0]);
                for layout in &mut master.layouts {
                    layout.name = "Different".into();
                    layout.kind = kind;
                }
                let mut layout = master.layouts[0].clone();
                layout.id = id;
                layout.name = name.to_ascii_uppercase();
                layout.kind = deckcraft_model::LayoutType::Custom;
                master.layouts.push(layout);
                Ok(json!(id))
            })
            .unwrap();
        let target: deckcraft_model::LayoutId = serde_json::from_value(target).unwrap();
        let masters = dst.active().unwrap().doc.masters.len();
        for count in 2..=3 {
            dst.execute("edit.paste", &json!({"clipboard": base64_encode(&bytes), "text": "unrelated fallback"})).unwrap();
            let st = dst.active().unwrap();
            assert_eq!(st.doc.slides.len(), count);
            assert_eq!(st.doc.masters.len(), masters);
            assert_eq!(st.doc.slides[count - 1].layout, target);
            assert_ne!(st.doc.slides[0].id, st.doc.slides[count - 1].id);
        }
    }

    #[test]
    fn system_clipboard_rejects_unknown_and_incomplete_payloads() {
        assert!(Clipboard::from_system_bytes(b"not json").is_none());
        let mut s = Session::with_new();
        s.execute("edit.copy", &json!({"scope": "slides"})).unwrap();
        let bytes = s.clipboard.to_system_bytes().unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        v["version"] = json!(2);
        assert!(Clipboard::from_system_bytes(&serde_json::to_vec(&v).unwrap()).is_none());
        v["version"] = json!(1);
        v["media"] = json!(["!!!"]);
        assert!(Clipboard::from_system_bytes(&serde_json::to_vec(&v).unwrap()).is_none());
        assert!(Clipboard::from_system_bytes(&vec![0; MAX_BYTES + 1]).is_none());
    }

    fn copied_rect() -> Vec<u8> {
        let mut s = Session::with_new();
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 120, 200, 80]})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        s.clipboard.to_system_bytes().unwrap()
    }

    #[test]
    fn system_clipboard_rejects_an_oversized_offer_before_parsing_it() {
        let bytes = copied_rect();
        assert!(Clipboard::from_system_bytes(&bytes).is_some());
        // The same valid offer, padded with JSON whitespace past the cap.
        let mut padded = bytes.clone();
        padded.resize(MAX_BYTES + 1, b' ');
        assert!(Clipboard::from_system_bytes(&padded).is_none());
        // Through the command: the oversized offer falls back to the plain text.
        assert_pastes_fallback(&base64_encode(&padded));
    }

    #[test]
    fn system_clipboard_garbage_is_rejected_without_panicking() {
        let bytes = copied_rect();
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for len in [0, 1, 4, 16, 255, 4096] {
            let noise: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            assert!(Clipboard::from_system_bytes(&noise).is_none());
        }
        for cut in (0..bytes.len()).step_by(7) {
            assert!(Clipboard::from_system_bytes(&bytes[..cut]).is_none(), "prefix {cut}");
        }
        for v in [json!(null), json!([]), json!({}), json!({"version": "1"}), json!({"version": -1}), json!({"version": 1})] {
            assert!(Clipboard::from_system_bytes(&serde_json::to_vec(&v).unwrap()).is_none(), "{v}");
        }
        assert!(Clipboard::from_system_bytes(&[b'['; 100_000]).is_none());
        // Corrupt single bytes: whatever still parses must paste without panicking.
        for i in (0..bytes.len()).step_by(11) {
            let mut bad = bytes.clone();
            bad[i] = next() as u8;
            if let Some(clip) = Clipboard::from_system_bytes(&bad) {
                let mut dst = Session::with_new();
                dst.clipboard = clip;
                let _ = dst.execute("edit.paste", &json!({}));
            }
        }
    }

    #[test]
    fn system_clipboard_ignores_a_newer_version_whatever_its_layout() {
        let bytes = copied_rect();
        let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        v["version"] = json!(2);
        v["extra"] = json!({"from": "a newer build"});
        let newer_same_layout = serde_json::to_vec(&v).unwrap();
        let newer_other_layout = serde_json::to_vec(&json!({"version": 2, "clipboard": "elsewhere", "media": {}})).unwrap();
        for newer in [newer_same_layout, newer_other_layout] {
            assert!(Clipboard::from_system_bytes(&newer).is_none());
            assert_pastes_fallback(&base64_encode(&newer));
        }
    }

    /// `edit.paste` with this `clipboard` param ignores it and pastes the plain-text alternative.
    fn assert_pastes_fallback(clipboard: &str) {
        let mut dst = Session::with_new();
        dst.execute("edit.paste", &json!({"clipboard": clipboard, "text": "fallback"})).unwrap();
        assert!(!dst.clipboard.foreign);
        let shapes = &dst.active().unwrap().doc.slides[0].shapes;
        assert!(shapes.iter().any(|s| s.text.as_ref().is_some_and(|t| t.text() == "fallback")));
        assert!(!shapes.iter().any(|s| s.xfrm.is_some_and(|x| (x.x, x.y) == (100.0, 120.0))));
    }

    #[test]
    fn system_clipboard_caps_counts() {
        let bytes = copied_rect();
        let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let mut many = v.clone();
        many["clipboard"]["shapes"] = json!(vec![json!({}); MAX_SHAPES + 1]);
        assert!(Clipboard::from_system_bytes(&serde_json::to_vec(&many).unwrap()).is_none());
        let mut many = v.clone();
        many["clipboard"]["slides"] = json!(vec![json!({}); MAX_SLIDES + 1]);
        assert!(Clipboard::from_system_bytes(&serde_json::to_vec(&many).unwrap()).is_none());
        v["clipboard"]["media"] = json!(vec![json!({}); MAX_MEDIA + 1]);
        v["media"] = json!(vec![json!(""); MAX_MEDIA + 1]);
        assert!(Clipboard::from_system_bytes(&serde_json::to_vec(&v).unwrap()).is_none());
    }

    #[test]
    fn system_clipboard_numbers_get_the_file_bounds() {
        let mut src = Session::with_new();
        src.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 120, 200, 80]})).unwrap();
        src.execute("edit.copy", &json!({})).unwrap();
        let sh = &mut src.clipboard.shapes[0];
        let x = sh.xfrm.as_mut().unwrap();
        (x.x, x.w) = (1e300, 1e300);
        sh.line = Some(deckcraft_model::Line { width: Some(1e18), ..Default::default() });
        src.clipboard.text = Some(serde_json::from_value(json!({"paragraphs": [{"runs": [{"text": "x", "props": {"size": 1e12}}]}]})).unwrap());
        let clip = Clipboard::from_system_bytes(&src.clipboard.to_system_bytes().unwrap()).unwrap();
        let x = clip.shapes[0].xfrm.unwrap();
        assert!(x.x <= 1e6 && x.w <= 1e6, "{x:?}");
        assert!(clip.shapes[0].line.as_ref().unwrap().width.unwrap() <= 1584.0);
        assert_eq!(clip.text.unwrap().paragraphs[0].runs[0].props.size, Some(4000.0));
        // An ordinary selection comes back unchanged.
        let plain = Clipboard::from_system_bytes(&copied_rect()).unwrap();
        assert_eq!(plain.shapes[0].xfrm.unwrap().x, 100.0);
    }
}

#[cfg(test)]
mod repeated_paste_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::Session;
    use serde_json::json;

    #[test]
    fn repeated_foreign_offer_offsets_after_first_paste_and_new_offer_resets() {
        let mut src = Session::with_new();
        src.execute("shape.insert", &json!({"preset":"rect", "rect":[100,120,200,80]})).unwrap();
        src.execute("edit.copy", &json!({})).unwrap();
        let bytes = src.clipboard.to_system_bytes().unwrap();
        let mut dst = Session::with_new();
        for (native, count) in [(true, 0), (true, 1), (false, 2)] {
            let params = if native { json!({"clipboard":base64_encode(&bytes)}) } else { json!({}) };
            dst.execute("edit.paste", &params).unwrap();
            let doc = dst.doc().unwrap();
            let x = doc.shape(doc.selection.shapes[0]).unwrap().xfrm.unwrap();
            let offset = dst.prefs.duplicate_offset * count as f64;
            assert_eq!((x.x, x.y), (100.0 + offset, 120.0 + offset));
        }
        src.execute("shape.insert", &json!({"preset":"rect", "rect":[200,220,200,80]})).unwrap();
        src.execute("edit.copy", &json!({})).unwrap();
        let bytes = src.clipboard.to_system_bytes().unwrap();
        dst.execute("edit.paste", &json!({"clipboard":base64_encode(&bytes)})).unwrap();
        let doc = dst.doc().unwrap();
        let x = doc.shape(doc.selection.shapes[0]).unwrap().xfrm.unwrap();
        assert_eq!((x.x, x.y), (200.0, 220.0));
    }

    /// As with shapes copied here: on a slide other than the last paste's, the position stays.
    #[test]
    fn foreign_offer_keeps_its_position_on_another_slide() {
        let mut src = Session::with_new();
        src.execute("shape.insert", &json!({"preset":"rect", "rect":[100,120,200,80]})).unwrap();
        src.execute("edit.copy", &json!({})).unwrap();
        let clipboard = base64_encode(&src.clipboard.to_system_bytes().unwrap());
        let mut dst = Session::with_new();
        dst.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let offset = dst.prefs.duplicate_offset;
        let steps = [(0, "paste", 0.0), (0, "paste", 1.0), (1, "paste", 0.0), (1, "keepSource", 1.0), (1, "picture", 1.0), (0, "paste", 0.0)];
        for (i, (slide, how, count)) in steps.into_iter().enumerate() {
            dst.execute("slide.go", &json!({"index": slide})).unwrap();
            // The picture is drawn with an 8 pt margin around the shape.
            let margin = match how {
                "paste" => {
                    dst.execute("edit.paste", &json!({"clipboard": clipboard})).unwrap();
                    0.0
                }
                "picture" => {
                    dst.execute("edit.pasteSpecial", &json!({"as": how})).unwrap();
                    8.0
                }
                _ => {
                    dst.execute("edit.pasteSpecial", &json!({"as": how})).unwrap();
                    0.0
                }
            };
            let doc = dst.doc().unwrap();
            let x = doc.shape(doc.selection.shapes[0]).unwrap().xfrm.unwrap();
            let want = (100.0 - margin + offset * count, 120.0 - margin + offset * count);
            assert!((x.x - want.0).abs() < 0.01 && (x.y - want.1).abs() < 0.01, "step {i}: {x:?}, want {want:?}");
        }
    }
}
