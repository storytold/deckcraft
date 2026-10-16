//! Notes layout export uses ordinary editable shapes and live page-thumbnail elements.
use super::*;
use deckcraft_model::notes::{NotesDateTime, NotesFields, NotesMaster};

fn append(out: &mut String, value: &str) -> Result<()> {
    if out.len().saturating_add(value.len()) > crate::MAX_PART as usize {
        return Err(OdpError("notes XML exceeds package part limit".into()));
    }
    out.push_str(value);
    Ok(())
}

#[derive(Clone, Copy)]
enum Role {
    Body,
    Page,
    Header,
    Footer,
    Date,
    Number,
}

impl Role {
    fn class(self) -> &'static str {
        match self {
            Self::Body => "notes",
            Self::Page => "page",
            Self::Header => "header",
            Self::Footer => "footer",
            Self::Date => "date-time",
            Self::Number => "page-number",
        }
    }
}

fn role(shape: &Shape) -> Option<Role> {
    Some(match shape.ph_type()? {
        PhType::Body => Role::Body,
        PhType::SlideImage => Role::Page,
        PhType::Header => Role::Header,
        PhType::Footer => Role::Footer,
        PhType::Date => Role::Date,
        PhType::SlideNum => Role::Number,
        _ => return None,
    })
}

fn paint_style(master: &Master, shape: &Shape) -> String {
    format!("NotesPaint{}_{}", master.id.0, shape.id.0)
}

