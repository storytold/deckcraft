//! Compile-time embedded translation catalog loaded from UTF-8 `.tsv` files.
//!
//! File format (`.tsv`):
//! - One entry per line: `<english-key>\t<translated-value>`
//! - Blank lines and lines starting with `#` (after trimming leading spaces) are ignored.
//! - Escape sequences inside keys and values: `\n` (newline), `\t` (tab), `\\` (backslash).
//! - Context-disambiguated keys use the convention `<context>::<english>` (see [`super::tc`]).
//! - Plural keys use `<english>#one` and `<english>#other` (or `#zero`, `#few`, `#many`
//!   for languages that need finer categories; Vietnamese uses the invariant `#other` form,
//!   falling back to `#one` or the base key if only one is supplied).

use std::collections::HashMap;
use std::sync::OnceLock;

/// Parsed translation catalog for a single target language.
#[derive(Debug, Default)]
pub struct Catalog {
    entries: HashMap<String, String>,
}

impl Catalog {
    /// Parse a UTF-8 `.tsv` catalog source string.
    #[must_use]
    pub fn parse(tsv: &str) -> Self {
        let mut entries = HashMap::new();
        for raw_line in tsv.lines() {
            let line = raw_line.trim_end_matches('\r');
            let trimmed_start = line.trim_start_matches(' ');
            if trimmed_start.is_empty() || trimmed_start.starts_with('#') {
                continue;
            }
            if let Some((raw_k, raw_v)) = line.split_once('\t') {
                let k = unescape(raw_k);
                let v = unescape(raw_v);
                if !k.is_empty() && !v.is_empty() {
                    entries.insert(k, v);
                }
            }
        }
        Self { entries }
    }

    /// Look up an exact key, returning the translated slice when present.
    #[must_use]
    pub fn get<'a>(&'a self, key: &str) -> Option<&'a str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Look up a context-qualified key (`"<ctx>::<key>"`), falling back to `<key>`.
    #[must_use]
    pub fn get_ctx<'a>(&'a self, ctx: &str, key: &str) -> Option<&'a str> {
        let mut buf = String::with_capacity(ctx.len() + 2 + key.len());
        buf.push_str(ctx);
        buf.push_str("::");
        buf.push_str(key);
        self.get(&buf).or_else(|| self.get(key))
    }

    /// Number of entries in the catalog.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the catalog has zero entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over `(key, value)` pairs in the catalog.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Raw UTF-8 `.tsv` source for the Vietnamese (`vi`) catalog.
pub const VI_TSV: &str = include_str!("vi.tsv");

/// Lazily parsed global Vietnamese catalog.
#[must_use]
pub fn vi_catalog() -> &'static Catalog {
    static VI: OnceLock<Catalog> = OnceLock::new();
    VI.get_or_init(|| Catalog::parse(VI_TSV))
}
