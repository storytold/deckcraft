use super::*;

/// Built-in AutoCorrect replacements (common slips and symbols).
pub const AUTOCORRECT: &[(&str, &str)] = &[
    ("teh", "the"),
    ("adn", "and"),
    ("recieve", "receive"),
    ("recieved", "received"),
    ("seperate", "separate"),
    ("occured", "occurred"),
    ("occurence", "occurrence"),
    ("thier", "their"),
    ("wich", "which"),
    ("acheive", "achieve"),
    ("accross", "across"),
    ("adress", "address"),
    ("alot", "a lot"),
    ("appearence", "appearance"),
    ("arguement", "argument"),
    ("basicly", "basically"),
    ("beggining", "beginning"),
    ("beleive", "believe"),
    ("calender", "calendar"),
    ("commitee", "committee"),
    ("definately", "definitely"),
    ("dont", "don't"),
    ("doesnt", "doesn't"),
    ("didnt", "didn't"),
    ("isnt", "isn't"),
    ("embarass", "embarrass"),
    ("enviroment", "environment"),
    ("existance", "existence"),
    ("foward", "forward"),
    ("freind", "friend"),
    ("goverment", "government"),
    ("happend", "happened"),
    ("i", "I"),
    ("ive", "I've"),
    ("immediatly", "immediately"),
    ("independant", "independent"),
    ("knowlege", "knowledge"),
    ("libary", "library"),
    ("neccessary", "necessary"),
    ("necesary", "necessary"),
    ("noticable", "noticeable"),
    ("occassion", "occasion"),
    ("posession", "possession"),
    ("prefered", "preferred"),
    ("publically", "publicly"),
    ("realy", "really"),
    ("recomend", "recommend"),
    ("refered", "referred"),
    ("relevent", "relevant"),
    ("succesful", "successful"),
    ("tommorow", "tomorrow"),
    ("tomorow", "tomorrow"),
    ("truely", "truly"),
    ("untill", "until"),
    ("wierd", "weird"),
    ("youre", "you're"),
    ("(c)", "©"),
    ("(r)", "®"),
    ("(tm)", "™"),
    ("(e)", "€"),
    ("...", "…"),
    ("->", "→"),
    ("<-", "←"),
    ("=>", "⇒"),
    ("--", "–"),
];

const DAYS: &[&str] = &["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

/// The replacement for `word` from your entries, then the built-in list.
fn replacement(prefs: &crate::Prefs, word: &str) -> Option<String> {
    if let Some((_, to)) = prefs.ac_entries.iter().find(|(k, _)| k == word) {
        return Some(to.clone());
    }
    if let Some((_, to)) = prefs.ac_entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(word)) {
        return Some(to.clone());
    }
    let (k, to) = AUTOCORRECT.iter().find(|(k, _)| k.eq_ignore_ascii_case(word))?;
    if prefs.ac_removed.iter().any(|r| r.eq_ignore_ascii_case(k)) {
        return None;
    }
    // "i" only on its own, lower case.
    if *k == "i" && word != "i" {
        return None;
    }
    Some(if word.chars().next().is_some_and(char::is_uppercase) { ed::change_case(to, "sentence") } else { to.to_string() })
}

