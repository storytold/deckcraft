//! Rich text on the system clipboard, as HTML.
//!
//! Pages and Keynote put copied text on the pasteboard as HTML (and plain text). DeckCraft writes
//! the same pair when it copies, and reads it back on paste: bold, italic and underline survive.
//! Anything else in the markup is skipped. A hostile fragment is capped, never executed.

use std::borrow::Cow;

use deckcraft_model::text::{Paragraph, Run, RunProps, TextBody};

const MAX_HTML: usize = 1 << 20;
const MAX_PARAS: usize = 2_000;
const MAX_RUNS: usize = 10_000;

/// HTML for `body`: one `<p>` per paragraph, `<b>`, `<i>` and `<u>` for the formatting.
pub fn body_to_html(body: &TextBody) -> String {
    let mut out = String::new();
    for (i, p) in body.paragraphs.iter().enumerate() {
        let prefix = if i > 0 { "<br><p>" } else { "<p>" };
        if !push_if_fits(&mut out, prefix, 4) {
            return plain_html(body);
        }
        for r in &p.runs {
            let (open, close) = tags(&r.props);
            if !push_if_fits(&mut out, &open, close.len() + 4) {
                return plain_html(body);
            }
            match &r.kind {
                deckcraft_model::text::RunKind::Break => {
                    if !push_if_fits(&mut out, "<br>", close.len() + 4) {
                        return plain_html(body);
                    }
                }
                _ => {
                    if !push_escaped_bounded(&mut out, &r.text, close.len() + 4) {
                        return plain_html(body);
                    }
                }
            }
            out.push_str(&close);
        }
        out.push_str("</p>");
    }
    out
}

/// The text without formatting, or "" (no HTML on the clipboard) when even that is too big: a
/// cut-off prefix would paste as if it were the whole text.
fn plain_html(body: &TextBody) -> String {
    let mut out = String::new();
    for (i, p) in body.paragraphs.iter().enumerate() {
        let prefix = if i > 0 { "<br><p>" } else { "<p>" };
        if !push_if_fits(&mut out, prefix, 4) {
            return String::new();
        }
        for r in &p.runs {
            let fits = if matches!(&r.kind, deckcraft_model::text::RunKind::Break) {
                push_if_fits(&mut out, "<br>", 4)
            } else {
                push_escaped_bounded(&mut out, &r.text, 4)
            };
            if !fits {
                return String::new();
            }
        }
        out.push_str("</p>");
    }
    out
}

fn push_if_fits(out: &mut String, value: &str, reserve: usize) -> bool {
    if out.len().checked_add(value.len()).and_then(|n| n.checked_add(reserve)).is_none_or(|n| n > MAX_HTML) {
        return false;
    }
    out.push_str(value);
    true
}

fn push_escaped_bounded(out: &mut String, text: &str, reserve: usize) -> bool {
    for c in text.chars() {
        match c {
            '&' => {
                if !push_if_fits(out, "&amp;", reserve) {
                    return false;
                }
            }
            '<' => {
                if !push_if_fits(out, "&lt;", reserve) {
                    return false;
                }
            }
            '>' => {
                if !push_if_fits(out, "&gt;", reserve) {
                    return false;
                }
            }
            _ => {
                if out.len().checked_add(c.len_utf8()).and_then(|n| n.checked_add(reserve)).is_none_or(|n| n > MAX_HTML) {
                    return false;
                }
                out.push(c);
            }
        }
    }
    true
}

fn tags(props: &RunProps) -> (String, String) {
    let mut open = String::new();
    let mut close = String::new();
    if props.bold == Some(true) {
        open.push_str("<b>");
        close.insert_str(0, "</b>");
    }
    if props.italic == Some(true) {
        open.push_str("<i>");
        close.insert_str(0, "</i>");
    }
    if props.underline.as_deref().is_some_and(|u| !u.eq_ignore_ascii_case("none")) {
        open.push_str("<u>");
        close.insert_str(0, "</u>");
    }
    (open, close)
}

