//! The clipboard as bytes, so what one DeckCraft window copies can be pasted in another.
//!
//! Each window is its own process with its own [`Session`], and the session's clipboard holds the
//! copied shapes or slides themselves (the system clipboard only gets their words). The host
//! writes [`Session::clipboard_packet`] to a file the windows share when the clipboard changes
//! ([`Session::clipboard_serial`]) and hands a packet from another window to
//! [`Session::load_clipboard_packet`]. Pasting then works as it does inside one window, including
//! the rule that words copied in another application win over a stale copy here.
//!
//! The file is not trusted: a packet that is damaged, from another version or too large is
//! refused, never half-loaded.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::cmd::{base64_decode, base64_encode};
use crate::{Clipboard, Session};

const VERSION: u32 = 1;
/// The largest packet written or read (copied pictures and sounds travel inside it).
const MAX_PACKET: usize = 512 << 20;
/// The most media items a packet may carry.
const MAX_MEDIA: usize = 10_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Out<'a> {
    version: u32,
    owner: u64,
    seq: u64,
    clipboard: &'a Clipboard,
    /// The bytes of `clipboard.media`, base64, in the same order (the model keeps them out of its JSON).
    media_data: Vec<String>,
}

/// Every field is required: a packet missing one (`{"version":1}`) is refused, not read as an
/// empty clipboard that would replace this window's copy.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct In {
    version: u32,
    owner: u64,
    seq: u64,
    clipboard: Clipboard,
    media_data: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Origin {
    version: u32,
    owner: u64,
    seq: u64,
}

/// Whether `c` holds anything to paste (what [`Session::clipboard_packet`] writes at all).
fn has_content(c: &Clipboard) -> bool {
    !(c.shapes.is_empty() && c.slides.is_empty() && c.text.is_none() && c.plain.is_empty())
}

