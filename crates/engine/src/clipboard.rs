//! The internal clipboard carried on the system clipboard, so a copy in one DeckCraft window
//! pastes in another (a separate process), and so the CLI can move shapes between decks.
//!
//! A copy serialises [`Clipboard`] (shapes or slides, their media, the text, the source theme) to
//! a payload string: `v1:<token>:<base64 JSON>`. The UI appends it to the HTML it already puts on
//! the system clipboard, as an HTML comment (`<!--deckcraft-clipboard:v1:…-->`), and the plain
//! text alternative stays what it was: other apps see text and HTML as before. On paste the
//! payload is read back from the HTML (or from `edit.paste {payload}`) and becomes the session's
//! clipboard, unless its token says it is this session's own copy; the usual paste then runs
//! (fresh ids, media brought over, theme mapping).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use deckcraft_model::text::TextBody;
use deckcraft_model::{MediaId, MediaItem, Shape, Slide};
use serde::{Deserialize, Serialize};

use crate::cmd::{base64_decode, base64_encode};
use crate::{Clipboard, Session};

/// Largest payload carried (the `v1:…` string); a bigger copy stays process-local.
pub const MAX_PAYLOAD: usize = 64 << 20;
const VERSION: &str = "v1";
/// The HTML comment that holds the payload: `<!--deckcraft-clipboard:v1:…-->`.
const MARK_OPEN: &str = "<!--deckcraft-clipboard:";
const MARK_CLOSE: &str = "-->";

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Payload {
    shapes: Vec<Shape>,
    placeholders: Vec<Shape>,
    slides: Vec<Slide>,
    media: Vec<Media>,
    text: Option<TextBody>,
    plain: String,
    source_theme: Option<Theme>,
}

/// [`crate::SourceTheme`] as it travels: the theme's own colours, its colour map and its fonts.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Theme {
    colors: deckcraft_color::ColorScheme,
    color_map: Vec<(String, String)>,
    fonts: deckcraft_model::theme::FontScheme,
}

/// A [`MediaItem`] with its bytes (the model keeps them out of its JSON).
#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Media {
    id: MediaId,
    name: String,
    content_type: String,
    link: Option<String>,
    /// Base64.
    data: String,
}

/// A token no other copy, in this process or another, gets: this process's random id with a
/// counter in the low bits. Never 0 (a clipboard nothing was copied to).
pub fn fresh_token() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) & 0xFF_FFFF;
    (crate::recovery::instance_id() & !0xFF_FFFF) | n
}

impl Clipboard {
    /// An empty clipboard for a new copy, with a token of its own.
    pub fn fresh() -> Self {
        Self { token: fresh_token(), ..Self::default() }
    }

    /// Nothing copied: a paste of it would do nothing.
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty() && self.slides.is_empty() && self.text.is_none() && self.plain.is_empty()
    }
}

/// The payload for `clip`, or `None` when there is nothing to carry or it is over [`MAX_PAYLOAD`].
pub fn encode(clip: &Clipboard) -> Option<String> {
    if clip.is_empty() {
        return None;
    }
    // The media bytes are base64 twice (once in the JSON, once around it): 16/9 of their size,
    // so a copy that cannot fit is refused before anything is allocated (a large video would
    // otherwise be copied several times over on the UI thread, only to be thrown away).
    let media_bytes: usize = clip.media.iter().map(|m| m.data.len()).sum();
    if media_bytes.checked_mul(16).is_none_or(|v| v / 9 > MAX_PAYLOAD) {
        log::warn!(
            "the copy holds {} MiB of media, over the {} MiB clipboard payload limit: it stays in this window",
            media_bytes >> 20,
            MAX_PAYLOAD >> 20
        );
        return None;
    }
    let media = clip
        .media
        .iter()
        .map(|m| Media { id: m.id, name: m.name.clone(), content_type: m.content_type.clone(), link: m.link.clone(), data: base64_encode(&m.data) })
        .collect();
    let p = Payload {
        shapes: clip.shapes.clone(),
        placeholders: clip.placeholders.clone(),
        slides: clip.slides.clone(),
        media,
        text: clip.text.clone(),
        plain: clip.plain.clone(),
        source_theme: clip.source_theme.as_ref().map(|t| Theme { colors: t.colors.clone(), color_map: t.color_map.clone(), fonts: t.fonts.clone() }),
    };
    let json = serde_json::to_vec(&p).ok()?;
    let payload = format!("{VERSION}:{:x}:{}", clip.token, base64_encode(&json));
    if payload.len() > MAX_PAYLOAD {
        log::warn!(
            "the copy is {} MiB as a clipboard payload, over the {} MiB limit: it stays in this window",
            payload.len() >> 20,
            MAX_PAYLOAD >> 20
        );
        return None;
    }
    Some(payload)
}

