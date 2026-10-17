//! The presentation ↔ Yjs mapping (DeckCraft schema 1) and the per-user undo.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use deckcraft_model::text::TextBody;
use deckcraft_model::{Layout, Master, MediaId, MediaItem, Presentation, Shape, ShapeId, Slide, SlideId, Theme};
use serde_json::{Map as JsonMap, Value as Json, json};
use yrs::types::{Event, PathSegment};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, Array, ArrayPrelim, ArrayRef, Assoc, DeepObservable, Doc, IndexedSequence, Map, MapPrelim, MapRef, Observable, OffsetKind, Options, Origin,
    Out, ReadTxn, StateVector, StickyIndex, Text, TextPrelim, TextRef, Transact, TransactionMut, UndoManager, Update, WriteTxn,
};

use crate::json::lenient;
use crate::text::{self, Atom};
use crate::{APP, CollabError, Rng, SCHEMA, id_str, parse_id};

/// Transaction origins.
const LOCAL: &str = "deckcraft/local";
const REMOTE: &str = "deckcraft/remote";
const SEED: &str = "seed";

/// Root names.
const META: &str = "meta";
const SLIDES: &str = "slides";
const PRES: &str = "presentation";
const MASTERS: &str = "masters";
const LAYOUTS: &str = "layouts";
const THEME: &str = "theme";
const MEDIA: &str = "media";

/// Keys of a slide map that aren't slide properties.
const SLIDE_SHAPES: &str = "shapes";
const NOTES: &str = "notes";
const NOTES_BODY: &str = "notesBody";
/// Keys of a shape map that aren't shape properties.
const CHILDREN: &str = "children";
const TEXT: &str = "text";
const TEXT_BODY: &str = "textBody";
const SHAPE_PREFIX: &str = "shape:";

/// Updates kept while they wait for earlier ones.
const MAX_HELD: usize = 1_000;
/// Deepest group nesting read.
const MAX_DEPTH: usize = 64;
/// Most slides and shapes per slide read from a room (a hostile peer's lists are cut there).
const MAX_ITEMS: usize = 100_000;
/// Longest LCS table when reordering a list (beyond, a simpler reorder).
const MAX_LCS: usize = 4_000_000;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

fn origin_is(txn: &TransactionMut, name: &str) -> bool {
    txn.origin().is_some_and(|o| *o == Origin::from(name))
}

/// Remote updates and undos change the model; our own commits and the seed already did.
fn recorded(txn: &TransactionMut) -> bool {
    !origin_is(txn, LOCAL) && !origin_is(txn, SEED)
}

fn slide_root(id: u32) -> String {
    format!("slide:{}", id_str(id))
}

fn shape_key(id: u32) -> String {
    format!("{SHAPE_PREFIX}{}", id_str(id))
}

fn out_str(o: Option<Out>) -> Option<Arc<str>> {
    match o {
        Some(Out::Any(Any::String(s))) => Some(s),
        _ => None,
    }
}

fn sub_map<R: ReadTxn>(m: &MapRef, txn: &R, key: &str) -> Option<MapRef> {
    match m.get(txn, key) {
        Some(Out::YMap(x)) => Some(x),
        _ => None,
    }
}

fn sub_array<R: ReadTxn>(m: &MapRef, txn: &R, key: &str) -> Option<ArrayRef> {
    match m.get(txn, key) {
        Some(Out::YArray(x)) => Some(x),
        _ => None,
    }
}

fn sub_text<R: ReadTxn>(m: &MapRef, txn: &R, key: &str) -> Option<TextRef> {
    match m.get(txn, key) {
        Some(Out::YText(x)) => Some(x),
        _ => None,
    }
}

fn ensure_array(m: &MapRef, txn: &mut TransactionMut, key: &str) -> ArrayRef {
    match sub_array(m, txn, key) {
        Some(a) => a,
        None => m.insert(txn, key, ArrayPrelim::default()),
    }
}

/// The strings of an array, first occurrence kept (schema 1: readers keep the first of a
/// duplicated id), anything else skipped.
fn ids_of<R: ReadTxn>(a: Option<&ArrayRef>, txn: &R) -> Vec<String> {
    let Some(a) = a else { return Vec::new() };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for v in a.iter(txn) {
        if let Out::Any(Any::String(s)) = v
            && seen.insert(s.clone())
        {
            out.push(s.to_string());
            if out.len() >= MAX_ITEMS {
                break;
            }
        }
    }
    out
}

/// A JSON value stored as JSON text.
fn get_json<R: ReadTxn>(m: &MapRef, txn: &R, key: &str) -> Option<Json> {
    out_str(m.get(txn, key)).and_then(|s| serde_json::from_str(&s).ok())
}

fn put_json(m: &MapRef, txn: &mut TransactionMut, key: &str, v: &Json) {
    m.insert(txn, key, v.to_string());
}

/// The entries of a map that hold JSON text, but `skip` and the `shape:` keys.
fn json_entries<R: ReadTxn>(m: &MapRef, txn: &R, skip: &[&str]) -> JsonMap<String, Json> {
    let mut obj = JsonMap::new();
    for (k, v) in m.iter(txn) {
        if skip.contains(&k) || k.starts_with(SHAPE_PREFIX) {
            continue;
        }
        if let Out::Any(Any::String(s)) = v
            && let Ok(j) = serde_json::from_str::<Json>(&s)
        {
            obj.insert(k.to_string(), j);
        }
    }
    obj
}

/// Writes the keys of `new` that differ from `old`, and removes those `new` lost.
fn diff_keys(m: &MapRef, txn: &mut TransactionMut, old: &JsonMap<String, Json>, new: &JsonMap<String, Json>) {
    for (k, v) in new {
        if old.get(k) != Some(v) {
            put_json(m, txn, k, v);
        }
    }
    for k in old.keys() {
        if !new.contains_key(k) {
            m.remove(txn, k);
        }
    }
}

fn object(v: Json) -> JsonMap<String, Json> {
    match v {
        Json::Object(o) => o,
        _ => JsonMap::new(),
    }
}

/// A slide's properties as schema 1 stores them (`id`, `shapes` and `notes` are structure).
fn slide_props(s: &Slide) -> JsonMap<String, Json> {
    let mut o = object(serde_json::to_value(s).unwrap_or(Json::Null));
    for k in ["id", "shapes", "notes"] {
        o.remove(k);
    }
    o
}

/// A shape's properties (`id`, `text` and a group's children are structure).
fn shape_props(s: &Shape) -> JsonMap<String, Json> {
    let mut o = object(serde_json::to_value(s).unwrap_or(Json::Null));
    o.remove("id");
    o.remove("text");
    if let Some(Json::Object(k)) = o.get_mut("kind") {
        k.remove("children");
    }
    o
}

/// A text body's properties but its paragraphs (`{body, listStyle}`).
fn body_props(b: &TextBody) -> JsonMap<String, Json> {
    let mut o = object(serde_json::to_value(b).unwrap_or(Json::Null));
    o.remove("paragraphs");
    o
}

/// The presentation-wide part: everything but slides, masters, media and the id counter.
fn shell(p: &Presentation) -> Presentation {
    Presentation { slides: vec![], masters: vec![], media: vec![], next_id: 0, chat: Default::default(), ..p.clone() }
}

