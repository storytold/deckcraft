//! Spelling squiggles: misspelt words on the current slide (shapes and table cells) and in the
//! speaker notes get a red wavy underline. Checks run on a worker thread so typing never waits for
//! a dictionary; the web build has no threads, so there the same checker runs on the UI thread a
//! few milliseconds per frame until it is done. Results are for one version of the text; while a
//! newer check runs nothing stale is drawn. The word the caret is in isn't marked, as in
//! PowerPoint.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use deckcraft_engine::DocState;
use deckcraft_geom::Point;
use deckcraft_model::{Paragraph, ShapeId};

/// Milliseconds of checking per frame on the UI thread (the web build).
pub const FRAME_BUDGET_MS: f64 = 4.0;

/// Where a checked paragraph is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tag {
    Shape {
        id: ShapeId,
        para: usize,
    },
    Cell {
        id: ShapeId,
        row: usize,
        col: usize,
        para: usize,
    },
    /// A line of the notes, starting at character `start` of the notes text.
    Notes {
        start: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Slide,
    Notes,
}

/// What a check is for: a hash of the text's identity, and the user dictionary version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    of: u64,
    user: u64,
}

struct Job {
    kind: Kind,
    key: Key,
    default_lang: String,
    paras: Vec<(Tag, Paragraph)>,
    repaint: Option<egui::Context>,
}

/// Misspelt character ranges, per paragraph.
struct Done {
    kind: Kind,
    key: Key,
    ranges: Vec<(Tag, usize, usize)>,
}

/// Return the shared application clock in milliseconds for spelling-squiggle animation timing.
fn now_ms() -> f64 {
    crate::now_ms()
}

/// The checking itself, step by step: the newest job of each kind, a paragraph at a time, with
/// results cached by paragraph content (typing re-checks only the paragraph typed in).
#[derive(Default)]
struct Checker {
    cache: HashMap<u64, Vec<(usize, usize)>>,
    user: Option<u64>,
    /// Jobs with how far they got and what they found so far.
    jobs: Vec<(Job, usize, Vec<(Tag, usize, usize)>)>,
}

fn hash_of(v: &impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

impl Checker {
    fn push(&mut self, job: Job) {
        self.jobs.retain(|(j, _, _)| j.kind != job.kind);
        self.jobs.push((job, 0, vec![]));
    }

    fn idle(&self) -> bool {
        self.jobs.is_empty()
    }

    /// Check until `budget_ms` is spent (or everything is done); finished jobs.
    fn step(&mut self, budget_ms: f64) -> Vec<(Done, Option<egui::Context>)> {
        let t0 = now_ms();
        let mut out = vec![];
        while let Some((job, at, found)) = self.jobs.first_mut() {
            if self.user != Some(job.key.user) {
                self.cache.clear();
                self.user = Some(job.key.user);
            }
            while *at < job.paras.len() {
                let (tag, p) = &job.paras[*at];
                let k = hash_of(&(serde_json::to_string(&p.runs).unwrap_or_default(), &job.default_lang));
                let r = self
                    .cache
                    .entry(k)
                    .or_insert_with(|| deckcraft_engine::spell::misspelt(p, &job.default_lang, &[]).into_iter().map(|w| (w.start, w.end)).collect());
                found.extend(r.iter().map(|(a, b)| (*tag, *a, *b)));
                *at += 1;
                if now_ms() - t0 > budget_ms {
                    return out;
                }
            }
            let (job, _, ranges) = self.jobs.remove(0);
            out.push((Done { kind: job.kind, key: job.key, ranges }, job.repaint));
        }
        if self.cache.len() > 20_000 {
            self.cache.clear();
        }
        out
    }
}

fn worker(rx: Receiver<Job>, tx: Sender<Done>) {
    let mut c = Checker::default();
    loop {
        if c.idle() {
            match rx.recv() {
                Ok(j) => c.push(j),
                Err(_) => return,
            }
        }
        while let Ok(j) = rx.try_recv() {
            c.push(j);
        }
        // In slices, so a newer job can replace one being checked.
        for (d, repaint) in c.step(20.0) {
            if tx.send(d).is_err() {
                return;
            }
            if let Some(r) = repaint {
                r.request_repaint();
            }
        }
    }
}

fn default_lang(st: &DocState) -> String {
    for sl in &st.doc.slides {
        let mut found = None;
        deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
            if found.is_none()
                && let Some(t) = &sh.text
            {
                found = t.paragraphs.iter().flat_map(|p| &p.runs).find_map(|r| r.props.lang.clone());
            }
        });
        if let Some(l) = found {
            return l;
        }
    }
    "en-US".into()
}

/// A word under a squiggle: `(tag, start, end)`.
type Mark = (Tag, usize, usize);

