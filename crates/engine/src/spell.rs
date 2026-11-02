//! Spell checking with Hunspell-format dictionaries (`xx_YY.aff` + `xx_YY.dic`).
//!
//! A subset of Hunspell: `SET` (UTF-8 and ISO-8859-1), `FLAG` (default, `long`, `num`, `UTF-8`),
//! `PFX`/`SFX` with strip, conditions and cross products, continuation flags on affixes (one
//! level: a suffix that allows a further suffix), `NEEDAFFIX`, `FORBIDDENWORD`, `NOSUGGEST`,
//! `KEEPCASE`, `TRY`, `REP` and `WORDCHARS`. Compounding, `ICONV`/`OCONV` and morphology are not
//! read; words they would accept are reported as misspelt.
//!
//! No dictionary ships with DeckCraft. They are looked up in the folders the system's own spell
//! checkers use (and `DECKCRAFT_DICTIONARIES`), so whatever the user installed is used.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

type Flag = u32;

#[derive(Clone, Copy, Debug, PartialEq)]
enum FlagMode {
    Char,
    Long,
    Num,
}

#[derive(Clone, Debug)]
enum Cond {
    Any,
    Is(char),
    In(Vec<char>, bool),
}

impl Cond {
    fn matches(&self, c: char) -> bool {
        match self {
            Cond::Any => true,
            Cond::Is(x) => *x == c,
            Cond::In(set, neg) => set.contains(&c) != *neg,
        }
    }
}

fn parse_cond(s: &str) -> Vec<Cond> {
    let mut out = vec![];
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match c {
            '.' => out.push(Cond::Any),
            '[' => {
                let mut set = vec![];
                let mut neg = false;
                for (i, d) in it.by_ref().enumerate() {
                    if d == ']' {
                        break;
                    }
                    if i == 0 && d == '^' {
                        neg = true;
                    } else {
                        set.push(d);
                    }
                }
                out.push(Cond::In(set, neg));
            }
            c => out.push(Cond::Is(c)),
        }
    }
    out
}

#[derive(Clone, Debug)]
struct Affix {
    flag: Flag,
    cross: bool,
    strip: String,
    add: String,
    cond: Vec<Cond>,
    cont: Vec<Flag>,
}

impl Affix {
    /// Prefix: does `root` start with the condition? Suffix: does it end with it?
    fn cond_ok(&self, root: &[char], prefix: bool) -> bool {
        if self.cond.len() > root.len() {
            return false;
        }
        if prefix {
            self.cond.iter().zip(root).all(|(c, r)| c.matches(*r))
        } else {
            self.cond.iter().rev().zip(root.iter().rev()).all(|(c, r)| c.matches(*r))
        }
    }
}

/// A loaded dictionary.
#[derive(Debug, Default)]
pub struct Dictionary {
    words: HashMap<String, Vec<Vec<Flag>>>,
    pfx: Vec<Affix>,
    sfx: Vec<Affix>,
    try_chars: Vec<char>,
    rep: Vec<(String, String)>,
    word_chars: Vec<char>,
    forbidden: Option<Flag>,
    need_affix: Option<Flag>,
    no_suggest: Option<Flag>,
    keep_case: Option<Flag>,
    only_in_compound: Option<Flag>,
    compound: Option<Flag>,
    compound_begin: Option<Flag>,
    compound_middle: Option<Flag>,
    compound_end: Option<Flag>,
    compound_min: usize,
    compound_max: usize,
}

fn decode(bytes: &[u8], latin1: bool) -> String {
    if latin1 { bytes.iter().map(|b| char::from(*b)).collect() } else { String::from_utf8_lossy(bytes).into_owned() }
}

fn parse_flags(s: &str, mode: FlagMode) -> Vec<Flag> {
    match mode {
        FlagMode::Char => s.chars().map(|c| c as u32).collect(),
        FlagMode::Long => {
            let c: Vec<char> = s.chars().collect();
            c.chunks(2).map(|p| p.iter().fold(0u32, |a, c| a.wrapping_mul(0x11_0000).wrapping_add(*c as u32))).collect()
        }
        FlagMode::Num => s.split(',').filter_map(|n| n.trim().parse().ok()).collect(),
    }
}

fn one_flag(s: Option<&str>, mode: FlagMode) -> Option<Flag> {
    s.and_then(|s| parse_flags(s, mode).first().copied())
}

