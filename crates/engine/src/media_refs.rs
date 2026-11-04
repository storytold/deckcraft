//! Every [`MediaId`] the model can reference, found by type rather than by scanning JSON keys.
//!
//! One walker, written once ([`walkers!`]) and expanded twice: [`visit`] reads ids, [`visit_mut`]
//! rewrites them in place. It covers pictures (and their artistic-effect originals), picture fills
//! wherever a fill can sit (shapes, lines, text runs, table cells and styles, chart series and
//! points, backgrounds, theme format schemes), media clips and their posters, 3-D models, OLE and
//! SmartArt previews and kept parts, SmartArt node pictures, picture bullets, hyperlink, transition and
//! animation sounds, and embedded font faces. Adding a `MediaId` field to the model means adding
//! it here; the shape kinds are matched exhaustively so a new kind can't be missed.

use std::collections::HashSet;

use deckcraft_model::{MediaId, Presentation, Shape, Slide};

macro_rules! walkers {
    ($($m:tt)?) => {
        use deckcraft_model::chart::Chart;
        use deckcraft_model::style::{Fill, Line, PictureFill};
        use deckcraft_model::table::{Table, TableBorder, TablePartStyle, TableStyleDef};
        use deckcraft_model::text::{Hyperlink, ListStyle, ParaProps, RunProps, TextBody, Bullet};
        use deckcraft_model::theme::Theme;
        use deckcraft_model::{Background, EmbeddedFont, Layout, Master, MediaId, Shape, ShapeKind, Slide};

        /// Deepest group nesting walked (the model's other walkers stop at 64 too).
        const MAX_DEPTH: usize = 64;

        pub type F<'a> = dyn FnMut(&$($m)? MediaId) + 'a;

        pub fn picture(p: &$($m)? PictureFill, f: &mut F) {
            f(&$($m)? p.media);
            if let Some(a) = &$($m)? p.artistic {
                f(&$($m)? a.original);
            }
        }

        pub fn fill(x: &$($m)? Fill, f: &mut F) {
            if let Fill::Picture(p) = x {
                picture(p, f);
            }
        }

        fn opt_fill(x: &$($m)? Option<Fill>, f: &mut F) {
            if let Some(x) = x {
                fill(x, f);
            }
        }

        pub fn line(l: &$($m)? Line, f: &mut F) {
            opt_fill(&$($m)? l.fill, f);
        }

        fn opt_line(l: &$($m)? Option<Line>, f: &mut F) {
            if let Some(l) = l {
                line(l, f);
            }
        }

        fn link(h: &$($m)? Option<Hyperlink>, f: &mut F) {
            if let Some(Hyperlink { sound: Some(id), .. }) = h {
                f(id);
            }
        }

        fn run(r: &$($m)? RunProps, f: &mut F) {
            opt_fill(&$($m)? r.fill, f);
            opt_line(&$($m)? r.outline, f);
            link(&$($m)? r.link, f);
            link(&$($m)? r.hover, f);
        }

        fn para(p: &$($m)? ParaProps, f: &mut F) {
            if let Some(Bullet::Picture { media }) = &$($m)? p.bullet {
                f(media);
            }
        }

        pub fn list(l: &$($m)? ListStyle, f: &mut F) {
            for lv in (&$($m)? l.levels).into_iter().flatten() {
                para(&$($m)? lv.para, f);
                run(&$($m)? lv.run, f);
            }
        }

        pub fn text(t: &$($m)? TextBody, f: &mut F) {
            list(&$($m)? t.list_style, f);
            for p in &$($m)? t.paragraphs {
                para(&$($m)? p.props, f);
                run(&$($m)? p.def_props, f);
                run(&$($m)? p.end_props, f);
                for r in &$($m)? p.runs {
                    run(&$($m)? r.props, f);
                }
            }
        }

        fn table(t: &$($m)? Table, f: &mut F) {
            opt_fill(&$($m)? t.background, f);
            for row in &$($m)? t.rows {
                for c in &$($m)? row.cells {
                    text(&$($m)? c.text, f);
                    opt_fill(&$($m)? c.fill, f);
                    for b in &$($m)? c.borders {
                        opt_line(b, f);
                    }
                    opt_line(&$($m)? c.diag_down, f);
                    opt_line(&$($m)? c.diag_up, f);
                }
            }
        }

        fn table_part(s: &$($m)? TablePartStyle, f: &mut F) {
            opt_fill(&$($m)? s.fill, f);
            for b in &$($m)? s.borders {
                if let Some(TableBorder::Line { line: l }) = b {
                    line(l, f);
                }
            }
        }

        pub fn table_style(t: &$($m)? TableStyleDef, f: &mut F) {
            for (_, s) in &$($m)? t.parts {
                table_part(s, f);
            }
            if let Some(s) = &$($m)? t.background {
                table_part(s, f);
            }
        }

        fn chart(c: &$($m)? Chart, f: &mut F) {
            for s in &$($m)? c.series {
                opt_fill(&$($m)? s.fill, f);
                opt_line(&$($m)? s.line, f);
                for (_, x) in &$($m)? s.point_fills {
                    fill(x, f);
                }
            }
            opt_fill(&$($m)? c.plot_fill, f);
            opt_fill(&$($m)? c.chart_fill, f);
        }

        fn shape_at(s: &$($m)? Shape, f: &mut F, depth: usize) {
            if depth > MAX_DEPTH {
                return;
            }
            match &$($m)? s.kind {
                ShapeKind::Shape | ShapeKind::Connector { .. } | ShapeKind::Ink { .. } => {}
                ShapeKind::Picture { fill } => picture(fill, f),
                ShapeKind::Group { children, .. } => {
                    for c in children {
                        shape_at(c, f, depth + 1);
                    }
                }
                ShapeKind::Table(t) => table(t, f),
                ShapeKind::Chart(c) => chart(c, f),
                ShapeKind::Media(m) => {
                    f(&$($m)? m.media);
                    if let Some(p) = &$($m)? m.poster {
                        f(p);
                    }
                }
                ShapeKind::Model3d { media, poster } => {
                    f(media);
                    if let Some(p) = poster {
                        f(p);
                    }
                }
                // Every field named, so a new one can't slip past.
                ShapeKind::Opaque { xml: _, preview, label: _, parts } => {
                    if let Some(p) = preview {
                        f(p);
                    }
                    // SmartArt data, OLE embeddings… kept as media items (#209).
                    for (_, _, id) in parts {
                        f(id);
                    }
                }
            }
            opt_fill(&$($m)? s.fill, f);
            opt_line(&$($m)? s.line, f);
            if let Some(t) = &$($m)? s.text {
                text(t, f);
            }
            link(&$($m)? s.click, f);
            link(&$($m)? s.hover, f);
            if let Some(d) = &$($m)? s.diagram {
                for n in &$($m)? d.nodes {
                    if let Some(p) = &$($m)? n.picture {
                        f(p);
                    }
                }
            }
        }

        /// A shape and, for a group, everything in it.
        pub fn shape(s: &$($m)? Shape, f: &mut F) {
            shape_at(s, f, 0);
        }

        pub fn shapes(v: &$($m)? [Shape], f: &mut F) {
            for s in v {
                shape_at(s, f, 0);
            }
        }

        fn background(b: &$($m)? Option<Background>, f: &mut F) {
            if let Some(Background::Fill { fill: x }) = b {
                fill(x, f);
            }
        }

        /// A slide: shapes, background, transition and animation sounds, notes.
        pub fn slide(s: &$($m)? Slide, f: &mut F) {
            shapes(&$($m)? s.shapes, f);
            background(&$($m)? s.background, f);
            if let Some(t) = &$($m)? s.transition
                && let Some(id) = &$($m)? t.sound
            {
                f(id);
            }
            for a in &$($m)? s.animations {
                if let Some(id) = &$($m)? a.sound {
                    f(id);
                }
            }
            text(&$($m)? s.notes, f);
        }

        pub fn layout(l: &$($m)? Layout, f: &mut F) {
            shapes(&$($m)? l.shapes, f);
            background(&$($m)? l.background, f);
        }

        pub fn theme(t: &$($m)? Theme, f: &mut F) {
            for x in &$($m)? t.format.fills {
                fill(x, f);
            }
            for x in &$($m)? t.format.bg_fills {
                fill(x, f);
            }
            for l in &$($m)? t.format.lines {
                line(l, f);
            }
        }

        /// A master with its theme, text styles and layouts.
        pub fn master(m: &$($m)? Master, f: &mut F) {
            theme(&$($m)? m.theme, f);
            shapes(&$($m)? m.shapes, f);
            background(&$($m)? m.background, f);
            list(&$($m)? m.title_style, f);
            list(&$($m)? m.body_style, f);
            list(&$($m)? m.other_style, f);
            for l in &$($m)? m.layouts {
                layout(l, f);
            }
        }

        pub fn font(e: &$($m)? EmbeddedFont, f: &mut F) {
            for id in [&$($m)? e.regular, &$($m)? e.bold, &$($m)? e.italic, &$($m)? e.bold_italic].into_iter().flatten() {
                f(id);
            }
        }
    };
}

