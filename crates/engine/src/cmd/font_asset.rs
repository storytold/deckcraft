//! Document fonts: the `p:embeddedFontLst` registry of faces the file carries, so its text draws
//! on a machine that does not have the family installed.
//!
//! An embedded font belongs to the presentation, not to a slide or a shape: it is a document
//! asset, like a media object. One entry is one typeface with up to four face slots (regular,
//! bold, italic, bold italic); the face bytes are ordinary media items with the font content type
//! (`application/x-fontdata`), so the `.pptx` writer turns them into `ppt/fonts/fontN.fntdata`
//! and a commit keeps them. These commands are the single-user and collaborative way to add,
//! describe and remove an entry; the collaboration adapter normalises `font.create`, `font.set`
//! and `font.remove` into operations and restores an undo image through `font.restore`.

use deckcraft_model::{EmbeddedFont, MediaId, Presentation};
use serde_json::{Value, json};

use super::insert;
use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

/// The content type a face object is stored under (the same one the `.pptx` reader uses).
const FONT_CONTENT_TYPE: &str = "application/x-fontdata";
/// Largest font file one entry may carry. A face is drawn whole, so a reasonable installed font
/// fits far below this; the media channel has its own, smaller per-object cap.
const MAX_FONT_BYTES: usize = 16 << 20;
/// Longest typeface name a document may carry.
const MAX_TYPEFACE: usize = 255;
/// Longest whole-entry image a `font.restore` may carry, as JSON bytes.
const MAX_FONT_JSON: usize = 8 << 10;

/// The four face slots, in package order.
const FACES: [&str; 4] = ["regular", "bold", "italic", "boldItalic"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "font.create",
            "Embed Font…",
            ["File", "Info"],
            None,
            "{typeface, face?: regular|bold|italic|boldItalic (default regular), data: base64 TTF/OTF/TTC, name?} | {typeface, face?, path: font file, name?} → {typeface, face, media, faces}",
            has_doc,
            create
        ),
        cmd!(
            "font.set",
            "Font Descriptors",
            [],
            None,
            "{typeface, panose?: 20 hex digits, pitchFamily?: 0..255, charset?: 0..255} — set the entry's descriptor fields; at least one",
            has_doc,
            set
        ),
        cmd!("font.remove", "Remove Embedded Font", [], None, "{typeface} — drops the entry, not the face objects", has_doc, remove),
        cmd!(
            "font.restore",
            "Restore Embedded Font",
            [],
            None,
            "{typeface, font: the whole model entry as JSON (null: no entry)} — restores the entry an undoable step replaced; collaboration undo uses this",
            has_doc,
            restore
        ),
    ]
}

/// A trimmed typeface name that is a usable registry key.
fn typeface_param(p: &Value, cmd: &str) -> Result<String> {
    let raw = str_param(p, "typeface").ok_or_else(|| bad(cmd, "missing `typeface`"))?;
    let name = raw.trim();
    if name.is_empty() {
        return Err(bad(cmd, "`typeface` is empty"));
    }
    if name.chars().count() > MAX_TYPEFACE {
        return Err(bad(cmd, format!("`typeface` is longer than {MAX_TYPEFACE} characters")));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(bad(cmd, "`typeface` contains a control character"));
    }
    Ok(name.to_string())
}

/// The entry a typeface names, case-insensitively (font names are not case-sensitive).
fn entry_of<'a>(doc: &'a Presentation, typeface: &str) -> Option<&'a EmbeddedFont> {
    doc.embedded_fonts.iter().find(|f| f.typeface.eq_ignore_ascii_case(typeface))
}

fn entry_of_mut<'a>(doc: &'a mut Presentation, typeface: &str) -> Option<&'a mut EmbeddedFont> {
    doc.embedded_fonts.iter_mut().find(|f| f.typeface.eq_ignore_ascii_case(typeface))
}

/// One face slot of an entry by its command name.
fn slot<'a>(entry: &'a mut EmbeddedFont, face: &str) -> Option<&'a mut Option<MediaId>> {
    match face {
        "regular" => Some(&mut entry.regular),
        "bold" => Some(&mut entry.bold),
        "italic" => Some(&mut entry.italic),
        "boldItalic" => Some(&mut entry.bold_italic),
        _ => None,
    }
}