/// The token a payload carries, without decoding the rest.
pub fn token_of(payload: &str) -> Option<u64> {
    let rest = payload.strip_prefix(VERSION)?.strip_prefix(':')?;
    let (token, _) = rest.split_once(':')?;
    u64::from_str_radix(token, 16).ok()
}

/// The clipboard a payload holds. The token comes with it.
pub fn decode(payload: &str) -> Result<Clipboard, String> {
    if payload.len() > MAX_PAYLOAD {
        return Err(format!("the clipboard payload is over {} MiB", MAX_PAYLOAD >> 20));
    }
    let bad = || "not a DeckCraft clipboard payload (v1:<token>:<base64>)".to_string();
    let rest = payload.strip_prefix(VERSION).and_then(|r| r.strip_prefix(':')).ok_or_else(bad)?;
    let (token, body) = rest.split_once(':').ok_or_else(bad)?;
    let token = u64::from_str_radix(token, 16).map_err(|_| bad())?;
    let json = base64_decode(body).ok_or_else(bad)?;
    let p: Payload = serde_json::from_slice(&json).map_err(|e| format!("the clipboard payload does not parse: {e}"))?;
    let mut media = Vec::with_capacity(p.media.len());
    for m in p.media {
        let data = base64_decode(&m.data).ok_or_else(|| format!("media {} in the clipboard payload is not base64", m.name))?;
        media.push(MediaItem { id: m.id, name: m.name, content_type: m.content_type, data: Arc::new(data), link: m.link });
    }
    let source_theme = p.source_theme.map(|t| crate::SourceTheme { colors: t.colors, color_map: t.color_map, fonts: t.fonts });
    Ok(Clipboard {
        shapes: p.shapes,
        placeholders: p.placeholders,
        slides: p.slides,
        media,
        text: p.text,
        plain: p.plain,
        source_theme,
        pastes: 0,
        token,
    })
}

/// `html` (what the copy puts on the system clipboard; may be empty) with the payload for `clip`
/// appended as a comment. `plain` stands in for empty HTML so the HTML alternative still reads as
/// the copied words. `None` when there is no payload: the clipboard goes out as today.
pub fn embed(html: &str, plain: &str, clip: &Clipboard) -> Option<String> {
    let payload = encode(clip)?;
    let mut out =
        if html.is_empty() && !plain.is_empty() { crate::cmd::richclip::body_to_html(&TextBody::from_text(plain)) } else { html.to_string() };
    out.reserve(MARK_OPEN.len() + payload.len() + MARK_CLOSE.len());
    out.push_str(MARK_OPEN);
    out.push_str(&payload);
    out.push_str(MARK_CLOSE);
    Some(out)
}

/// The payload embedded in `html` by [`embed`], if any.
pub fn extract(html: &str) -> Option<&str> {
    let start = html.rfind(MARK_OPEN)? + MARK_OPEN.len();
    let rest = &html[start..];
    let end = rest.find(MARK_CLOSE)?;
    let payload = &rest[..end];
    payload.starts_with(VERSION).then_some(payload)
}