/// The formatted text of an HTML fragment, or `None` when it has no text.
///
/// Understands `<b>`, `<strong>`, `<i>`, `<em>`, `<u>`, `<br>`, paragraphs, and a `style` that
/// says bold, italic or underline. Other tags are ignored. The result is capped.
pub fn html_to_body(html: &str) -> Option<TextBody> {
    if html.len() > MAX_HTML {
        return None;
    }
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut bold = 0i32;
    let mut italic = 0i32;
    let mut underline = 0i32;
    let mut stack: Vec<(Cow<'static, str>, i32, i32, i32)> = Vec::new();
    let mut paragraphs: Vec<Paragraph> = vec![Paragraph::default()];
    let mut runs = 0usize;
    let mut skip = 0i32;
    while i < bytes.len() && paragraphs.len() <= MAX_PARAS && runs <= MAX_RUNS {
        if bytes[i] == b'<' {
            let end = bytes[i + 1..].iter().position(|c| *c == b'>').map(|n| i + 1 + n).unwrap_or(bytes.len());
            let raw = &html[i + 1..end];
            i = (end + 1).min(bytes.len());
            let token = raw.trim();
            if token.starts_with('!') || token.starts_with('?') {
                continue;
            }
            let closing = token.starts_with('/');
            let name_src = token.trim_start_matches('/').trim();
            let name_end = name_src.find(|c: char| c.is_whitespace() || c == '/' || c == '>').unwrap_or(name_src.len());
            let name = name_src[..name_end].to_ascii_lowercase();
            if name == "script" || name == "style" || name == "head" {
                if closing {
                    skip = (skip - 1).max(0);
                } else if !token.ends_with('/') {
                    skip += 1;
                }
                continue;
            }
            if skip > 0 {
                continue;
            }
            if closing {
                let kind = match tag_kind(&name) {
                    "" => name.as_str(),
                    kind => kind,
                };
                let search_start = stack.len().saturating_sub(64);
                if let Some(offset) = stack[search_start..].iter().rposition(|e| e.0.as_ref() == kind) {
                    let pos = search_start + offset;
                    let (db, di, du) = (stack[pos].1, stack[pos].2, stack[pos].3);
                    bold -= db;
                    italic -= di;
                    underline -= du;
                    stack.truncate(pos);
                }
                if is_block(&name) {
                    new_para(&mut paragraphs);
                }
                continue;
            }
            if is_block(&name) {
                new_para(&mut paragraphs);
            }
            if is_break(&name) {
                new_para(&mut paragraphs);
                continue;
            }
            let (db, di, du) = tag_style(&name, name_src);
            if db + di + du > 0 {
                bold += db;
                italic += di;
                underline += du;
                if !token.ends_with('/') && stack.len() < 128 {
                    let kind = match tag_kind(&name) {
                        "" => Cow::Owned(name.clone()),
                        kind => Cow::Borrowed(kind),
                    };
                    stack.push((kind, db, di, du));
                } else {
                    bold -= db;
                    italic -= di;
                    underline -= du;
                }
            }
            continue;
        }
        if skip > 0 {
            i += 1;
            continue;
        }
        let end = bytes[i..].iter().position(|c| *c == b'<').map(|n| i + n).unwrap_or(bytes.len());
        let mut text = decode_text(&html[i..end]);
        i = end;
        // A space at the start of a paragraph, or after a space, does not render in HTML.
        let after_space = paragraphs.last().and_then(|p| p.runs.last()).is_none_or(|r| r.text.ends_with(' '));
        if after_space {
            text = text.trim_start_matches(' ').to_string();
        }
        if text.is_empty() {
            continue;
        }
        push_run(&mut paragraphs, &mut runs, text, bold > 0, italic > 0, underline > 0);
    }
    while paragraphs.last().is_some_and(Paragraph::is_empty) && paragraphs.len() > 1 {
        paragraphs.pop();
    }
    trim_end_space(&mut paragraphs);
    if paragraphs.iter().all(Paragraph::is_empty) {
        return None;
    }
    for p in &mut paragraphs {
        p.normalize();
    }
    Some(TextBody { paragraphs, ..Default::default() })
}

fn tag_kind(name: &str) -> &'static str {
    match name {
        "b" | "strong" => "b",
        "i" | "em" => "i",
        "u" => "u",
        "span" | "font" => "span",
        "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "block",
        _ => "",
    }
}

