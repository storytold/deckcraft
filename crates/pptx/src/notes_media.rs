//! Every [`MediaId`] the model can reference, found by type rather than by scanning JSON keys.
//!
//! One walker, written once ([`walkers!`]) and expanded twice: [`visit`] reads ids, [`visit_mut`]
//! rewrites them in place. It covers pictures (and their artistic-effect originals), picture fills
//! wherever a fill can sit (shapes, lines, text runs, table cells and styles, chart series and
//! points, backgrounds, theme format schemes), media clips and their posters, 3-D models, OLE and
//! SmartArt previews and kept parts, SmartArt node pictures, picture bullets, hyperlink sounds and notes master resources. Adding a `MediaId` field to the model means adding
//! it here; the shape kinds are matched exhaustively so a new kind can't be missed.

macro_rules! walkers {
    ($($m:tt)?) => {
        use deckcraft_model::chart::Chart;
        use deckcraft_model::style::{Fill, Line, PictureFill};
        use deckcraft_model::table::{CellBlock, Table};
        use deckcraft_model::text::{Hyperlink, ListStyle, ParaProps, RunProps, TextBody, Bullet};
        use deckcraft_model::theme::Theme;
        use deckcraft_model::{Background, Layout, Master, MediaId, Shape, ShapeKind};

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
            opt_fill(&$($m)? p.background, f);
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
            table_at(t, f, 0);
        }

        fn table_at(t: &$($m)? Table, f: &mut F, depth: usize) {
            if depth > MAX_DEPTH { return; }
            opt_fill(&$($m)? t.background, f);
            for row in &$($m)? t.rows {
                for c in &$($m)? row.cells {
                    text(&$($m)? c.text, f);
                    for block in &$($m)? c.blocks {
                        match block {
                            CellBlock::Text(block) => text(&$($m)? block.body, f),
                            CellBlock::Table(block) => table_at(&$($m)? block.table, f, depth + 1),
                        }
                    }
                    opt_fill(&$($m)? c.fill, f);
                    for b in &$($m)? c.borders {
                        opt_line(b, f);
                    }
                    opt_line(&$($m)? c.diag_down, f);
                    opt_line(&$($m)? c.diag_up, f);
                }
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
