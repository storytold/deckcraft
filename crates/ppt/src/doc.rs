//! The document structure of the `PowerPoint Document` stream ([MS-PPT] §2.1–2.4): the current
//! user's pointer to the last edit, the chain of edits with their persist directories (which map
//! object ids to stream offsets, newer edits winning), the document container and the lists of
//! masters, slides and notes.

use std::collections::{HashMap, HashSet};

use crate::PptError;
use crate::record::{Rec, Records};

pub mod rt {
    //! Record types used here ([MS-PPT] §2.13.24, [MS-ODRAW] §2.1.4).
    pub const DOCUMENT: u16 = 0x03E8;
    pub const DOCUMENT_ATOM: u16 = 0x03E9;
    pub const SLIDE: u16 = 0x03EE;
    pub const SLIDE_ATOM: u16 = 0x03EF;
    pub const NOTES: u16 = 0x03F0;
    pub const NOTES_ATOM: u16 = 0x03F1;
    pub const ENVIRONMENT: u16 = 0x03F2;
    pub const SLIDE_PERSIST_ATOM: u16 = 0x03F3;
    pub const MAIN_MASTER: u16 = 0x03F8;
    pub const SLIDE_SHOW_SLIDE_INFO: u16 = 0x03F9;
    pub const DRAWING_GROUP: u16 = 0x040B;
    pub const DRAWING: u16 = 0x040C;
    pub const COLOR_SCHEME: u16 = 0x07F0;
    pub const FONT_COLLECTION: u16 = 0x07D5;
    pub const OUTLINE_TEXT_REF: u16 = 0x0F9E;
    pub const TEXT_HEADER: u16 = 0x0F9F;
    pub const TEXT_CHARS: u16 = 0x0FA0;
    pub const STYLE_TEXT_PROP: u16 = 0x0FA1;
    pub const TEXT_MASTER_STYLE: u16 = 0x0FA3;
    pub const TEXT_CF_EXCEPTION: u16 = 0x0FA4;
    pub const TEXT_PF_EXCEPTION: u16 = 0x0FA5;
    pub const TEXT_RULER: u16 = 0x0FA6;
    pub const TEXT_BYTES: u16 = 0x0FA8;
    pub const FONT_ENTITY: u16 = 0x0FB7;
    pub const SLIDE_NUMBER_MC: u16 = 0x0FD8;
    pub const SLIDE_LIST_WITH_TEXT: u16 = 0x0FF0;
    pub const USER_EDIT: u16 = 0x0FF5;
    pub const CURRENT_USER: u16 = 0x0FF6;
    pub const DATETIME_MC: u16 = 0x0FF7;
    pub const GENERIC_DATE_MC: u16 = 0x0FF8;
    pub const PLACEHOLDER_ATOM: u16 = 0x0BC3;
    pub const PERSIST_DIRECTORY: u16 = 0x1772;
    pub const CRYPT_SESSION: u16 = 0x2F14;

    pub const DGG_CONTAINER: u16 = 0xF000;
    pub const BSTORE: u16 = 0xF001;
    pub const DG_CONTAINER: u16 = 0xF002;
    pub const SPGR_CONTAINER: u16 = 0xF003;
    pub const SP_CONTAINER: u16 = 0xF004;
    pub const FBSE: u16 = 0xF007;
    pub const FSPGR: u16 = 0xF009;
    pub const FSP: u16 = 0xF00A;
    pub const FOPT: u16 = 0xF00B;
    pub const CLIENT_TEXTBOX: u16 = 0xF00D;
    pub const CHILD_ANCHOR: u16 = 0xF00F;
    pub const CLIENT_ANCHOR: u16 = 0xF010;
    pub const CLIENT_DATA: u16 = 0xF011;
    pub const SECONDARY_FOPT: u16 = 0xF121;
    pub const TERTIARY_FOPT: u16 = 0xF122;
}