fn is_break(name: &str) -> bool {
    name == "br" || name == "hr"
}

fn is_block(name: &str) -> bool {
    matches!(name, "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote")
}

fn tag_style(name: &str, src: &str) -> (i32, i32, i32) {
    let mut bold = i32::from(name == "b" || name == "strong");
    let mut italic = i32::from(name == "i" || name == "em");
    let mut underline = i32::from(name == "u");
    if let Some(style) = attr(src, "style") {
        let s = style.to_ascii_lowercase();
        if let Some(on) = weight_bold(&s) {
            bold = i32::from(on);
        }
        if s.contains("font-style:italic") || s.contains("font-style: italic") {
            italic = 1;
        }
        if s.contains("text-decoration:underline")
            || s.contains("text-decoration: underline")
            || s.contains("text-decoration-line:underline")
            || s.contains("text-decoration-line: underline")
        {
            underline = 1;
        }
    }
    (bold, italic, underline)
}

/// Whether a `font-weight` in `style` turns bold on (`Some(true)`) or off (`Some(false)`).
fn weight_bold(style: &str) -> Option<bool> {
    let at = style.find("font-weight")?;
    let rest = style[at + "font-weight".len()..].trim_start_matches(|c: char| c == ':' || c.is_whitespace());
    if rest.starts_with("bold") {
        return Some(true);
    }
    if rest.starts_with("normal") || rest.starts_with("lighter") {
        return Some(false);
    }
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let weight: u32 = digits.parse().ok()?;
    Some(weight >= 600)
}

fn attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let at = lower.match_indices(key).find_map(|(at, _)| {
        let named = lower[..at].ends_with(|c: char| c.is_ascii_whitespace());
        let eq = tag.get(at + key.len()..)?.trim_start().starts_with('=');
        (named && eq).then_some(at)
    })?;
    let after = tag.get(at + key.len()..)?.trim_start();
    let after = after.strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = after.get(quote.len_utf8()..)?;
    let end = rest.find(quote)?;
    rest.get(..end)
}

fn new_para(paragraphs: &mut Vec<Paragraph>) {
    if paragraphs.len() >= MAX_PARAS {
        return;
    }
    if paragraphs.last().is_some_and(|p| !p.is_empty()) {
        trim_end_space(paragraphs);
        paragraphs.push(Paragraph::default());
    }
}

/// Whitespace at the end of a paragraph does not render in HTML.
fn trim_end_space(paragraphs: &mut [Paragraph]) {
    if let Some(run) = paragraphs.last_mut().and_then(|p| p.runs.last_mut()) {
        let len = run.text.trim_end_matches(' ').len();
        run.text.truncate(len);
    }
}

fn push_run(paragraphs: &mut [Paragraph], runs: &mut usize, text: String, bold: bool, italic: bool, underline: bool) {
    if *runs >= MAX_RUNS {
        return;
    }
    let Some(p) = paragraphs.last_mut() else { return };
    let mut props = RunProps::default();
    if bold {
        props.bold = Some(true);
    }
    if italic {
        props.italic = Some(true);
    }
    if underline {
        props.underline = Some("sng".into());
    }
    p.runs.push(Run::with(text, props));
    *runs += 1;
}

/// Text with each run of HTML whitespace (spaces, tabs, line breaks) collapsed to one space.
fn collapse_space(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r') {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

fn decode_text(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&collapse_space(&rest[..at]));
        rest = &rest[at..];
        let mut search_window = rest.len().min(32);
        while !rest.is_char_boundary(search_window) {
            search_window -= 1;
        }
        let Some(end) = rest[..search_window].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity.strip_prefix('#').and_then(numeric_char),
        };
        if let Some(c) = decoded {
            out.push(c);
            rest = &rest[end + 1..];
        } else {
            // Not a well-formed entity (e.g. a bare "R&D"): keep the '&' as typed.
            out.push('&');
            rest = &rest[1..];
        }
    }
    if !rest.is_empty() && !rest.starts_with('&') {
        out.push_str(&collapse_space(rest));
    }
    out
}

