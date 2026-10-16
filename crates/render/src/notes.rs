//! Notes-page rendering through the ordinary slide renderer. Live thumbnails exist only in
//! this transient drawing document; native/ODP documents keep editable slide-image roles.
use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::{
    Layout, LayoutId, LayoutType, Master, PhType, Placeholder, Presentation, Shape, ShapeId, ShapeKind, Slide,
    notes::{NotesDateTime, NotesFields},
    style::PictureFill,
    text::{RunKind, TextBody},
};

use crate::{Image, RenderOpts, SlideFields, render_slide};

fn field_text(kind: &str, fields: &NotesFields, fallback: &SlideFields) -> Option<String> {
    use deckcraft_text::Fields;
    match kind {
        "header" => fields.header.clone(),
        "footer" => fields.footer.clone().or_else(|| fallback.field("footer")),
        "notesdatetime" => match &fields.date_time {
            Some(NotesDateTime::Fixed { text }) => Some(text.clone()),
            Some(NotesDateTime::Current { format, lang }) => fallback.field_in(format, lang.as_deref()),
            None => fallback.field("datetime1"),
        },
        "slidenum" => fallback.field("slidenum"),
        _ => None,
    }
}

fn visible(kind: Option<PhType>, fields: &NotesFields) -> bool {
    match kind {
        Some(PhType::Header) => fields.display_header.unwrap_or(fields.header.is_some()),
        Some(PhType::Footer) => fields.display_footer.unwrap_or(fields.footer.is_some()),
        Some(PhType::Date) => fields.display_date_time.unwrap_or(fields.date_time.is_some()),
        Some(PhType::SlideNum) => fields.display_page_number.unwrap_or(false),
        _ => true,
    }
}

#[allow(clippy::too_many_arguments)]
fn prepare_shapes(
    shapes: &mut [Shape],
    body: Option<ShapeId>,
    canonical: &TextBody,
    master: &Master,
    fields: &NotesFields,
    fallback: &SlideFields,
    thumbnail: Option<deckcraft_model::MediaId>,
    aspect: f64,
    depth: usize,
) {
    if depth > 64 {
        return;
    }
    for shape in shapes {
        shape.hidden |= !visible(shape.ph_type(), fields);
        if body == Some(shape.id) {
            let mut text = canonical.clone();
            if let Some(parent) = master.shapes.iter().find(|parent| parent.ph_type() == Some(PhType::Body)).and_then(|parent| parent.text.as_ref()) {
                text.body.inherit(&parent.body);
                for level in 0..9 {
                    if let Some(inherited) = parent.list_style.level(level) {
                        let mut value = text.list_style.level(level).cloned().unwrap_or_default();
                        value.para.inherit(&inherited.para);
                        value.run.inherit(&inherited.run);
                        text.list_style.set(level, value);
                    }
                }
                for paragraph in &mut text.paragraphs {
                    if let Some(default) =
                        parent.paragraphs.iter().find(|default| default.level == paragraph.level).or_else(|| parent.paragraphs.first())
                    {
                        paragraph.props.inherit(&default.props);
                        paragraph.def_props.inherit(&default.def_props);
                    }
                }
            }
            shape.text = Some(text);
        }
        if shape.ph_type() == Some(PhType::SlideImage)
            && let Some(media) = thumbnail
        {
            if let Some(x) = &mut shape.xfrm {
                // Fit inside the frame while retaining its centre, rotation and mirrors.
                let (width, height) = if x.w > x.h * aspect { (x.h * aspect, x.h) } else { (x.w, x.w / aspect) };
                x.x += (x.w - width) * 0.5;
                x.y += (x.h - height) * 0.5;
                x.w = width;
                x.h = height;
            }
            shape.kind = ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } };
            shape.text = None;
        }
        if let Some(text) = &mut shape.text {
            for paragraph in &mut text.paragraphs {
                for run in &mut paragraph.runs {
                    if let RunKind::Field { field } = &run.kind
                        && let Some(value) = field_text(field, fields, fallback)
                    {
                        run.text = value;
                        run.kind = RunKind::Text;
                    }
                }
            }
        }
        if let ShapeKind::Group { children, .. } = &mut shape.kind {
            prepare_shapes(children, body, canonical, master, fields, fallback, thumbnail, aspect, depth + 1);
        }
    }
}