/// `headerToken` of a `CurrentUserAtom` for an encrypted document.
const ENCRYPTED_TOKEN: u32 = 0xF3D1_C4DF;
/// `headerToken` of an unencrypted PowerPoint 97+ document.
const PLAIN_TOKEN: u32 = 0xE391_C05F;
/// Edits followed at most along the `offsetLastEdit` chain.
const MAX_EDITS: usize = 4096;

/// A master, slide or notes page as listed in a `SlideListWithTextContainer`.
#[derive(Clone, Debug)]
pub struct Listed<'a> {
    /// The container (`SlideContainer`, `MainMasterContainer`, `NotesContainer`).
    pub rec: Rec<'a>,
    /// The id other records use for it (`SlideAtom.masterIdRef`, `NotesAtom.slideIdRef`).
    pub slide_id: u32,
    /// The outline text stored in the list after its `SlidePersistAtom` (placeholders point at it
    /// with an `OutlineTextRefAtom`).
    pub outline: Vec<Records<'a>>,
}

/// The parsed structure of a `.ppt`.
pub struct Doc<'a> {
    pub document: Rec<'a>,
    pub masters: Vec<Listed<'a>>,
    pub slides: Vec<Listed<'a>>,
    pub notes: Vec<Listed<'a>>,
}

/// Read the structure from the `Current User` and `PowerPoint Document` streams.
pub fn read<'a>(current_user: &[u8], stream: &'a [u8]) -> Result<Doc<'a>, PptError> {
    let damaged = |s: &str| PptError::Damaged(s.to_string());
    // The current user names the newest edit; without it (some writers omit the stream), scan
    // for the last UserEditAtom in the stream.
    let mut edit_at = None;
    if let Some(cu) = Rec::at(current_user, 0).filter(|r| r.kind == rt::CURRENT_USER) {
        match cu.u32(4) {
            Some(ENCRYPTED_TOKEN) => return Err(PptError::Encrypted),
            Some(PLAIN_TOKEN) => edit_at = cu.u32(8).map(|v| v as usize),
            _ => return Err(PptError::TooOld),
        }
    }
    let edit_at = match edit_at.filter(|&at| Rec::at(stream, at).is_some_and(|r| r.kind == rt::USER_EDIT)) {
        Some(at) => at,
        None => Records::new(stream).filter(|r| r.kind == rt::USER_EDIT).last().map(|r| r.offset).ok_or_else(|| damaged("no edit record"))?,
    };
    // Walk the edits newest first; the first entry seen for a persist id wins.
    let mut persist: HashMap<u32, usize> = HashMap::new();
    let mut doc_ref = None;
    let mut seen = HashSet::new();
    let mut at = edit_at;
    for _ in 0..MAX_EDITS {
        if !seen.insert(at) {
            break;
        }
        let Some(edit) = Rec::at(stream, at).filter(|r| r.kind == rt::USER_EDIT) else { break };
        if doc_ref.is_none() {
            doc_ref = edit.u32(16);
            // An encryptSessionPersistIdRef follows only in encrypted documents.
            if edit.data.len() >= 32 {
                return Err(PptError::Encrypted);
            }
        }
        if let Some(dir) = edit.u32(12).and_then(|o| Rec::at(stream, o as usize)).filter(|r| r.kind == rt::PERSIST_DIRECTORY) {
            read_persist_directory(dir.data, &mut persist);
        }
        match edit.u32(8) {
            Some(0) | None => break,
            Some(prev) => at = prev as usize,
        }
    }
    let lookup = |id: u32| persist.get(&id).and_then(|&o| Rec::at(stream, o));
    let document = doc_ref.and_then(lookup).filter(|r| r.kind == rt::DOCUMENT).ok_or_else(|| damaged("no document container"))?;
    if lookup_kind(&persist, stream, rt::CRYPT_SESSION) {
        return Err(PptError::Encrypted);
    }
    let mut doc = Doc { document, masters: vec![], slides: vec![], notes: vec![] };
    for list in document.children_of(rt::SLIDE_LIST_WITH_TEXT) {
        let out = match list.instance {
            0 => &mut doc.slides,
            1 => &mut doc.masters,
            2 => &mut doc.notes,
            _ => continue,
        };
        let mut current: Option<Listed> = None;
        for r in list.children() {
            if r.kind == rt::SLIDE_PERSIST_ATOM {
                if let Some(l) = current.take() {
                    out.push(l);
                }
                let (Some(pid), Some(slide_id)) = (r.u32(0), r.u32(12)) else { continue };
                match lookup(pid) {
                    Some(rec) => current = Some(Listed { rec, slide_id, outline: vec![] }),
                    None => log::warn!("ppt: slide persist id {pid} has no object"),
                }
            } else if r.kind == rt::TEXT_HEADER
                && let Some(l) = current.as_mut()
            {
                // An outline text block starts at its header and runs to the next header or persist atom.
                let rest = list.data.get(r.offset..).unwrap_or(&[]);
                l.outline.push(Records::new(rest));
            }
        }
        if let Some(l) = current.take() {
            out.push(l);
        }
    }
    Ok(doc)
}

