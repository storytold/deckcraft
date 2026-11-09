//! Literal LF restarts paragraph formatting without changing the authored paragraph.
//! Expand only for layout, then map every editing/painting index back to the source.
use super::TextLayout;
use deckcraft_model::text::{Paragraph, Run, RunKind, TextBody};

// Match the importer's paragraph budget. Over-budget text keeps the ordinary
// line-break path and its source content instead of multiplying paragraph state.
const MAX_LAYOUT_PARAGRAPHS: usize = 100_000;

#[derive(Clone, Copy)]
struct Source {
    para: usize,
    offset: usize,
}

pub(super) struct Expanded {
    pub body: TextBody,
    pub literal_ends: Vec<bool>,
    source: Vec<Source>,
    original_count: usize,
}

fn empty_like(p: &Paragraph) -> Paragraph {
    Paragraph { level: p.level, props: p.props.clone(), def_props: p.def_props.clone(), end_props: p.end_props.clone(), runs: Vec::new() }
}

impl Expanded {
    pub fn new(body: &TextBody) -> Option<Self> {
        if !body.paragraphs.iter().flat_map(|p| &p.runs).any(|r| r.kind == RunKind::Text && r.text.contains('\n')) {
            return None;
        }
        let mut count = body.paragraphs.len();
        for run in body.paragraphs.iter().flat_map(|p| &p.runs) {
            if run.kind == RunKind::Text {
                count = count.saturating_add(run.text.bytes().filter(|&b| b == b'\n').take(MAX_LAYOUT_PARAGRAPHS + 1).count());
                if count > MAX_LAYOUT_PARAGRAPHS {
                    return None;
                }
            }
        }
        let mut expanded = Self {
            body: TextBody { body: body.body.clone(), list_style: body.list_style.clone(), paragraphs: Vec::new() },
            literal_ends: Vec::new(),
            source: Vec::new(),
            original_count: body.paragraphs.len(),
        };
        for (pi, p) in body.paragraphs.iter().enumerate() {
            let mut current = empty_like(p);
            let (mut start, mut offset) = (0_usize, 0_usize);
            for run in &p.runs {
                if run.kind != RunKind::Text || !run.text.contains('\n') {
                    current.runs.push(run.clone());
                    offset = offset.saturating_add(run.char_len());
                    continue;
                }
                let mut pieces = run.text.split('\n').peekable();
                while let Some(piece) = pieces.next() {
                    if !piece.is_empty() {
                        current.runs.push(Run::with(piece, run.props.clone()));
                    }
                    offset = offset.saturating_add(piece.chars().count());
                    if pieces.peek().is_some() {
                        // The literal separator owns the preceding paragraph mark's
                        // formatting; the original end mark still owns the last one.
                        current.end_props = run.props.clone();
                        expanded.body.paragraphs.push(current);
                        expanded.literal_ends.push(true);
                        expanded.source.push(Source { para: pi, offset: start });
                        offset = offset.saturating_add(1);
                        start = offset;
                        current = empty_like(p);
                    }
                }
            }
            expanded.body.paragraphs.push(current);
            expanded.literal_ends.push(false);
            expanded.source.push(Source { para: pi, offset: start });
        }
        Some(expanded)
    }

    pub fn restore(&self, layout: &mut TextLayout) {
        for line in &mut layout.lines {
            if let Some(s) = self.source.get(line.para) {
                line.para = s.para;
                line.start = line.start.saturating_add(s.offset);
                line.end = line.end.saturating_add(s.offset);
            }
        }
        for run in layout.runs.iter_mut().chain(&mut layout.empty_bullets) {
            if let Some(s) = self.source.get(run.para) {
                run.para = s.para;
                run.chars.0 = run.chars.0.saturating_add(s.offset);
                run.chars.1 = run.chars.1.saturating_add(s.offset);
                for (ch, _) in &mut run.cells {
                    *ch = ch.saturating_add(s.offset);
                }
            }
        }
        for deco in &mut layout.decos {
            if let Some(s) = self.source.get(deco.para) {
                deco.para = s.para;
            }
        }
        let old = std::mem::take(&mut layout.para_lines);
        layout.para_lines = vec![(0, 0); self.original_count];
        for (s, (first, count)) in self.source.iter().zip(old) {
            if let Some(entry) = layout.para_lines.get_mut(s.para) {
                if entry.1 == 0 {
                    entry.0 = first;
                }
                entry.1 = entry.1.saturating_add(count);
            }
        }
        let old = std::mem::take(&mut layout.boundaries);
        layout.boundaries = vec![Vec::new(); self.original_count];
        for (s, boundaries) in self.source.iter().zip(old) {
            if let Some(target) = layout.boundaries.get_mut(s.para) {
                // Each segment's final boundary belongs to its separator. The
                // next segment starts one character later, so no boundary is lost.
                target.extend(boundaries);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separator_expansion_is_bounded_without_altering_source_text() {
        let text = "\n".repeat(MAX_LAYOUT_PARAGRAPHS);
        let body = TextBody { paragraphs: vec![Paragraph::new(&text)], ..Default::default() };
        assert!(Expanded::new(&body).is_none());
        assert_eq!(body.paragraphs.len(), 1);
        assert_eq!(body.paragraphs[0].text(), text);
    }
}