/// Read every media id under a part of the model.
pub mod visit {
    walkers!();
}

/// Rewrite every media id under a part of the model in place.
pub mod visit_mut {
    walkers!(mut);
}

/// Every media reference in a presentation: slides, masters (with their layouts and themes),
/// the notes and handout masters, default text style, table styles and embedded fonts.
pub fn visit_presentation(p: &Presentation, f: &mut visit::F) {
    for s in &p.slides {
        visit::slide(s, f);
    }
    for m in p.masters.iter().chain(p.notes_master.iter()).chain(p.handout_master.iter()) {
        visit::master(m, f);
    }
    visit::list(&p.default_text_style, f);
    for t in &p.table_styles {
        visit::table_style(t, f);
    }
    for e in &p.embedded_fonts {
        visit::font(e, f);
    }
}

/// The media ids a presentation references, each once, in the order first met.
pub fn presentation_media(p: &Presentation) -> Vec<MediaId> {
    collect(|f| visit_presentation(p, f))
}

/// The media ids a slide references (shapes, background, sounds, notes), each once.
pub fn slide_media(s: &Slide) -> Vec<MediaId> {
    collect(|f| visit::slide(s, f))
}

/// The media ids shapes reference (groups included), each once.
pub fn shapes_media(v: &[Shape]) -> Vec<MediaId> {
    collect(|f| visit::shapes(v, f))
}

