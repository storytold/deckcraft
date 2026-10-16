use std::{
    collections::BTreeMap,
    io::{Cursor, Write},
};

use deckcraft_model::{
    resolve::{self, Ctx},
    text::{Align, Bullet, RunKind},
    *,
};
use zip::{CompressionMethod, write::SimpleFileOptions};

use crate::{
    MIME, OdpError, Result,
    xml::{esc, header},
};

fn has_paragraph_background(ctx: &Ctx<'_>, shape: &Shape, body: &TextBody) -> bool {
    body.paragraphs.iter().any(|paragraph| resolve::para(ctx, shape, paragraph).background.is_some())
}

fn has_paragraph_line_height(ctx: &Ctx<'_>, shape: &Shape, body: &TextBody) -> bool {
    body.paragraphs.iter().any(|paragraph| resolve::para(ctx, shape, paragraph).line_height.is_some())
}

/// Write native ODF drawing objects. Unsupported objects fail before a file is written.
pub fn export(p: &Presentation) -> Result<Vec<u8>> {
    deckcraft_model::line_height::validate(p).map_err(|error| OdpError(error.into()))?;
    deckcraft_model::presentation_template::validate(p).map_err(|error| OdpError(error.to_string()))?;
    deckcraft_model::table::validate_constants(p).map_err(|error| OdpError(error.to_string()))?;
    if let Some((_, warning)) = p.tags.iter().find(|(key, _)| key == "odp:warning") {
        deckcraft_model::table::reject_constants(p).map_err(|error| OdpError(error.to_string()))?;
        return crate::preserve::export_text_edits(p)
            .map_err(|error| OdpError(format!("import retained unsupported source content ({warning}); {error}")));
    }
    crate::validate_source_diagnostics(p)?;
    let meta = metadata(&p.props)?;
    deckcraft_model::comments::validate(p, true).map_err(|e| OdpError(e.to_string()))?;
    let normalized_comments;
    let p = if p.slides.iter().any(|s| !s.comments.is_empty()) {
        let mut q = p.clone();
        deckcraft_model::comments::normalize(&mut q).map_err(|e| OdpError(e.to_string()))?;
        normalized_comments = q;
        &normalized_comments
    } else {
        p
    };
    let master_plan = masters_writer::MasterExportPlan::build(p)?;
    let templates = templates_writer::TemplateExportPlan::build(p)?;
    table_guard::validate(p, &master_plan)?;
    let mut writer = Writer {
        transform: deckcraft_geom::Affine::IDENTITY,
        owner: "slide0".into(),
        styles: String::new(),
        definitions: String::new(),
        objects: Vec::new(),
        notes_layouts: String::new(),
        notes_declarations: String::new(),
        next: 0,
        depth: 0,
        images: BTreeMap::new(),
        pres: p,
        comment_markers: BTreeMap::new(),
    };
    let size = p.slide_size;
    if !size.width.is_finite() || !size.height.is_finite() || size.width <= 0.0 || size.height <= 0.0 || size.width > 5000.0 || size.height > 5000.0 {
        return Err(OdpError("invalid slide dimensions".into()));
    }
    if p.slides.len() > 10_000 {
        return Err(OdpError("too many slides to export".into()));
    }
    let mut comment_state = comments_writer::State::new();
    if p.slides.iter().any(|s| !s.comments.is_empty()) {
        writer.styles.push_str(&format!("<style:style style:name=\"{}\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"none\" draw:stroke=\"none\"/></style:style>",comments_writer::holder_style()));
    }
    let mut body = String::new();
    for (i, slide) in p.slides.iter().enumerate() {
        let ctx = Ctx::for_slide(p, slide).ok_or_else(|| OdpError("slide has no master".into()))?;
        writer.owner = format!("slide{}", slide.id.0);
        let shared_master = master_plan.shares_master(slide);
        let (show_layout, show_master) = resolve::show_master_shapes(slide, ctx.layout);
        let bg = if shared_master && slide.background.is_none() && ctx.layout.is_none_or(|layout| layout.background.is_none()) {
            String::new()
        } else {
            let (bg, ph) = resolve::background(&ctx, Some(slide));
            writer.fill_attrs(&ctx, Some(&bg), ph, (size.width, size.height))?
        };
        let master_visibility = if shared_master { format!(" presentation:background-objects-visible=\"{show_master}\"") } else { String::new() };
        let transition = objects::transition_attrs(slide.transition.as_ref())?;
        let page_style = writer.style(
            "drawing-page",
            &format!(
                "<style:drawing-page-properties {bg} {transition}{master_visibility} presentation:visibility=\"{}\"/>",
                if slide.hidden { "hidden" } else { "visible" }
            ),
        );
        let name = if slide.name.is_empty() { format!("Slide {}", i + 1) } else { slide.name.clone() };
        if let Some(error) = p.notes_page_error(slide) {
            return Err(OdpError(error.into()));
        }
        let master_name = master_plan.master_name(slide)?;
        body.push_str(&format!(
            "<draw:page xml:id=\"{}\" draw:name=\"{}\" draw:style-name=\"{page_style}\" draw:master-page-name=\"{master_name}\"",
            timing::page_id(slide.id),
            esc(&name)
        ));
        templates.page_attrs(slide, &mut body)?;
        body.push('>');
        body.push_str("<draw:layer-set><draw:layer draw:name=\"Hidden\" draw:display=\"none\"/></draw:layer-set>");
        if show_master && !shared_master {
            let mc = Ctx::for_master(p, ctx.master);
            writer.with_owner(format!("master{}-slide{}", ctx.master.id.0, slide.id.0), |writer| {
                for s in &ctx.master.shapes {
                    if s.ph.is_none() {
                        writer.shape(&mc, s, &mut body)?;
                    }
                }
                Ok(())
            })?;
        }
        if show_layout && let Some(layout) = ctx.layout {
            let lc = Ctx::for_layout(p, ctx.master, layout);
            writer.with_owner(format!("layout{}-slide{}", layout.id.0, slide.id.0), |writer| {
                for s in &layout.shapes {
                    if s.ph.is_none() {
                        writer.shape(&lc, s, &mut body)?;
                    }
                }
                Ok(())
            })?;
        }
        let floating_comments = comment_state.page(&mut writer, &ctx, slide, i)?;
        for s in &slide.shapes {
            writer.shape(&ctx, s, &mut body)?;
        }
        if !writer.comment_markers.is_empty() {
            return Err(OdpError("annotation range owner was not emitted".into()));
        }
        for xml in floating_comments {
            body.push_str(&xml);
        }
        writer.timing(&ctx, slide, &mut body)?;
        writer.notes_page(slide, i + 1, &mut body)?;
        body.push_str("</draw:page>");
    }
    let mut master_pages = String::from("<draw:layer-set><draw:layer draw:name=\"Hidden\" draw:display=\"none\"/></draw:layer-set>");
    master_plan.emit(&mut writer, &mut master_pages)?;
    templates.emit(&mut writer.definitions)?;
    let body = format!("<office:body><office:presentation>{}{body}</office:presentation></office:body>", writer.notes_declarations);
    let content = format!(
        "{}<office:automatic-styles>{}</office:automatic-styles>{body}</office:document-content>",
        header("office:document-content"),
        writer.styles
    );
    let styles = format!(
        "{}<office:styles>{}</office:styles><office:automatic-styles>{}{}<style:page-layout style:name=\"Page\"><style:page-layout-properties fo:page-width=\"{}pt\" fo:page-height=\"{}pt\" style:print-orientation=\"{}\"/></style:page-layout></office:automatic-styles><office:master-styles>{master_pages}</office:master-styles></office:document-styles>",
        header("office:document-styles"),
        writer.definitions,
        writer.styles,
        writer.notes_layouts,
        size.width,
        size.height,
        if size.width >= size.height { "landscape" } else { "portrait" }
    );
    let comment_extension = comment_state.finish(p)?;
    let mut manifest = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{MIME}\" manifest:version=\"1.3\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>"
    );
    if content.len() > 32 * 1024 * 1024 || styles.len() > 32 * 1024 * 1024 {
        return Err(OdpError("export XML exceeds package part limit".into()));
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // ODF part 2 §3.3: first entry, stored, no extra field, exact ASCII MIME bytes.
    entry(&mut zip, "mimetype", MIME.as_bytes(), CompressionMethod::Stored)?;
    entry(&mut zip, "content.xml", content.as_bytes(), CompressionMethod::Deflated)?;
    entry(&mut zip, "styles.xml", styles.as_bytes(), CompressionMethod::Deflated)?;
    for (id, path) in writer.images {
        let media = p.media.iter().find(|m| m.id == id).ok_or_else(|| OdpError("missing image".into()))?;
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path=\"{path}\" manifest:media-type=\"{}\"/>", esc(&media.content_type)));
        if media.data.len() > 32 * 1024 * 1024 {
            return Err(OdpError("embedded media exceeds package part limit".into()));
        }
        entry(&mut zip, &path, &media.data, CompressionMethod::Deflated)?;
    }
    for (path, xml) in writer.objects {
        manifest.push_str(&format!("<manifest:file-entry manifest:full-path=\"{path}/\" manifest:media-type=\"application/vnd.oasis.opendocument.chart\"/><manifest:file-entry manifest:full-path=\"{path}/content.xml\" manifest:media-type=\"text/xml\"/>"));
        entry(&mut zip, &format!("{path}/content.xml"), xml.as_bytes(), CompressionMethod::Deflated)?;
    }
    if let Some(extension) = comment_extension {
        for binding in &extension.media {
            let media = p.media(binding.id).ok_or_else(|| OdpError("comment resource missing".into()))?;
            manifest.push_str(&format!(
                "<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{}\"/>",
                esc(&binding.part),
                esc(&binding.content_type)
            ));
            entry(&mut zip, &binding.part, &media.data, CompressionMethod::Deflated)?;
        }
        manifest.push_str(&format!(
            "<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{}\"/>",
            crate::comments_extension::PART,
            crate::comments_extension::MIME
        ));
        entry(&mut zip, crate::comments_extension::PART, &extension.encode().map_err(|e| OdpError(e.to_string()))?, CompressionMethod::Deflated)?;
    }
    manifest.push_str("<manifest:file-entry manifest:full-path=\"meta.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>");
    entry(&mut zip, "meta.xml", meta.as_bytes(), CompressionMethod::Deflated)?;
    entry(&mut zip, "META-INF/manifest.xml", manifest.as_bytes(), CompressionMethod::Deflated)?;
    Ok(zip.finish().map_err(|e| OdpError(e.to_string()))?.into_inner())
}