fn pres_props(p: &Presentation) -> JsonMap<String, Json> {
    let mut o = object(serde_json::to_value(shell(p)).unwrap_or(Json::Null));
    for k in ["slides", "masters", "media", "nextId", "chat"] {
        o.remove(k);
    }
    o.insert(MASTERS.into(), Json::Array(p.masters.iter().map(|m| Json::from(id_str(m.id.0))).collect()));
    o
}

fn master_props(m: &Master) -> JsonMap<String, Json> {
    let mut o = object(serde_json::to_value(Master { layouts: vec![], theme: Theme::default(), ..m.clone() }).unwrap_or(Json::Null));
    o.remove("id");
    o.remove("theme");
    o.insert("layouts".into(), Json::Array(m.layouts.iter().map(|l| Json::from(id_str(l.id.0))).collect()));
    o
}

fn layout_props(l: &Layout) -> JsonMap<String, Json> {
    let mut o = object(serde_json::to_value(l).unwrap_or(Json::Null));
    o.remove("id");
    o
}

/// Every shape of a tree by id (groups' children included), depth-first.
fn flatten<'a>(shapes: &'a [Shape], out: &mut Vec<&'a Shape>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    for s in shapes {
        out.push(s);
        flatten(s.children(), out, depth + 1);
    }
}

/// Refuses an update that writes another app's or schema's `meta` (it's dropped, not applied:
/// the room goes on syncing). Only updates that mention `meta` are looked at, in a scratch
/// document.
fn foreign_meta(update: &[u8]) -> Result<(), CollabError> {
    if !update.windows(4).any(|w| w == b"meta") {
        return Ok(());
    }
    let Ok(u) = Update::decode_v1(update) else { return Ok(()) };
    let scratch = Doc::new();
    let applied = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut txn = scratch.transact_mut();
        txn.apply_update(u).is_ok()
    }));
    if !matches!(applied, Ok(true)) {
        return Ok(());
    }
    let txn = scratch.transact();
    meta_of(&txn).map(|_| ())
}

/// `Ok(true)` when `meta` is ours, `Ok(false)` when it isn't there yet.
fn meta_of<R: ReadTxn>(txn: &R) -> Result<bool, CollabError> {
    let Some(meta) = txn.get_map(META) else { return Ok(false) };
    let app = out_str(meta.get(txn, "app"));
    let schema = match meta.get(txn, "schema") {
        Some(Out::Any(Any::Number(n))) => Some(crate::json::to_json(&Any::Number(n))).and_then(|j| j.as_f64()).map(|f| f as u64),
        _ => None,
    };
    match (app, schema) {
        (None, None) => Ok(false),
        (Some(a), Some(s)) if &*a == APP && s == u64::from(SCHEMA) => Ok(true),
        (a, s) => Err(CollabError::Mismatch { app: a.map(|a| a.to_string()).unwrap_or_else(|| "another app".into()), schema: s.unwrap_or(0) }),
    }
}

/// Makes `arr` hold `desired` (ids, in order) with few edits: duplicates and ids that went are
/// removed, the longest run already in order stays, the others are inserted where they belong.
/// Anything in the array that isn't a string is left alone.
fn reorder(arr: &ArrayRef, txn: &mut TransactionMut, desired: &[String]) {
    let want: HashSet<&str> = desired.iter().map(String::as_str).collect();
    // Remove duplicates and ids not wanted, from the end.
    let raw: Vec<Option<Arc<str>>> = arr
        .iter(txn)
        .map(|v| match v {
            Out::Any(Any::String(s)) => Some(s),
            _ => None,
        })
        .collect();
    let mut seen = HashSet::new();
    let mut drop = Vec::new();
    for (i, v) in raw.iter().enumerate() {
        if let Some(s) = v
            && (!want.contains(&**s) || !seen.insert(s.clone()))
        {
            drop.push(i as u32);
        }
    }
    for i in drop.into_iter().rev() {
        arr.remove(txn, i);
    }
    let have = ids_of(Some(arr), txn);
    if have == desired {
        return;
    }
    // The longest common subsequence stays.
    let keep: HashSet<String> = lcs(&have, desired);
    let gone: Vec<u32> = {
        let mut out = Vec::new();
        for (i, v) in arr.iter(txn).enumerate() {
            if let Out::Any(Any::String(s)) = v
                && !keep.contains(&*s)
            {
                out.push(i as u32);
            }
        }
        out
    };
    for i in gone.into_iter().rev() {
        arr.remove(txn, i);
    }
    // Insert the rest where they belong.
    for (i, id) in desired.iter().enumerate() {
        if keep.contains(id) {
            continue;
        }
        let at = raw_index(arr, txn, i);
        arr.insert(txn, at, id.as_str());
    }
}

/// The array index of the `n`-th string (its length when there are fewer).
fn raw_index<R: ReadTxn>(arr: &ArrayRef, txn: &R, n: usize) -> u32 {
    let mut count = 0;
    for (i, v) in arr.iter(txn).enumerate() {
        if matches!(v, Out::Any(Any::String(_))) {
            if count == n {
                return i as u32;
            }
            count += 1;
        }
    }
    arr.len(txn)
}