fn resolve_geometry(shapes: &mut [Shape], context: &deckcraft_model::resolve::Ctx<'_>, depth: usize) {
    if depth > 64 {
        return;
    }
    for shape in shapes {
        if shape.xfrm.is_none() {
            shape.xfrm = Some(deckcraft_model::resolve::xfrm(context, shape));
        }
        if let ShapeKind::Group { children, .. } = &mut shape.kind {
            resolve_geometry(children, context, depth + 1);
        }
    }
}

/// A notes-sized transient drawing document, also usable for real text and link placement.
/// The source remains untouched, including its editable thumbnail and master identities.
pub fn notes_page_document(pres: &Presentation, index: usize, opts: &RenderOpts<'_>) -> Option<Presentation> {
    let source = pres.slides.get(index)?;
    if pres.notes_page_error(source).is_some() {
        return None;
    }
    let size = pres.notes_page_size(source);
    if !deckcraft_model::notes::valid_size(size) {
        return None;
    }
    let mut document = pres.clone();
    let mut master = pres
        .notes_master_for(source)
        .cloned()
        .unwrap_or_else(|| Master { theme: pres.first_master().map(|master| master.theme.clone()).unwrap_or_default(), ..Default::default() });
    let page = source.notes_page.as_ref();
    let mut shapes = page.map(|page| page.shapes.clone()).unwrap_or_default();
    let mut body = page.and_then(|page| page.body_shape);
    let show_master = page.is_none_or(|page| page.show_master_shapes);
    if show_master {
        // Master placeholders materialize for drawing when no local role overrides them.
        // Master decorations remain on the master and retain drawing order there.
        let mut inherited_shapes = Vec::new();
        for placeholder in master.shapes.iter().filter(|shape| shape.ph.is_some()) {
            let kind = placeholder.ph_type();
            let mut overridden = false;
            deckcraft_model::walk(&shapes, &mut |shape, _| overridden |= shape.ph_type() == kind);
            if overridden {
                continue;
            }
            let inherited = placeholder.clone();
            if kind == Some(PhType::Body) && body.is_none() {
                body = Some(inherited.id);
            }
            inherited_shapes.push(inherited);
        }
        inherited_shapes.append(&mut shapes);
        shapes = inherited_shapes;
    }
    if body.is_none() && !source.notes.is_empty() {
        let id = ShapeId(document.alloc_id()?);
        shapes.push(Shape {
            id,
            xfrm: Some(Xfrm::new(size.width * 0.1, size.height * 0.53, size.width * 0.8, size.height * 0.36)),
            ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }),
            ..Default::default()
        });
        body = Some(id);
    }
    let mut has_thumbnail = false;
    for items in [&shapes, &master.shapes] {
        deckcraft_model::walk(items, &mut |shape, _| has_thumbnail |= shape.ph_type() == Some(PhType::SlideImage));
    }
    if !has_thumbnail && page.is_none() {
        let id = ShapeId(document.alloc_id()?);
        shapes.insert(
            0,
            Shape {
                id,
                xfrm: Some(Xfrm::new(size.width * 0.1, size.height * 0.1, size.width * 0.8, size.height * 0.4)),
                ph: Some(Placeholder { kind: PhType::SlideImage, ..Default::default() }),
                ..Default::default()
            },
        );
        has_thumbnail = true;
    }
    // Resolve inherited geometry before replacing the semantic thumbnail with a picture.
    if let Some(context) = deckcraft_model::resolve::Ctx::for_notes(pres, source) {
        resolve_geometry(&mut shapes, &context, 0);
    }
    let thumbnail = if has_thumbnail {
        let scale = opts.scale.max(0.01).min(2048.0 / pres.slide_size.width.max(pres.slide_size.height).max(1.0));
        let image = render_slide(pres, index, &RenderOpts { scale, now: opts.now, date_text: opts.date_text.clone(), ..Default::default() });
        document.add_media("live-notes-thumbnail.png", "image/png", image.to_png())
    } else {
        None
    };
    let mut fields = pres.notes_fields(source);
    if fields.footer.is_none() && pres.header_footer.footer {
        fields.footer = Some(pres.header_footer.footer_text.clone());
    }
    let fallback = SlideFields::new(pres.slide_number(index), pres.header_footer.footer_text.clone(), opts);
    let aspect = pres.slide_size.width.max(1.0) / pres.slide_size.height.max(1.0);
    prepare_shapes(&mut shapes, body, &source.notes, &master, &fields, &fallback, thumbnail, aspect, 0);
    // This pass resolves fields/thumbnails only; it never needs canonical-body inheritance.
    prepare_shapes(&mut master.shapes, None, &TextBody::default(), &Master::default(), &fields, &fallback, thumbnail, aspect, 0);
    master.layouts = vec![Layout { id: LayoutId(0), kind: LayoutType::Blank, show_master_shapes: true, ..Default::default() }];
    document.slide_size = size;
    document.masters = vec![Arc::new(master)];
    document.slides = vec![Arc::new(Slide {
        id: source.id,
        layout: LayoutId(0),
        shapes,
        background: page.and_then(|page| page.background.clone()),
        show_master_shapes: show_master,
        ..Default::default()
    })];
    Some(document)
}

