//! Rich text on the system clipboard, as HTML.
//!
//! Pages and Keynote put copied text on the pasteboard as HTML (and plain text). DeckCraft writes
//! the same pair when it copies, and reads it back on paste: bold, italic and underline survive.
//! Anything else in the markup is skipped. A hostile fragment is capped, never executed.

use deckcraft_model::text::{Paragraph, Run, RunProps, TextBody};

const MAX_HTML: usize = 1 << 20;
const MAX_PARAS: usize = 2_000;
const MAX_RUNS: usize = 10_000;

/// HTML for `body`: one `<p>` per paragraph, `<b>`, `<i>` and `<u>` for the formatting.
pub fn body_to_html(body: &TextBody) -> String {
    let mut out = String::new();
    for (i, p) in body.paragraphs.iter().enumerate() {
        if i > 0 {
            out.push_str("<br>");
        }
        out.push_str("<p>");
        for r in &p.runs {
            let (open, close) = tags(&r.props);
            out.push_str(&open);
            match &r.kind {
                deckcraft_model::text::RunKind::Break => out.push_str("<br>"),
                _ => push_escaped(&mut out, &r.text),
            }
            out.push_str(&close);
            if out.len() > MAX_HTML {
                break;
            }
        }
        out.push_str("</p>");
    }
    out
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

fn push_escaped(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
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
    let mut stack: Vec<(&'static str, i32, i32, i32)> = Vec::new();
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
                let kind = tag_kind(&name);
                if !kind.is_empty()
                    && let Some(pos) = stack.iter().rposition(|e| e.0 == kind)
                {
                    let (_, db, di, du) = stack[pos];
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
            if is_break(&name) {
                new_para(&mut paragraphs);
                continue;
            }
            let (db, di, du) = tag_style(&name, name_src);
            if db + di + du > 0 {
                bold += db;
                italic += di;
                underline += du;
                if !token.ends_with('/') {
                    stack.push((tag_kind(&name), db, di, du));
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
        let text = decode_text(&html[i..end]);
        i = end;
        if text.is_empty() {
            continue;
        }
        push_run(&mut paragraphs, &mut runs, text, bold > 0, italic > 0, underline > 0);
    }
    while paragraphs.last().is_some_and(Paragraph::is_empty) && paragraphs.len() > 1 {
        paragraphs.pop();
    }
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
    let at = lower.find(key)?;
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
        paragraphs.push(Paragraph::default());
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

fn decode_text(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at].replace(['\n', '\r'], ""));
        rest = &rest[at..];
        let Some(end) = rest.find(';') else {
            out.push_str(&rest.replace(['\n', '\r'], ""));
            break;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
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
        out.push_str(&rest.replace(['\n', '\r'], ""));
    }
    out
}

fn numeric_char(digits: &str) -> Option<char> {
    let hex = digits.strip_prefix('x').or_else(|| digits.strip_prefix('X'));
    let n = if let Some(hex) = hex { u32::from_str_radix(hex, 16).ok()? } else { digits.parse().ok()? };
    char::from_u32(n).filter(|c| !c.is_control() || *c == '\n')
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
    fn bare_ampersand_keeps_text_up_to_next_semicolon() {
        let text = |html: &str| html_to_body(html).unwrap().text();
        assert_eq!(text("<p>R&D costs; up</p>"), "R&D costs; up");
        assert_eq!(text("<p>AT&T; Q&A</p>"), "AT&T; Q&A");
        assert_eq!(text("<p>A &amp; B &#38; C &#x26; D</p>"), "A & B & C & D");
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
    fn a_huge_fragment_is_refused() {
        let html = "x".repeat(MAX_HTML + 1);
        assert!(html_to_body(&html).is_none());
    }
}