/// The ids of a longest common subsequence of `a` and `b` (both without duplicates).
fn lcs(a: &[String], b: &[String]) -> HashSet<String> {
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MAX_LCS {
        // Too long for the table: keep the common start and end.
        let mut out = HashSet::new();
        let mut i = 0;
        while i < n.min(m) && a.get(i) == b.get(i) {
            if let Some(x) = a.get(i) {
                out.insert(x.clone());
            }
            i += 1;
        }
        let mut j = 0;
        while j < n.min(m) - i && a.get(n - 1 - j) == b.get(m - 1 - j) {
            if let Some(x) = a.get(n - 1 - j) {
                out.insert(x.clone());
            }
            j += 1;
        }
        return out;
    }
    let w = m + 1;
    let mut t = vec![0u32; (n + 1) * w];
    let at = |i: usize, j: usize| i * w + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let v = if a.get(i) == b.get(j) {
                t.get(at(i + 1, j + 1)).copied().unwrap_or(0) + 1
            } else {
                t.get(at(i + 1, j)).copied().unwrap_or(0).max(t.get(at(i, j + 1)).copied().unwrap_or(0))
            };
            if let Some(c) = t.get_mut(at(i, j)) {
                *c = v;
            }
        }
    }
    let mut out = HashSet::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a.get(i) == b.get(j) {
            if let Some(x) = a.get(i) {
                out.insert(x.clone());
            }
            i += 1;
            j += 1;
        } else if t.get(at(i + 1, j)).copied().unwrap_or(0) >= t.get(at(i, j + 1)).copied().unwrap_or(0) {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// A text position in a peer's selection: in a shape's text, or the slide's notes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextAt {
    /// The shape whose text it is (`None`: the notes).
    pub shape: Option<u32>,
    pub notes: bool,
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

/// The model after remote changes (or an undo), and what changed for the highlight of others'
/// edits.
#[derive(Debug)]
pub struct Refreshed {
    pub doc: Presentation,
    /// Shapes others changed: (slide id, shape ids).
    pub changed: Vec<(u32, Vec<u32>)>,
    /// Slides were added, removed or moved, or presentation-wide settings changed.
    pub structural: bool,
    /// The chat changed (others sent, edited, deleted or cleared).
    pub chat: bool,
}

#[derive(Default, Debug)]
struct SlideChanges {
    shapes: HashSet<u32>,
}

/// What remote transactions (and undos) changed since the last [`Replica::refresh`].
#[derive(Default, Debug)]
struct Changes {
    list: bool,
    pres: bool,
    masters: bool,
    media: bool,
    meta: bool,
    chat: bool,
    slides: HashMap<u32, SlideChanges>,
}

impl Changes {
    fn any(&self) -> bool {
        self.list || self.pres || self.masters || self.media || self.chat || !self.slides.is_empty()
    }
}

/// A shared presentation: the Yjs document, kept in step with the engine's model.
pub struct Replica {
    doc: Doc,
    client_id: u64,
    undo: UndoManager,
    changes: Arc<Mutex<Changes>>,
    outbox: Arc<Mutex<Vec<Vec<u8>>>>,
    /// Slide roots observed and in the undo scope.
    observed: HashSet<u32>,
    /// The model as last synced; `None` until the room's document is there (joining).
    base: Option<Arc<Presentation>>,
    rng: Rng,
    /// Updates waiting for earlier ones ([`Replica::apply_update`]).
    held: Vec<Vec<u8>>,
}

impl Replica {
    /// An empty replica with Yjs client id `client_id` (below 2^53).
    pub fn new(client_id: u64) -> Replica {
        let client_id = client_id & ((1u64 << 53) - 1);
        // Garbage collection off: with it, yrs 0.28 can free items the undo manager and later
        // encodings still point at (GridCraft's convergence fuzz crashed that way). Offsets in
        // texts are UTF-16 code units, as in Yjs.
        let mut options = Options::with_client_id(yrs::block::ClientID::new(client_id));
        options.skip_gc = true;
        options.offset_kind = OffsetKind::Utf16;
        let doc = Doc::with_options(options);
        let changes: Arc<Mutex<Changes>> = Default::default();
        let outbox: Arc<Mutex<Vec<Vec<u8>>>> = Default::default();
        let ob = outbox.clone();
        if let Err(e) = doc.observe_update_v1("deckcraft/out", move |txn, e| {
            if !origin_is(txn, REMOTE) {
                lock(&ob).push(e.update.clone());
            }
        }) {
            log::warn!("collab: can't observe updates: {e}");
        }
        let slides = doc.get_or_insert_array(SLIDES);
        let pres = doc.get_or_insert_map(PRES);
        let masters = doc.get_or_insert_map(MASTERS);
        let layouts = doc.get_or_insert_map(LAYOUTS);
        let theme = doc.get_or_insert_map(THEME);
        let media = doc.get_or_insert_map(MEDIA);
        let meta = doc.get_or_insert_map(META);
        // The chat: observed, but never in the undo scope (Undo never removes a message).
        let chat = doc.get_or_insert_array(crate::chat::CHAT);
        let chat_meta = doc.get_or_insert_map(crate::chat::CHAT_META);
        let ch = changes.clone();
        chat.observe_deep("deckcraft/chat", move |txn, _| {
            if recorded(txn) {
                lock(&ch).chat = true;
            }
        });
        let ch = changes.clone();
        chat_meta.observe("deckcraft/chatMeta", move |txn, _| {
            if recorded(txn) {
                lock(&ch).chat = true;
            }
        });
        let ch = changes.clone();
        slides.observe("deckcraft/slides", move |txn, _| {
            if recorded(txn) {
                lock(&ch).list = true;
            }
        });
        let ch = changes.clone();
        pres.observe("deckcraft/presentation", move |txn, _| {
            if recorded(txn) {
                lock(&ch).pres = true;
            }
        });
        for m in [&masters, &layouts, &theme] {
            let ch = changes.clone();
            m.observe("deckcraft/masters", move |txn, _| {
                if recorded(txn) {
                    lock(&ch).masters = true;
                }
            });
        }
        let ch = changes.clone();
        media.observe_deep("deckcraft/media", move |txn, _| {
            if recorded(txn) {
                lock(&ch).media = true;
            }
        });
        let ch = changes.clone();
        meta.observe("deckcraft/meta", move |txn, _| {
            if recorded(txn) {
                lock(&ch).meta = true;
            }
        });
        let options = yrs::undo::Options {
            capture_timeout_millis: 0,
            tracked_origins: [Origin::from(LOCAL)].into_iter().collect(),
            capture_transaction: None,
            timestamp: Arc::new(|| 0u64),
            init_undo_stack: vec![],
            init_redo_stack: vec![],
        };
        let mut undo = UndoManager::with_options(options);
        for root in [&pres, &masters, &layouts, &theme, &media] {
            undo.expand_scope(&doc, root);
        }
        undo.expand_scope(&doc, &slides);
        Replica { doc, client_id, undo, changes, outbox, observed: HashSet::new(), base: None, rng: Rng::new(client_id), held: Vec::new() }
    }

    pub fn client_id(&self) -> u64 {
        self.client_id
    }

    /// The room's document is here (always after [`Replica::seed`]; after joining, once a peer's
    /// state arrived).
    pub fn is_ready(&self) -> bool {
        self.base.is_some()
    }

    /// The presentation as last synced.
    pub fn base(&self) -> Option<&Arc<Presentation>> {
        self.base.as_ref()
    }

    /// Takes `p` as the synced presentation: the same shared content as the current one, after
    /// local-only changes (the id counter).
    pub fn rebase(&mut self, p: Arc<Presentation>) {
        if self.base.is_some() {
            self.base = Some(p);
        }
    }

    /// Checks `meta`: `Ok(true)` ours, `Ok(false)` not there yet, an error for another app or
    /// schema.
    pub fn check_meta(&self) -> Result<bool, CollabError> {
        meta_of(&self.doc.transact())
    }

    /// The room's title (`meta.title`).
    pub fn title(&self) -> Option<String> {
        let txn = self.doc.transact();
        txn.get_map(META).and_then(|m| out_str(m.get(&txn, "title"))).map(|s| s.to_string())
    }

    fn take_outbox(&self) -> Result<Option<Vec<u8>>, CollabError> {
        let ups: Vec<Vec<u8>> = std::mem::take(&mut *lock(&self.outbox));
        match ups.len() {
            0 => Ok(None),
            1 => Ok(ups.into_iter().next()),
            _ => yrs::merge_updates_v1(ups.iter().map(|u| u.as_slice()).collect::<Vec<_>>())
                .map(Some)
                .map_err(|e| CollabError::Other(format!("merging updates: {e}"))),
        }
    }

    /// Observes a slide's root (remote changes) and adds it to the undo scope.
    fn observe_slide(&mut self, id: u32) {
        if !self.observed.insert(id) {
            return;
        }
        let root = self.doc.get_or_insert_map(slide_root(id));
        let ch = self.changes.clone();
        root.observe_deep("deckcraft/slide", move |txn, events| {
            if !recorded(txn) {
                return;
            }
            let mut ch = lock(&ch);
            let sc = ch.slides.entry(id).or_default();
            for e in events.iter() {
                let first = match e.path().front() {
                    Some(PathSegment::Key(k)) => Some(k.clone()),
                    _ => None,
                };
                match first {
                    Some(k) => {
                        if let Some(n) = k.strip_prefix(SHAPE_PREFIX).and_then(parse_id) {
                            sc.shapes.insert(n);
                        }
                    }
                    None => {
                        if let Event::Map(m) = e {
                            for k in m.keys(txn).keys() {
                                if let Some(n) = k.strip_prefix(SHAPE_PREFIX).and_then(parse_id) {
                                    sc.shapes.insert(n);
                                }
                            }
                        }
                    }
                }
            }
        });
        self.undo.expand_scope(&self.doc, &root);
    }

    // ------------------------------------------------------------ sharing and joining

    /// Shares `p`: builds the shared document from it (one transaction, origin `seed`). Returns
    /// the update (the whole document).
    pub fn seed(&mut self, p: Arc<Presentation>, title: &str) -> Result<Vec<u8>, CollabError> {
        let doc = self.doc.clone();
        {
            let mut txn = doc.transact_mut_with(SEED);
            let meta = txn.get_or_insert_map(META);
            meta.insert(&mut txn, "app", APP);
            meta.insert(&mut txn, "schema", i64::from(SCHEMA));
            meta.insert(&mut txn, "title", title);
            write_all(&mut txn, None, &p);
        }
        for s in &p.slides {
            self.observe_slide(s.id.0);
        }
        let mut p = (*p).clone();
        reseed_ids(&mut p, &mut self.rng);
        self.base = Some(Arc::new(p));
        *lock(&self.changes) = Changes::default();
        Ok(self.take_outbox()?.unwrap_or_default())
    }

    // ------------------------------------------------------------ local edits

    /// Turns the change from the last synced presentation to `cur` into one transaction with the
    /// local origin (one undo step). Returns the update to send, `None` when nothing shared
    /// changed.
    pub fn commit(&mut self, cur: &Arc<Presentation>) -> Result<Option<Vec<u8>>, CollabError> {
        let Some(base) = self.base.clone() else { return Err(CollabError::NotReady) };
        if Arc::ptr_eq(&base, cur) {
            return Ok(None);
        }
        let doc = self.doc.clone();
        {
            let mut txn = doc.transact_mut_with(LOCAL);
            write_all(&mut txn, Some(&base), cur);
        }
        for s in &cur.slides {
            self.observe_slide(s.id.0);
        }
        self.base = Some(cur.clone());
        self.undo.reset();
        self.take_outbox()
    }

    /// Shares only the chat's change (a viewer chats; their other edits never leave). Returns the
    /// update to send.
    pub fn commit_chat(&mut self, chat: &Arc<deckcraft_model::chat::Chat>) -> Result<Option<Vec<u8>>, CollabError> {
        let Some(base) = self.base.clone() else { return Err(CollabError::NotReady) };
        if Arc::ptr_eq(&base.chat, chat) {
            return Ok(None);
        }
        let doc = self.doc.clone();
        {
            let mut txn = doc.transact_mut_with(LOCAL);
            crate::chat::write(&mut txn, Some(&base.chat), chat);
        }
        self.base = Some(Arc::new(Presentation { chat: chat.clone(), ..(*base).clone() }));
        self.take_outbox()
    }

    // ------------------------------------------------------------ remote changes

    /// Applies a remote update (sync step 2 or an incremental update) to the shared document.
    /// The model follows at the next [`Replica::refresh`]. An update that skips some of a peer's
    /// changes waits until the gap is filled, by a later update or a sync step 2 (see
    /// [`Replica::has_gaps`]): applied early, its changes inside existing shared types wouldn't be
    /// reported.
    pub fn apply_update(&mut self, update: &[u8]) -> Result<(), CollabError> {
        if update.len() > crate::MAX_UPDATE {
            return Err(CollabError::TooLarge(update.len()));
        }
        let u = Update::decode_v1(update).map_err(|e| CollabError::Corrupt(e.to_string()))?;
        foreign_meta(update)?;
        if self.gap(&u) {
            if self.held.len() >= MAX_HELD {
                self.held.remove(0);
            }
            self.held.push(update.to_vec());
            return Ok(());
        }
        self.integrate(u)?;
        loop {
            let ready = self.held.iter().position(|h| Update::decode_v1(h).is_ok_and(|u| !self.gap(&u)));
            let Some(i) = ready else { break };
            let h = self.held.remove(i);
            if let Ok(u) = Update::decode_v1(&h)
                && let Err(e) = self.integrate(u)
            {
                log::warn!("collab: a held update failed: {e}");
            }
        }
        Ok(())
    }

    /// Whether `u` starts past what we have of one of its clients.
    fn gap(&self, u: &Update) -> bool {
        let have = self.doc.transact().state_vector();
        u.state_vector_lower().iter().any(|(c, clock)| *clock > have.get(c))
    }

    fn integrate(&mut self, u: Update) -> Result<(), CollabError> {
        let doc = self.doc.clone();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut txn = doc.transact_mut_with(REMOTE);
            txn.apply_update(u).map_err(|e| CollabError::Corrupt(e.to_string()))
        }));
        match r {
            Ok(r) => r,
            Err(_) => Err(CollabError::Corrupt("the update couldn't be applied".into())),
        }
    }

    /// Some updates wait for others that haven't arrived: ask the peers for them (sync step 1).
    pub fn has_gaps(&self) -> bool {
        !self.held.is_empty()
    }

    /// Whether remote changes (or an undo) are waiting for [`Replica::refresh`].
    pub fn has_changes(&self) -> bool {
        let ch = lock(&self.changes);
        ch.any() || ch.meta || self.base.is_none()
    }

    /// Brings remote changes into the model. `cur` is the engine's presentation, already
    /// committed (so it equals the last synced one). While joining, `cur` is the starting point
    /// whose masters are kept when the room has none, and this builds the first presentation once
    /// the room's document arrived. `None`: nothing to do.
    pub fn refresh(&mut self, cur: &Arc<Presentation>) -> Result<Option<Refreshed>, CollabError> {
        if self.base.is_none() {
            if !self.check_meta()? {
                return Ok(None);
            }
            *lock(&self.changes) = Changes::default();
            let mut p = {
                let txn = self.doc.transact();
                read_all(&txn, cur)
            };
            reseed_ids(&mut p, &mut self.rng);
            for s in &p.slides {
                self.observe_slide(s.id.0);
            }
            self.base = Some(Arc::new(p.clone()));
            return Ok(Some(Refreshed { doc: p, changed: vec![], structural: true, chat: true }));
        }
        let ch = std::mem::take(&mut *lock(&self.changes));
        if ch.meta {
            self.check_meta()?;
        }
        if !ch.any() {
            return Ok(None);
        }
        let mut p = (**cur).clone();
        let mut changed = Vec::new();
        {
            let txn = self.doc.transact();
            if ch.pres || ch.masters {
                let keep = std::mem::take(&mut p.slides);
                let media = std::mem::take(&mut p.media);
                let next = p.next_id;
                let masters = if ch.masters { read_masters(&txn).unwrap_or_else(|| cur.masters.clone()) } else { cur.masters.clone() };
                p = read_shell(&txn);
                p.slides = keep;
                p.media = media;
                p.masters = masters;
                p.next_id = next;
            }
            if ch.media {
                p.media = read_media(&txn);
            }
            p.chat = if ch.chat { Arc::new(crate::chat::read(&txn)) } else { cur.chat.clone() };
            let by_id: HashMap<u32, Arc<Slide>> = cur.slides.iter().map(|s| (s.id.0, s.clone())).collect();
            let order: Vec<u32> = if ch.list {
                let arr = txn.get_array(SLIDES);
                ids_of(arr.as_ref(), &txn).iter().filter_map(|s| parse_id(s)).collect()
            } else {
                cur.slides.iter().map(|s| s.id.0).collect()
            };
            let mut slides = Vec::with_capacity(order.len());
            for id in order {
                let dirty = ch.slides.get(&id);
                match (by_id.get(&id), dirty) {
                    (Some(s), None) => slides.push(s.clone()),
                    _ => {
                        if let Some(s) = read_slide(&txn, id) {
                            if let Some(d) = dirty {
                                changed.push((id, d.shapes.iter().copied().collect()));
                            }
                            slides.push(Arc::new(s));
                        }
                    }
                }
            }
            p.slides = slides;
        }
        for s in &p.slides {
            let id = s.id.0;
            self.observe_slide(id);
        }
        p.next_id = cur.next_id;
        if block_taken(&p) {
            reseed_ids(&mut p, &mut self.rng);
        }
        self.base = Some(Arc::new(p.clone()));
        Ok(Some(Refreshed { doc: p, changed, structural: ch.list || ch.pres || ch.masters, chat: ch.chat }))
    }

    // ------------------------------------------------------------ undo

    /// Undoes this person's last change (only theirs). Returns the update to send; the model
    /// follows at [`Replica::refresh`]. `None`: nothing to undo.
    pub fn undo(&mut self) -> Result<Option<Vec<u8>>, CollabError> {
        self.pop(true)
    }

    /// Redoes the last undone change.
    pub fn redo(&mut self) -> Result<Option<Vec<u8>>, CollabError> {
        self.pop(false)
    }

    fn pop(&mut self, undo: bool) -> Result<Option<Vec<u8>>, CollabError> {
        if self.base.is_none() {
            return Err(CollabError::NotReady);
        }
        let can = if undo { self.undo.can_undo() } else { self.undo.can_redo() };
        if !can {
            return Ok(None);
        }
        let um = &mut self.undo;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| if undo { um.undo_blocking() } else { um.redo_blocking() }));
        if r.is_err() {
            return Err(CollabError::Other("undo failed".into()));
        }
        self.take_outbox()
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    // ------------------------------------------------------------ sync

    /// Sync step 1: our state vector.
    pub fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    /// Whether a peer's state vector shows changes this document lacks (then ask back with our
    /// own sync step 1).
    pub fn lacks(&self, sv: &[u8]) -> bool {
        let Ok(theirs) = StateVector::decode_v1(sv) else { return false };
        let ours = self.doc.transact().state_vector();
        theirs.iter().any(|(c, clock)| *clock > ours.get(c))
    }

    /// Sync step 2: what a peer with state vector `sv` lacks.
    pub fn diff_for(&self, sv: &[u8]) -> Result<Vec<u8>, CollabError> {
        let sv = StateVector::decode_v1(sv).map_err(|e| CollabError::Corrupt(e.to_string()))?;
        Ok(self.doc.transact().encode_state_as_update_v1(&sv))
    }

    /// The whole document as one update (snapshots, recovery).
    pub fn full_state(&self) -> Vec<u8> {
        self.doc.transact().encode_state_as_update_v1(&StateVector::default())
    }

    // ------------------------------------------------------------ selections

    /// The Yjs text holding a text position (a shape's text, or the slide's notes).
    fn text_of<R: ReadTxn>(&self, txn: &R, slide: u32, shape: Option<u32>) -> Option<TextRef> {
        let root = txn.get_map(slide_root(slide).as_str())?;
        match shape {
            Some(id) => sub_text(&sub_map(&root, txn, &shape_key(id))?, txn, TEXT),
            None => sub_text(&root, txn, NOTES),
        }
    }

    /// The paragraphs of a text position's text in the synced model.
    fn paragraphs_of(&self, slide: u32, shape: Option<u32>) -> Option<Vec<deckcraft_model::text::Paragraph>> {
        let base = self.base.as_ref()?;
        let s = base.slide(SlideId(slide))?;
        Some(match shape {
            Some(id) => s.shape(ShapeId(id))?.text.as_ref()?.paragraphs.clone(),
            None => s.notes.paragraphs.clone(),
        })
    }

    /// A text position as a Yjs relative position (base64), which follows the text as others
    /// edit it.
    pub fn sticky(&self, slide: u32, shape: Option<u32>, pos: (usize, usize)) -> Option<String> {
        let paras = self.paragraphs_of(slide, shape)?;
        let at = text::offset_of(&paras, pos);
        let txn = self.doc.transact();
        let t = self.text_of(&txn, slide, shape)?;
        let at = at.min(t.len(&txn));
        // At the very end there's nothing after to stick to: then the character before.
        let si = t.sticky_index(&txn, at, Assoc::After).or_else(|| t.sticky_index(&txn, at, Assoc::Before))?;
        Some(crate::b64_encode(&si.encode_v1()))
    }

    /// Where a relative position [`Replica::sticky`] made is now, in the synced model.
    pub fn resolve(&self, slide: u32, shape: Option<u32>, sticky: &str) -> Option<(usize, usize)> {
        let bytes = crate::b64_decode(sticky).ok()?;
        let si = StickyIndex::decode_v1(&bytes).ok()?;
        let at = {
            let txn = self.doc.transact();
            let off = si.get_offset(&txn)?;
            off.index
        };
        let paras = self.paragraphs_of(slide, shape)?;
        Some(text::pos_of(&paras, at))
    }

    /// A selection as awareness carries it: `{slide, shapes: [ids], text?: {shape?, notes?,
    /// anchor, head}}`.
    pub fn selection_to_ids(&self, slide: u32, shapes: &[u32], text: Option<&TextAt>) -> Json {
        let mut sel = json!({"slide": id_str(slide), "shapes": shapes.iter().take(1000).map(|s| id_str(*s)).collect::<Vec<_>>()});
        if let Some(t) = text {
            let shape = if t.notes { None } else { t.shape };
            if let (Some(a), Some(h)) = (self.sticky(slide, shape, t.anchor), self.sticky(slide, shape, t.head)) {
                let mut o = json!({"anchor": a, "head": h});
                match shape {
                    Some(id) => o["shape"] = json!(id_str(id)),
                    None => o["notes"] = json!(true),
                }
                sel["text"] = o;
            }
        }
        sel
    }

    /// A peer's selection in the model: `(slide index, shape ids, text)`; shapes that are gone
    /// are left out.
    pub fn selection_from_ids(&self, sel: &Json) -> Option<(usize, Vec<u32>, Option<TextAt>)> {
        let base = self.base.as_ref()?;
        let slide = parse_id(sel.get("slide")?.as_str()?)?;
        let index = base.slide_index(SlideId(slide))?;
        let s = base.slides.get(index)?;
        let shapes: Vec<u32> = sel
            .get("shapes")
            .and_then(Json::as_array)
            .map(|a| a.iter().take(1000).filter_map(|v| parse_id(v.as_str()?)).filter(|id| s.shape(ShapeId(*id)).is_some()).collect())
            .unwrap_or_default();
        let text = sel.get("text").and_then(|t| {
            let notes = t.get("notes").and_then(Json::as_bool) == Some(true);
            let shape = if notes { None } else { Some(parse_id(t.get("shape")?.as_str()?)?) };
            let anchor = self.resolve(slide, shape, t.get("anchor")?.as_str()?)?;
            let head = self.resolve(slide, shape, t.get("head")?.as_str()?)?;
            Some(TextAt { shape, notes, anchor, head })
        });
        Some((index, shapes, text))
    }

    /// The presentation this replica's own document holds (tests: model vs document).
    pub fn debug_build(&self, cur: &Arc<Presentation>) -> Presentation {
        let txn = self.doc.transact();
        read_all(&txn, cur)
    }
}