impl Dictionary {
    /// Parse a dictionary from the bytes of its `.aff` and `.dic` files.
    pub fn parse(aff: &[u8], dic: &[u8]) -> Dictionary {
        let head = String::from_utf8_lossy(aff);
        let latin1 = head.lines().any(|l| {
            let mut w = l.split_whitespace();
            w.next() == Some("SET") && w.next().is_some_and(|e| e.to_ascii_uppercase().replace('-', "").starts_with("ISO8859"))
        });
        let aff = decode(aff, latin1);
        let mode = aff
            .lines()
            .find_map(|l| {
                let mut w = l.split_whitespace();
                (w.next() == Some("FLAG")).then(|| match w.next() {
                    Some("long") => FlagMode::Long,
                    Some("num") => FlagMode::Num,
                    _ => FlagMode::Char,
                })
            })
            .unwrap_or(FlagMode::Char);
        let mut d = Dictionary { compound_min: 3, ..Dictionary::default() };
        let mut lines = aff.lines().map(|l| l.split('#').next().unwrap_or("")).peekable();
        while let Some(l) = lines.next() {
            let w: Vec<&str> = l.split_whitespace().collect();
            match w.as_slice() {
                ["TRY", s, ..] => d.try_chars = s.chars().collect(),
                ["WORDCHARS", s, ..] => d.word_chars = s.chars().collect(),
                ["FORBIDDENWORD", f, ..] => d.forbidden = one_flag(Some(f), mode),
                ["NEEDAFFIX", f, ..] | ["PSEUDOROOT", f, ..] => d.need_affix = one_flag(Some(f), mode),
                ["NOSUGGEST", f, ..] => d.no_suggest = one_flag(Some(f), mode),
                ["KEEPCASE", f, ..] => d.keep_case = one_flag(Some(f), mode),
                ["ONLYINCOMPOUND", f, ..] => d.only_in_compound = one_flag(Some(f), mode),
                ["COMPOUNDFLAG", f, ..] => d.compound = one_flag(Some(f), mode),
                ["COMPOUNDBEGIN", f, ..] => d.compound_begin = one_flag(Some(f), mode),
                ["COMPOUNDMIDDLE", f, ..] => d.compound_middle = one_flag(Some(f), mode),
                ["COMPOUNDEND", f, ..] | ["COMPOUNDLAST", f, ..] => d.compound_end = one_flag(Some(f), mode),
                ["COMPOUNDMIN", n, ..] => d.compound_min = n.parse::<usize>().unwrap_or(3).max(1),
                ["COMPOUNDWORDMAX", n, ..] => d.compound_max = n.parse().unwrap_or(0),
                ["REP", n] => {
                    for _ in 0..n.parse::<usize>().unwrap_or(0) {
                        let r: Vec<&str> = lines.next().unwrap_or("").split_whitespace().collect();
                        if let [_, a, b, ..] = r.as_slice() {
                            d.rep.push((a.replace('_', " "), b.replace('_', " ")));
                        }
                    }
                }
                [kind @ ("PFX" | "SFX"), flag, cross, n] if n.parse::<usize>().is_ok() => {
                    let prefix = *kind == "PFX";
                    let Some(flag) = one_flag(Some(flag), mode) else { continue };
                    let cross = cross.eq_ignore_ascii_case("y");
                    for _ in 0..n.parse::<usize>().unwrap_or(0) {
                        let r: Vec<&str> = lines.next().unwrap_or("").split_whitespace().collect();
                        let [_, _, strip, add, rest @ ..] = r.as_slice() else { continue };
                        let (add, cont) = match add.split_once('/') {
                            Some((a, c)) => (a, parse_flags(c, mode)),
                            None => (*add, vec![]),
                        };
                        let a = Affix {
                            flag,
                            cross,
                            strip: if *strip == "0" { String::new() } else { strip.to_string() },
                            add: if add == "0" { String::new() } else { add.to_string() },
                            cond: parse_cond(rest.first().copied().unwrap_or(".")),
                            cont,
                        };
                        if prefix { d.pfx.push(a) } else { d.sfx.push(a) }
                    }
                }
                _ => {}
            }
        }
        let dic = decode(dic, latin1);
        for (i, l) in dic.lines().enumerate() {
            if i == 0 && l.trim().parse::<usize>().is_ok() {
                continue;
            }
            let entry = l.split(['\t']).next().unwrap_or("").trim_end();
            // Morphological fields follow a space (`word/flags po:noun`).
            let entry = entry.split(' ').next().unwrap_or("");
            if entry.is_empty() {
                continue;
            }
            let (word, flags) = match entry.split_once('/') {
                Some((w, f)) if !w.is_empty() => (w, parse_flags(f, mode)),
                _ => (entry, vec![]),
            };
            d.words.entry(word.to_string()).or_default().push(flags);
        }
        d
    }

    /// Add `word` (no flags), as a user's own dictionary entry.
    pub fn add_word(&mut self, word: &str) {
        self.words.entry(word.to_string()).or_default().push(vec![]);
    }

    fn has(&self, flags: &[Flag], f: Option<Flag>) -> bool {
        f.is_some_and(|f| flags.contains(&f))
    }