fn entry(zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>, name: &str, bytes: &[u8], method: CompressionMethod) -> Result<()> {
    zip.start_file(name, SimpleFileOptions::default().compression_method(method)).map_err(|e| OdpError(e.to_string()))?;
    zip.write_all(bytes).map_err(|e| OdpError(e.to_string()))
}

struct Writer<'a> {
    transform: deckcraft_geom::Affine,
    owner: String,
    styles: String,
    definitions: String,
    objects: Vec<(String, String)>,
    notes_layouts: String,
    notes_declarations: String,
    next: usize,
    depth: usize,
    images: BTreeMap<MediaId, String>,
    pres: &'a Presentation,
    comment_markers: BTreeMap<ShapeId, crate::comments_codec::Markers>,
}

impl Writer<'_> {
    fn shape_identity(&self, id: ShapeId) -> String {
        format!("{}-shape{}", self.owner, id.0)
    }

    fn with_owner<T>(&mut self, owner: String, run: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let previous = std::mem::replace(&mut self.owner, owner);
        let result = run(self);
        self.owner = previous;
        result
    }

    fn style(&mut self, family: &str, properties: &str) -> String {
        self.next += 1;
        let id = format!("S{}", self.next);
        self.styles.push_str(&format!("<style:style style:name=\"{id}\" style:family=\"{family}\">{properties}</style:style>"));
        id
    }

    fn shape(&mut self, ctx: &Ctx<'_>, s: &Shape, out: &mut String) -> Result<()> {
        if self.depth >= 64 || self.next >= 200_000 {
            return Err(OdpError("shape nesting exceeds limit".into()));
        }
        self.depth += 1;
        let result = self.shape_inner(ctx, s, out);
        self.depth -= 1;
        result
    }

    fn shape_inner(&mut self, ctx: &Ctx<'_>, s: &Shape, out: &mut String) -> Result<()> {
        if s.text.as_ref().is_some_and(|body| body.is_empty() && !has_paragraph_background(ctx, s, body) && !has_paragraph_line_height(ctx, s, body))
            && s.ph.is_some()
            && !self.comment_markers.contains_key(&s.id)
        {
            return Ok(());
        }
        if s.hover.is_some() {
            return Err(OdpError("mouse-over shape actions cannot be exported faithfully yet".into()));
        }
        let link = s.click.as_ref().map(|link| objects::link_href(self.pres, link)).transpose()?;
        if let Some(href) = &link {
            out.push_str(&format!("<draw:a xlink:href=\"{}\" xlink:type=\"simple\">", esc(href)));
        }
        let layer = if s.hidden { "draw:layer=\"Hidden\"" } else { "" };
        let x = resolve::xfrm(ctx, s);
        if !x.is_finite() || [x.x, x.y, x.w, x.h, x.rot].iter().any(|v| v.abs() > 1_000_000.0) || x.w < 0.0 || x.h < 0.0 {
            return Err(OdpError("invalid shape dimensions".into()));
        }
        if let ShapeKind::Group { children, child } = &s.kind {
            if child.w.abs() < 1e-12 || child.h.abs() < 1e-12 {
                return Err(OdpError("invalid group dimensions".into()));
            }
            let matrix = deckcraft_geom::group_child_affine(&x, child.rect());
            out.push_str(&format!("<draw:g {layer} xml:id=\"{}\" draw:name=\"{}\">", self.shape_identity(s.id), esc(&s.name)));
            let previous = self.transform;
            self.transform = previous * matrix;
            let result = children.iter().try_for_each(|child| self.shape(ctx, child, out));
            self.transform = previous;
            result?;
            out.push_str("</draw:g>");
            if link.is_some() {
                out.push_str("</draw:a>");
            }
            return Ok(());
        }
        let (fill, ph) = resolve::fill(ctx, s);
        let mut f = self.fill_attrs(ctx, fill.as_ref(), ph, (x.w, x.h))?;
        if let ShapeKind::Picture { fill } = &s.kind {
            f.push_str(&self.picture_attrs(fill)?);
        }
        let (line, ph) = resolve::line(ctx, s);
        let stroke = self.line_attrs(ctx, &line, ph)?;
        let effects = self.effect_attrs(ctx, s)?;
        let body = resolve::body(ctx, s);
        styles::finite(&[body.inset_l.unwrap_or(0.0), body.inset_r.unwrap_or(0.0), body.inset_t.unwrap_or(0.0), body.inset_b.unwrap_or(0.0)])?;
        let wrap = if body.wrap == Some(false) { "no-wrap" } else { "wrap" };
        let protect = if s.locked { " style:protect=\"position size\"" } else { "" };
        let (text_attrs, text_children) = self.text_body_style(&body)?;
        let style = self.style("graphic", &format!("<style:graphic-properties {f} {stroke} {effects} {text_attrs} fo:wrap-option=\"{wrap}\" {protect} fo:padding-left=\"{}pt\" fo:padding-right=\"{}pt\" fo:padding-top=\"{}pt\" fo:padding-bottom=\"{}pt\" draw:textarea-vertical-align=\"{}\">{text_children}</style:graphic-properties>", body.inset_l.unwrap_or(0.0), body.inset_r.unwrap_or(0.0), body.inset_t.unwrap_or(0.0), body.inset_b.unwrap_or(0.0), match body.anchor { Some(deckcraft_model::text::Anchor::Middle) => "middle", Some(deckcraft_model::text::Anchor::Bottom) => "bottom",Some(deckcraft_model::text::Anchor::Justified|deckcraft_model::text::Anchor::Distributed)=>"justify", _ => "top" }));
        let tag = match &s.kind {
            ShapeKind::Picture { .. } | ShapeKind::Table(_) | ShapeKind::Chart(_) | ShapeKind::Media(_) => "draw:frame",
            ShapeKind::Shape if s.text_box => "draw:frame",
            ShapeKind::Shape | ShapeKind::Connector { .. } => match s.geom.preset_name() {
                Some("rect") => "draw:rect",
                Some("ellipse") => "draw:ellipse",
                Some("line") => "draw:line",
                _ => "draw:custom-shape",
            },
            _ => return Err(OdpError(format!("{} objects cannot be exported yet; use PPTX or DeckCraft to keep them", s.kind_name()))),
        };
        let pos = if self.transform != deckcraft_geom::Affine::IDENTITY || x.rot.abs() > 1e-8 || x.flip_h || x.flip_v {
            if tag == "draw:line" {
                format!(
                    "svg:x1=\"0pt\" svg:y1=\"0pt\" svg:x2=\"{}pt\" svg:y2=\"{}pt\" draw:transform=\"{}\"",
                    x.w,
                    x.h,
                    crate::geometry::matrix_attr(self.transform * x.affine())
                )
            } else {
                format!(
                    "svg:x=\"0pt\" svg:y=\"0pt\" svg:width=\"{}pt\" svg:height=\"{}pt\" draw:transform=\"{}\"",
                    x.w,
                    x.h,
                    crate::geometry::matrix_attr(self.transform * x.affine())
                )
            }
        } else if tag == "draw:line" {
            format!("svg:x1=\"{}pt\" svg:y1=\"{}pt\" svg:x2=\"{}pt\" svg:y2=\"{}pt\"", x.x, x.y, x.x + x.w, x.y + x.h)
        } else {
            format!("svg:x=\"{}pt\" svg:y=\"{}pt\" svg:width=\"{}pt\" svg:height=\"{}pt\"", x.x, x.y, x.w, x.h)
        };
        out.push_str(&format!(
            "<{tag} {layer} xml:id=\"{}\" draw:name=\"{}\" draw:style-name=\"{style}\" {pos}>",
            self.shape_identity(s.id),
            esc(&s.name)
        ));
        if !s.title.is_empty() {
            out.push_str(&format!("<svg:title>{}</svg:title>", esc(&s.title)));
        }
        if !s.descr.is_empty() {
            out.push_str(&format!("<svg:desc>{}</svg:desc>", esc(&s.descr)));
        }
        if let ShapeKind::Picture { fill } = &s.kind {
            let path = self.image_path(fill.media)?;
            out.push_str(&format!("<draw:image xlink:href=\"{path}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>"));
        } else if let ShapeKind::Chart(chart) = &s.kind {
            let path = format!("Object{}", self.objects.len() + 1);
            let xml = self.chart(ctx, chart, x.w, x.h)?;
            self.objects.push((path.clone(), xml));
            out.push_str(&format!("<draw:object xlink:href=\"{path}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>"));
        } else if let ShapeKind::Media(clip) = &s.kind {
            self.media(clip, out)?;
        } else if let ShapeKind::Table(table) = &s.kind {
            self.table(ctx, table, out)?;
        } else if let Some(body) = &s.text {
            if tag == "draw:frame" {
                out.push_str("<draw:text-box>");
            }
            self.text_content(ctx, s, body, out)?;
            if tag == "draw:frame" {
                out.push_str("</draw:text-box>");
            }
        }
        if tag == "draw:custom-shape" {
            use deckcraft_geom::{Affine, BezPath, preset};
            let (name, paths, adj) = match &s.geom {
                Geom::Preset { name, adj } => {
                    if x.w <= 0.0 || x.h <= 0.0 || adj.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.0) {
                        return Err(OdpError("invalid preset dimensions or adjustments".into()));
                    }
                    let mut geometry = preset::build(name, x.w, x.h, adj).ok_or_else(|| OdpError(format!("unknown preset '{name}'")))?;
                    for p in &mut geometry.paths {
                        p.path = Affine::scale_non_uniform(1000.0 / x.w, 1000.0 / x.h) * p.path.clone();
                    }
                    (format!("ooxml-{name}"), geometry.paths, adj.iter().map(ToString::to_string).collect::<Vec<_>>().join(" "))
                }
                Geom::Custom { paths } => {
                    let mut out = Vec::new();
                    for p in paths {
                        if !p.w.is_finite() || !p.h.is_finite() || p.w <= 0.0 || p.h <= 0.0 || p.d.len() > 1_000_000 {
                            return Err(OdpError("invalid custom path dimensions".into()));
                        }
                        let path = BezPath::from_svg(&p.d).map_err(|e| OdpError(e.to_string()))?;
                        out.push(preset::SubPath {
                            path: Affine::scale_non_uniform(1000.0 / p.w, 1000.0 / p.h) * path,
                            fill: p.fill,
                            stroke: p.stroke,
                            even_odd: false,
                        });
                    }
                    ("non-primitive".into(), out, String::new())
                }
            };
            let path = paths
                .into_iter()
                .map(|p| {
                    format!("{} {} {} N", p.path.to_svg(), if p.fill == preset::FillMode::None { "F" } else { "" }, if p.stroke { "" } else { "S" })
                })
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!(
                "<draw:enhanced-geometry svg:viewBox=\"0 0 1000 1000\" draw:type=\"{}\" draw:modifiers=\"{adj}\" draw:enhanced-path=\"{}\"/>",
                esc(&name),
                esc(&path)
            ));
        }
        out.push_str(&format!("</{tag}>"));
        if link.is_some() {
            out.push_str("</draw:a>");
        }
        Ok(())
    }
}

fn text(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            ' ' => out.push_str("<text:s/>"),
            '\t' => out.push_str("<text:tab/>"),
            '\n' | '\u{b}' => out.push_str("<text:line-break/>"),
            c if c >= ' ' || c == '\r' => out.push_str(&esc(&c.to_string())),
            _ => {}
        }
    }
    out
}

#[path = "write_objects.rs"]
mod objects;
#[path = "write_styles.rs"]
mod styles;
use objects::metadata;
#[path = "write_comments.rs"]
pub(crate) mod comments_writer;

#[path = "write_timing.rs"]
mod timing;

#[path = "write_text.rs"]
mod text_writer;

#[path = "write_notes.rs"]
mod notes_writer;

#[path = "write_masters.rs"]
mod masters_writer;

#[path = "write_table_guard.rs"]
mod table_guard;
#[path = "write_table.rs"]
mod table_writer;
#[path = "write_templates.rs"]
mod templates_writer;