// ---------------------------------------------------------------- writing

/// Writes `new` (`old` is what the document holds: `None` when it's empty).
fn write_all(txn: &mut TransactionMut, old: Option<&Presentation>, new: &Presentation) {
    write_pres(txn, old, new);
    write_masters(txn, old, new);
    write_media(txn, old, new);
    write_slides(txn, old, new);
    crate::chat::write(txn, old.map(|o| &o.chat), &new.chat);
}

fn write_pres(txn: &mut TransactionMut, old: Option<&Presentation>, new: &Presentation) {
    let same_masters = |o: &Presentation| o.masters.len() == new.masters.len() && o.masters.iter().zip(&new.masters).all(|(a, b)| a.id == b.id);
    if old.is_some_and(|o| same_masters(o) && shell(o) == shell(new)) {
        return;
    }
    let m = txn.get_or_insert_map(PRES);
    let before = old.map(pres_props).unwrap_or_default();
    diff_keys(&m, txn, &before, &pres_props(new));
}

fn write_masters(txn: &mut TransactionMut, old: Option<&Presentation>, new: &Presentation) {
    let olds: HashMap<u32, &Arc<Master>> = old.map(|o| o.masters.iter().map(|m| (m.id.0, m)).collect()).unwrap_or_default();
    if old.is_some_and(|o| o.masters.len() == new.masters.len() && o.masters.iter().zip(&new.masters).all(|(a, b)| Arc::ptr_eq(a, b))) {
        return;
    }
    let masters = txn.get_or_insert_map(MASTERS);
    let layouts = txn.get_or_insert_map(LAYOUTS);
    let theme = txn.get_or_insert_map(THEME);
    let mut kept = HashSet::new();
    for m in &new.masters {
        kept.insert(m.id.0);
        let prev = olds.get(&m.id.0).copied();
        if prev.is_some_and(|p| Arc::ptr_eq(p, m)) {
            continue;
        }
        let key = id_str(m.id.0);
        let now = master_props(m);
        if prev.is_none_or(|p| master_props(p) != now) {
            put_json(&masters, txn, &key, &Json::Object(now));
        }
        if prev.is_none_or(|p| p.theme != m.theme) {
            put_json(&theme, txn, &key, &serde_json::to_value(&m.theme).unwrap_or(Json::Null));
        }
        let prev_layouts: HashMap<u32, &Layout> = prev.map(|p| p.layouts.iter().map(|l| (l.id.0, l)).collect()).unwrap_or_default();
        for l in &m.layouts {
            if prev_layouts.get(&l.id.0).is_none_or(|p| *p != l) {
                put_json(&layouts, txn, &id_str(l.id.0), &Json::Object(layout_props(l)));
            }
        }
        let now_ids: HashSet<u32> = m.layouts.iter().map(|l| l.id.0).collect();
        for id in prev_layouts.keys() {
            if !now_ids.contains(id) {
                layouts.remove(txn, &id_str(*id));
            }
        }
    }
    for (id, m) in &olds {
        if !kept.contains(id) {
            masters.remove(txn, &id_str(*id));
            theme.remove(txn, &id_str(*id));
            for l in &m.layouts {
                layouts.remove(txn, &id_str(l.id.0));
            }
        }
    }
}

