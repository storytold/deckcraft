//! Review › Thesaurus: synonyms from a small built-in English word list (`data/thesaurus.txt`),
//! looked up offline so it works in the browser build too.

const DATA: &str = include_str!("../data/thesaurus.txt");

/// One sense of a word: its headword and synonyms.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Sense {
    pub word: String,
    pub synonyms: Vec<String>,
}

fn senses_of(head: &str) -> Vec<Sense> {
    DATA.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('|'))
        .filter(|(h, _)| h.trim() == head)
        .map(|(h, s)| Sense { word: h.trim().to_string(), synonyms: s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect() })
        .collect()
}

/// The senses for `word`, trying simple English endings when the word itself isn't listed
/// ("goals" → "goal", "improved" → "improve", "growing" → "grow"). Synonyms take the word's
/// capitalisation ("Goal" → "Aim", "GOAL" → "AIM").
pub fn lookup(word: &str) -> Vec<Sense> {
    let w: String = word.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '-').chars().take(64).collect();
    let lower = w.to_lowercase();
    if lower.is_empty() {
        return vec![];
    }
    let mut tries = vec![lower.clone()];
    for (suffix, add) in [("ies", "y"), ("es", ""), ("s", ""), ("ied", "y"), ("ed", "e"), ("ed", ""), ("ing", "e"), ("ing", "")] {
        if let Some(stem) = lower.strip_suffix(suffix)
            && stem.chars().count() >= 2
        {
            tries.push(format!("{stem}{add}"));
        }
    }
    let Some(found) = tries.iter().map(|t| senses_of(t)).find(|v| !v.is_empty()) else { return vec![] };
    let upper = w.chars().count() > 1 && w.chars().all(|c| !c.is_lowercase());
    let title = !upper && w.chars().next().is_some_and(char::is_uppercase);
    found
        .into_iter()
        .map(|s| Sense {
            word: s.word,
            synonyms: s
                .synonyms
                .into_iter()
                .map(|x| {
                    if upper {
                        x.to_uppercase()
                    } else if title {
                        let mut c = x.chars();
                        c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
                    } else {
                        x
                    }
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn senses_endings_and_case() {
        let g = lookup("goal");
        assert_eq!(g.len(), 1);
        assert!(g[0].synonyms.contains(&"objective".to_string()));
        assert_eq!(lookup("change").len(), 2, "verb and noun senses");
        assert_eq!(lookup("Goals")[0].synonyms[0], "Aim");
        assert_eq!(lookup("GOAL")[0].synonyms[0], "AIM");
        assert_eq!(lookup("improved")[0].word, "improve");
        assert_eq!(lookup("growing")[0].word, "grow");
        assert_eq!(lookup("strategies")[0].word, "strategy");
        assert_eq!(lookup("“goal,”")[0].word, "goal", "punctuation around the word");
        assert!(lookup("").is_empty() && lookup("zzxq").is_empty() && lookup("   ").is_empty());
        assert!(lookup(&"a".repeat(10_000)).is_empty());
    }

    #[test]
    fn data_is_well_formed() {
        for l in DATA.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let (h, s) = l.split_once('|').unwrap_or_else(|| panic!("no `|`: {l}"));
            assert_eq!(h, h.trim().to_lowercase(), "{l}");
            let syn: Vec<&str> = s.split(',').map(str::trim).collect();
            assert!(syn.len() >= 2 && syn.iter().all(|x| !x.is_empty() && *x != h), "{l}");
            let mut d = syn.clone();
            d.sort();
            d.dedup();
            assert_eq!(d.len(), syn.len(), "duplicate synonym: {l}");
        }
    }

    #[test]
    fn commands_look_up_and_replace_the_word_at_the_caret() {
        use serde_json::json;
        let mut s = crate::Session::with_new();
        let id = s.execute("insert.textBox", &json!({"rect": [50, 50, 400, 40]})).unwrap()["id"].as_u64().unwrap();
        s.execute("text.edit", &json!({"id": id})).unwrap();
        s.execute("text.insert", &json!({"text": "Our Goal is growth."})).unwrap();
        s.execute("text.edit", &json!({"id": id, "at": [0, 6]})).unwrap();
        let r = s.execute("review.thesaurus", &json!({})).unwrap();
        assert_eq!(r["word"], "Goal");
        assert_eq!(r["senses"][0]["synonyms"][0], "Aim");
        s.execute("review.replaceWord", &json!({"with": "Aim"})).unwrap();
        let text = |s: &crate::Session| {
            s.doc().unwrap().shapes().iter().find(|x| x.id.0 as u64 == id).and_then(|x| x.text.as_ref()).map(|t| t.paragraphs[0].text()).unwrap()
        };
        assert_eq!(text(&s), "Our Aim is growth.");
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(&s), "Our Goal is growth.");
        assert_eq!(s.execute("review.thesaurus", &json!({"word": "growth."})).unwrap()["senses"][0]["word"], "growth");
        assert!(s.execute("review.replaceWord", &json!({"with": ""})).is_err());
    }
}