fn read_persist_directory(b: &[u8], persist: &mut HashMap<u32, usize>) {
    let mut c = crate::record::Cursor::new(b);
    while let Some(head) = c.u32() {
        let first = head & 0x000F_FFFF;
        let count = head >> 20;
        for k in 0..count {
            let Some(off) = c.u32() else { return };
            persist.entry(first.saturating_add(k)).or_insert(off as usize);
        }
    }
}

fn lookup_kind(persist: &HashMap<u32, usize>, stream: &[u8], kind: u16) -> bool {
    persist.values().any(|&o| Rec::at(stream, o).is_some_and(|r| r.kind == kind))
}

/// The `DocumentAtom`: slide and notes sizes in master units (576 per inch), first slide number.
pub struct DocumentAtom {
    pub slide_w: i32,
    pub slide_h: i32,
    pub notes_w: i32,
    pub notes_h: i32,
    pub first_slide_number: u16,
}

impl Doc<'_> {
    pub fn document_atom(&self) -> Option<DocumentAtom> {
        let a = self.document.child(rt::DOCUMENT_ATOM)?;
        Some(DocumentAtom {
            slide_w: a.i32(0)?,
            slide_h: a.i32(4)?,
            notes_w: a.i32(8)?,
            notes_h: a.i32(12)?,
            first_slide_number: a.u16(32).unwrap_or(1),
        })
    }
}

/// The `SlideAtom` of a slide or master.
#[derive(Clone, Copy, Debug, Default)]
pub struct SlideAtom {
    /// `SlideLayoutType`.
    pub geom: u32,
    pub master_id: u32,
    pub follow_master_objects: bool,
    pub follow_master_scheme: bool,
    pub follow_master_background: bool,
}

pub fn slide_atom(container: &Rec) -> SlideAtom {
    let Some(a) = container.child(rt::SLIDE_ATOM) else {
        return SlideAtom { follow_master_objects: true, follow_master_scheme: true, follow_master_background: true, ..Default::default() };
    };
    let flags = a.u16(20).unwrap_or(0x7);
    SlideAtom {
        geom: a.u32(0).unwrap_or(0),
        master_id: a.u32(12).unwrap_or(0),
        follow_master_objects: flags & 0x1 != 0,
        follow_master_scheme: flags & 0x2 != 0,
        follow_master_background: flags & 0x4 != 0,
    }
}

/// Font names by index (`FontCollectionContainer` in the environment).
pub fn fonts(doc: &Doc) -> Vec<String> {
    let Some(coll) = doc.document.child(rt::ENVIRONMENT).and_then(|e| e.child(rt::FONT_COLLECTION)) else { return vec![] };
    coll.children_of(rt::FONT_ENTITY)
        .map(|f| {
            let units: Vec<u16> = (0..32).filter_map(|i| f.u16(i * 2)).take_while(|&u| u != 0).collect();
            String::from_utf16_lossy(&units)
        })
        .collect()
}