fn numeric_char(digits: &str) -> Option<char> {
    let hex = digits.strip_prefix('x').or_else(|| digits.strip_prefix('X'));
    // `parse` and `from_str_radix` accept a leading '+', which is not a valid reference.
    let (body, radix) = hex.map_or((digits, 10), |h| (h, 16));
    if body.is_empty() || !body.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let n = u32::from_str_radix(body, radix).ok()?;
    char::from_u32(n).filter(|c| !c.is_control() || matches!(*c, '\n' | '\t'))
}

/// Do the HTML's words and the paste's plain text say the same thing?
pub fn same_words(html_plain: &str, pasted: &str) -> bool {
    let a = fold(html_plain);
    let b = fold(pasted);
    a == b
}

fn fold(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n").replace('\u{00a0}', " ").trim_matches(['\n', ' ', '\t']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_on_other_tags_ends_at_their_close() {
        type Prop = fn(&RunProps) -> bool;
        let cases: [(&str, &str, Prop); 3] = [
            ("a", "font-weight:bold", |p| p.bold == Some(true)),
            ("td", "font-style:italic", |p| p.italic == Some(true)),
            ("mark", "text-decoration:underline", |p| p.underline.is_some()),
        ];
        for (tag, style, prop) in cases {
            let body = html_to_body(&format!("<p><{tag} style=\"{style}\">link</{tag}> plain</p>")).unwrap();
            let runs = &body.paragraphs[0].runs;
            assert_eq!(runs.len(), 2, "{tag}");
            assert!(prop(&runs[0].props), "{tag}");
            assert_eq!(runs[0].text, "link");
            assert_eq!(runs[1].text, " plain");
            assert!(runs[1].props.bold.is_none() && runs[1].props.italic.is_none() && runs[1].props.underline.is_none(), "{tag}");
        }
    }

    #[test]
    fn text_before_opening_block_tag_is_separate_paragraph() {
        for tag in ["div", "p", "li", "h1"] {
            let body = html_to_body(&format!("Hello<{tag}>world</{tag}>")).unwrap();
            let texts: Vec<String> = body.paragraphs.iter().map(|p| p.runs.iter().map(|r| r.text.as_str()).collect()).collect();
            assert_eq!(texts, ["Hello", "world"], "{tag}");
        }
    }

    #[test]
    fn multibyte_after_ampersand_is_kept() {
        let text = "é".repeat(20);
        assert_eq!(decode_text(&format!("R&D {text}")), format!("R&D {text}"));
        let cjk = "漢".repeat(20);
        assert_eq!(decode_text(&format!("&{cjk}")), format!("&{cjk}"));
    }

    #[test]
    fn signed_numeric_entities_are_kept_as_typed() {
        assert_eq!(decode_text("a&#+65;b"), "a&#+65;b");
        assert_eq!(decode_text("a&#x+41;b"), "a&#x+41;b");
        assert_eq!(decode_text("a&#65;b&#x41;"), "aAbA");
    }

    #[test]
    fn numeric_tab_entity_decodes_to_tab() {
        assert_eq!(decode_text("a&#9;b"), "a\tb");
        assert_eq!(decode_text("a&#x9;b"), "a\tb");
    }

    #[test]
    fn google_docs_normal_weight_wrapper_is_not_bold() {
        let html = "<meta charset=\"utf-8\"><b style=\"font-weight:normal;\" id=\"docs-internal-guid-1\"><span style=\"font-weight:400\">plain words</span></b>";
        let body = html_to_body(html).unwrap();
        assert_eq!(body.text(), "plain words");
        assert_eq!(body.paragraphs[0].runs[0].props.bold, None);
        let light = html_to_body("<b style=\"font-weight:300\">thin</b>").unwrap();
        assert_eq!(light.paragraphs[0].runs[0].props.bold, None);
        let plain = html_to_body("<b>strong</b>").unwrap();
        assert_eq!(plain.paragraphs[0].runs[0].props.bold, Some(true));
        let heavy = html_to_body("<span style=\"font-weight:700\">heavy</span>").unwrap();
        assert_eq!(heavy.paragraphs[0].runs[0].props.bold, Some(true));
    }

    #[test]
    fn style_attribute_is_found_after_attributes_containing_style() {
        for tag in [
            "<span class=\"style1\" style=\"font-weight:bold\">x</span>",
            "<span data-style=\"x\" style=\"font-weight:bold\">x</span>",
            "<span class=\"mystyle\" style=\"font-weight:bold\">x</span>",
        ] {
            let body = html_to_body(tag).unwrap();
            assert_eq!(body.paragraphs[0].runs[0].props.bold, Some(true), "{tag}");
        }
        let none = html_to_body("<span class=\"style\">x</span>").unwrap();
        assert_eq!(none.paragraphs[0].runs[0].props.bold, None);
    }

    #[test]
    fn bold_italic_and_underline_round_trip() {
        let mut body = TextBody::from_text("Hi there");
        body.paragraphs[0].runs = vec![
            Run::with("Hi", RunProps { bold: Some(true), ..Default::default() }),
            Run::new(" "),
            Run::with("there", RunProps { italic: Some(true), underline: Some("sng".into()), ..Default::default() }),
        ];
        let html = body_to_html(&body);
        assert!(html.contains("<b>Hi</b>"), "{html}");
        assert!(html.contains("<i><u>there</u></i>") || html.contains("<u><i>there</i></u>"), "{html}");
        let back = html_to_body(&html).unwrap();
        assert_eq!(back.text(), "Hi there");
        assert_eq!(back.paragraphs[0].runs[0].props.bold, Some(true));
        let last = back.paragraphs[0].runs.last().unwrap();
        assert_eq!(last.props.italic, Some(true));
        assert!(last.props.underline.is_some());
    }

    #[test]
    fn oversized_escaped_rich_text_puts_no_html_on_the_clipboard() {
        let mut body = TextBody::from_text(&"&<🦀".repeat(100_000));
        body.paragraphs[0].runs[0].props.bold = Some(true);
        body.paragraphs.push(Paragraph::new("later paragraph"));

        // Neither the formatted nor the plain HTML fits, and a truncated prefix would lose text.
        assert!(body_to_html(&body).is_empty());
    }

    #[test]
    fn bare_ampersand_keeps_text_up_to_next_semicolon() {
        let text = |html: &str| html_to_body(html).unwrap().text();
        assert_eq!(text("<p>R&D costs; up</p>"), "R&D costs; up");
        assert_eq!(text("<p>AT&T; Q&A</p>"), "AT&T; Q&A");
        assert_eq!(text("<p>A &amp; B &#38; C &#x26; D</p>"), "A & B & C & D");

        // Verify many bare ampersands do not cause quadratic scan freeze
        let hostile = format!("<p>{}</p>", "&".repeat(2000));
        assert_eq!(text(&hostile), "&".repeat(2000));
    }

    #[test]
    fn pages_style_and_escapes() {
        let html = "<html><head><style>p{}</style></head><body><p><span style=\"font-weight: bold\">A &amp; B</span></p><p><span style=\"font-style: italic\">C</span></p></body></html>";
        let body = html_to_body(html).unwrap();
        assert_eq!(body.text(), "A & B\nC");
        assert_eq!(body.paragraphs[0].runs[0].props.bold, Some(true));
        assert_eq!(body.paragraphs[1].runs[0].props.italic, Some(true));
        assert!(same_words(&body.text(), "A & B\nC"));
        assert!(!same_words(&body.text(), "other"));
    }

    #[test]
    fn a_newline_in_html_text_is_a_space() {
        assert_eq!(decode_text("Hello\nworld"), "Hello world");
        let body = html_to_body("<p>Hello\r\n  world</p>").unwrap();
        assert_eq!(body.text(), "Hello world");
        let body = html_to_body("<p>\n<b>Hello</b>\n<i>world</i>\n</p>\n<p>next</p>").unwrap();
        assert_eq!(body.text(), "Hello world\nnext");
        assert!(same_words(&body.text(), "Hello world\nnext"));
        assert_eq!(body.paragraphs[0].runs[0].props.bold, Some(true));
        assert_eq!(body.paragraphs[0].runs.last().unwrap().props.italic, Some(true));
    }

    #[test]
    fn a_huge_fragment_is_refused() {
        let html = "x".repeat(MAX_HTML + 1);
        assert!(html_to_body(&html).is_none());
    }
}