#[derive(Default)]
pub struct Squiggles {
    tx: Option<Sender<Job>>,
    rx: Option<Receiver<Done>>,
    /// No worker thread (the web build, or forced for tests): check on the UI thread.
    inline: Option<Checker>,
    /// Check on the UI thread even where threads exist (tests of the web path).
    pub force_inline: bool,
    pending: [Option<Key>; 2],
    done: [Option<Done>; 2],
    /// Underlines in slide coordinates for the slide's `done`, and the key they were laid out for.
    lines: Vec<(Mark, Point, Point)>,
    lines_for: Option<Key>,
    /// Off (tests or the setting).
    pub disabled: bool,
    /// The misspelt word a context menu is open on.
    pub menu_word: Option<MenuWord>,
    /// The same for the notes pane (`Tag::Notes`, `start` in the line).
    pub notes_menu: Option<MenuWord>,
    /// Words skipped with Ignore All this session (lower-case).
    ignored: Vec<String>,
}

/// A misspelt word under the pointer, with corrections.
#[derive(Clone, Debug)]
pub struct MenuWord {
    pub tag: Tag,
    pub start: usize,
    pub word: String,
    pub suggestions: Vec<String>,
    /// A repeated word: Delete Repeated Word removes characters `from..start + word`.
    pub delete_from: Option<usize>,
}

/// Proofing languages offered by Review › Language: (name, tag).
pub const LANGS: &[(&str, &str)] = &[
    ("English (United States)", "en-US"),
    ("English (United Kingdom)", "en-GB"),
    ("English (Australia)", "en-AU"),
    ("English (Canada)", "en-CA"),
    ("Español (España)", "es-ES"),
    ("Español (México)", "es-MX"),
    ("Français (France)", "fr-FR"),
    ("Français (Canada)", "fr-CA"),
    ("Deutsch (Deutschland)", "de-DE"),
    ("Deutsch (Schweiz)", "de-CH"),
    ("Italiano", "it-IT"),
    ("Nederlands", "nl-NL"),
    ("Português (Brasil)", "pt-BR"),
    ("Português (Portugal)", "pt-PT"),
    ("Polski", "pl-PL"),
    ("Svenska", "sv-SE"),
    ("Dansk", "da-DK"),
    ("Norsk bokmål", "nb-NO"),
    ("Suomi", "fi-FI"),
    ("Čeština", "cs-CZ"),
    ("Українська", "uk-UA"),
    ("Русский", "ru-RU"),
    ("Беларуская", "be-BY"),
    ("Türkçe", "tr-TR"),
    ("Ελληνικά", "el-GR"),
    ("日本語", "ja-JP"),
    ("中文(中国)", "zh-CN"),
    ("한국어", "ko-KR"),
    ("العربية", "ar-SA"),
    ("עברית", "he-IL"),
];

/// The display name of a language tag (the tag itself when it isn't in [`LANGS`]).
pub fn lang_name(tag: &str) -> String {
    LANGS.iter().find(|(_, t)| t.eq_ignore_ascii_case(tag)).map_or_else(|| tag.to_string(), |(n, _)| n.to_string())
}

/// A language name the current interface fonts can actually draw: the native name when every
/// character has a glyph, otherwise the tag. On a machine without a Korean, Chinese or Hebrew
/// face, egui 0.37's strict missing-glyph policy would abort the frame instead of drawing boxes,
/// so state the fallback explicitly (and never skip the row).
pub fn display_lang_name(ctx: &egui::Context, tag: &str) -> String {
    let name = lang_name(tag);
    if ctx.fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(13.0), &name)) { name } else { tag.to_string() }
}

/// The flagged word (misspelt or repeated) of `pp` starting at `start`, with what the menu offers.
pub fn menu_word(pp: &Paragraph, default: &str, tag: Tag, start: usize) -> Option<MenuWord> {
    let w = deckcraft_engine::spell::misspelt(pp, default, &[]).into_iter().find(|w| w.start == start)?;
    let suggestions =
        if w.repeated.is_some() { vec![] } else { deckcraft_engine::spell::dictionary(&w.lang).map(|d| d.suggest(&w.text, 5)).unwrap_or_default() };
    Some(MenuWord { tag, start, word: w.text, suggestions, delete_from: w.repeated })
}

fn slot(k: Kind) -> usize {
    match k {
        Kind::Slide => 0,
        Kind::Notes => 1,
    }
}

impl Squiggles {
    fn start(&mut self) -> bool {
        if self.tx.is_some() || self.inline.is_some() {
            return true;
        }
        if !cfg!(target_arch = "wasm32") && !self.force_inline {
            let (jtx, jrx) = channel();
            let (dtx, drx) = channel();
            if std::thread::Builder::new().name("spelling".into()).spawn(move || worker(jrx, dtx)).is_ok() {
                self.tx = Some(jtx);
                self.rx = Some(drx);
                return true;
            }
        }
        self.inline = Some(Checker::default());
        true
    }