/// How many face slots an entry fills.
fn face_count(entry: &EmbeddedFont) -> usize {
    entry.faces().into_iter().flatten().count()
}

/// Whether `bytes` is a TrueType/OpenType file (or a collection) with a table directory that fits
/// inside the object. `deckcraft-fonts` decides what it can actually draw; this is the hostile
/// input check that keeps a bogus blob out of the document and out of the package writer.
fn looks_like_font(bytes: &[u8]) -> bool {
    let Some(sfnt) = deckcraft_fonts::embedded_sfnt(bytes) else { return false };
    if sfnt.len() < 12 {
        return false;
    }
    if sfnt.starts_with(b"ttcf") {
        let n = u32::from_be_bytes([sfnt[8], sfnt[9], sfnt[10], sfnt[11]]) as usize;
        if n == 0 || n > 1024 {
            return false;
        }
        let table = 12usize.saturating_add(n.saturating_mul(4));
        return sfnt.len() >= table
            && (0..n).all(|i| {
                let at = 12 + i * 4;
                u32::from_be_bytes([sfnt[at], sfnt[at + 1], sfnt[at + 2], sfnt[at + 3]]) as usize >= 12
            });
    }
    let num_tables = u16::from_be_bytes([sfnt[4], sfnt[5]]) as usize;
    if num_tables == 0 || num_tables > 4096 {
        return false;
    }
    sfnt.len() >= 12usize.saturating_add(num_tables.saturating_mul(16))
}

/// Make every face of one entry available to the running session, so text in that family draws
/// with the document's own bytes. A face already loaded stays; the part is kept either way.
fn install(doc: &Presentation, entry: &EmbeddedFont) {
    for id in entry.faces().into_iter().flatten() {
        let Some(item) = doc.media(id) else { continue };
        let Some(sfnt) = deckcraft_fonts::embedded_sfnt(&item.data) else { continue };
        deckcraft_fonts::FontDb::global().add_font(sfnt.to_vec());
    }
}

/// Answer for one entry, without its bytes.
fn answer(entry: &EmbeddedFont) -> Value {
    json!({
        "typeface": entry.typeface,
        "faces": face_count(entry),
        "regular": entry.regular.map(|id| id.0),
        "bold": entry.bold.map(|id| id.0),
        "italic": entry.italic.map(|id| id.0),
        "boldItalic": entry.bold_italic.map(|id| id.0),
        "panose": entry.panose,
        "pitchFamily": entry.pitch_family,
        "charset": entry.charset,
    })
}

/// Embed one face of a typeface from a font file, creating the document's entry if it has none.
fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let typeface = typeface_param(p, "font.create")?;
    let face = match str_param(p, "face") {
        None => "regular",
        Some(name) if FACES.contains(&name) => name,
        Some(other) => return Err(bad("font.create", format!("`face` is regular, bold, italic or boldItalic, not `{other}`"))),
    }
    .to_string();
    let (name, bytes) = insert::media_bytes(p, "font.create")?;
    if bytes.is_empty() {
        return Err(bad("font.create", "the font file is empty"));
    }
    if bytes.len() > MAX_FONT_BYTES {
        return Err(bad("font.create", format!("the font file is larger than {} MB", MAX_FONT_BYTES >> 20)));
    }
    if !looks_like_font(&bytes) {
        return Err(bad("font.create", format!("{name} is not a TrueType or OpenType font DeckCraft can read")));
    }
    s.edit(|doc, _| {
        let id = doc.add_media(&name, FONT_CONTENT_TYPE, bytes.clone()).ok_or_else(|| bad("font.create", "the document has no free media id"))?;
        match entry_of_mut(doc, &typeface) {
            Some(entry) => {
                let target = slot(entry, &face).ok_or_else(|| bad("font.create", format!("unknown face `{face}`")))?;
                *target = Some(id);
            }
            None => {
                let mut entry = EmbeddedFont { typeface: typeface.clone(), ..Default::default() };
                let target = slot(&mut entry, &face).ok_or_else(|| bad("font.create", format!("unknown face `{face}`")))?;
                *target = Some(id);
                doc.embedded_fonts.push(entry);
            }
        }
        Ok(())
    })?;
    let doc = s.doc()?.doc.clone();
    let entry = entry_of(&doc, &typeface).ok_or_else(|| bad("font.create", "the entry was not stored"))?;
    install(&doc, entry);
    let face_id = match face.as_str() {
        "regular" => entry.regular,
        "bold" => entry.bold,
        "italic" => entry.italic,
        _ => entry.bold_italic,
    };
    let mut out = answer(entry);
    out["face"] = json!(face);
    out["media"] = json!(face_id.map(|id| id.0));
    Ok(out)
}