fn write_media(txn: &mut TransactionMut, old: Option<&Presentation>, new: &Presentation) {
    let olds: HashMap<u32, &MediaItem> = old.map(|o| o.media.iter().map(|m| (m.id.0, m)).collect()).unwrap_or_default();
    let same =
        |a: &MediaItem, b: &MediaItem| Arc::ptr_eq(&a.data, &b.data) && a.name == b.name && a.content_type == b.content_type && a.link == b.link;
    if old.is_some_and(|o| o.media.len() == new.media.len() && o.media.iter().zip(&new.media).all(|(a, b)| a.id == b.id && same(a, b))) {
        return;
    }
    let media = txn.get_or_insert_map(MEDIA);
    let mut kept = HashSet::new();
    for m in &new.media {
        kept.insert(m.id.0);
        if olds.get(&m.id.0).is_some_and(|o| same(o, m)) {
            continue;
        }
        let mut fields: Vec<(&str, yrs::In)> = vec![
            ("name", yrs::In::Any(Any::from(m.name.as_str()))),
            ("contentType", yrs::In::Any(Any::from(m.content_type.as_str()))),
            ("data", yrs::In::Any(Any::Buffer(Arc::from(m.data.as_slice())))),
        ];
        if let Some(l) = &m.link {
            fields.push(("link", yrs::In::Any(Any::from(l.as_str()))));
        }
        media.insert(txn, id_str(m.id.0), MapPrelim::from_iter(fields));
    }
    for id in olds.keys() {
        if !kept.contains(id) {
            media.remove(txn, &id_str(*id));
        }
    }
}