/// Make `payload` the session's clipboard, unless its token says it is the session's own copy
/// (the internal clipboard then stays, pastes offset included). `Ok(true)` when it was adopted.
pub fn adopt(s: &mut Session, payload: &str) -> Result<bool, String> {
    // Token 0 is a session that has copied nothing yet, never a copy of its own.
    if token_of(payload).is_some_and(|t| t != 0 && t == s.clipboard.token) {
        return Ok(false);
    }
    s.clipboard = decode(payload)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use deckcraft_color::Rgba;
    use deckcraft_model::style::{ColorRef, PictureFill};
    use deckcraft_model::{Fill, Shape, ShapeId, ShapeKind, Xfrm};
    use serde_json::json;

    use super::*;

    /// Session a: slide 0 holds a red box and a picture of `bytes`; both selected.
    fn deck_with_a_picture(bytes: &[u8]) -> Session {
        let mut s = Session::with_new();
        let st = s.doc_mut().unwrap();
        let doc = Arc::make_mut(&mut st.doc);
        let media = doc.add_media("p.png", "image/png", bytes.to_vec()).expect("media");
        let (p, b) = (ShapeId(doc.alloc_id().expect("id")), ShapeId(doc.alloc_id().expect("id")));
        let slide = Arc::make_mut(&mut doc.slides[0]);
        slide.shapes.push(Shape {
            id: p,
            kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
            xfrm: Some(Xfrm::new(10.0, 10.0, 50.0, 50.0)),
            ..Default::default()
        });
        slide.shapes.push(Shape {
            id: b,
            fill: Some(Fill::Solid { color: ColorRef::rgb(Rgba::rgb(200, 0, 0)) }),
            xfrm: Some(Xfrm::new(100.0, 10.0, 80.0, 40.0)),
            text: Some(TextBody::from_text("box")),
            ..Default::default()
        });
        st.selection.slides.clear();
        st.selection.shapes = vec![p, b];
        s
    }

    #[test]
    fn shapes_with_a_picture_round_trip() {
        let mut a = deck_with_a_picture(b"picture bytes");
        a.execute("edit.copy", &json!({})).unwrap();
        let payload = encode(&a.clipboard).expect("payload");
        assert!(payload.starts_with("v1:"), "{payload}");
        assert_eq!(token_of(&payload), Some(a.clipboard.token));
        let back = decode(&payload).unwrap();
        assert_eq!(back.token, a.clipboard.token);
        assert_eq!(back.shapes, a.clipboard.shapes);
        assert_eq!(back.placeholders, a.clipboard.placeholders);
        assert_eq!(back.media, a.clipboard.media);
        assert_eq!(back.media[0].data.as_ref(), b"picture bytes");
        assert_eq!(back.plain, "box");
        assert_eq!(format!("{:?}", back.source_theme), format!("{:?}", a.clipboard.source_theme));
        assert_eq!(back.pastes, 0);
    }

    #[test]
    fn slides_and_text_round_trip() {
        let mut a = deck_with_a_picture(b"slide picture");
        a.execute("edit.copy", &json!({"scope": "slides"})).unwrap();
        let back = decode(&encode(&a.clipboard).unwrap()).unwrap();
        assert_eq!(back.slides, a.clipboard.slides);
        assert_eq!(back.media[0].data.as_ref(), b"slide picture");
        assert!(back.shapes.is_empty());

        let text = Clipboard { text: Some(TextBody::from_text("one\ntwo")), plain: "one\ntwo".into(), ..Clipboard::fresh() };
        let back = decode(&encode(&text).unwrap()).unwrap();
        assert_eq!(back.text, text.text);
        assert_eq!(back.plain, "one\ntwo");
        assert_eq!(back.token, text.token);
    }

    #[test]
    fn an_empty_clipboard_has_no_payload_and_a_huge_one_stays_local() {
        assert_eq!(encode(&Clipboard::default()), None);
        let big = deckcraft_model::MediaItem { id: MediaId(1), name: "big.bin".into(), data: Arc::new(vec![7u8; MAX_PAYLOAD]), ..Default::default() };
        let clip = Clipboard { shapes: vec![Shape::default()], media: vec![big], ..Clipboard::fresh() };
        assert_eq!(encode(&clip), None, "over the cap");
        // Refused on the estimate (16/9 of the media bytes), before encoding anything: a media
        // item a little over 9/16 of the cap is out, one well under it is carried.
        let bytes =
            |n: usize| deckcraft_model::MediaItem { id: MediaId(1), name: "m.bin".into(), data: Arc::new(vec![1u8; n]), ..Default::default() };
        let over = Clipboard { shapes: vec![Shape::default()], media: vec![bytes(MAX_PAYLOAD / 16 * 9 + 1024)], ..Clipboard::fresh() };
        assert_eq!(encode(&over), None);
        let under = Clipboard { shapes: vec![Shape::default()], media: vec![bytes(MAX_PAYLOAD / 4)], ..Clipboard::fresh() };
        assert!(encode(&under).is_some());
        assert!(decode(&"v".repeat(MAX_PAYLOAD + 1)).is_err());
        assert!(decode("v1:12:not base64!").is_err());
        assert!(decode("hello").is_err());
    }

    #[test]
    fn embed_and_extract() {
        assert_eq!(extract("<p>Hi</p>"), None, "HTML from another app carries nothing");
        assert_eq!(extract("<!--deckcraft-clipboard:v0:x-->"), None);
        let clip = Clipboard { shapes: vec![Shape::default()], ..Clipboard::fresh() };
        let payload = encode(&clip).unwrap();
        let html = embed("<p>Hi</p>", "Hi", &clip).unwrap();
        assert!(html.starts_with("<p>Hi</p><!--deckcraft-clipboard:v1:"), "{html}");
        assert_eq!(extract(&html), Some(payload.as_str()));
        // Without HTML the plain words become the HTML, so another app still pastes them.
        let html = embed("", "Slide 1\nSlide 2", &clip).unwrap();
        assert!(html.starts_with("<p>Slide 1</p><br><p>Slide 2</p><!--"), "{html}");
        assert_eq!(extract(&html), Some(payload.as_str()));
        // Nothing to carry: nothing embedded.
        assert_eq!(embed("<p>Hi</p>", "Hi", &Clipboard::default()), None);
    }

    #[test]
    fn tokens_differ_per_copy_and_adopt_skips_our_own() {
        let mut a = deck_with_a_picture(b"x");
        a.execute("edit.copy", &json!({})).unwrap();
        let t1 = a.clipboard.token;
        assert_ne!(t1, 0);
        let payload = encode(&a.clipboard).unwrap();
        a.clipboard.pastes = 3;
        assert_eq!(adopt(&mut a, &payload), Ok(false), "our own copy: the internal clipboard stays");
        assert_eq!(a.clipboard.pastes, 3);
        a.execute("edit.copy", &json!({})).unwrap();
        assert_ne!(a.clipboard.token, t1, "a new copy gets a new token");
        assert_eq!(adopt(&mut a, &payload), Ok(true), "an older copy of ours is another clipboard");
        assert_eq!(a.clipboard.token, t1);
        assert!(adopt(&mut a, "v1:zz:").is_err());
    }

    #[test]
    fn a_payload_pastes_into_a_fresh_session() {
        let mut a = deck_with_a_picture(b"from deck a");
        a.execute("edit.copy", &json!({})).unwrap();
        let payload = encode(&a.clipboard).unwrap();
        let mut b = Session::with_new();
        let r = b.execute("edit.paste", &json!({"text": "box", "payload": payload})).unwrap();
        assert_eq!(r["ids"].as_array().map(Vec::len), Some(2));
        let (cmd, journaled) = b.journal.last().expect("journaled");
        assert_eq!((cmd.as_str(), journaled), ("edit.paste", &json!({"text": "box"})), "the journal keeps no clipboard payload");
        let d = b.doc().unwrap();
        let shapes = &d.doc.slides[0].shapes;
        let pic = shapes.iter().find(|x| matches!(x.kind, ShapeKind::Picture { .. })).expect("picture");
        let ShapeKind::Picture { fill } = &pic.kind else { panic!("not a picture") };
        assert_eq!(d.doc.media(fill.media).unwrap().data.as_ref(), b"from deck a");
        let text = shapes.iter().find(|x| x.text.as_ref().is_some_and(|t| t.text() == "box")).expect("box");
        assert_eq!(text.fill, Some(Fill::Solid { color: ColorRef::rgb(Rgba::rgb(200, 0, 0)) }));
        assert_eq!(text.xfrm.map(|x| (x.x, x.y, x.w, x.h)), Some((100.0, 10.0, 80.0, 40.0)), "another deck: no offset");
        assert_eq!(b.clipboard.token, a.clipboard.token, "the payload is now b's clipboard");
        // Pasting the same payload again goes through the internal clipboard (the pastes offset).
        b.execute("edit.paste", &json!({"text": "box", "payload": encode(&a.clipboard).unwrap()})).unwrap();
        assert_eq!(b.clipboard.pastes, 2);
    }

    #[test]
    fn a_payload_with_token_zero_still_pastes_into_a_fresh_session() {
        let mut a = deck_with_a_picture(b"zero");
        a.execute("edit.copy", &json!({})).unwrap();
        a.clipboard.token = 0;
        let payload = encode(&a.clipboard).unwrap();
        let mut b = Session::with_new();
        assert_eq!(b.clipboard.token, 0, "a fresh session has copied nothing");
        let r = b.execute("edit.paste", &json!({"text": "box", "payload": payload})).unwrap();
        assert_eq!(r["ids"].as_array().map(Vec::len), Some(2), "token 0 is not b's own copy");
    }

    #[test]
    fn foreign_media_ids_get_fresh_ids_and_leave_room_to_grow() {
        let mut a = deck_with_a_picture(b"far away");
        a.execute("edit.copy", &json!({})).unwrap();
        let old = a.clipboard.media[0].id;
        let far = MediaId(u32::MAX);
        a.clipboard.media[0].id = far;
        for sh in &mut a.clipboard.shapes {
            if let ShapeKind::Picture { fill } = &mut sh.kind
                && fill.media == old
            {
                fill.media = far;
            }
        }
        let payload = encode(&a.clipboard).unwrap();
        let mut b = Session::with_new();
        b.execute("edit.paste", &json!({"text": "box", "payload": payload})).unwrap();
        let d = b.doc().unwrap();
        let pic = d.doc.slides[0].shapes.iter().find(|x| matches!(x.kind, ShapeKind::Picture { .. })).expect("picture");
        let ShapeKind::Picture { fill } = &pic.kind else { panic!("not a picture") };
        assert_ne!(fill.media, far, "the foreign id is replaced");
        assert_eq!(d.doc.media(fill.media).unwrap().data.as_ref(), b"far away");
        assert!(d.doc.next_id < u32::MAX, "ids are left for later edits");
        b.execute("edit.paste", &json!({"text": "box"})).expect("pasting again still finds ids");
        let media = |b: &Session| b.doc().unwrap().doc.media.iter().filter(|m| m.data.as_ref() == b"far away").count();
        assert_eq!(media(&b), 1, "the second paste reuses the picture's bytes");
        b.clipboard.token = 0;
        b.execute("edit.paste", &json!({"text": "box", "payload": encode(&a.clipboard).unwrap()})).unwrap();
        assert_eq!(media(&b), 1, "adopting the same copy again reuses them too");
    }

    #[test]
    fn a_payload_embedded_in_html_pastes_slides() {
        let mut a = deck_with_a_picture(b"slide picture");
        a.execute("edit.copy", &json!({"scope": "slides"})).unwrap();
        let html = embed("", &a.clipboard.plain, &a.clipboard).unwrap();
        let mut b = Session::with_new();
        let r = b.execute("edit.paste", &json!({"text": a.clipboard.plain, "html": html})).unwrap();
        assert_eq!(r["slides"], 1);
        assert_eq!(b.journal.last().map(|(_, p)| p.get("html").is_none()), Some(true), "nor the HTML it rode in");
        let d = b.doc().unwrap();
        assert_eq!(d.doc.slides.len(), 2);
        let pic = d.doc.slides[1].shapes.iter().find(|x| matches!(x.kind, ShapeKind::Picture { .. })).expect("picture");
        let ShapeKind::Picture { fill } = &pic.kind else { panic!("not a picture") };
        assert_eq!(d.doc.media(fill.media).unwrap().data.as_ref(), b"slide picture");
    }

    /// Copy or Cut with a caret and nothing selected replaces no clipboard, and says so, so the
    /// UI publishes nothing (not the previous copy over again).
    #[test]
    fn a_caret_copy_replaces_nothing_and_says_so() {
        let mut a = deck_with_a_picture(b"p");
        let r = a.execute("edit.copy", &json!({"payload": true})).unwrap();
        assert_eq!(r["copied"], true);
        assert!(r.get("payload").is_some());
        let token = a.clipboard.token;
        let id = a.doc().unwrap().doc.slides[0].shapes.iter().find(|x| x.text.is_some()).unwrap().id.0;
        a.execute("text.edit", &json!({"id": id})).unwrap();
        let r = a.execute("edit.copy", &json!({"payload": true})).unwrap();
        assert_eq!((r["copied"].as_bool(), r.get("payload")), (Some(false), None), "{r}");
        assert_eq!(a.clipboard.token, token, "the clipboard is as it was");
        let r = a.execute("edit.cut", &json!({})).unwrap();
        assert_eq!(r["copied"], false);
    }

    #[test]
    fn copy_returns_the_payload_on_request_and_paste_special_takes_it() {
        let mut a = deck_with_a_picture(b"p");
        let r = a.execute("edit.copy", &json!({})).unwrap();
        assert!(r.get("payload").is_none(), "not by default: the CLI output stays small");
        let r = a.execute("edit.copy", &json!({"payload": true})).unwrap();
        let payload = r["payload"].as_str().expect("payload").to_string();
        assert_eq!(decode(&payload).unwrap().shapes, a.clipboard.shapes);
        let mut b = Session::with_new();
        b.execute("edit.pasteSpecial", &json!({"as": "keepSource", "payload": payload})).unwrap();
        let d = b.doc().unwrap();
        assert!(d.doc.slides[0].shapes.iter().any(|x| x.text.as_ref().is_some_and(|t| t.text() == "box")));
        let mut c = Session::with_new();
        c.execute("edit.pasteText", &json!({"payload": payload})).unwrap();
        let d = c.doc().unwrap();
        assert!(d.doc.slides[0].shapes.iter().any(|x| x.text.as_ref().is_some_and(|t| t.text() == "box")));
        assert!(d.doc.slides[0].shapes.iter().all(|x| !matches!(x.kind, ShapeKind::Picture { .. })), "text only");
        // HTML from another app, or a corrupt payload in HTML, falls back to the usual paste.
        let mut e = Session::with_new();
        e.execute("edit.paste", &json!({"text": "Hi", "html": "<p>Hi</p><!--deckcraft-clipboard:v1:1:@@@-->"})).unwrap();
        assert!(e.doc().unwrap().doc.slides[0].shapes.iter().any(|x| x.text.as_ref().is_some_and(|t| t.text() == "Hi")));
        assert!(e.execute("edit.paste", &json!({"payload": "v1:1:@@@"})).is_err(), "an explicit payload must parse");
    }
}