pub fn render_notes_page(pres: &Presentation, index: usize, opts: &RenderOpts<'_>) -> Image {
    notes_page_document(pres, index, opts).map(|document| render_slide(&document, 0, opts)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::{
        Background, ColorRef, Fill, MasterId, Rgba, Size,
        notes::{NotesMaster, NotesPage},
    };

    fn fixture() -> Presentation {
        let mut p = Presentation { slide_size: Size::new(100., 50.), ..Default::default() };
        let master = Master {
            id: MasterId(900),
            background: Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(30, 40, 180))) }),
            shapes: vec![
                Shape {
                    id: ShapeId(901),
                    xfrm: Some(Xfrm::new(40., 20., 120., 60.)),
                    ph: Some(Placeholder { kind: PhType::SlideImage, ..Default::default() }),
                    ..Default::default()
                },
                Shape {
                    id: ShapeId(902),
                    xfrm: Some(Xfrm::new(20., 180., 160., 70.)),
                    ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }),
                    ..Default::default()
                },
                Shape {
                    id: ShapeId(903),
                    xfrm: Some(Xfrm::new(10., 270., 20., 20.)),
                    fill: Some(Fill::solid(ColorRef::rgb(Rgba::rgb(20, 180, 40)))),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        p.notes_masters.push(NotesMaster { master: Arc::new(master), size: Size::new(200., 300.), ..Default::default() });
        let slide = Arc::make_mut(&mut p.slides[0]);
        slide.background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(220, 20, 30))) });
        slide.notes = TextBody::from_text("Canonical notes");
        slide.notes_page = Some(NotesPage { master: Some(MasterId(900)), ..Default::default() });
        p.fix_next_id();
        p
    }

    #[test]
    fn notes_master_background_decoration_and_live_thumbnail_render_without_mutating_source() {
        let mut p = fixture();
        let original = p.clone();
        let a = render_notes_page(&p, 0, &Default::default());
        assert_eq!((a.width, a.height), (200, 300));
        assert_eq!(a.pixel(5, 5), [30, 40, 180, 255]);
        assert_eq!(a.pixel(20, 280), [20, 180, 40, 255]);
        assert_eq!(a.pixel(100, 50), [220, 20, 30, 255]);
        assert_eq!(p, original);
        Arc::make_mut(&mut p.slides[0]).background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(10, 160, 170))) });
        let b = render_notes_page(&p, 0, &Default::default());
        assert_eq!(b.pixel(100, 50), [10, 160, 170, 255]);
        assert_eq!(b.pixel(5, 5), a.pixel(5, 5));
        let page = Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap();
        page.background = Some(Background::Fill { fill: Fill::None });
        let c = render_notes_page(&p, 0, &Default::default());
        assert_eq!(c.pixel(5, 5), [255, 255, 255, 255]);
        assert_eq!(c.pixel(20, 280), [20, 180, 40, 255]);
    }

    #[test]
    fn notes_canonical_body_inherits_master_geometry_and_live_owner_survives_reordering() {
        let mut p = fixture();
        p.ensure_notes_body(0).unwrap();
        let body = p.slides[0].notes_page.as_ref().unwrap().body_shape.unwrap();
        assert!(p.slides[0].notes_page.as_ref().unwrap().shapes[0].xfrm.is_none());
        let doc = notes_page_document(&p, 0, &Default::default()).unwrap();
        let frame = deckcraft_model::find_shape(&doc.slides[0].shapes, body).unwrap();
        assert_eq!(frame.text.as_ref().unwrap().text(), "Canonical notes");
        assert_eq!(frame.xfrm.as_ref().unwrap().y, 180.);
        Arc::make_mut(&mut p.notes_masters[0].master).shapes[1].xfrm.as_mut().unwrap().y = 140.;
        let doc = notes_page_document(&p, 0, &Default::default()).unwrap();
        assert_eq!(deckcraft_model::find_shape(&doc.slides[0].shapes, body).unwrap().xfrm.as_ref().unwrap().y, 140.);
        let mut second = (*p.slides[0]).clone();
        second.id = deckcraft_model::SlideId(950);
        second.background = Some(Background::Fill { fill: Fill::solid(ColorRef::rgb(Rgba::rgb(10, 160, 170))) });
        p.slides.push(Arc::new(second));
        p.slides.swap(0, 1);
        assert_eq!(render_notes_page(&p, 0, &Default::default()).pixel(100, 50), [10, 160, 170, 255]);
        assert_eq!(render_notes_page(&p, 1, &Default::default()).pixel(100, 50), [220, 20, 30, 255]);
    }

    #[test]
    fn notes_fields_keep_explicit_empty_override_and_page_visibility() {
        use deckcraft_model::text::{Paragraph, Run};
        let mut p = fixture();
        p.notes_masters[0].fields = NotesFields { header: Some("Inherited header".into()), display_header: Some(true), ..Default::default() };
        Arc::make_mut(&mut p.notes_masters[0].master).shapes.push(Shape {
            id: ShapeId(904),
            xfrm: Some(Xfrm::new(10., 100., 180., 30.)),
            ph: Some(Placeholder { kind: PhType::Header, ..Default::default() }),
            text: Some(TextBody {
                paragraphs: vec![Paragraph {
                    runs: vec![Run { kind: RunKind::Field { field: "header".into() }, ..Run::new("") }],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        });
        let doc = notes_page_document(&p, 0, &Default::default()).unwrap();
        let frame = doc.slides[0].shapes.iter().find(|shape| shape.ph_type() == Some(PhType::Header)).unwrap();
        assert_eq!(frame.text.as_ref().unwrap().text(), "Inherited header");
        let page = Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap();
        page.fields.header = Some(String::new());
        page.fields.display_header = Some(false);
        let doc = notes_page_document(&p, 0, &Default::default()).unwrap();
        let frame = doc.slides[0].shapes.iter().find(|shape| shape.ph_type() == Some(PhType::Header)).unwrap();
        assert!(frame.hidden);
        assert!(frame.text.as_ref().unwrap().is_empty());
        assert_eq!(p.notes_masters[0].fields.header.as_deref(), Some("Inherited header"));
    }
}