/// What a just-typed `word` becomes (`None`: unchanged). `sentence_start`: nothing but spaces
/// or a sentence's end (`.`, `!`, `?`) before it in the paragraph. `typing`: one character was
/// typed (the case rules apply only then).
pub fn correct_word(prefs: &crate::Prefs, word: &str, sentence_start: bool, typing: bool) -> Option<String> {
    let mut out = replacement(prefs, word);
    if typing && out.is_none() {
        let chars: Vec<char> = word.chars().collect();
        let letters = chars.iter().all(|c| c.is_alphabetic());
        if prefs.ac_fractions {
            out = match word {
                "1/2" => Some("½".into()),
                "1/4" => Some("¼".into()),
                "3/4" => Some("¾".into()),
                _ => None,
            };
        }
        if out.is_none() && letters && chars.len() >= 3 {
            let upper = |c: &char| c.is_uppercase();
            let rest_lower = chars.get(2..).is_some_and(|r| r.iter().all(|c| c.is_lowercase()));
            let rest_upper = chars.get(1..).is_some_and(|r| r.iter().all(upper));
            if prefs.ac_two_initials && chars.first().is_some_and(upper) && chars.get(1).is_some_and(upper) && rest_lower {
                out =
                    Some(chars.iter().enumerate().map(|(i, c)| if i == 1 { c.to_lowercase().collect::<String>() } else { c.to_string() }).collect());
            } else if prefs.ac_caps_lock && chars.first().is_some_and(|c| c.is_lowercase()) && rest_upper {
                out = Some(
                    chars
                        .iter()
                        .enumerate()
                        .map(|(i, c)| if i == 0 { c.to_uppercase().collect::<String>() } else { c.to_lowercase().collect() })
                        .collect(),
                );
            }
        }
        if out.is_none() && prefs.ac_days && DAYS.contains(&word) {
            out = Some(ed::change_case(word, "sentence"));
        }
    }
    if typing && prefs.ac_sentence_caps && sentence_start {
        let w = out.clone().unwrap_or_else(|| word.to_string());
        let mut cs = w.chars();
        if let Some(f) = cs.next()
            && f.is_lowercase()
            && w.chars().all(char::is_alphabetic)
        {
            out = Some(f.to_uppercase().chain(cs).collect());
        }
    }
    out.filter(|o| o != word)
}

/// After a space or punctuation, correct the word just typed.
pub(super) fn autocorrect(s: &mut Session, typing: bool) -> Result<()> {
    if !s.prefs.autocorrect {
        return Ok(());
    }
    let st = s.doc()?;
    let Some(t) = st.selection.text.clone() else { return Ok(()) };
    let Some(body) = body_of(st, &t) else { return Ok(()) };
    let line: Vec<char> = body.paragraphs.get(t.caret.0).map(|p| p.text().chars().collect()).unwrap_or_default();
    let end = t.caret.1;
    if end == 0 || !line.get(end - 1).is_some_and(|c| c.is_whitespace() || ".,;:!?".contains(*c)) {
        return Ok(());
    }
    if typing && s.prefs.ac_lists {
        // A lead such as `i.` or `a.` is kept as typed, so the space after it starts its list.
        let lead: String = line.get(..end).map(|l| l.iter().collect()).unwrap_or_default();
        if auto_list_style(&lead).is_some() || start_auto_list(s, &t, &line, end)? {
            return Ok(());
        }
    }
    let mut word_end = end - 1;
    let mut start = word_end;
    while start > 0 && line.get(start - 1).is_some_and(|c| !c.is_whitespace()) {
        start -= 1;
    }
    // "word," and "word." also count: the punctuation was typed before the space.
    let mut word: String = line.get(start..word_end).map(|w| w.iter().collect()).unwrap_or_default();
    if replacement(&s.prefs, &word).is_none() {
        let trimmed = word.trim_end_matches(|c: char| ".,;:!?".contains(c));
        if !trimmed.is_empty() && trimmed.len() < word.len() && !word.ends_with("...") {
            word_end -= word.chars().count() - trimmed.chars().count();
            word = trimmed.to_string();
        }
    }
    let before = line.get(..start).map(|b| b.iter().rev().find(|c| !c.is_whitespace()).copied()).unwrap_or(None);
    let sentence_start = before.is_none_or(|c| ".!?".contains(c));
    // "word..." becomes "word…", the word corrected too.
    let ellipsis = word.chars().count() > 3 && word.ends_with("...") && replacement(&s.prefs, "...").is_some();
    let rep = if ellipsis {
        let stem = word.trim_end_matches('.');
        Some(format!("{}…", correct_word(&s.prefs, stem, sentence_start, typing).unwrap_or_else(|| stem.to_string())))
    } else {
        correct_word(&s.prefs, &word, sentence_start, typing)
    };
    let Some(rep) = rep else {
        if typing && s.prefs.ac_ordinals && is_ordinal(&word) {
            return superscript_suffix(s, &t, word_end);
        }
        return Ok(());
    };
    let delta = rep.chars().count() as isize - word.chars().count() as isize;
    let st = s.doc_mut()?;
    let mut d = (*st.doc).clone();
    let sel = st.selection.clone();
    if let Some(b) = body_mut(&mut d, &sel, &t) {
        ed::delete(b, (t.caret.0, start), (t.caret.0, word_end));
        ed::insert(b, (t.caret.0, start), &rep, None);
    }
    st.doc = Arc::new(d);
    let c = (t.caret.0, (end as isize + delta).max(0) as usize);
    if let Some(tt) = st.selection.text.as_mut() {
        tt.caret = c;
        tt.anchor = c;
    }
    st.revision += 1;
    Ok(())
}