    /// Take in finished checks (and, on the UI thread, spend this frame's budget).
    fn receive(&mut self, budget_ms: f64) {
        let mut got = vec![];
        if let Some(rx) = &self.rx {
            while let Ok(d) = rx.try_recv() {
                got.push(d);
            }
        }
        if let Some(c) = self.inline.as_mut() {
            got.extend(c.step(budget_ms).into_iter().map(|(d, _)| d));
        }
        for d in got {
            let i = slot(d.kind);
            if self.pending[i] == Some(d.key) {
                self.pending[i] = None;
            }
            self.done[i] = Some(d);
        }
    }

    fn ask(&mut self, kind: Kind, key: Key, default_lang: String, paras: impl FnOnce() -> Vec<(Tag, Paragraph)>, repaint: Option<&egui::Context>) {
        let i = slot(kind);
        if self.done[i].as_ref().is_some_and(|d| d.key == key) || self.pending[i] == Some(key) {
            return;
        }
        let job = Job { kind, key, default_lang, paras: paras(), repaint: repaint.cloned() };
        if let Some(c) = self.inline.as_mut() {
            c.push(job);
            self.pending[i] = Some(key);
            if let Some(r) = repaint {
                r.request_repaint();
            }
        } else if self.tx.as_ref().is_some_and(|tx| tx.send(job).is_ok()) {
            self.pending[i] = Some(key);
        }
    }

    fn is_ignored(&self, word: &str) -> bool {
        !self.ignored.is_empty() && self.ignored.contains(&word.to_lowercase())
    }