/// A packet is a JSON object. (serde would also build these structs from a JSON list.)
fn is_object(bytes: &[u8]) -> bool {
    bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{')
}

/// Who wrote `bytes` and which copy it is: `(owner, seq)`. `None` for anything that is not a packet
/// of this version.
pub fn packet_origin(bytes: &[u8]) -> Option<(u64, u64)> {
    if bytes.len() > MAX_PACKET || !is_object(bytes) {
        return None;
    }
    let o: Origin = serde_json::from_slice(bytes).ok()?;
    (o.version == VERSION).then_some((o.owner, o.seq))
}

impl Session {
    /// Replace the clipboard with a copy, and count it.
    pub(crate) fn set_clipboard(&mut self, c: Clipboard) {
        self.clipboard = c;
        self.clipboard_serial = self.clipboard_serial.wrapping_add(1);
    }

    /// Changes whenever something is copied or cut here, or a clipboard is taken from another window.
    pub fn clipboard_serial(&self) -> u64 {
        self.clipboard_serial
    }

    /// The clipboard as a packet stamped with the writer's `owner` and a copy number `seq`. `None`
    /// when there is nothing on it (or it is too large to share).
    pub fn clipboard_packet(&self, owner: u64, seq: u64) -> Option<Vec<u8>> {
        let c = &self.clipboard;
        if !has_content(c) {
            return None;
        }
        let media_data = c.media.iter().map(|m| base64_encode(&m.data)).collect();
        let bytes = serde_json::to_vec(&Out { version: VERSION, owner, seq, clipboard: c, media_data }).ok()?;
        (bytes.len() <= MAX_PACKET).then_some(bytes)
    }

    /// Make a packet from another window this session's clipboard. Returns its `(owner, seq)`, or
    /// `None` (the clipboard untouched) when it is not a usable packet.
    pub fn load_clipboard_packet(&mut self, bytes: &[u8]) -> Option<(u64, u64)> {
        if bytes.len() > MAX_PACKET || !is_object(bytes) {
            return None;
        }
        let p: In = serde_json::from_slice(bytes).ok()?;
        if p.version != VERSION || !has_content(&p.clipboard) || p.clipboard.media.len() != p.media_data.len() || p.media_data.len() > MAX_MEDIA {
            return None;
        }
        let mut c = p.clipboard;
        for (m, data) in c.media.iter_mut().zip(&p.media_data) {
            m.data = Arc::new(base64_decode(data)?);
        }
        // How many times it was pasted here is this window's own count.
        c.pastes = 0;
        self.set_clipboard(c);
        Some((p.owner, p.seq))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::MediaItem;
    use serde_json::json;

    fn session_with_copied_text_box() -> Session {
        let mut s = Session::with_new();
        s.execute("insert.textBox", &json!({"rect": [40, 40, 400, 80], "text": "Copy me"})).unwrap();
        s.execute("text.exit", &json!({})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        s
    }

    #[test]
    fn a_copy_changes_the_serial() {
        let mut s = Session::with_new();
        let before = s.clipboard_serial();
        s.execute("insert.textBox", &json!({"rect": [40, 40, 400, 80], "text": "x"})).unwrap();
        s.execute("text.exit", &json!({})).unwrap();
        s.execute("edit.copy", &json!({})).unwrap();
        assert_ne!(s.clipboard_serial(), before);
    }

    #[test]
    fn what_one_window_copies_another_can_paste() {
        let a = session_with_copied_text_box();
        let packet = a.clipboard_packet(7, 1).expect("something was copied");
        assert_eq!(packet_origin(&packet), Some((7, 1)));

        let mut b = Session::with_new();
        assert!(b.clipboard.shapes.is_empty());
        let serial = b.clipboard_serial();
        assert_eq!(b.load_clipboard_packet(&packet), Some((7, 1)));
        assert_ne!(b.clipboard_serial(), serial, "taking a clipboard counts as a change");
        assert_eq!(b.clipboard.shapes.len(), a.clipboard.shapes.len());
        assert_eq!(b.clipboard.plain, "Copy me");

        let shapes_before = b.active().unwrap().current_slide().unwrap().shapes.len();
        b.execute("edit.paste", &json!({})).unwrap();
        assert_eq!(b.active().unwrap().current_slide().unwrap().shapes.len(), shapes_before + 1, "the copied shape lands in the other window");
    }

    #[test]
    fn copied_slides_travel_with_their_pictures() {
        let mut a = Session::with_new();
        a.clipboard.media.push(MediaItem {
            name: "p.png".into(),
            content_type: "image/png".into(),
            data: Arc::new(vec![1, 2, 3, 250]),
            ..Default::default()
        });
        a.clipboard.plain = "a slide".into();
        let packet = a.clipboard_packet(1, 2).unwrap();
        let mut b = Session::with_new();
        b.load_clipboard_packet(&packet).unwrap();
        assert_eq!(*b.clipboard.media[0].data, vec![1, 2, 3, 250]);
        assert_eq!(b.clipboard.media[0].name, "p.png");
    }

    #[test]
    fn an_empty_clipboard_makes_no_packet() {
        assert!(Session::with_new().clipboard_packet(1, 1).is_none());
    }

    #[test]
    fn damaged_or_foreign_packets_are_refused_and_change_nothing() {
        let mut s = session_with_copied_text_box();
        let serial = s.clipboard_serial();
        let keep = s.clipboard.plain.clone();
        for bad in [
            &b""[..],
            b"not json",
            b"{\"version\":99}",
            b"{\"version\":1,\"clipboard\":{},\"mediaData\":[\"AA==\"]}",
            b"[1,2,3]",
            // Missing fields are not read as an empty clipboard that would replace the copy here.
            b"{\"version\":1}",
            b"{\"version\":1,\"owner\":3,\"seq\":4,\"mediaData\":[]}",
            // A complete packet that holds nothing to paste.
            b"{\"version\":1,\"owner\":3,\"seq\":4,\"clipboard\":{},\"mediaData\":[]}",
        ] {
            assert_eq!(s.load_clipboard_packet(bad), None, "{:?}", String::from_utf8_lossy(bad));
        }
        assert_eq!(s.clipboard_serial(), serial);
        assert_eq!(s.clipboard.plain, keep);
        assert_eq!(packet_origin(b"{\"version\":99}"), None);
        assert_eq!(packet_origin(b"{\"version\":1}"), None, "no owner, no copy number");
        assert_eq!(packet_origin(b"{\"version\":1,\"owner\":3,\"seq\":4}"), Some((3, 4)));
    }
}