    /// `word` as a root carrying all of `need` (and not forbidden).
    fn root(&self, word: &str, need: &[Flag], affixed: bool) -> bool {
        self.words.get(word).is_some_and(|hs| {
            hs.iter().any(|f| {
                !self.has(f, self.forbidden)
                    && (affixed || !self.has(f, self.need_affix))
                    && !self.has(f, self.only_in_compound)
                    && need.iter().all(|n| f.contains(n))
            })
        })
    }

    fn forbidden_word(&self, word: &str) -> bool {
        self.words.get(word).is_some_and(|hs| hs.iter().any(|f| self.has(f, self.forbidden)))
    }

    /// Strip suffixes from `w` (one, or two through continuation flags); `extra` flags must also be
    /// on the root, `cross` means the suffix must allow a prefix.
    fn suffixed(&self, w: &[char], extra: &[Flag], cross: bool) -> bool {
        for s in &self.sfx {
            if cross && !s.cross {
                continue;
            }
            let Some(stem) = strip_end(w, &s.add, &s.strip) else { continue };
            if !s.cond_ok(&stem, false) {
                continue;
            }
            let root: String = stem.iter().collect();
            let mut need = extra.to_vec();
            need.push(s.flag);
            if self.root(&root, &need, true) {
                return true;
            }
            // A second suffix whose continuation allows this one (`walk/A` + `ing/B`…).
            for s2 in &self.sfx {
                if !s.cont.is_empty() || !s2.cont.contains(&s.flag) {
                    continue;
                }
                let Some(stem2) = strip_end(&stem, &s2.add, &s2.strip) else { continue };
                if s2.cond_ok(&stem2, false) {
                    let mut need = extra.to_vec();
                    need.push(s2.flag);
                    if self.root(&stem2.iter().collect::<String>(), &need, true) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// A root (or, for the last part, a suffixed root) carrying `flag`, as a compound part.
    fn compound_part(&self, part: &[char], flag: Flag, last: bool) -> bool {
        let word: String = part.iter().collect();
        let ok = self.words.get(&word).is_some_and(|hs| hs.iter().any(|f| !self.has(f, self.forbidden) && f.contains(&flag)));
        ok || (last && self.suffixed_compound_end(part, flag))
    }

    /// The last part of a compound with a suffix: the root must carry the compound `flag`.
    fn suffixed_compound_end(&self, w: &[char], flag: Flag) -> bool {
        self.sfx.iter().any(|s| {
            strip_end(w, &s.add, &s.strip).is_some_and(|stem| {
                s.cond_ok(&stem, false)
                    && self
                        .words
                        .get(&stem.iter().collect::<String>())
                        .is_some_and(|hs| hs.iter().any(|f| !self.has(f, self.forbidden) && f.contains(&flag) && f.contains(&s.flag)))
            })
        })
    }

    /// `w` made of dictionary parts allowed in compounds (`COMPOUNDFLAG`, or the begin, middle and
    /// end flags), each at least `COMPOUNDMIN` characters long.
    fn compounded(&self, w: &[char], index: usize) -> bool {
        if self.compound.is_none() && self.compound_end.is_none() {
            return false;
        }
        let min = self.compound_min.max(1);
        if index >= 8 || (self.compound_max > 0 && index + 1 >= self.compound_max) || w.len() < 2 * min {
            return false;
        }
        let role = if index == 0 { self.compound_begin } else { self.compound_middle };
        for i in min..=w.len() - min {
            let (head, rest) = w.split_at(i);
            let head_ok = [self.compound, role].into_iter().flatten().any(|f| self.compound_part(head, f, false));
            if !head_ok {
                continue;
            }
            if [self.compound, self.compound_end].into_iter().flatten().any(|f| self.compound_part(rest, f, true)) {
                return true;
            }
            if self.compounded(rest, index + 1) {
                return true;
            }
        }
        false
    }

    fn check_exact(&self, word: &str) -> bool {
        if self.forbidden_word(word) {
            return false;
        }
        if self.root(word, &[], false) {
            return true;
        }
        let w: Vec<char> = word.chars().collect();
        if self.suffixed(&w, &[], false) {
            return true;
        }
        for p in &self.pfx {
            let Some(stem) = strip_start(&w, &p.add, &p.strip) else { continue };
            if !p.cond_ok(&stem, true) {
                continue;
            }
            if self.root(&stem.iter().collect::<String>(), &[p.flag], true) {
                return true;
            }
            if p.cross && self.suffixed(&stem, &[p.flag], true) {
                return true;
            }
        }
        self.compounded(&w, 0)
    }

    /// Is `word` spelt correctly? Capitalised and upper-case forms of a lower-case word pass.
    /// Words longer than 100 characters are not checked (they pass).
    pub fn check(&self, word: &str) -> bool {
        // Compound splitting tries every split point, so checking costs O(n²) in the word's
        // length: a 100,000-character "word" took seconds per check under a compounding
        // dictionary, on every edit (#1175).
        if word.chars().nth(MAX_CHECK_CHARS).is_some() {
            return true;
        }
        if word.contains('\u{2019}') {
            return self.check(&word.replace('\u{2019}', "'"));
        }
        if word.is_empty() || word.chars().all(|c| !c.is_alphabetic()) {
            return true;
        }
        if self.check_exact(word) {
            return true;
        }
        let lower = word.to_lowercase();
        let upper_tail = word.chars().skip(1).all(|c| !c.is_lowercase());
        let cap_only = word.chars().next().is_some_and(char::is_uppercase) && word.chars().skip(1).all(|c| !c.is_uppercase());
        let keep = |w: &str| self.words.get(w).is_some_and(|hs| hs.iter().any(|f| self.has(f, self.keep_case)));
        if (cap_only || upper_tail) && !keep(&lower) && self.check_exact(&lower) {
            return true;
        }
        if upper_tail && !cap_only {
            let cap = capitalise(&lower);
            if !keep(&cap) && self.check_exact(&cap) {
                return true;
            }
        }
        // A trailing full stop on an abbreviation (`etc.`) is part of the word in some dictionaries.
        false
    }

    fn suggestible(&self, w: &str) -> bool {
        self.check(w) && !self.words.get(w).is_some_and(|hs| hs.iter().all(|f| self.has(f, self.no_suggest) && !f.is_empty()))
    }

    /// Up to `n` corrections for a misspelt `word`, best first.
    pub fn suggest(&self, word: &str, n: usize) -> Vec<String> {
        // Edit-distance candidates grow with the word: nothing is suggested for a run-on token.
        if word.chars().count() > MAX_SUGGEST_CHARS {
            return vec![];
        }
        let cap = word.chars().next().is_some_and(char::is_uppercase);
        let upper = word.chars().count() > 1 && word.chars().any(char::is_uppercase) && !word.chars().any(char::is_lowercase);
        let lower = if upper || (cap && word.chars().skip(1).all(|c| !c.is_uppercase())) { word.to_lowercase() } else { word.to_string() };
        let w: Vec<char> = lower.chars().collect();
        let mut out: Vec<String> = vec![];
        let mut seen = HashSet::new();
        let mut push = |s: String, out: &mut Vec<String>| {
            if s == lower {
                return;
            }
            // Checked in capitals, so a KEEPCASE entry ("NASA") is offered for an all-caps word.
            let s = if upper { s.to_uppercase() } else { s };
            if seen.insert(s.to_lowercase()) && s.split(' ').all(|p| self.suggestible(p)) {
                out.push(s);
            }
        };
        for (a, b) in &self.rep {
            let mut from = 0;
            while let Some(i) = lower.get(from..).and_then(|t| t.find(a.as_str())) {
                let at = from + i;
                push(format!("{}{}{}", &lower[..at], b, &lower[at + a.len()..]), &mut out);
                from = at + a.len().max(1);
            }
        }
        let try_chars: Vec<char> = if self.try_chars.is_empty() { "esianrtolcdugmphbyfvkwzxjq".chars().collect() } else { self.try_chars.clone() };
        let s = |v: &[char]| v.iter().collect::<String>();
        for i in 0..w.len().saturating_sub(1) {
            let mut v = w.clone();
            v.swap(i, i + 1);
            push(s(&v), &mut out);
        }
        for i in 0..w.len() {
            let mut v = w.clone();
            v.remove(i);
            push(s(&v), &mut out);
        }
        for i in 0..w.len() {
            for c in &try_chars {
                if *c != w[i] {
                    let mut v = w.clone();
                    v[i] = *c;
                    push(s(&v), &mut out);
                }
            }
        }
        for i in 0..=w.len() {
            for c in &try_chars {
                let mut v = w.clone();
                v.insert(i, *c);
                push(s(&v), &mut out);
            }
        }
        for i in 1..w.len() {
            push(format!("{} {}", s(&w[..i]), s(&w[i..])), &mut out);
        }
        out.truncate(n);
        if cap && !upper {
            out = out.into_iter().map(|x| capitalise(&x)).collect();
        }
        out
    }

    /// Characters that belong inside words besides letters and digits (the `WORDCHARS` list).
    pub fn word_chars(&self) -> &[char] {
        &self.word_chars
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn strip_end(w: &[char], add: &str, strip: &str) -> Option<Vec<char>> {
    let add: Vec<char> = add.chars().collect();
    if w.len() <= add.len() && strip.is_empty() || !w.ends_with(&add) {
        return None;
    }
    let mut v = w.get(..w.len() - add.len())?.to_vec();
    v.extend(strip.chars());
    (!v.is_empty()).then_some(v)
}

fn strip_start(w: &[char], add: &str, strip: &str) -> Option<Vec<char>> {
    let add: Vec<char> = add.chars().collect();
    if w.len() <= add.len() && strip.is_empty() || !w.starts_with(&add) {
        return None;
    }
    let mut v: Vec<char> = strip.chars().collect();
    v.extend_from_slice(w.get(add.len()..)?);
    (!v.is_empty()).then_some(v)
}

/// The folders searched for dictionaries, in order.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut out = vec![];
    if let Some(v) = std::env::var_os("DECKCRAFT_DICTIONARIES") {
        out.extend(std::env::split_paths(&v));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        out.extend(home.iter().map(|h| h.join("Library/Application Support/DeckCraft/Dictionaries")));
        out.extend(home.iter().map(|h| h.join("Library/Spelling")));
        out.push("/Library/Spelling".into());
    } else if cfg!(target_os = "windows") {
        out.extend(std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DeckCraft").join("Dictionaries")));
    } else {
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        out.extend(data.iter().map(|d| d.join("deckcraft/dictionaries")));
        out.extend(data.iter().map(|d| d.join("hunspell")));
        for d in ["/usr/share/hunspell", "/usr/share/myspell", "/usr/share/myspell/dicts", "/usr/local/share/hunspell", "/app/share/hunspell"] {
            out.push(d.into());
        }
    }
    out
}

/// `en-US` → `en_US`; the language alone (`en`) as a fallback.
fn names(lang: &str) -> (String, String) {
    let full = lang.replace('-', "_");
    let base = full.split('_').next().unwrap_or("").to_lowercase();
    (full, base)
}

/// A language tag as dictionaries are named: 1–16 ASCII letters, digits, `-` or `_`.
fn valid_lang(lang: &str) -> bool {
    (1..=16).contains(&lang.len()) && lang.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The `.aff`/`.dic` pair for `lang` in `dirs`: the exact locale first, then any of its language.
pub fn find_in(dirs: &[PathBuf], lang: &str) -> Option<(PathBuf, PathBuf)> {
    // A language tag names a file in the dictionary folders and nothing else: no `..`, no separators.
    if !valid_lang(lang) {
        return None;
    }
    let (full, base) = names(lang);
    let pair = |d: &Path, stem: &str| {
        let (a, b) = (d.join(format!("{stem}.aff")), d.join(format!("{stem}.dic")));
        (a.is_file() && b.is_file()).then_some((a, b))
    };
    for d in dirs {
        if let Some(p) = pair(d, &full) {
            return Some(p);
        }
    }
    for d in dirs {
        if let Some(p) = pair(d, &base) {
            return Some(p);
        }
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        let mut stems: Vec<String> = rd
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter_map(|n| n.strip_suffix(".aff").map(str::to_string))
            .filter(|n| n.to_lowercase().starts_with(&format!("{base}_")))
            .collect();
        stems.sort();
        if let Some(p) = stems.iter().find_map(|s| pair(d, s)) {
            return Some(p);
        }
    }
    None
}

/// Longest word `Dictionary::suggest` generates corrections for.
const MAX_SUGGEST_CHARS: usize = 64;

/// Longest word `Dictionary::check` checks, as Hunspell's word length limit.
const MAX_CHECK_CHARS: usize = 100;

type Cache = Mutex<HashMap<String, Option<Arc<Dictionary>>>>;

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The dictionary for `lang` (a BCP-47 tag such as `en-US`), loaded once per process.
pub fn dictionary(lang: &str) -> Option<Arc<Dictionary>> {
    let cache = cache();
    let key = lang.to_string();
    if let Ok(c) = cache.lock()
        && let Some(d) = c.get(&key)
    {
        return d.clone();
    }
    let loaded = find_in(&search_dirs(), lang).and_then(|(a, d)| {
        let (a, d) = (std::fs::read(a).ok()?, std::fs::read(d).ok()?);
        Some(Arc::new(Dictionary::parse(&a, &d)))
    });
    if let Ok(mut c) = cache.lock() {
        c.insert(key, loaded.clone());
    }
    loaded
}

/// Where Add to Dictionary keeps words: `user.dic` in DeckCraft's own dictionaries folder, one
/// word per line (the Hunspell `.dic` format without a count).
pub fn user_dictionary_path() -> Option<PathBuf> {
    if let Ok(p) = user_override().lock()
        && let Some(p) = p.clone()
    {
        return Some(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dir = if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Application Support/DeckCraft/Dictionaries"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("DeckCraft").join("Dictionaries"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".local/share")))
            .map(|d| d.join("deckcraft/dictionaries"))
    };
    dir.map(|d| d.join("user.dic"))
}

fn user_override() -> &'static Mutex<Option<PathBuf>> {
    static P: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    P.get_or_init(Default::default)
}

struct UserWords {
    words: Option<HashSet<String>>,
    generation: u64,
}

fn user_state() -> &'static Mutex<UserWords> {
    static U: OnceLock<Mutex<UserWords>> = OnceLock::new();
    U.get_or_init(|| Mutex::new(UserWords { words: None, generation: 0 }))
}

/// Keep the user dictionary at `path` instead (tests, or a portable install).
pub fn set_user_dictionary_path(path: PathBuf) {
    if let Ok(mut p) = user_override().lock() {
        *p = Some(path);
    }
    if let Ok(mut u) = user_state().lock() {
        u.words = None;
        u.generation += 1;
    }
}

/// Bumped whenever the user dictionary changes, so cached results can be dropped.
pub fn user_generation() -> u64 {
    user_state().lock().map(|u| u.generation).unwrap_or(0)
}

fn with_user<T>(f: impl FnOnce(&HashSet<String>) -> T) -> Option<T> {
    let mut u = user_state().lock().ok()?;
    if u.words.is_none() {
        let text = user_dictionary_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        u.words = Some(text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_string).collect());
    }
    u.words.as_ref().map(f)
}

/// Add `word` to the user dictionary file (and to checks from now on).
pub fn add_user_word(word: &str) -> std::io::Result<()> {
    use std::io::Write;
    let word = word.trim();
    if word.is_empty() || word.contains(['\n', '\r']) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a word"));
    }
    if with_user(|u| u.contains(word)).unwrap_or(false) {
        return Ok(());
    }
    let path = user_dictionary_path().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no folder for the user dictionary"))?;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(f, "{word}")?;
    if let Ok(mut u) = user_state().lock() {
        if let Some(w) = u.words.as_mut() {
            w.insert(word.to_string());
        }
        u.generation += 1;
    }
    Ok(())
}

/// Is `word` right: in the user dictionary (as written, or lower-case), else by `d`.
pub fn is_correct(d: &Dictionary, word: &str) -> bool {
    with_user(|u| u.contains(word) || u.contains(&word.to_lowercase())).unwrap_or(false) || d.check(word)
}

/// A word of a paragraph: its characters `start..end` (paragraph character offsets, as text
/// positions count them) and the language of the run it starts in.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub lang: String,
    /// The same word twice in a row: this is the second, and deleting `cut..end` removes it with
    /// the space before it.
    pub repeated: Option<usize>,
}

/// The words of `p`, each with its run's language (`lang` on the run, else `default`). Words
/// with digits are skipped; apostrophes inside a word belong to it; hyphenated words are checked
/// part by part.
pub fn paragraph_words(p: &deckcraft_model::Paragraph, default: &str) -> Vec<Word> {
    let mut chars: Vec<(char, usize)> = vec![];
    for (ri, r) in p.runs.iter().enumerate() {
        chars.extend(r.content().chars().map(|c| (c, ri)));
    }
    let lang_of = |ri: usize| p.runs.get(ri).and_then(|r| r.props.lang.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| default.to_string());
    let inside = |c: char| c.is_alphanumeric() || c == '\'' || c == '’' || c == '-';
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        if !inside(chars[i].0) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && inside(chars[i].0) {
            i += 1;
        }
        let (mut a, mut b) = (start, i);
        while a < b && !chars[a].0.is_alphanumeric() {
            a += 1;
        }
        while b > a && !chars[b - 1].0.is_alphanumeric() {
            b -= 1;
        }
        let text: String = chars[a..b].iter().map(|c| c.0).collect();
        if text.is_empty() || text.chars().any(|c| c.is_ascii_digit()) || !text.chars().any(char::is_alphabetic) {
            continue;
        }
        let mut k = a;
        for part in text.split('-') {
            let n = part.chars().count();
            if !part.is_empty()
                && let Some(c) = chars.get(k)
            {
                if p.runs.get(c.1).is_some_and(|r| r.props.no_proof == Some(true)) {
                    // Marked "do not check spelling".
                } else {
                    out.push(Word { start: k, end: k + n, text: part.to_string(), lang: lang_of(c.1), repeated: None });
                }
            }
            k += n + 1;
        }
    }
    out
}

/// Words that repeat the word just before them (`the the`), with only spaces between, as
/// PowerPoint flags them. Hyphenated parts and different languages don't count.
pub fn repeated(p: &deckcraft_model::Paragraph, default: &str) -> Vec<Word> {
    let words = paragraph_words(p, default);
    let chars: Vec<char> = p.runs.iter().flat_map(|r| r.content().chars()).collect();
    let mut out = vec![];
    for pair in words.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let gap = chars.get(a.end..b.start).unwrap_or(&[]);
        if !gap.is_empty()
            && gap.iter().all(|c| *c == ' ' || *c == '\u{a0}' || *c == '\t')
            && a.lang == b.lang
            && a.text.to_lowercase() == b.text.to_lowercase()
        {
            out.push(Word { repeated: Some(a.end), ..b.clone() });
        }
    }
    out
}

/// The misspelt and repeated words of `p`, each checked in its own language (languages with no dictionary
/// installed are skipped), leaving out `ignore` (lower-case).
pub fn misspelt(p: &deckcraft_model::Paragraph, default: &str, ignore: &[String]) -> Vec<Word> {
    let mut dicts: HashMap<String, Option<Arc<Dictionary>>> = HashMap::new();
    paragraph_words(p, default)
        .into_iter()
        .filter(|w| !ignore.contains(&w.text.to_lowercase()))
        .filter(|w| {
            let d = dicts.entry(w.lang.clone()).or_insert_with(|| dictionary(&w.lang));
            d.as_ref().is_some_and(|d| !is_correct(d, &w.text))
        })
        .chain(repeated(p, default))
        .collect::<Vec<_>>()
        .into_iter()
        .fold(vec![], |mut v: Vec<Word>, w| {
            // A repeated misspelt word is reported once, as repeated.
            if let Some(i) = v.iter().position(|x| x.start == w.start) {
                v[i] = w;
            } else {
                v.push(w);
            }
            v
        })
}

/// Use `d` for `lang` from now on in this process (a dictionary the app loaded itself).
pub fn install(lang: &str, d: Dictionary) {
    if let Ok(mut c) = cache().lock() {
        c.insert(lang.to_string(), Some(Arc::new(d)));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // A contributor-original miniature dictionary in Hunspell format.
    pub(crate) const AFF: &str = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwzxjq\nFORBIDDENWORD !\nNEEDAFFIX ?\nKEEPCASE K\n\
REP 1\nREP f ph\n\
PFX U Y 1\nPFX U 0 un .\n\
SFX S Y 2\nSFX S y ies [^aeiou]y\nSFX S 0 s [^y]\n\
SFX G Y 2\nSFX G e ing e\nSFX G 0 ing [^e]\n\
SFX D Y 1\nSFX D 0 ed/S .\n";
    pub(crate) const DIC: &str = "9\nslide/S\nmake/G\nhappy\ncity/S\ndo/U\nwork/USG\nphoto/S\nNASA/K\nfoo/?S\n";

    fn d() -> Dictionary {
        Dictionary::parse(AFF.as_bytes(), DIC.as_bytes())
    }

    #[test]
    fn curly_apostrophe_matches_ascii_entry() {
        let d = Dictionary::parse(AFF.as_bytes(), "2\ndon't\nwork\n".as_bytes());
        assert!(d.check("don't"));
        assert!(d.check("don\u{2019}t"));
    }

    #[test]
    fn roots_affixes_and_cross_products_check() {
        let d = d();
        for w in ["slide", "slides", "making", "cities", "undo", "unworks", "working", "Slide", "SLIDES", "NASA", "foos"] {
            assert!(d.check(w), "{w}");
        }
        for w in ["slidez", "makeing", "citys", "unslide", "Nasa", "nasa", "foo", "teh"] {
            assert!(!d.check(w), "{w}");
        }
    }

    #[test]
    fn a_language_tag_cannot_leave_the_dictionary_folders() {
        let dir = std::env::temp_dir().join(format!("deckcraft-dict-lang-{}", std::process::id()));
        let inner = dir.join("dicts");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(dir.join("evil.aff"), "SET UTF-8\n").unwrap();
        std::fs::write(dir.join("evil.dic"), "1\nword\n").unwrap();
        assert!(find_in(std::slice::from_ref(&inner), "../evil").is_none());
        assert!(find_in(std::slice::from_ref(&inner), "en/../../evil").is_none());
        assert!(!valid_lang("") && !valid_lang("a".repeat(17).as_str()) && valid_lang("en-US") && valid_lang("pt_BR"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggestions_come_from_edits_and_replacements() {
        let d = d();
        assert_eq!(d.suggest("sldie", 3).first().map(String::as_str), Some("slide"));
        assert!(d.suggest("foto", 5).contains(&"photo".to_string()));
        assert!(d.suggest("Slidse", 5).contains(&"Slides".to_string()));
        assert!(d.suggest("workingslides", 5).contains(&"working slides".to_string()));
    }

    /// #1175: compound splitting is quadratic in the word's length, so a word longer than
    /// `MAX_CHECK_CHARS` is not checked; shorter ones still are.
    #[test]
    fn a_word_longer_than_the_check_limit_is_not_checked() {
        let d = Dictionary::parse(b"COMPOUNDFLAG X\nCOMPOUNDMIN 3\n", b"2\nfoot/X\nball/X\n".as_slice());
        assert!(d.check("footballfoot"), "a compound is checked");
        let under: String = "footb".repeat(MAX_CHECK_CHARS / 5);
        assert_eq!(under.chars().count(), MAX_CHECK_CHARS);
        assert!(!d.check(&under), "a word at the limit is still checked");
        // Unchecked, not seconds of compound splitting.
        assert!(d.check(&"footb".repeat(20_000)));
        assert!(d.check(&format!("{under}x")));
    }

    #[test]
    fn all_caps_words_get_uppercase_suggestions() {
        let d = d();
        assert!(d.suggest("SLDE", 5).contains(&"SLIDE".to_string()));
        assert!(d.suggest("FOTO", 5).contains(&"PHOTO".to_string()));
        assert!(d.suggest("NSSA", 5).contains(&"NASA".to_string()));
    }

    #[test]
    fn compounds_join_flagged_parts() {
        let aff = "COMPOUNDFLAG X\nCOMPOUNDMIN 3\nONLYINCOMPOUND O\nSFX S Y 1\nSFX S 0 en .\n";
        let d = Dictionary::parse(aff.as_bytes(), b"4\nhaus/X\ntuer/XS\nschluessel/X\nfugs/OX\n".as_slice());
        for w in ["haustuer", "haustueren", "haustuerschluessel", "haus", "hausfugstuer"] {
            assert!(d.check(w), "{w}");
        }
        for w in ["hausx", "fugs", "tuerhausz"] {
            assert!(!d.check(w), "{w}");
        }
    }

    #[test]
    fn runs_give_words_their_language() {
        use deckcraft_model::{Paragraph, Run, RunProps};
        let p = Paragraph {
            runs: vec![
                Run::with("Hello wor", RunProps { lang: Some("en-GB".into()), ..Default::default() }),
                Run::with("ld, Grüße well-known 3rd l'été", RunProps { lang: Some("de-DE".into()), ..Default::default() }),
            ],
            ..Default::default()
        };
        let w = paragraph_words(&p, "en-US");
        let got: Vec<(&str, &str, usize)> = w.iter().map(|w| (w.text.as_str(), w.lang.as_str(), w.start)).collect();
        assert_eq!(
            got,
            vec![
                ("Hello", "en-GB", 0),
                ("world", "en-GB", 6),
                ("Grüße", "de-DE", 13),
                ("well", "de-DE", 19),
                ("known", "de-DE", 24),
                ("l'été", "de-DE", 34)
            ]
        );
    }

    #[test]
    fn repeated_words_and_no_proof_runs() {
        use deckcraft_model::{Paragraph, Run, RunProps};
        let p = Paragraph::new("See the the plan, plan. The THE end");
        let r: Vec<(usize, usize, Option<usize>)> = repeated(&p, "en-US").iter().map(|w| (w.start, w.end, w.repeated)).collect();
        assert_eq!(r, vec![(8, 11, Some(7)), (28, 31, Some(27))]);
        let p = Paragraph {
            runs: vec![Run::new("check "), Run::with("xyzzy qq", RunProps { no_proof: Some(true), ..Default::default() }), Run::new(" done")],
            ..Default::default()
        };
        let w: Vec<String> = paragraph_words(&p, "en-US").into_iter().map(|w| w.text).collect();
        assert_eq!(w, vec!["check", "done"]);
    }

    #[test]
    fn long_and_numeric_flags_and_latin1() {
        let d = Dictionary::parse(b"FLAG long\nSFX Aa Y 1\nSFX Aa 0 s .\n", b"1\ncat/Aa\n");
        assert!(d.check("cats"));
        let d = Dictionary::parse(b"FLAG num\nSFX 101 Y 1\nSFX 101 0 s .\n", b"1\ndog/7,101\n");
        assert!(d.check("dogs"));
        let d = Dictionary::parse(b"SET ISO8859-1\n", b"1\ncaf\xe9\n");
        assert!(d.check("café"));
    }

    #[test]
    fn dictionaries_are_found_by_locale_then_language() {
        let dir = std::env::temp_dir().join(format!("deckcraft-dict-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        for n in ["en_GB", "de_DE"] {
            let _ = std::fs::write(dir.join(format!("{n}.aff")), "");
            let _ = std::fs::write(dir.join(format!("{n}.dic")), "");
        }
        let dirs = vec![dir.clone()];
        let stem = |l: &str| find_in(&dirs, l).and_then(|(a, _)| a.file_stem().map(|s| s.to_string_lossy().into_owned()));
        assert_eq!(stem("de-DE").as_deref(), Some("de_DE"));
        assert_eq!(stem("en-US").as_deref(), Some("en_GB"));
        assert_eq!(stem("fr-FR"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