fn write_slides(txn: &mut TransactionMut, old: Option<&Presentation>, new: &Presentation) {
    let olds: HashMap<u32, &Arc<Slide>> = old.map(|o| o.slides.iter().map(|s| (s.id.0, s)).collect()).unwrap_or_default();
    for s in &new.slides {
        match olds.get(&s.id.0) {
            Some(o) if Arc::ptr_eq(o, s) => {}
            Some(o) => {
                let root = txn.get_or_insert_map(slide_root(s.id.0));
                diff_slide(txn, &root, o, s);
            }
            None => {
                let root = txn.get_or_insert_map(slide_root(s.id.0));
                write_new_slide(txn, &root, s);
            }
        }
    }
    let same_order = old.is_some_and(|o| o.slides.len() == new.slides.len() && o.slides.iter().zip(&new.slides).all(|(a, b)| a.id == b.id));
    if !same_order {
        let arr = txn.get_or_insert_array(SLIDES);
        let desired: Vec<String> = new.slides.iter().map(|s| id_str(s.id.0)).collect();
        reorder(&arr, txn, &desired);
    }
}

/// Writes a slide that isn't in the room (or that comes back: whatever its root still holds goes).
fn write_new_slide(txn: &mut TransactionMut, root: &MapRef, s: &Slide) {
    let stale: Vec<String> = root.keys(txn).map(str::to_string).collect();
    for k in stale {
        root.remove(txn, &k);
    }
    diff_keys(root, txn, &JsonMap::new(), &slide_props(s));
    write_notes(txn, root, None, &s.notes);
    let mut all = Vec::new();
    flatten(&s.shapes, &mut all, 0);
    for sh in all {
        write_new_shape(txn, root, sh);
    }
    let arr = ensure_array(root, txn, SLIDE_SHAPES);
    reorder(&arr, txn, &s.shapes.iter().map(|x| id_str(x.id.0)).collect::<Vec<_>>());
}

