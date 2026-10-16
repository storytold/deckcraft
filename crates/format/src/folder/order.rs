//! `slides.order`: slide order and sections, one slide key per line (R8).
//!
//! Grammar: empty lines and lines starting with `#` are ignored; `section: <name>` opens a section;
//! any other line is a slide key. Slides before the first section belong to none.

use std::collections::HashSet;

use super::key::is_key;

pub const HEADER: &str = "# slides.order v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Section(String),
    Slide(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Order {
    pub entries: Vec<Entry>,
}

/// Sections: name and the keys of their slides.
pub type Sections = Vec<(String, Vec<String>)>;

impl Order {
    pub fn parse(text: &str) -> Result<Order, String> {
        let mut entries = vec![];
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim_end_matches('\r');
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix("section:") {
                entries.push(Entry::Section(name.strip_prefix(' ').unwrap_or(name).to_string()));
            } else {
                let key = line.trim();
                if !is_key(key) {
                    return Err(format!("slides.order, line {}: invalid slide key `{key}`", n + 1));
                }
                entries.push(Entry::Slide(key.to_string()));
            }
        }
        Ok(Order { entries })
    }

    /// The file text, starting with `header` (a `#` comment line).
    pub fn render(&self, header: &str) -> String {
        let mut s = String::from(header);
        s.push('\n');
        for e in &self.entries {
            match e {
                Entry::Section(name) => {
                    s.push_str("section: ");
                    s.push_str(name);
                }
                Entry::Slide(k) => s.push_str(k),
            }
            s.push('\n');
        }
        s
    }

    pub fn keys(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter_map(|e| match e {
                Entry::Slide(k) => Some(k.as_str()),
                Entry::Section(_) => None,
            })
            .collect()
    }

    /// The sections the file describes.
    pub fn sections(&self) -> Sections {
        let mut out: Sections = vec![];
        for e in &self.entries {
            match e {
                Entry::Section(name) => out.push((name.clone(), vec![])),
                Entry::Slide(k) => {
                    if let Some(last) = out.last_mut() {
                        last.1.push(k.clone());
                    }
                }
            }
        }
        out
    }

    /// The order of `keys` with `sections`; `None` when the sections can't be written this way
    /// (not contiguous and in slide order, a slide in two sections, a line break in a name).
    pub fn build(keys: &[String], sections: &Sections) -> Option<Order> {
        let mut entries = vec![];
        let in_section: HashSet<&String> = sections.iter().flat_map(|(_, ks)| ks).collect();
        for k in keys.iter().filter(|k| !in_section.contains(k)) {
            entries.push(Entry::Slide(k.clone()));
        }
        for (name, ks) in sections {
            if name.contains(['\n', '\r']) {
                return None;
            }
            entries.push(Entry::Section(name.clone()));
            entries.extend(ks.iter().map(|k| Entry::Slide(k.clone())));
        }
        let o = Order { entries };
        let same_keys = o.keys().iter().copied().eq(keys.iter().map(String::as_str));
        (same_keys && o.sections() == *sections).then_some(o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(n: u8) -> String {
        format!("01JA7Q3M5K8X9Y2Z3A4B5C6D7{}", char::from(n))
    }

    #[test]
    fn roundtrip_with_sections() {
        let keys = vec![k(b'A'), k(b'B'), k(b'C')];
        let secs = vec![("Intro".to_string(), vec![k(b'A')]), ("Appendix: more".to_string(), vec![k(b'B'), k(b'C')])];
        let o = Order::build(&keys, &secs).unwrap();
        let text = o.render(HEADER);
        assert!(text.starts_with("# slides.order v1\nsection: Intro\n"), "{text}");
        let p = Order::parse(&text).unwrap();
        assert_eq!(p, o);
        assert_eq!(p.sections(), secs);
        assert_eq!(p.keys(), keys.iter().map(String::as_str).collect::<Vec<_>>());
    }

    #[test]
    fn slides_before_the_first_section_and_empty_sections() {
        let keys = vec![k(b'A'), k(b'B')];
        let secs = vec![("Empty".to_string(), vec![]), ("Rest".to_string(), vec![k(b'B')])];
        let o = Order::build(&keys, &secs).unwrap();
        assert_eq!(Order::parse(&o.render(HEADER)).unwrap().sections(), secs);
    }

    #[test]
    fn unwritable_sections_are_refused() {
        let keys = vec![k(b'A'), k(b'B'), k(b'C')];
        let split = vec![("S1".to_string(), vec![k(b'A'), k(b'C')]), ("S2".to_string(), vec![k(b'B')])];
        assert!(Order::build(&keys, &split).is_none());
        let twice = vec![("S1".to_string(), vec![k(b'A')]), ("S2".to_string(), vec![k(b'A'), k(b'B'), k(b'C')])];
        assert!(Order::build(&keys, &twice).is_none());
        let newline = vec![("a\nb".to_string(), keys.clone())];
        assert!(Order::build(&keys, &newline).is_none());
    }

    #[test]
    fn bad_key_is_an_error_and_comments_are_skipped() {
        assert!(Order::parse("# x\nnot-a-key\n").is_err());
        assert_eq!(Order::parse("# deckgit slides.order v1\n\r\n# note\n01JA7Q3M5K8X9Y2Z3A4B5C6D7A\r\n").unwrap().keys().len(), 1);
    }
}