    /// Ask for a check of the current slide if it changed, take in finished checks, and return the
    /// underlines to draw (slide coordinates).
    pub fn update(&mut self, st: &DocState, repaint: Option<&egui::Context>) -> Vec<(Point, Point)> {
        if self.disabled || !self.start() {
            return vec![];
        }
        self.receive(FRAME_BUDGET_MS);
        let key =
            Key { of: hash_of(&(Arc::as_ptr(&st.doc) as usize, st.revision, st.selection.slide)), user: deckcraft_engine::spell::user_generation() };
        let paras = || {
            let mut paras = vec![];
            if let Some(sl) = st.doc.slides.get(st.selection.slide) {
                deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
                    if let Some(t) = &sh.text {
                        for (i, p) in t.paragraphs.iter().enumerate() {
                            paras.push((Tag::Shape { id: sh.id, para: i }, p.clone()));
                        }
                    }
                    if let deckcraft_model::ShapeKind::Table(tb) = &sh.kind {
                        for (r, row) in tb.rows.iter().enumerate() {
                            for (c, cell) in row.cells.iter().enumerate() {
                                if cell.is_covered() {
                                    continue;
                                }
                                for (i, p) in cell.text.paragraphs.iter().enumerate() {
                                    paras.push((Tag::Cell { id: sh.id, row: r, col: c, para: i }, p.clone()));
                                }
                            }
                        }
                    }
                });
            }
            paras
        };
        self.ask(Kind::Slide, key, default_lang(st), paras, repaint);
        let Some(d) = self.done[0].as_ref().filter(|d| d.key == key) else { return vec![] };
        if self.lines_for != Some(key) {
            self.lines.clear();
            for &(tag, a, b) in &d.ranges {
                let under = match tag {
                    Tag::Shape { id, para } => deckcraft_engine::cmd::text::range_underlines(st, id, &[(para, a, b)]),
                    Tag::Cell { id, row, col, para } => deckcraft_engine::cmd::text::cell_range_underlines(st, id, row, col, &[(para, a, b)]),
                    Tag::Notes { .. } => vec![],
                };
                self.lines.extend(under.into_iter().map(|(p, q)| ((tag, a, b), p, q)));
            }
            self.lines_for = Some(key);
        }
        // Not under the word the caret is in.
        let typing = st.selection.text.as_ref().filter(|t| !t.notes).map(|t| (t.shape, t.cell, t.caret));
        let under_caret = |tag: &Tag, a: usize, b: usize| {
            typing.is_some_and(|(s, cell, (cp, cc))| {
                let here = match *tag {
                    Tag::Shape { id, para } => cell.is_none() && id == s && para == cp,
                    Tag::Cell { id, row, col, para } => cell == Some((row, col)) && id == s && para == cp,
                    Tag::Notes { .. } => false,
                };
                here && cc >= a && cc <= b
            })
        };
        self.lines
            .iter()
            .filter(|((tag, a, b), _, _)| !under_caret(tag, *a, *b))
            .filter(|((tag, a, b), _, _)| self.ignored.is_empty() || !word_of(st, *tag, *a, *b).is_some_and(|w| self.is_ignored(&w)))
            .map(|(_, a, b)| (*a, *b))
            .collect()
    }

    /// The misspelt character ranges of the notes text being shown (`caret` is the cursor's
    /// character index, whose word isn't marked); empty until the check is in.
    pub fn notes(&mut self, text: &str, lang: &str, caret: Option<usize>, repaint: Option<&egui::Context>) -> Vec<(usize, usize)> {
        if self.disabled || !self.start() {
            return vec![];
        }
        self.receive(FRAME_BUDGET_MS);
        let key = Key { of: hash_of(&(text, lang)), user: deckcraft_engine::spell::user_generation() };
        let paras = || {
            let mut out = vec![];
            let mut start = 0;
            for line in text.split('\n') {
                out.push((Tag::Notes { start }, Paragraph::new(line)));
                start += line.chars().count() + 1;
            }
            out
        };
        self.ask(Kind::Notes, key, lang.to_string(), paras, repaint);
        let Some(d) = self.done[1].as_ref().filter(|d| d.key == key) else { return vec![] };
        let chars: Vec<char> = text.chars().collect();
        d.ranges
            .iter()
            .filter_map(|&(tag, a, b)| match tag {
                Tag::Notes { start } => Some((start + a, start + b)),
                _ => None,
            })
            .filter(|(a, b)| !caret.is_some_and(|c| c >= *a && c <= *b))
            .filter(|(a, b)| {
                self.ignored.is_empty() || !self.is_ignored(&chars.get(*a..*b).map(|c| c.iter().collect::<String>()).unwrap_or_default())
            })
            .collect()
    }

    /// Skip `word` everywhere for the rest of the session.
    pub fn ignore(&mut self, word: &str) {
        self.ignored.push(word.to_lowercase());
    }

    /// The squiggled word at slide point `p` (within `tol` points of its underline).
    pub fn word_at(&self, st: &DocState, p: Point, tol: f64) -> Option<MenuWord> {
        let ((tag, start, end), _, _) = *self.lines.iter().find(|(_, a, b)| {
            let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
            // From the line's top (a word is about a line high above its underline) to just below it.
            p.x >= x0 - tol && p.x <= x1 + tol && p.y <= a.y.max(b.y) + tol && p.y >= a.y.min(b.y) - 24.0
        })?;
        let _ = end;
        menu_word(&paragraph_of(st, tag)?, &default_lang(st), tag, start)
    }

    /// Wait (tests) until the slide's check for `st` is in.
    pub fn settle(&mut self, st: &DocState) -> Vec<(Point, Point)> {
        for _ in 0..2000 {
            let v = self.update(st, None);
            if self.pending[0].is_none() && self.done[0].is_some() {
                return v;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        vec![]
    }

    /// Wait (tests) until the notes check for `text` is in.
    pub fn settle_notes(&mut self, text: &str, lang: &str) -> Vec<(usize, usize)> {
        for _ in 0..2000 {
            let v = self.notes(text, lang, None, None);
            if self.pending[1].is_none() && self.done[1].is_some() {
                return v;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        vec![]
    }
}

fn paragraph_of(st: &DocState, tag: Tag) -> Option<Paragraph> {
    let sl = st.doc.slides.get(st.selection.slide)?;
    match tag {
        Tag::Shape { id, para } => deckcraft_model::find_shape(&sl.shapes, id)?.text.as_ref()?.paragraphs.get(para).cloned(),
        Tag::Cell { id, row, col, para } => match &deckcraft_model::find_shape(&sl.shapes, id)?.kind {
            deckcraft_model::ShapeKind::Table(tb) => tb.cell(row, col)?.text.paragraphs.get(para).cloned(),
            _ => None,
        },
        Tag::Notes { .. } => None,
    }
}

fn word_of(st: &DocState, tag: Tag, a: usize, b: usize) -> Option<String> {
    Some(paragraph_of(st, tag)?.text().chars().skip(a).take(b - a).collect())
}

/// A red wavy line from `a` to `b` (screen points).
pub fn paint(painter: &egui::Painter, a: egui::Pos2, b: egui::Pos2, scale: f32) {
    let d = b - a;
    let len = d.length();
    if len < 1.0 {
        return;
    }
    let (u, n) = (d / len, egui::vec2(-d.y, d.x) / len);
    let step = (2.0 * scale).clamp(1.5, 3.0);
    let amp = step * 0.75;
    let mut pts = vec![];
    let mut t = 0.0;
    let mut up = false;
    while t <= len {
        pts.push(a + u * t + n * if up { -amp } else { amp });
        up = !up;
        t += step;
    }
    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.0, egui::Color32::from_rgb(0xE0, 0x20, 0x20))));
}