/// Resolve strict notes-role defaults, never ordinary slide placeholder compatibility.
fn effective(shape: &Shape, master: Option<&Master>, body: Option<&TextBody>) -> Shape {
    let mut out = shape.clone();
    if let Some(parent) = master.and_then(|master| master.shapes.iter().find(|parent| parent.ph_type() == shape.ph_type() && shape.ph.is_some())) {
        out.xfrm = out.xfrm.or(parent.xfrm);
        if out.fill.is_none() {
            out.fill.clone_from(&parent.fill);
        }
        if out.line.is_none() {
            out.line.clone_from(&parent.line);
        }
        if out.effects.is_none() {
            out.effects.clone_from(&parent.effects);
        }
        if out.style.is_none() {
            out.style.clone_from(&parent.style);
        }
        if let Some(text) = body.or(out.text.as_ref()) {
            let mut text = text.clone();
            if let Some(default) = &parent.text {
                text.body.inherit(&default.body);
                for paragraph in &mut text.paragraphs {
                    if let Some(default) = default.paragraphs.first() {
                        paragraph.props.inherit(&default.props);
                        paragraph.def_props.inherit(&default.def_props);
                    }
                }
            }
            out.text = Some(text);
        }
    } else if let Some(body) = body {
        out.text = Some(body.clone());
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn normalize(
    ctx: &Ctx<'_>,
    shape: &Shape,
    master: Option<&Master>,
    canonical: Option<(ShapeId, &TextBody)>,
    is_master: bool,
    depth: usize,
    roles: &mut Vec<(ShapeId, Role, bool)>,
) -> Result<Shape> {
    if depth >= 64 || roles.len() >= 128 {
        return Err(OdpError("notes shape nesting or count exceeds limit".into()));
    }
    let current = canonical.filter(|(id, _)| *id == shape.id);
    if current.is_some() && shape.ph_type() != Some(PhType::Body) {
        return Err(OdpError("canonical ODF notes bodies require an explicit body placeholder role".into()));
    }
    let mut out = effective(shape, master, current.map(|(_, body)| body));
    if let ShapeKind::Group { children, .. } = &mut out.kind {
        for child in children {
            *child = normalize(ctx, child, master, canonical, is_master, depth + 1, roles)?;
        }
    }
    let role = if current.is_some() { Some(Role::Body) } else { role(&out) };
    if let Some(role) = role {
        if !matches!(out.kind, ShapeKind::Shape) {
            return Err(OdpError("notes placeholders must be drawing frames or thumbnails".into()));
        }
        if !is_master && matches!(role, Role::Header | Role::Footer | Role::Date | Role::Number) {
            return Err(OdpError("ODF header/footer placeholders must remain on the notes master".into()));
        }
        if is_master && !matches!(role, Role::Page) {
            let body = out.text.get_or_insert_with(TextBody::default);
            if body.paragraphs.is_empty() {
                body.paragraphs.push(Paragraph::default());
            }
        }
        if let Some(body) = &out.text {
            let mut body = body.clone();
            body.body = resolve::body(ctx, &out);
            for paragraph in &mut body.paragraphs {
                let source = paragraph.clone();
                paragraph.props = resolve::para(ctx, &out, &source);
                paragraph.def_props = resolve::run(ctx, &out, &source, &source.def_props);
                // Keep inherited default kerning implicit. Turning the theme's threshold
                // into an explicit source override changes the existing export contract.
                paragraph.def_props.kern = source.def_props.kern;
                paragraph.end_props = resolve::run(ctx, &out, &source, &source.end_props);
                for run in &mut paragraph.runs {
                    let kern = run.props.kern;
                    run.props = resolve::run(ctx, &out, &source, &run.props);
                    run.props.kern = kern;
                }
                paragraph.runs.insert(0, Run::new(""));
            }
            out.text = Some(body);
        }
        out.ph = None;
        match role {
            Role::Page => {
                if out.text.as_ref().is_some_and(|body| !body.is_empty()) {
                    return Err(OdpError("live notes thumbnails cannot contain text".into()));
                }
                out.text = None;
                out.text_box = false;
                out.geom = Geom::default();
            }
            _ => {
                out.text_box = true;
                if out.text.is_none() {
                    out.text = Some(TextBody::default());
                }
            }
        }
        roles.push((out.id, role, shape.xfrm.is_some()));
    }
    Ok(out)
}

impl Writer<'_> {
    fn notes_layout(&mut self, size: Size) -> Result<String> {
        styles::finite(&[size.width, size.height])?;
        if size.width <= 0.0 || size.height <= 0.0 || size.width > 5000.0 || size.height > 5000.0 || self.next >= 200_000 {
            return Err(OdpError("invalid notes page size or excessive layouts".into()));
        }
        self.next += 1;
        let name = format!("NotesPage{}", self.next);
        append(
            &mut self.notes_layouts,
            &format!(
                "<style:page-layout style:name=\"{name}\"><style:page-layout-properties fo:page-width=\"{}pt\" fo:page-height=\"{}pt\" fo:margin=\"0pt\" style:print-orientation=\"{}\"/></style:page-layout>",
                size.width,
                size.height,
                if size.width >= size.height { "landscape" } else { "portrait" }
            ),
        )?;
        Ok(name)
    }

    fn notes_fields(&mut self, fields: &NotesFields) -> Result<(String, String)> {
        let mut attrs = String::new();
        for (value, field) in [(&fields.header, "header"), (&fields.footer, "footer")] {
            if let Some(value) = value {
                if value.len() > crate::MAX_PART as usize || self.next >= 200_000 {
                    return Err(OdpError("notes field declarations exceed limits".into()));
                }
                self.next += 1;
                let name = format!("NotesField{}", self.next);
                append(
                    &mut self.notes_declarations,
                    &format!("<presentation:{field}-decl presentation:name=\"{name}\">{}</presentation:{field}-decl>", esc(value)),
                )?;
                attrs.push_str(&format!(" presentation:use-{field}-name=\"{name}\""));
            }
        }
        if let Some(date) = &fields.date_time {
            if self.next >= 200_000 {
                return Err(OdpError("notes field declarations exceed limits".into()));
            }
            let (source, value) = match date {
                NotesDateTime::Fixed { text } if text.len() <= crate::MAX_PART as usize => ("fixed", text.as_str()),
                NotesDateTime::Current { format, lang } if format == "datetime1" && lang.is_none() => ("current-date", ""),
                _ => return Err(OdpError("notes automatic-date format cannot be exported faithfully yet".into())),
            };
            self.next += 1;
            let name = format!("NotesField{}", self.next);
            append(
                &mut self.notes_declarations,
                &format!(
                    "<presentation:date-time-decl presentation:name=\"{name}\" presentation:source=\"{source}\">{}</presentation:date-time-decl>",
                    esc(value)
                ),
            )?;
            attrs.push_str(&format!(" presentation:use-date-time-name=\"{name}\""));
        }
        let mut props = String::new();
        for (key, value) in [
            ("header", fields.display_header),
            ("footer", fields.display_footer),
            ("date-time", fields.display_date_time),
            ("page-number", fields.display_page_number),
        ] {
            if let Some(value) = value {
                props.push_str(&format!(" presentation:display-{key}=\"{value}\""));
            }
        }
        Ok((attrs, props))
    }

    #[allow(clippy::too_many_arguments)]
    fn notes_shapes(
        &mut self,
        ctx: &Ctx<'_>,
        shapes: &[Shape],
        master: Option<&Master>,
        canonical: Option<(ShapeId, &TextBody)>,
        page_number: Option<usize>,
        out: &mut String,
    ) -> Result<()> {
        for shape in shapes {
            let mut roles = Vec::new();
            let normalized = normalize(ctx, shape, master, canonical, page_number.is_none(), 0, &mut roles)?;
            let styles_start = self.styles.len();
            let mut xml = String::new();
            self.shape(ctx, &normalized, &mut xml)?;
            let mut sources = Vec::new();
            for (id, _, _) in &roles {
                let source = find_shape(std::slice::from_ref(shape), *id).ok_or_else(|| OdpError("missing notes source frame".into()))?;
                let body = canonical.filter(|(body, _)| body == id).map(|(_, body)| body).or(source.text.as_ref());
                if let Some(master) = master
                    && let Some(parent) =
                        if page_number.is_none() { Some(source) } else { master.shapes.iter().find(|parent| parent.ph_type() == source.ph_type()) }
                {
                    let body = canonical.filter(|(body, _)| body == id).map(|(_, body)| body).or(source.text.as_ref());
                    if body.is_some_and(|body| !body.list_style.is_empty())
                        || parent.text.as_ref().is_some_and(|body| {
                            !body.list_style.is_empty()
                                || body.paragraphs.iter().any(|p| p.props.bullet.is_some())
                                || body.paragraphs.first().is_some_and(|first| {
                                    body.paragraphs.iter().any(|p| p.level != first.level || p.props != first.props || p.def_props != first.def_props)
                                })
                        })
                        || [&master.title_style, &master.body_style, &master.other_style].into_iter().any(|style| !style.is_empty())
                    {
                        return Err(OdpError("live notes-master list or multiple paragraph-level defaults are not represented yet".into()));
                    }
                    sources.push(crate::notes_styles::RoleSource {
                        identity: self.shape_identity(*id),
                        source,
                        body,
                        shared: paint_style(master, parent),
                        paragraph: format!("NotesParagraph{}_{}", master.id.0, parent.id.0),
                    });
                } else {
                    sources.push(crate::notes_styles::RoleSource {
                        identity: self.shape_identity(*id),
                        source,
                        body,
                        shared: String::new(),
                        paragraph: String::new(),
                    });
                }
            }
            if !sources.is_empty() {
                let rewritten = crate::notes_styles::rewrite(
                    self.styles.get(styles_start..).ok_or_else(|| OdpError("invalid generated notes style range".into()))?,
                    &xml,
                    &sources,
                    page_number.is_none(),
                )?;
                self.styles.truncate(styles_start);
                append(&mut self.styles, &rewritten.styles)?;
                append(&mut self.definitions, &rewritten.definitions)?;
                xml = rewritten.drawing;
            }
            for (id, role, transformed) in roles {
                let identity = format!("xml:id=\"{}\"", self.shape_identity(id));
                let at = xml.find(&identity).ok_or_else(|| OdpError("notes placeholder was omitted by drawing export".into()))?;
                let start = xml.get(..at).and_then(|prefix| prefix.rfind('<')).ok_or_else(|| OdpError("invalid notes drawing output".into()))?;
                let class = format!(" presentation:class=\"{}\" presentation:user-transformed=\"{transformed}\"", role.class());
                if matches!(role, Role::Page) {
                    let close = xml
                        .get(at..)
                        .and_then(|tail| tail.find("</draw:rect>"))
                        .map(|offset| at + offset)
                        .ok_or_else(|| OdpError("invalid live thumbnail output".into()))?;
                    xml.replace_range(close..close + "</draw:rect>".len(), "</draw:page-thumbnail>");
                    let number = page_number.map(|number| format!(" draw:page-number=\"{number}\"")).unwrap_or_default();
                    xml.replace_range(start..start + "<draw:rect".len(), &format!("<draw:page-thumbnail{class}{number}"));
                } else {
                    let tag = "<draw:frame";
                    if !xml.get(start..).is_some_and(|value| value.starts_with(tag)) {
                        return Err(OdpError("notes text placeholder is not a frame".into()));
                    }
                    xml.insert_str(start + tag.len(), &class);
                }
            }
            append(out, &xml)?;
        }
        Ok(())
    }

    pub(super) fn notes_master(&mut self, notes: &NotesMaster, out: &mut String) -> Result<()> {
        self.with_owner(format!("notesMaster{}", notes.master.id.0), |writer| writer.notes_master_for_owner(notes, out))
    }

    fn notes_master_for_owner(&mut self, notes: &NotesMaster, out: &mut String) -> Result<()> {
        let pres = self.pres;
        let ctx = Ctx::for_master(pres, &notes.master);
        for shape in notes.master.shapes.iter().filter(|shape| role(shape).is_some()) {
            let (fill, _) = resolve::fill(&ctx, shape);
            if !matches!(fill, None | Some(Fill::None | Fill::Solid { .. })) {
                return Err(OdpError("shared notes-master paint supports solid fills and explicit no-fill only".into()));
            }
        }
        let layout = self.notes_layout(notes.size)?;
        let (fields, flags) = self.notes_fields(&notes.fields)?;
        let fill = match &notes.master.background {
            Some(Background::Fill { fill }) => self.fill_attrs(&ctx, Some(fill), None, (notes.size.width, notes.size.height))?,
            None => String::new(),
            _ => return Err(OdpError("unsupported notes master background".into())),
        };
        let style = self.style("drawing-page", &format!("<style:drawing-page-properties {fill} {flags}/>"));
        append(out, &format!("<presentation:notes style:page-layout-name=\"{layout}\" draw:style-name=\"{style}\"{fields}>"))?;
        self.notes_shapes(&ctx, &notes.master.shapes, Some(&notes.master), None, None, out)?;
        append(out, "</presentation:notes>")
    }

    pub(super) fn notes_page(&mut self, slide: &Slide, page_number: usize, out: &mut String) -> Result<()> {
        self.with_owner(format!("notesPage{}", slide.id.0), |writer| writer.notes_page_for_owner(slide, page_number, out))
    }

    fn notes_page_for_owner(&mut self, slide: &Slide, page_number: usize, out: &mut String) -> Result<()> {
        let pres = self.pres;
        let Some(page) = &slide.notes_page else {
            let ctx = Ctx::for_slide(pres, slide).ok_or_else(|| OdpError("notes page has no slide style context".into()))?;
            let shape = Shape { text: Some(slide.notes.clone()), ..Default::default() };
            if !slide.notes.is_empty()
                || has_paragraph_background(&ctx, &shape, &slide.notes)
                || has_paragraph_line_height(&ctx, &shape, &slide.notes)
            {
                append(
                    out,
                    "<presentation:notes><draw:frame presentation:class=\"notes\" svg:x=\"0pt\" svg:y=\"0pt\" svg:width=\"540pt\" svg:height=\"720pt\"><draw:text-box>",
                )?;
                self.text_content(&ctx, &shape, &slide.notes, out)?;
                append(out, "</draw:text-box></draw:frame></presentation:notes>")?;
            }
            return Ok(());
        };
        if page_number == 0 || page_number > 10_000 {
            return Err(OdpError("invalid notes page number".into()));
        }
        let master = pres.notes_master_for(slide);
        if page.master.is_some() && master.is_none() {
            return Err(OdpError("missing notes master".into()));
        }
        let ctx = Ctx::for_notes(pres, slide).ok_or_else(|| OdpError("notes page has no style context".into()))?;
        let size = pres.notes_page_size(slide);
        let layout =
            page.size.map(|size| self.notes_layout(size)).transpose()?.map(|name| format!(" style:page-layout-name=\"{name}\"")).unwrap_or_default();
        let (fields, flags) = self.notes_fields(&page.fields)?;
        let fill = match &page.background {
            Some(Background::Fill { fill }) => self.fill_attrs(&ctx, Some(fill), None, (size.width, size.height))?,
            None => String::new(),
            _ => return Err(OdpError("unsupported notes page background".into())),
        };
        let style = self.style(
            "drawing-page",
            &format!("<style:drawing-page-properties {fill} {flags} presentation:background-objects-visible=\"{}\"/>", page.show_master_shapes),
        );
        append(out, &format!("<presentation:notes{layout} draw:style-name=\"{style}\"{fields}>"))?;
        if page.body_shape.is_none() && !slide.notes.is_empty() {
            return Err(OdpError("speaker notes require a designated editable body frame".into()));
        }
        self.notes_shapes(&ctx, &page.shapes, master, page.body_shape.map(|id| (id, &slide.notes)), Some(page_number), out)?;
        append(out, "</presentation:notes>")
    }
}
