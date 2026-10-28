/// Seconds for a Trim Media field. Three decimal places, trailing zeros dropped, so the text
/// round-trips the millisecond value the clip stores.
pub(super) fn ms_as_seconds(ms: u32) -> String {
    let s = format!("{:.3}", f64::from(ms) / 1000.0);
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() { "0".to_string() } else { trimmed.to_string() }
}

/// Parse a seconds field back to milliseconds. Blank, junk and negatives become 0; the result is
/// rounded so `0.333` is 333 ms, not 332.
pub(super) fn seconds_text_to_ms(text: &str) -> u32 {
    let v = text.trim().parse::<f64>().ok().filter(|v| v.is_finite()).unwrap_or(0.0);
    let ms = (v.max(0.0) * 1000.0).round();
    if ms >= f64::from(u32::MAX) { u32::MAX } else { ms as u32 }
}

/// Start, end trim, fade in and fade out, as the Trim Media dialog shows them.
pub(super) struct TrimSeed {
    pub(super) start: String,
    pub(super) end: String,
    pub(super) fade_in: String,
    pub(super) fade_out: String,
}

impl TrimSeed {
    fn zeros() -> Self {
        TrimSeed { start: "0".into(), end: "0".into(), fade_in: "0".into(), fade_out: "0".into() }
    }
    fn from_clip(m: &deckcraft_model::MediaClip) -> Self {
        TrimSeed {
            start: ms_as_seconds(m.trim_start_ms),
            end: ms_as_seconds(m.trim_end_ms),
            fade_in: ms_as_seconds(m.fade_in_ms),
            fade_out: ms_as_seconds(m.fade_out_ms),
        }
    }
}

/// The first selected audio or video. Other shapes in the selection are skipped. No media means
/// the old zeros, which OK then stores.
pub(super) fn trim_seed(st: Option<&deckcraft_engine::DocState>) -> TrimSeed {
    let Some(st) = st else { return TrimSeed::zeros() };
    st.selected_shapes()
        .into_iter()
        .find_map(|sh| match &sh.kind {
            deckcraft_model::ShapeKind::Media(m) => Some(TrimSeed::from_clip(m)),
            _ => None,
        })
        .unwrap_or_else(TrimSeed::zeros)
}

#[cfg(test)]
mod trim_media_tests {
    use std::sync::Arc;

    use deckcraft_model::{MediaClip, Presentation, ShapeId, ShapeKind};

    use super::{ms_as_seconds, seconds_text_to_ms, trim_seed};

    #[test]
    fn trim_seconds_round_trip_the_stored_milliseconds() {
        for ms in [0, 1, 250, 333, 1000, 1500, 60_000] {
            let text = ms_as_seconds(ms);
            assert_eq!(seconds_text_to_ms(&text), ms, "{text}");
        }
        assert_eq!(seconds_text_to_ms(""), 0);
        assert_eq!(seconds_text_to_ms("nope"), 0);
        assert_eq!(seconds_text_to_ms("-2"), 0);
        assert_eq!(seconds_text_to_ms("1.5"), 1500);
        assert_eq!(seconds_text_to_ms("999999999"), u32::MAX);
    }

    #[test]
    fn trim_dialog_reads_the_selected_clip() {
        assert_eq!(trim_seed(None).start, "0");
        let mut doc = Presentation::default();
        let media_id = {
            let slide = Arc::make_mut(&mut doc.slides[0]);
            let title = slide.shapes[0].id;
            let clip = MediaClip { trim_start_ms: 1500, trim_end_ms: 250, fade_in_ms: 333, fade_out_ms: 1000, ..Default::default() };
            let mut media = slide.shapes[0].clone();
            media.id = ShapeId(900);
            media.kind = ShapeKind::Media(clip);
            slide.shapes.push(media);
            (title, ShapeId(900))
        };
        let mut st = deckcraft_engine::DocState::new(doc, None, "t".into());
        st.selection.shapes = vec![media_id.0];
        assert_eq!(trim_seed(Some(&st)).start, "0", "a title shape is not a clip");
        st.selection.shapes = vec![media_id.0, media_id.1];
        let seed = trim_seed(Some(&st));
        assert_eq!(seed.start, "1.5");
        assert_eq!(seed.end, "0.25");
        assert_eq!(seed.fade_in, "0.333");
        assert_eq!(seed.fade_out, "1");
    }
}