fn collect(walk: impl FnOnce(&mut visit::F)) -> Vec<MediaId> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    walk(&mut |id: &MediaId| {
        if seen.insert(*id) {
            out.push(*id);
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use deckcraft_model::style::{Artistic, Fill, PictureFill};
    use deckcraft_model::text::{Action, Bullet, Hyperlink};
    use deckcraft_model::{Background, MediaClip, ShapeKind, defaults};

    use super::*;

    fn pf(media: u32) -> PictureFill {
        PictureFill { media: MediaId(media), ..Default::default() }
    }

    /// One reference of each kind, every one a different id.
    fn deck() -> Presentation {
        let mut p = defaults::new_presentation(None);
        let mut s = (*p.slides.first().cloned().unwrap_or_default()).clone();
        let pic = Shape {
            kind: ShapeKind::Picture {
                fill: PictureFill { artistic: Some(Artistic { effect: "artisticBlur".into(), original: MediaId(9002) }), ..pf(9001) },
            },
            ..Default::default()
        };
        let clip = Shape {
            kind: ShapeKind::Media(MediaClip { media: MediaId(9003), poster: Some(MediaId(9004)), ..Default::default() }),
            ..Default::default()
        };
        let mut text = deckcraft_model::text::TextBody::from_text("x");
        if let Some(para) = text.paragraphs.first_mut() {
            para.props.bullet = Some(Bullet::Picture { media: MediaId(9007) });
        }
        let filled = Shape {
            fill: Some(Fill::Picture(pf(9005))),
            click: Some(Hyperlink {
                action: Action::NextSlide,
                tooltip: String::new(),
                highlight_click: false,
                sound: Some(MediaId(9006)),
                stop_sound: false,
                zoom_return: None,
            }),
            text: Some(text),
            ..Default::default()
        };
        let group = Shape { kind: ShapeKind::Group { children: vec![pic, clip], child: Default::default() }, ..Default::default() };
        s.shapes = vec![group, filled];
        s.background = Some(Background::Fill { fill: Fill::Picture(pf(9008)) });
        p.slides = vec![Arc::new(s)];
        if let Some(m) = p.masters.first_mut() {
            Arc::make_mut(m).theme.format.bg_fills.push(Fill::Picture(pf(9009)));
        }
        p.embedded_fonts.push(deckcraft_model::EmbeddedFont { regular: Some(MediaId(9010)), ..Default::default() });
        p
    }

    #[test]
    fn every_kind_of_reference_is_found() {
        let p = deck();
        let mut got = presentation_media(&p);
        got.sort();
        assert_eq!(got, (9001..=9010).map(MediaId).collect::<Vec<_>>());
        let mut slide = p.slides.first().map(|s| slide_media(s)).unwrap_or_default();
        slide.sort();
        assert_eq!(slide, (9001..=9008).map(MediaId).collect::<Vec<_>>());
    }

    #[test]
    fn visit_mut_rewrites_every_reference() {
        let p = deck();
        let mut s = (**p.slides.first().expect("slide")).clone();
        visit_mut::slide(&mut s, &mut |id: &mut MediaId| id.0 += 100);
        let mut got = slide_media(&s);
        got.sort();
        assert_eq!(got, (9101..=9108).map(MediaId).collect::<Vec<_>>());
    }

    /// SmartArt and OLE objects keep their parts as media items (#209); those count as used.
    #[test]
    fn kept_parts_of_unmodelled_objects_are_references() {
        let opaque = Shape {
            kind: ShapeKind::Opaque {
                xml: String::new(),
                preview: Some(MediaId(9101)),
                label: "object".into(),
                parts: vec![("rId2".into(), "oleObject".into(), MediaId(9102))],
            },
            ..Default::default()
        };
        assert_eq!(shapes_media(std::slice::from_ref(&opaque)), vec![MediaId(9101), MediaId(9102)]);
    }
}