/// One descriptor field from the arguments: `None` = absent, a value out of range is refused.
fn u8_param(p: &Value, key: &str, cmd: &str) -> Result<Option<u8>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            value.as_u64().and_then(|v| u8::try_from(v).ok()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a number from 0 to 255")))
        }
    }
}

/// Set the descriptor fields of one entry as final values.
fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let typeface = typeface_param(p, "font.set")?;
    let panose = match p.get("panose") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let text = value.as_str().ok_or_else(|| bad("font.set", "`panose` must be a string of 20 hex digits"))?;
            let text = text.trim();
            if text.len() != 20 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(bad("font.set", "`panose` must be 20 hex digits"));
            }
            Some(text.to_string())
        }
    };
    let pitch_family = u8_param(p, "pitchFamily", "font.set")?;
    let charset = u8_param(p, "charset", "font.set")?;
    if panose.is_none() && pitch_family.is_none() && charset.is_none() {
        return Err(bad("font.set", "nothing to set: give `panose`, `pitchFamily` or `charset`"));
    }
    s.edit(|doc, _| {
        let entry = entry_of_mut(doc, &typeface).ok_or_else(|| bad("font.set", format!("no embedded font named `{typeface}`")))?;
        if let Some(value) = panose.clone() {
            entry.panose = Some(value);
        }
        if let Some(value) = pitch_family {
            entry.pitch_family = Some(value);
        }
        if let Some(value) = charset {
            entry.charset = Some(value);
        }
        Ok(())
    })?;
    let doc = s.doc()?.doc.clone();
    let entry = entry_of(&doc, &typeface).ok_or_else(|| bad("font.set", "the entry disappeared"))?;
    Ok(answer(entry))
}

/// Drop one entry. The face objects stay in the media list; a save writes the objects the model
/// no longer references only if a later version needs them.
fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    let typeface = typeface_param(p, "font.remove")?;
    s.edit(|doc, _| {
        let before = doc.embedded_fonts.len();
        doc.embedded_fonts.retain(|f| !f.typeface.eq_ignore_ascii_case(&typeface));
        if doc.embedded_fonts.len() == before {
            return Err(bad("font.remove", format!("no embedded font named `{typeface}`")));
        }
        Ok(())
    })?;
    Ok(json!({"typeface": typeface, "removed": true}))
}