/// A list AutoFormat started: the text being edited right after it, the typed lead with its
/// space, the paragraph's own formatting before, and the revision the change left.
#[derive(Clone, Debug)]
pub struct AutoList {
    sel: TextSel,
    typed: String,
    props: deckcraft_model::text::ParaProps,
    revision: u64,
}

/// The list a paragraph that starts with `lead` and a space or tab becomes, as the command and
/// parameters that make it: `*` •, `-` or `–` –, `--` or `—` ■, `>` ➢, `<>` ◆, and the numbering
/// styles `1.`, `1)`, `I.`, `i.`, `A.`, `a.` and `a)`.
fn auto_list_style(lead: &str) -> Option<(&'static str, Value)> {
    let bullet = |c: &str| Some(("format.bullets", json!({"on": true, "char": c})));
    let number = |scheme: &str| Some(("format.numbering", json!({"on": true, "scheme": scheme})));
    match lead {
        "*" => bullet("•"),
        "-" | "–" => bullet("–"),
        "--" | "—" => bullet("■"),
        ">" => bullet("➢"),
        "<>" => bullet("◆"),
        "1." => number("arabicPeriod"),
        "1)" => number("arabicParenR"),
        "I." => number("romanUcPeriod"),
        "i." => number("romanLcPeriod"),
        "A." => number("alphaUcPeriod"),
        "a." => number("alphaLcPeriod"),
        "a)" => number("alphaLcParenR"),
        _ => None,
    }
}

/// AutoFormat's automatic bulleted and numbered lists: when the caret's paragraph holds only a
/// list lead (`*`, `1.`…) before the space or tab just typed, the lead and the space are removed
/// and the paragraph becomes a list paragraph in that style. `false` when nothing changed.
fn start_auto_list(s: &mut Session, t: &TextSel, line: &[char], end: usize) -> Result<bool> {
    if t.is_range() || !line.get(end - 1).is_some_and(|c| *c == ' ' || *c == '\t') {
        return Ok(false);
    }
    let lead: String = line.get(..end - 1).map(|l| l.iter().collect()).unwrap_or_default();
    let Some((id, params)) = auto_list_style(&lead) else { return Ok(false) };
    let typed: String = line.get(..end).map(|l| l.iter().collect()).unwrap_or_default();
    let para = t.caret.0;
    let props = body_of(s.doc()?, t).and_then(|b| b.paragraphs.get(para)).map(|p| p.props.clone()).unwrap_or_default();
    edit_text(s, |body, tt| {
        ed::delete(body, (para, 0), (para, end));
        tt.anchor = (para, 0);
        tt.caret = (para, 0);
    })?;
    s.execute(id, &params)?;
    let st = s.doc_mut()?;
    if let Some(sel) = st.selection.text.clone() {
        st.auto_list = Some(AutoList { sel, typed, props, revision: st.revision });
    }
    Ok(true)
}

/// Backspace right after AutoFormat started a list: the paragraph gets back the typed lead and
/// space and the formatting it had. `false` when the last change was anything else.
pub(super) fn undo_auto_list(s: &mut Session) -> Result<bool> {
    let st = s.doc_mut()?;
    let Some(a) = st.auto_list.take() else { return Ok(false) };
    if a.revision != st.revision || st.selection.text.as_ref() != Some(&a.sel) {
        return Ok(false);
    }
    edit_text(s, |body, t| {
        if let Some(p) = body.paragraphs.get_mut(a.sel.caret.0) {
            p.props = a.props.clone();
        }
        let c = ed::insert(body, a.sel.caret, &a.typed, None);
        t.anchor = c;
        t.caret = c;
    })?;
    Ok(true)
}