fn write_notes(txn: &mut TransactionMut, root: &MapRef, old: Option<&TextBody>, new: &TextBody) {
    let (ob, nb) = (old.map(body_props), body_props(new));
    if ob.as_ref() != Some(&nb) {
        put_json(root, txn, NOTES_BODY, &Json::Object(nb));
    }
    if old.is_some_and(|o| o.paragraphs == new.paragraphs) && sub_text(root, txn, NOTES).is_some() {
        return;
    }
    write_text(txn, root, NOTES, &new.paragraphs);
}

/// Makes the Yjs text under `key` hold `paragraphs`, editing what it holds.
fn write_text(txn: &mut TransactionMut, m: &MapRef, key: &str, paragraphs: &[deckcraft_model::text::Paragraph]) {
    let new = text::atoms_of(paragraphs);
    match sub_text(m, txn, key) {
        Some(t) => {
            let old: Vec<Atom> = text::read_atoms(&t, txn);
            text::apply_diff(&t, txn, &old, &new);
        }
        None => {
            let t: TextRef = m.insert(txn, key, TextPrelim::new(""));
            text::write(&t, txn, &new);
        }
    }
}

/// Writes one shape's own map (its children are written on their own).
fn write_new_shape(txn: &mut TransactionMut, root: &MapRef, s: &Shape) {
    let m: MapRef = root.insert(txn, shape_key(s.id.0), MapPrelim::default());
    diff_keys(&m, txn, &JsonMap::new(), &shape_props(s));
    if s.is_group() {
        let arr = ensure_array(&m, txn, CHILDREN);
        reorder(&arr, txn, &s.children().iter().map(|c| id_str(c.id.0)).collect::<Vec<_>>());
    }
    if let Some(b) = &s.text {
        put_json(&m, txn, TEXT_BODY, &Json::Object(body_props(b)));
        write_text(txn, &m, TEXT, &b.paragraphs);
    }
}

fn diff_slide(txn: &mut TransactionMut, root: &MapRef, old: &Slide, new: &Slide) {
    let (op, np) = (slide_props(old), slide_props(new));
    if op != np {
        diff_keys(root, txn, &op, &np);
    }
    if old.notes != new.notes || sub_text(root, txn, NOTES).is_none() {
        write_notes(txn, root, Some(&old.notes), &new.notes);
    }
    if old.shapes == new.shapes {
        return;
    }
    let (mut o, mut n) = (Vec::new(), Vec::new());
    flatten(&old.shapes, &mut o, 0);
    flatten(&new.shapes, &mut n, 0);
    let olds: HashMap<u32, &Shape> = o.into_iter().map(|s| (s.id.0, s)).collect();
    let mut kept = HashSet::new();
    for s in n {
        if !kept.insert(s.id.0) {
            continue;
        }
        match (olds.get(&s.id.0), sub_map(root, txn, &shape_key(s.id.0))) {
            (Some(prev), Some(m)) => {
                if *prev != s {
                    diff_shape(txn, &m, prev, s);
                }
            }
            _ => write_new_shape(txn, root, s),
        }
    }
    for id in olds.keys() {
        if !kept.contains(id) {
            root.remove(txn, &shape_key(*id));
        }
    }
    let arr = ensure_array(root, txn, SLIDE_SHAPES);
    reorder(&arr, txn, &new.shapes.iter().map(|x| id_str(x.id.0)).collect::<Vec<_>>());
}

fn diff_shape(txn: &mut TransactionMut, m: &MapRef, old: &Shape, new: &Shape) {
    let (op, np) = (shape_props(old), shape_props(new));
    if op != np {
        diff_keys(m, txn, &op, &np);
    }
    if new.is_group() || old.is_group() {
        let ids: Vec<String> = new.children().iter().map(|c| id_str(c.id.0)).collect();
        let arr = ensure_array(m, txn, CHILDREN);
        reorder(&arr, txn, &ids);
    }
    match (&old.text, &new.text) {
        (_, None) => {
            m.remove(txn, TEXT);
            m.remove(txn, TEXT_BODY);
        }
        (o, Some(nb)) => {
            let ob = o.as_ref().map(body_props);
            let now = body_props(nb);
            if ob.as_ref() != Some(&now) {
                put_json(m, txn, TEXT_BODY, &Json::Object(now));
            }
            if o.as_ref().is_none_or(|o| o.paragraphs != nb.paragraphs) || sub_text(m, txn, TEXT).is_none() {
                write_text(txn, m, TEXT, &nb.paragraphs);
            }
        }
    }
}

// ---------------------------------------------------------------- reading

/// The presentation-wide part (slides, masters and media empty).
fn read_shell<R: ReadTxn>(txn: &R) -> Presentation {
    let obj = txn.get_map(PRES).map(|m| json_entries(&m, txn, &[MASTERS, "slides", "media", "nextId", "chat"])).unwrap_or_default();
    let mut p: Presentation = lenient(obj);
    p.slides.clear();
    p.masters.clear();
    p.media.clear();
    p
}