/// Put back the whole entry an undoable step replaced (`null` = the document had none).
fn restore(s: &mut Session, p: &Value) -> Result<Value> {
    let typeface = typeface_param(p, "font.restore")?;
    let value = p.get("font").cloned().unwrap_or(Value::Null);
    let bytes = serde_json::to_vec(&value).map_err(|e| bad("font.restore", format!("invalid value: {e}")))?;
    if bytes.len() > MAX_FONT_JSON {
        return Err(bad("font.restore", "the value is too large"));
    }
    let entry: Option<EmbeddedFont> = if value.is_null() {
        None
    } else {
        Some(serde_json::from_value(value).map_err(|e| bad("font.restore", format!("the value is not a font entry: {e}")))?)
    };
    if let Some(entry) = &entry {
        if !entry.typeface.eq_ignore_ascii_case(&typeface) {
            return Err(bad("font.restore", "the value names another typeface"));
        }
        let doc = s.doc()?.doc.clone();
        for id in entry.faces().into_iter().flatten() {
            if doc.media(id).is_none() {
                return Err(bad("font.restore", format!("the face object {id} is not in the document")));
            }
        }
    }
    s.edit(|doc, _| {
        doc.embedded_fonts.retain(|f| !f.typeface.eq_ignore_ascii_case(&typeface));
        if let Some(entry) = entry.clone() {
            doc.embedded_fonts.push(entry);
        }
        Ok(())
    })?;
    let doc = s.doc()?.doc.clone();
    if let Some(entry) = entry_of(&doc, &typeface) {
        install(&doc, entry);
        Ok(answer(entry))
    } else {
        Ok(json!({"typeface": typeface, "removed": true}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A small original font (ColrSquares) that the model can carry and the font database reads.
    fn font() -> Vec<u8> {
        include_bytes!("../../../render/tests/fixtures/colr-squares.ttf").to_vec()
    }

    fn session() -> Session {
        Session::with_new()
    }

    fn entry(s: &Session, typeface: &str) -> EmbeddedFont {
        let doc = s.doc().expect("doc").doc.clone();
        doc.embedded_fonts.iter().find(|f| f.typeface == typeface).cloned().unwrap_or_else(|| panic!("no entry {typeface}"))
    }

    #[test]
    fn a_font_is_embedded_once_and_its_faces_stack_up() {
        let mut s = session();
        let r = s.execute("font.create", &json!({"typeface": "Deck Test Face", "data": crate::cmd::base64_encode(&font())})).expect("create");
        assert_eq!(r["typeface"], "Deck Test Face");
        assert_eq!(r["face"], "regular");
        assert_eq!(r["faces"], 1);
        let media = r["media"].as_u64().expect("media id");
        let doc = s.doc().expect("doc").doc.clone();
        assert!(doc.media(MediaId(media as u32)).is_some(), "the face is a media object");
        assert_eq!(doc.media(MediaId(media as u32)).expect("item").content_type, FONT_CONTENT_TYPE);
        assert_eq!(entry(&s, "Deck Test Face").regular, Some(MediaId(media as u32)));

        // A second face of the same typeface joins the entry instead of making a new one.
        let r = s
            .execute("font.create", &json!({"typeface": "deck test face", "face": "bold", "data": crate::cmd::base64_encode(&font())}))
            .expect("bold");
        assert_eq!(r["faces"], 2, "two face slots now point at the same object");
        let doc = s.doc().expect("doc").doc.clone();
        assert_eq!(doc.embedded_fonts.len(), 1);
        assert_eq!(doc.embedded_fonts[0].typeface, "Deck Test Face", "the first spelling is kept");
        assert_eq!(doc.embedded_fonts[0].bold, Some(MediaId(media as u32)));

        // The entry survives a save and reimports with both faces.
        let bytes = crate::cmd::file::save_bytes(&doc, "pptx").expect("save");
        let (back, _lost) = deckcraft_pptx::import_checked(&bytes).expect("read");
        assert_eq!(back.embedded_fonts.len(), 1);
        assert_eq!(back.embedded_fonts[0].typeface, "Deck Test Face");
        assert!(back.embedded_fonts[0].regular.is_some() && back.embedded_fonts[0].bold.is_some());
    }

    #[test]
    fn a_font_file_is_validated_before_the_first_write() {
        let mut s = session();
        let doc = s.doc().expect("doc").doc.clone();
        let not_font = crate::cmd::base64_encode(b"this is not a font at all");
        let broken = {
            // A font magic with a table directory that cannot fit: magic alone is not enough.
            let mut b = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0, 0, 0, 0, 0, 0];
            b.extend([0u8; 8]);
            crate::cmd::base64_encode(&b)
        };
        for (why, args) in [
            ("not a font", json!({"typeface": "X", "data": not_font})),
            ("a broken sfnt", json!({"typeface": "X", "data": broken})),
            ("no bytes", json!({"typeface": "X"})),
            ("an empty typeface", json!({"typeface": "   ", "data": broken})),
            ("an unknown face", json!({"typeface": "X", "face": "black", "data": crate::cmd::base64_encode(&font())})),
        ] {
            assert!(s.execute("font.create", &args).is_err(), "{why} is refused");
        }
        assert!(s.doc().expect("doc").doc.embedded_fonts.is_empty(), "a refused create writes nothing");
        assert_eq!(s.doc().expect("doc").doc.media.len(), doc.media.len());
    }

    #[test]
    fn descriptors_are_set_and_removed_as_a_whole_entry() {
        let mut s = session();
        s.execute("font.create", &json!({"typeface": "Deck Test Face", "data": crate::cmd::base64_encode(&font())})).expect("create");
        let r = s
            .execute("font.set", &json!({"typeface": "Deck Test Face", "panose": "020B0604020202020204", "pitchFamily": 2, "charset": 0}))
            .expect("set");
        assert_eq!(r["panose"], "020B0604020202020204");
        assert_eq!(r["pitchFamily"], 2);
        assert_eq!(r["charset"], 0);
        let e = entry(&s, "Deck Test Face");
        assert_eq!(e.panose.as_deref(), Some("020B0604020202020204"));
        assert_eq!(e.pitch_family, Some(2));

        // One field at a time leaves the others.
        s.execute("font.set", &json!({"typeface": "Deck Test Face", "charset": 204})).expect("charset");
        let e = entry(&s, "Deck Test Face");
        assert_eq!(e.pitch_family, Some(2));
        assert_eq!(e.charset, Some(204));

        for (why, args) in [
            ("an unknown typeface", json!({"typeface": "Nope", "charset": 1})),
            ("nothing to set", json!({"typeface": "Deck Test Face"})),
            ("a bad panose", json!({"typeface": "Deck Test Face", "panose": "xyz"})),
            ("a charset out of range", json!({"typeface": "Deck Test Face", "charset": 300})),
        ] {
            assert!(s.execute("font.set", &args).is_err(), "{why} is refused");
        }

        s.execute("font.remove", &json!({"typeface": "Deck Test Face"})).expect("remove");
        assert!(s.doc().expect("doc").doc.embedded_fonts.is_empty());
        assert!(s.execute("font.remove", &json!({"typeface": "Deck Test Face"})).is_err(), "removing twice is refused");
        // The face object stays, so an undo image that names it is still valid.
        assert_eq!(s.doc().expect("doc").doc.media.len(), 1);

        // The engine's own undo puts the entry back whole (descriptors included).
        s.execute("edit.undo", &json!({})).expect("undo the remove");
        let e = entry(&s, "Deck Test Face");
        assert_eq!(e.charset, Some(204));
        assert_eq!(e.pitch_family, Some(2));
    }

    #[test]
    fn a_restore_puts_back_an_exact_entry_or_none() {
        let mut s = session();
        s.execute("font.create", &json!({"typeface": "Deck Test Face", "data": crate::cmd::base64_encode(&font())})).expect("create");
        let whole = entry(&s, "Deck Test Face");
        let value = serde_json::to_value(&whole).expect("json");
        // Remove, then restore the recorded entry.
        s.execute("font.remove", &json!({"typeface": "Deck Test Face"})).expect("remove");
        s.execute("font.restore", &json!({"typeface": "Deck Test Face", "font": value})).expect("restore");
        assert_eq!(entry(&s, "Deck Test Face"), whole);
        // And a restore to "no entry" takes it out again.
        s.execute("font.restore", &json!({"typeface": "Deck Test Face", "font": null})).expect("restore none");
        assert!(s.doc().expect("doc").doc.embedded_fonts.is_empty());

        // A value for another typeface, a missing face object and a malformed value are refused.
        for (why, args) in [
            ("another typeface", json!({"typeface": "Other", "font": serde_json::to_value(&whole).expect("json")})),
            ("a missing object", json!({"typeface": "Deck Test Face", "font": {"typeface": "Deck Test Face", "regular": 99999}})),
            ("a malformed value", json!({"typeface": "Deck Test Face", "font": {"typeface": "Deck Test Face", "regular": "x"}})),
        ] {
            assert!(s.execute("font.restore", &args).is_err(), "{why} is refused");
        }
        assert!(s.doc().expect("doc").doc.embedded_fonts.is_empty(), "a refused restore writes nothing");
    }
}
