//! Resolve uncached diagram numeric rules only after theme/font inheritance is available.
use crate::dgm::layout::{Placed, evaluate_measured};
use deckcraft_geom::{Rect, Xfrm, preset};
use deckcraft_model::{Geom, Presentation, Shape, ShapeKind, resolve::Ctx};
use std::sync::Arc;

pub(super) fn apply(pres: &mut Presentation) {
    let snapshot = pres.clone();
    for (i, master) in pres.masters.iter_mut().enumerate() {
        if !needs_rules(&master.shapes, 0) && !master.layouts.iter().any(|l| needs_rules(&l.shapes, 0)) {
            continue;
        }
        let Some(source) = snapshot.masters.get(i) else { continue };
        let master = Arc::make_mut(master);
        walk(&Ctx::for_master(&snapshot, source), &mut master.shapes, 0);
        for (j, layout) in master.layouts.iter_mut().enumerate() {
            if let Some(source_layout) = source.layouts.get(j) {
                walk(&Ctx::for_layout(&snapshot, source, source_layout), &mut layout.shapes, 0);
            }
        }
    }
    for slide in &mut pres.slides {
        if !needs_rules(&slide.shapes, 0) {
            continue;
        }
        if let Some(ctx) = Ctx::for_slide(&snapshot, slide) {
            walk(&ctx, &mut Arc::make_mut(slide).shapes, 0);
        }
    }
}
fn needs_rules(shapes: &[Shape], depth: usize) -> bool {
    depth <= 32
        && shapes.iter().any(|s| {
            s.diagram.as_ref().is_some_and(|d| {
                d.layout_xml.as_ref().is_some_and(|xml| xml.contains("rule"))
                    && d.kept.as_ref().is_some_and(|k| !k.package.rels.iter().any(|r| r.id == "drawing"))
            }) || needs_rules(s.children(), depth + 1)
        })
}
fn walk(ctx: &Ctx<'_>, shapes: &mut [Shape], depth: usize) {
    if depth > 32 {
        return;
    }
    for shape in shapes {
        if let Err(reason) = relayout(ctx, shape) {
            log::warn!("pptx: diagram numeric rules could not be evaluated: {reason}");
        }
        if let Some(children) = shape.children_mut() {
            walk(ctx, children, depth + 1);
        }
    }
}
fn adjusted(source: &Shape, placed: &Placed<'_>) -> Shape {
    let mut s = source.clone();
    s.xfrm = Some(placed.rect);
    if let Some(body) = &mut s.text {
        super::shapes::diagram_text_layout(body, placed, None);
        // The rule search has measured the final size. A second independent autofit pass
        // would violate ordered rules and could shrink below the author's explicit floor.
        body.body.autofit = Some(deckcraft_model::text::AutoFit::None);
    }
    s
}
fn text_rect(s: &Shape, rect: Xfrm) -> Rect {
    if let Geom::Preset { name, adj } = &s.geom
        && let Some(geometry) = preset::build(name, rect.w, rect.h, adj)
    {
        return geometry.text_rect;
    }
    Rect::new(0.0, 0.0, rect.w, rect.h)
}
#[derive(PartialEq)]
struct MeasureKey {
    size: [f64; 2],
    fonts: [Option<f64>; 3],
    margins: [f64; 4],
    rotation: f64,
}
impl From<&Placed<'_>> for MeasureKey {
    fn from(p: &Placed<'_>) -> Self {
        Self { size: [p.rect.w, p.rect.h], fonts: [p.font, p.secondary_font, p.min_font], margins: p.margins, rotation: p.text_rotation }
    }
}
// A rule often changes just one of hundreds of labels. Keep one measured result per shape,
// not all search candidates, and bound total shaped text independently of the solver budget.
#[derive(Default)]
struct Measurements {
    last: Vec<Option<(MeasureKey, [f64; 2])>>,
    bytes: usize,
}
fn measure(ctx: &Ctx<'_>, source: &[Shape], placed: &[Placed<'_>], prefer_words: bool, cache: &mut Measurements) -> Result<Vec<f64>, &'static str> {
    if source.len() != placed.len() {
        return Err("diagram rule shape binding changed");
    }
    let mut scores = Vec::with_capacity(placed.len());
    cache.last.resize_with(placed.len(), || None);
    for ((source, p), cached) in source.iter().zip(placed).zip(&mut cache.last) {
        if source.name != p.name {
            return Err("diagram rule shape order changed");
        }
        let key = MeasureKey::from(p);
        if let Some((_, score)) = cached.as_ref().filter(|(old, _)| *old == key) {
            scores.push(score[usize::from(prefer_words)]);
            continue;
        }
        let s = adjusted(source, p);
        let Some(body) = &s.text else {
            scores.push(0.0);
            *cached = Some((key, [0.0; 2]));
            continue;
        };
        let bytes = body.paragraphs.iter().flat_map(|p| &p.runs).map(|r| r.text.len()).sum::<usize>();
        cache.bytes = cache.bytes.saturating_add(bytes);
        if bytes > 65536 || cache.bytes > 32 * 1024 * 1024 {
            return Err("diagram rule text budget exceeded");
        }
        let result = deckcraft_text::layout(
            ctx,
            &s,
            body,
            &deckcraft_text::Opts { rect: text_rect(&s, p.rect), fields: &deckcraft_text::NoFields, prompt_color: None, no_shrink: true },
        );
        let w = result.inner.width().max(0.0);
        let h = result.inner.height().max(0.0);
        let overflow = (result.content_width - w - 0.01).max(0.0) / w.max(1.0) + (result.content_height - h - 0.01).max(0.0) / h.max(1.0);
        let score = [overflow, overflow + result.forced_line_breaks(body) as f64];
        scores.push(score[usize::from(prefer_words)]);
        *cached = Some((key, score));
    }
    Ok(scores)
}
fn relayout(ctx: &Ctx<'_>, shape: &mut Shape) -> Result<(), &'static str> {
    let Some(diagram) = &shape.diagram else { return Ok(()) };
    let Some(kept) = &diagram.kept else { return Ok(()) };
    if kept.package.rels.iter().any(|r| r.id == "drawing") {
        return Ok(());
    }
    let Some(layout) = diagram.layout_xml.as_ref().and_then(|s| crate::xml::parse(s.as_bytes()).ok()) else { return Ok(()) };
    if layout.root.find("rule").is_none() {
        return Ok(());
    }
    let Some(data_rel) = kept.package.rels.iter().find(|r| r.id == "dm") else { return Ok(()) };
    let Some(data) = kept
        .package
        .parts
        .iter()
        .find(|p| crate::opc::norm(&p.name) == crate::opc::norm(&data_rel.part))
        .and_then(|p| crate::xml::parse(&p.bytes).ok())
    else {
        return Ok(());
    };
    let ShapeKind::Group { children, child } = &shape.kind else { return Ok(()) };
    let mut cache = Measurements::default();
    let mut callback = |placed: &[Placed<'_>], prefer_words: bool| measure(ctx, children, placed, prefer_words, &mut cache);
    let placed = evaluate_measured(&layout.root, &data.root, *child, Some(&mut callback))?;
    let updated: Vec<_> = children.iter().zip(&placed).map(|(s, p)| adjusted(s, p)).collect();
    if updated.len() != children.len() {
        return Err("diagram rule result shape count changed");
    }
    shape.kind = ShapeKind::Group { children: updated, child: *child };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measured_text_is_reused_for_translation_but_not_width_or_font_changes() {
        let pres = Presentation { slides: vec![Arc::new(deckcraft_model::Slide::default())], ..Default::default() };
        let ctx = Ctx::for_slide(&pres, &pres.slides[0]).unwrap();
        let source = vec![Shape {
            name: "probe".into(),
            geom: Geom::preset("rect"),
            text: Some(deckcraft_model::text::TextBody::from_text("LongUnbreakableWord and several other words")),
            ..Default::default()
        }];
        let mut placed = vec![Placed {
            node_key: 1,
            shape: None,
            points: vec![],
            point_levels: vec![],
            name: "probe".into(),
            style: String::new(),
            rect: Xfrm::new(0., 0., 70., 40.),
            font: Some(32.),
            secondary_font: None,
            min_font: None,
            margins: [0.; 4],
            text_rotation: 0.,
            connector: false,
            params: Default::default(),
            endpoints: None,
        }];
        let mut cache = Measurements::default();
        let overflow = measure(&ctx, &source, &placed, false, &mut cache).unwrap();
        let bytes = cache.bytes;
        placed[0].rect.x = 500.;
        let words = measure(&ctx, &source, &placed, true, &mut cache).unwrap();
        assert_eq!(cache.bytes, bytes, "moving a shape or requesting word breaks does not reshape text");
        assert!(words[0] >= overflow[0] && overflow[0] > 0.);
        placed[0].rect.w = 600.;
        placed[0].rect.h = 400.;
        assert_eq!(measure(&ctx, &source, &placed, true, &mut cache).unwrap(), vec![0.]);
        assert_eq!(cache.bytes, bytes * 2);
        placed[0].font = Some(24.);
        measure(&ctx, &source, &placed, true, &mut cache).unwrap();
        assert_eq!(cache.bytes, bytes * 3);
    }
}