/// The masters in order with their themes and layouts; `None` when the room has none.
fn read_masters<R: ReadTxn>(txn: &R) -> Option<Vec<Arc<Master>>> {
    let masters = txn.get_map(MASTERS)?;
    let layouts = txn.get_map(LAYOUTS);
    let theme = txn.get_map(THEME);
    let order: Vec<String> = match txn.get_map(PRES).and_then(|m| get_json(&m, txn, MASTERS)) {
        Some(Json::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => {
            let mut k: Vec<String> = masters.keys(txn).map(str::to_string).collect();
            k.sort();
            k
        }
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for key in order {
        if !seen.insert(key.clone()) {
            continue;
        }
        let Some(n) = parse_id(&key) else { continue };
        let Some(Json::Object(mut obj)) = get_json(&masters, txn, &key) else { continue };
        let layout_ids: Vec<String> = match obj.remove("layouts") {
            Some(Json::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        };
        obj.insert("id".into(), Json::from(n));
        let mut m: Master = lenient(obj);
        m.id = deckcraft_model::MasterId(n);
        if let Some(t) = theme.as_ref().and_then(|t| get_json(t, txn, &key)) {
            m.theme = serde_json::from_value(t).unwrap_or_default();
        }
        let mut lseen = HashSet::new();
        for lk in layout_ids.into_iter().take(MAX_ITEMS) {
            if !lseen.insert(lk.clone()) {
                continue;
            }
            let Some(ln) = parse_id(&lk) else { continue };
            let Some(Json::Object(mut lo)) = layouts.as_ref().and_then(|l| get_json(l, txn, &lk)) else { continue };
            lo.insert("id".into(), Json::from(ln));
            let mut l: Layout = lenient(lo);
            l.id = deckcraft_model::LayoutId(ln);
            m.layouts.push(l);
        }
        out.push(Arc::new(m));
    }
    if out.is_empty() { None } else { Some(out) }
}

fn read_media<R: ReadTxn>(txn: &R) -> Vec<MediaItem> {
    let Some(media) = txn.get_map(MEDIA) else { return Vec::new() };
    let mut out = Vec::new();
    for (k, v) in media.iter(txn) {
        let (Some(n), Out::YMap(m)) = (parse_id(k), v) else { continue };
        let s = |key: &str| out_str(m.get(txn, key)).map(|x| x.to_string());
        let data = match m.get(txn, "data") {
            Some(Out::Any(Any::Buffer(b))) => b.to_vec(),
            _ => Vec::new(),
        };
        out.push(MediaItem {
            id: MediaId(n),
            name: s("name").unwrap_or_default(),
            content_type: s("contentType").unwrap_or_default(),
            data: Arc::new(data),
            link: s("link"),
        });
    }
    out.sort_by_key(|m| m.id.0);
    out
}

fn read_text<R: ReadTxn>(txn: &R, m: &MapRef, text_key: &str, body_key: &str) -> Option<TextBody> {
    let t = sub_text(m, txn, text_key)?;
    let mut body: TextBody = match get_json(m, txn, body_key) {
        Some(Json::Object(o)) => lenient(o),
        _ => TextBody::default(),
    };
    body.paragraphs = text::paragraphs_of(&text::read_atoms(&t, txn));
    Some(body)
}

fn read_shapes<R: ReadTxn>(txn: &R, root: &MapRef, ids: &[String], depth: usize, seen: &mut HashSet<u32>) -> Vec<Shape> {
    if depth > MAX_DEPTH {
        return Vec::new();
    }
    let mut out = Vec::new();
    for key in ids {
        let Some(n) = parse_id(key) else { continue };
        if !seen.insert(n) {
            continue;
        }
        let Some(m) = sub_map(root, txn, &format!("{SHAPE_PREFIX}{key}")) else { continue };
        let mut obj = json_entries(&m, txn, &[CHILDREN, TEXT, TEXT_BODY]);
        obj.insert("id".into(), Json::from(n));
        let group = obj.get("kind").and_then(|k| k.get("type")).and_then(Json::as_str) == Some("group");
        if group && let Some(Json::Object(k)) = obj.get_mut("kind") {
            k.insert("children".into(), Json::Array(vec![]));
        }
        let mut s: Shape = lenient(obj);
        s.id = ShapeId(n);
        if let Some(children) = s.children_mut() {
            let ids = ids_of(sub_array(&m, txn, CHILDREN).as_ref(), txn);
            *children = read_shapes(txn, root, &ids, depth + 1, seen);
        }
        s.text = read_text(txn, &m, TEXT, TEXT_BODY);
        out.push(s);
    }
    out
}

fn read_slide<R: ReadTxn>(txn: &R, id: u32) -> Option<Slide> {
    let root = txn.get_map(slide_root(id).as_str())?;
    let mut obj = json_entries(&root, txn, &[SLIDE_SHAPES, NOTES, NOTES_BODY]);
    obj.insert("id".into(), Json::from(id));
    let mut s: Slide = lenient(obj);
    s.id = SlideId(id);
    let ids = ids_of(sub_array(&root, txn, SLIDE_SHAPES).as_ref(), txn);
    s.shapes = read_shapes(txn, &root, &ids, 0, &mut HashSet::new());
    s.notes = read_text(txn, &root, NOTES, NOTES_BODY).unwrap_or_default();
    Some(s)
}

/// The whole presentation the document holds; `cur`'s masters stay when the room has none.
fn read_all<R: ReadTxn>(txn: &R, cur: &Arc<Presentation>) -> Presentation {
    let mut p = read_shell(txn);
    p.masters = read_masters(txn).unwrap_or_else(|| cur.masters.clone());
    p.media = read_media(txn);
    let arr = txn.get_array(SLIDES);
    let mut seen = HashSet::new();
    for key in ids_of(arr.as_ref(), txn) {
        let Some(n) = parse_id(&key) else { continue };
        if !seen.insert(n) {
            continue;
        }
        if let Some(s) = read_slide(txn, n) {
            p.slides.push(Arc::new(s));
        }
    }
    p.chat = Arc::new(crate::chat::read(txn));
    p.next_id = 0;
    p.fix_next_id();
    p
}

/// Whether an id in the block the counter hands out next (65,536 ids) is already taken.
fn block_taken(p: &Presentation) -> bool {
    let (lo, hi) = (p.next_id, p.next_id.saturating_add(1 << 16));
    let mut taken = false;
    for s in &p.slides {
        taken |= s.id.0 > lo && s.id.0 <= hi;
        deckcraft_model::walk(&s.shapes, &mut |sh, _| taken |= sh.id.0 > lo && sh.id.0 <= hi);
    }
    taken || p.media.iter().any(|m| m.id.0 > lo && m.id.0 <= hi)
}

/// Gives the presentation a random id counter (from 2^24 to 2^31) with a free block of 65,536
/// ids after it: in a live session people add slides and shapes at the same time, and their new
/// ids must not meet (schema 1, "Ids").
pub fn reseed_ids(p: &mut Presentation, rng: &mut Rng) {
    let mut used: Vec<u32> = Vec::new();
    for s in &p.slides {
        used.push(s.id.0);
        deckcraft_model::walk(&s.shapes, &mut |sh, _| used.push(sh.id.0));
    }
    for m in &p.masters {
        used.push(m.id.0);
        deckcraft_model::walk(&m.shapes, &mut |sh, _| used.push(sh.id.0));
        for l in &m.layouts {
            used.push(l.id.0);
            deckcraft_model::walk(&l.shapes, &mut |sh, _| used.push(sh.id.0));
        }
    }
    for m in &p.media {
        used.push(m.id.0);
    }
    used.sort_unstable();
    for _ in 0..32 {
        let base = rng.range(1 << 24, (1 << 31) - (1 << 20)) as u32;
        let i = used.partition_point(|x| *x <= base);
        if used.get(i).is_none_or(|next| *next > base.saturating_add(1 << 16)) {
            p.next_id = base;
            return;
        }
    }
    p.fix_next_id();
}

/// What schema 1 shares of a presentation, as canonical text: equal on two peers exactly when
/// they show the same shared content (tests and diagnostics compare peers with it). Runs are
/// merged as a reader would, media sorted by id, the id counter left out.
pub fn fingerprint(p: &Presentation) -> String {
    let mut q = p.clone();
    q.next_id = 0;
    q.media.sort_by_key(|m| m.id.0);
    let norm = |b: &mut TextBody| {
        let paras = text::paragraphs_of(&text::atoms_of(&b.paragraphs));
        b.paragraphs = paras;
    };
    for s in q.slides.iter_mut() {
        let s = Arc::make_mut(s);
        norm(&mut s.notes);
        fn walk(shapes: &mut [Shape], norm: &dyn Fn(&mut TextBody), depth: usize) {
            if depth > MAX_DEPTH {
                return;
            }
            for sh in shapes {
                if let Some(b) = sh.text.as_mut() {
                    norm(b);
                }
                if let Some(c) = sh.children_mut() {
                    walk(c, norm, depth + 1);
                }
            }
        }
        walk(&mut s.shapes, &norm, 0);
    }
    let media: Vec<String> = q.media.iter().map(|m| format!("{} {} {} {}", m.id.0, m.name, m.content_type, m.data.len())).collect();
    format!("{}\n{media:?}", serde_json::to_string(&q).unwrap_or_default())
}