/// `word` is a number with the ordinal suffix it takes (1st, 22nd, 103rd, 11th), as AutoFormat
/// superscripts it.
fn is_ordinal(word: &str) -> bool {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_lowercase());
    let suffix = word.get(digits.len()..).unwrap_or_default();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || suffix.len() != 2 {
        return false;
    }
    let d = digits.as_bytes();
    let teen = d.len() >= 2 && d.get(d.len() - 2) == Some(&b'1');
    let want = match d.last() {
        _ if teen => "th",
        Some(b'1') => "st",
        Some(b'2') => "nd",
        Some(b'3') => "rd",
        _ => "th",
    };
    suffix == want
}

/// Superscript the two letters before `end` in the caret's paragraph (an ordinal's suffix).
fn superscript_suffix(s: &mut Session, t: &crate::TextSel, end: usize) -> Result<()> {
    let (a, b) = ((t.caret.0, end.saturating_sub(2)), (t.caret.0, end));
    let st = s.doc_mut()?;
    if body_of(st, t).is_none_or(|body| ed::props_at(body, b).baseline.unwrap_or(0.0) > 0.0) {
        return Ok(());
    }
    let mut d = (*st.doc).clone();
    let sel = st.selection.clone();
    if let Some(body) = body_mut(&mut d, &sel, t) {
        ed::format(body, a, b, &|r| r.baseline = Some(0.3));
    }
    st.doc = Arc::new(d);
    st.revision += 1;
    Ok(())
}

pub(super) fn autocorrect_options(s: &mut Session, p: &Value) -> Result<Value> {
    let pr = &mut s.prefs;
    for (key, slot) in [
        ("on", &mut pr.autocorrect),
        ("twoInitials", &mut pr.ac_two_initials),
        ("sentenceCaps", &mut pr.ac_sentence_caps),
        ("days", &mut pr.ac_days),
        ("capsLock", &mut pr.ac_caps_lock),
        ("fractions", &mut pr.ac_fractions),
        ("ordinals", &mut pr.ac_ordinals),
        ("lists", &mut pr.ac_lists),
    ] {
        if let Some(v) = p.get(key).and_then(Value::as_bool) {
            *slot = v;
        }
    }
    if let Some(Value::Array(a)) = p.get("add") {
        let (Some(from), Some(to)) = (a.first().and_then(Value::as_str), a.get(1).and_then(Value::as_str)) else {
            return Err(bad("text.autocorrect", "`add` is [from, to]"));
        };
        let from = from.trim();
        if from.is_empty() || from.chars().any(char::is_whitespace) || from.chars().count() > 64 || to.chars().count() > 255 {
            return Err(bad("text.autocorrect", "`from` is one word (up to 64 characters), `to` up to 255"));
        }
        pr.ac_entries.retain(|(k, _)| k != from);
        pr.ac_removed.retain(|k| !k.eq_ignore_ascii_case(from));
        if pr.ac_entries.len() >= 2000 {
            return Err(bad("text.autocorrect", "the list is full"));
        }
        pr.ac_entries.push((from.to_string(), to.to_string()));
    }
    if let Some(from) = str_param(p, "remove") {
        let had = pr.ac_entries.len();
        pr.ac_entries.retain(|(k, _)| k != from);
        if had == pr.ac_entries.len()
            && AUTOCORRECT.iter().any(|(k, _)| k.eq_ignore_ascii_case(from))
            && !pr.ac_removed.iter().any(|r| r.eq_ignore_ascii_case(from))
        {
            pr.ac_removed.push(from.to_string());
        }
    }
    let list: Vec<(String, String)> = pr
        .ac_entries
        .iter()
        .cloned()
        .chain(
            AUTOCORRECT
                .iter()
                .filter(|(k, _)| {
                    !pr.ac_removed.iter().any(|r| r.eq_ignore_ascii_case(k)) && !pr.ac_entries.iter().any(|(e, _)| e.eq_ignore_ascii_case(k))
                })
                .map(|(k, v)| (k.to_string(), v.to_string())),
        )
        .collect();
    Ok(json!({
        "on": pr.autocorrect, "twoInitials": pr.ac_two_initials, "sentenceCaps": pr.ac_sentence_caps,
        "days": pr.ac_days, "capsLock": pr.ac_caps_lock, "fractions": pr.ac_fractions,
        "ordinals": pr.ac_ordinals, "lists": pr.ac_lists, "entries": list,
    }))
}
