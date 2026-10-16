//! Editable standard table projection with independently validated native semantics.
//! No cached speaker/cell text or original XML is an authoritative metadata value.
use super::{Exp, Out, shapes};
use crate::{
    PptxError,
    notes_projection::BodyStyle,
    opc,
    xml::{A, El, W},
};
use deckcraft_geom::Xfrm;
use deckcraft_model::{
    MediaId, Presentation, Shape, ShapeId, ShapeKind, SlideId, Table, TextBody,
    resolve::Ctx,
    table::{CellBlock, CellConstant, CellFormula, CellValue, FormulaOutcome, TablePath, TablePathStep},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
#[path = "table_codec.rs"]
mod codec;
#[path = "table_notes.rs"]
pub(crate) mod notes;

const MARKER: &str = "urn:deckcraft:pptx:pending-table:";
const MAX_BYTES: usize = 32 * 1024 * 1024;
fn invalid(message: impl std::fmt::Display) -> PptxError {
    PptxError::Unsupported(format!("table interchange: {message}"))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Formula {
    profile: String,
    expression: String,
    outcome: FormulaOutcome,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CellSemantic {
    path: TablePath,
    row: usize,
    col: usize,
    value: Option<CellValue>,
    formula: Option<Formula>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Body {
    path: TablePath,
    row: usize,
    col: usize,
    block: Option<u32>,
    frame: u32,
    cell: Option<(usize, usize)>,
    style: BodyStyle,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Media {
    wire: u32,
    target: String,
    external: bool,
    content_type: String,
    bytes: usize,
    digest: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Slide {
    wire: u32,
    part: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    projection: String,
    source: Shape,
    cells: Vec<CellSemantic>,
    bodies: Vec<Body>,
    media: Vec<Media>,
    slides: Vec<Slide>,
    ids: Vec<u32>,
    prefix: usize,
    counters: crate::notes_records::Counters,
    dependencies: Vec<crate::notes_records::Dependency>,
}

pub(crate) struct Pending {
    marker: String,
    part: crate::read::Part,
    core: El,
    record: Record,
    ids: HashMap<u32, ShapeId>,
}

pub(crate) fn enhanced(table: &Table) -> bool {
    table
        .rows
        .iter()
        .flat_map(|row| &row.cells)
        .any(|cell| cell.data.is_some() || cell.formula.is_some() || cell.number_format.is_some() || !cell.blocks.is_empty())
}

/// Whole-owner validation and accounting precede all projection clones/layout work.
pub(crate) fn check(p: &Presentation) -> Result<(), PptxError> {
    deckcraft_model::table::validate_constants(p).map_err(invalid)?;
    if p.media.len() > 100_000 {
        return Err(invalid("table media identity index exceeds package limits"));
    }
    let mut media_ids = HashSet::new();
    for media in &p.media {
        if !media_ids.insert(media.id) {
            return Err(invalid("ambiguous presentation media identity"));
        }
    }
    let mut budget = crate::notes_budget::Counter::new(MAX_BYTES);
    for master in
        p.masters.iter().chain(p.notes_master.iter()).chain(p.handout_master.iter()).chain(p.notes_masters.iter().map(|notes| &notes.master))
    {
        check_shapes(&master.shapes, &mut budget, 0)?;
        for layout in &master.layouts {
            check_shapes(&layout.shapes, &mut budget, 0)?;
        }
    }
    for slide in &p.slides {
        check_shapes(&slide.shapes, &mut budget, 0)?;
        if let Some(page) = &slide.notes_page {
            check_shapes(&page.shapes, &mut budget, 0)?;
        }
    }
    Ok(())
}
fn check_shapes(items: &[Shape], budget: &mut crate::notes_budget::Counter, depth: usize) -> Result<(), PptxError> {
    if depth > 64 {
        return Err(invalid("table owner group depth exceeds projection limit"));
    }
    for shape in items {
        if let ShapeKind::Table(table) = &shape.kind
            && enhanced(table)
        {
            table.validate_content().map_err(invalid)?;
            check_current_authority(table, 0)?;
            if shape.raw_ext.is_some() || shape.diagram.is_some() || shape.text.is_some() {
                return Err(invalid("enhanced table has an independent raw/text authority"));
            }
            if shape.xfrm.is_none_or(|x| [x.x, x.y, x.w, x.h, x.rot].iter().any(|v| !v.is_finite()) || x.w <= 0. || x.h <= 0.) {
                return Err(invalid("enhanced table requires finite positive explicit geometry"));
            }
            budget.value(shape)?;
        }
        check_shapes(shape.children(), budget, depth + 1)?;
    }
    Ok(())
}

fn check_current_authority(table: &Table, depth: usize) -> Result<(), PptxError> {
    if depth > deckcraft_model::table::MAX_NESTED_TABLE_DEPTH {
        return Err(invalid("nested table depth exceeds projection limit"));
    }
    for cell in table.rows.iter().flat_map(|row| &row.cells) {
        if matches!(cell.constant_state().map_err(invalid)?, deckcraft_model::table::CellConstantState::Stale)
            || matches!(cell.formula_state().map_err(invalid)?, deckcraft_model::table::CellFormulaState::Stale)
        {
            return Err(invalid("stale typed table display requires reconciliation before PPTX export"));
        }
        for block in &cell.blocks {
            if let CellBlock::Table(child) = block {
                check_current_authority(&child.table, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn number(part: &str, prefix: &str) -> Option<usize> {
    part.strip_prefix(prefix)?.strip_suffix(".xml")?.parse::<usize>().ok()?.checked_sub(1)
}
fn context<'a>(p: &'a Presentation, part: &str) -> Result<Ctx<'a>, PptxError> {
    if let Some(i) = number(part, "ppt/slides/slide") {
        return p.slides.get(i).and_then(|s| Ctx::for_slide(p, s)).ok_or_else(|| invalid("table slide context is missing"));
    }
    if let Some(i) = number(part, "ppt/notesSlides/notesSlide") {
        return p.slides.get(i).and_then(|s| Ctx::for_notes(p, s)).ok_or_else(|| invalid("table notes context is missing"));
    }
    if let Some(i) = number(part, "ppt/slideMasters/slideMaster") {
        return p.masters.get(i).map(|m| Ctx::for_master(p, m)).ok_or_else(|| invalid("table master context is missing"));
    }
    if let Some(i) = number(part, "ppt/slideLayouts/slideLayout") {
        return p
            .masters
            .iter()
            .flat_map(|m| m.layouts.iter().map(move |l| (m.as_ref(), l)))
            .nth(i)
            .map(|(m, l)| Ctx::for_layout(p, m, l))
            .ok_or_else(|| invalid("table layout context is missing"));
    }
    if part == "ppt/handoutMasters/handoutMaster1.xml" {
        return p.handout_master.as_deref().map(|m| Ctx::for_master(p, m)).ok_or_else(|| invalid("table handout context is missing"));
    }
    if part == "ppt/notesMasters/notesMaster1.xml" {
        return p.notes_master.as_deref().map(|m| Ctx::for_master(p, m)).ok_or_else(|| invalid("table notes master context is missing"));
    }
    Err(invalid("unknown enhanced table owner context"))
}

/// Intercept only enhanced native tables; ordinary children never recurse into this hook.
pub(crate) fn write(w: &mut W, x: &mut Exp<'_>, out: &mut Out, cx: &mut shapes::Ctx<'_>, source: &Shape, depth: usize) -> Result<(), PptxError> {
    let prefix = out.rels.items.len();
    let counters = crate::notes_records::Counters {
        image: x.n_image,
        media: x.n_media,
        chart: x.n_chart,
        diagram: x.n_diagram,
        tags: x.n_tags,
        chartex: x.n_chartex,
        kept: x.n_kept,
        placeholder: x.placeholder.clone(),
    };
    counters.validate()?;
    let ctx = context(x.p, &out.name)?;
    let root_id = cx.ids.get(source.id).ok_or_else(|| invalid("enhanced table has no owner-local identity"))?;
    let mut ids = Vec::new();
    let mut allocate = || {
        let id = cx.ids.fresh();
        cx.ids.map.insert(ShapeId(id), id);
        ids.push(id);
        Ok(id)
    };
    let (projection, mut bodies) = project(&ctx, source, root_id, &mut allocate)?;
    let mut too_deep = false;
    deckcraft_model::walk(std::slice::from_ref(&projection), &mut |_, nested| too_deep |= nested.saturating_add(depth) > 31);
    if too_deep {
        return Err(invalid("combined table/group projection depth exceeds DrawingML writer limit"));
    }
    let mut core = W::new();
    core.open("p:wrapper", A::new().a("xmlns:p", opc::NS_P).a("xmlns:a", opc::NS_A).a("xmlns:r", opc::NS_R));
    shapes::shape(&mut core, x, out, cx, &projection, depth);
    core.close("p:wrapper");
    let doc = crate::xml::parse(&core.finish())?;
    let mut root = doc.root.elements().next().cloned().ok_or_else(|| invalid("table projection is missing"))?;
    let mut skeleton = source.clone();
    skeleton.id = ShapeId(0);
    let mut cells = Vec::new();
    let ShapeKind::Table(table) = &mut skeleton.kind else {
        return Err(invalid("enhanced source is not a table"));
    };
    skeleton_table(table, &TablePath::default(), &mut cells, 0)?;
    let (media, slides) = normalize(x, out, &mut skeleton, &mut bodies)?;
    let seeds = media_seeds(&media);
    let dependencies = projection_dependencies(x, out, prefix, &seeds)?;
    let record = Record {
        projection: codec::projection(&enrich(&root))?,
        source: skeleton,
        cells,
        bodies,
        media,
        slides,
        ids,
        prefix,
        counters,
        dependencies,
    };
    // The metadata has one root shape/table skeleton and text-free style records.
    append(&mut root, &record)?;
    w.raw(&root.to_xml());
    Ok(())
}

fn skeleton_table(table: &mut Table, path: &TablePath, cells: &mut Vec<CellSemantic>, depth: usize) -> Result<(), PptxError> {
    if depth > 16 {
        return Err(invalid("nested table depth limit"));
    }
    for (row, values) in table.rows.iter_mut().enumerate() {
        for (col, cell) in values.cells.iter_mut().enumerate() {
            if cell.data.is_some() || cell.formula.is_some() {
                cells.push(CellSemantic {
                    path: path.clone(),
                    row,
                    col,
                    value: cell.data.as_ref().map(|d| d.value().clone()),
                    formula: cell.formula.as_ref().map(|f| Formula {
                        profile: f.profile().into(),
                        expression: f.expression().into(),
                        outcome: f.outcome().clone(),
                    }),
                });
            }
            cell.data = None;
            cell.formula = None;
            clear(&mut cell.text);
            for block in &mut cell.blocks {
                match block {
                    CellBlock::Text(block) => clear(&mut block.body),
                    CellBlock::Table(child) => {
                        let mut path = path.clone();
                        path.0.push(TablePathStep { row, col, child: child.id });
                        skeleton_table(&mut child.table, &path, cells, depth + 1)?;
                    }
                }
            }
        }
    }
    Ok(())
}
fn clear(body: &mut TextBody) {
    for p in &mut body.paragraphs {
        for r in &mut p.runs {
            r.text.clear();
        }
    }
}

fn normalize(x: &mut Exp<'_>, out: &Out, source: &mut Shape, bodies: &mut [Body]) -> Result<(Vec<Media>, Vec<Slide>), PptxError> {
    let mut media_ids = Vec::new();
    let mut seen = HashSet::new();
    crate::notes_media::visit::shapes(std::slice::from_ref(source), &mut |id| {
        if seen.insert(*id) {
            media_ids.push(*id);
        }
    });
    for body in &*bodies {
        crate::notes_media::visit::text(&body.style.skeleton, &mut |id| {
            if seen.insert(*id) {
                media_ids.push(*id);
            }
        });
    }
    let mut media = Vec::new();
    let mut mapping = HashMap::new();
    for (i, id) in media_ids.into_iter().enumerate() {
        let wire = u32::try_from(i + 1).map_err(invalid)?;
        let item = x.p.media(id).ok_or_else(|| invalid("table media is missing"))?;
        if item.content_type == "application/vnd.deckcraft.odp-source" {
            return Err(invalid("original source bytes cannot be embedded as table media"));
        }
        if item.data.len() > MAX_BYTES {
            return Err(invalid("table media exceeds byte limit"));
        }
        let content_type = item.content_type.clone();
        let bytes = item.data.len();
        let digest = if let Some(hash) = x.table_hashes.get(&id) {
            hash.clone()
        } else {
            let hash = codec::digest(&item.data);
            x.table_hashes.insert(id, hash.clone());
            hash
        };
        let (target, external) = if item.data.is_empty() {
            (item.link.clone().ok_or_else(|| invalid("table media has no bytes or link"))?, true)
        } else {
            (x.media_parts.get(&id).cloned().ok_or_else(|| invalid("table has a media dependency without a visible core projection"))?, false)
        };
        if !out.rels.items.iter().any(|(_, _, path, ext)| {
            *ext == external && if external { path == &target } else { opc::norm(&opc::resolve(&opc::dir_of(&out.name), path)) == opc::norm(&target) }
        }) {
            return Err(invalid("table media has no ordinary core relationship"));
        }
        mapping.insert(id, MediaId(wire));
        media.push(Media { wire, target, external, content_type, bytes, digest });
    }
    crate::notes_media::visit_mut::shapes(std::slice::from_mut(source), &mut |id| {
        if let Some(wire) = mapping.get(id) {
            *id = *wire;
        }
    });
    for body in &mut *bodies {
        crate::notes_media::visit_mut::text(&mut body.style.skeleton, &mut |id| {
            if let Some(wire) = mapping.get(id) {
                *id = *wire;
            }
        });
    }
    let mut slides = Vec::new();
    let mut slide_map = HashMap::new();
    let mut referenced = HashSet::new();
    collect_slide_ids(source, &mut referenced)?;
    for body in &*bodies {
        collect_slide_ids(&body.style.skeleton, &mut referenced)?;
    }
    for old in referenced {
        let part = x.slide_parts.get(&SlideId(old)).ok_or_else(|| invalid("table-linked slide is missing"))?;
        slide_map.insert(old, old);
        slides.push(Slide { wire: old, part: part.clone() });
    }
    slides.sort_by(|a, b| a.part.cmp(&b.part));
    // Stable wire ordinals follow actual standard slide part names, never native ids.
    slide_map.clear();
    for (i, binding) in slides.iter_mut().enumerate() {
        slide_map.insert(binding.wire, u32::try_from(i + 1).map_err(invalid)?);
        binding.wire = u32::try_from(i + 1).map_err(invalid)?;
    }
    crate::notes_projection::remap_slide_links(source, &slide_map)?;
    for body in bodies {
        crate::notes_projection::remap_slide_links(&mut body.style.skeleton, &slide_map)?;
    }
    Ok((media, slides))
}

fn collect_slide_ids<T: Serialize>(value: &T, out: &mut HashSet<u32>) -> Result<(), PptxError> {
    fn visit(value: &serde_json::Value, out: &mut HashSet<u32>, depth: usize) -> Result<(), PptxError> {
        if depth > 512 {
            return Err(invalid("table link depth limit"));
        }
        match value {
            serde_json::Value::Object(items) => {
                if items.get("kind").and_then(|v| v.as_str()) == Some("slide")
                    && let Some(id) = items.get("slide").and_then(|v| v.as_u64()).and_then(|v| u32::try_from(v).ok())
                {
                    out.insert(id);
                }
                for value in items.values() {
                    visit(value, out, depth + 1)?;
                }
            }
            serde_json::Value::Array(items) => {
                for value in items {
                    visit(value, out, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    visit(&serde_json::to_value(value).map_err(invalid)?, out, 0)
}

fn append(root: &mut El, record: &Record) -> Result<(), PptxError> {
    let mut w = W::new();
    w.open("p:wrapper", A::new().a("xmlns:p", opc::NS_P));
    codec::write_record(&mut w, "table", record)?;
    w.close("p:wrapper");
    root.children.extend(crate::xml::parse(&w.finish())?.root.children);
    Ok(())
}

fn validate_styles(p: &Presentation, table: &Table, depth: usize) -> Result<(), PptxError> {
    if depth > 16 {
        return Err(invalid("table style depth limit"));
    }
    let known = deckcraft_model::table::table_styles().iter().any(|(id, _)| id == &table.style);
    let definitions: Vec<_> = p.table_styles.iter().filter(|d| d.id.eq_ignore_ascii_case(&table.style)).collect();
    if definitions.len() > 1 || (!known && definitions.is_empty()) {
        return Err(invalid("enhanced table has an unknown or ambiguous table-style definition"));
    }
    for cell in table.rows.iter().flat_map(|row| &row.cells) {
        for block in &cell.blocks {
            if let CellBlock::Table(child) = block {
                validate_styles(p, &child.table, depth + 1)?;
            }
        }
    }
    Ok(())
}

struct CachedFields;
impl deckcraft_text::Fields for CachedFields {
    fn field(&self, _: &str) -> Option<String> {
        None
    }
}

fn project(
    ctx: &Ctx<'_>,
    source: &Shape,
    root_id: u32,
    allocate: &mut impl FnMut() -> Result<u32, PptxError>,
) -> Result<(Shape, Vec<Body>), PptxError> {
    let ShapeKind::Table(table) = &source.kind else {
        return Err(invalid("projection source is not a table"));
    };
    let original = source
        .xfrm
        .filter(|x| x.is_finite() && x.w > 0.0 && x.h > 0.0)
        .ok_or_else(|| invalid("enhanced table requires finite positive explicit geometry"))?;
    validate_styles(ctx.pres, table, 0)?;
    let plan = deckcraft_text::table_content::layout(ctx, table, &CachedFields).map_err(invalid)?;
    let mut bodies = Vec::new();
    let mut shape = source.clone();
    shape.id = ShapeId(root_id);
    shape.raw_ext = None;
    if !table.rows.iter().flat_map(|r| &r.cells).any(|c| !c.blocks.is_empty()) {
        // PresentationML graphic frames have an unrotated/unflipped placement in
        // the ordinary writer. A standard outer group carries the real transform
        // while its inner table remains an ordinary editable DrawingML table.
        // The native table paints intrinsic cells under original.affine(); its
        // box controls the transform pivot without scaling those cells. A group
        // with identical outer/child extents reproduces that affine exactly.
        let transformed = original.rot != 0.0 || original.flip_h || original.flip_v || original.w != table.width() || original.h != plan.height();
        let frame = if transformed { allocate()? } else { root_id };
        let mut projected = table.clone();
        for (r, row) in projected.rows.iter_mut().enumerate() {
            if let Some(h) = plan.heights.get(r) {
                row.height = *h;
            }
            for (c, cell) in row.cells.iter_mut().enumerate() {
                let original = table.cell(r, c).ok_or_else(|| invalid("table cell disappeared"))?;
                cell.text = deckcraft_text::table::cell_body(ctx, table, r, c, original);
                bodies.push(Body {
                    path: TablePath::default(),
                    row: r,
                    col: c,
                    block: None,
                    frame,
                    cell: Some((r, c)),
                    style: BodyStyle::new(&original.text, &cell.text)?,
                });
                cell.data = None;
                cell.formula = None;
                cell.number_format = None;
            }
        }
        if transformed {
            let width = table.width();
            let height = plan.height();
            let inner =
                Shape { id: ShapeId(frame), xfrm: Some(Xfrm::new(0., 0., width, height)), kind: ShapeKind::Table(projected), ..Default::default() };
            shape.kind = ShapeKind::Group { children: vec![inner], child: Xfrm::new(0., 0., original.w, original.h) };
        } else {
            shape.kind = ShapeKind::Table(projected);
        }
    } else {
        let mut children = Vec::new();
        project_nested(ctx, table, &plan, &TablePath::default(), &mut children, &mut bodies, allocate, 0)?;
        // Children retain intrinsic layout coordinates, including overflow. Keep
        // unit child-space mapping and the original rotation/flip pivot.
        shape.kind = ShapeKind::Group { children, child: Xfrm::new(0., 0., original.w, original.h) };
    }
    Ok((shape, bodies))
}

#[allow(clippy::too_many_arguments)]
fn project_nested(
    ctx: &Ctx<'_>,
    table: &Table,
    plan: &deckcraft_text::table_content::TableLayout,
    path: &TablePath,
    children: &mut Vec<Shape>,
    bodies: &mut Vec<Body>,
    allocate: &mut impl FnMut() -> Result<u32, PptxError>,
    depth: usize,
) -> Result<(), PptxError> {
    if depth > 16 {
        return Err(invalid("nested projection depth limit"));
    }
    let width: f64 = table.cols.iter().sum();
    let height = plan.height();
    let frame = allocate()?;
    let mut grid = table.clone();
    for (r, row) in grid.rows.iter_mut().enumerate() {
        if let Some(h) = plan.heights.get(r) {
            row.height = *h;
        }
        for (c, cell) in row.cells.iter_mut().enumerate() {
            cell.blocks.clear();
            cell.data = None;
            cell.formula = None;
            cell.number_format = None;
            let original = table.cell(r, c).ok_or_else(|| invalid("nested grid cell disappeared"))?;
            if original.blocks.is_empty() {
                cell.text = deckcraft_text::table::cell_body(ctx, table, r, c, original);
                bodies.push(Body {
                    path: path.clone(),
                    row: r,
                    col: c,
                    block: None,
                    frame,
                    cell: Some((r, c)),
                    style: BodyStyle::new(&original.text, &cell.text)?,
                });
            } else {
                cell.text = TextBody::default();
            }
        }
    }
    children.push(Shape { id: ShapeId(frame), xfrm: Some(Xfrm::new(0., 0., width, height)), kind: ShapeKind::Table(grid), ..Default::default() });
    for cell in &plan.cells {
        for block in &cell.blocks {
            match block {
                deckcraft_text::table_content::BlockLayout::Text { id, body, rect } => {
                    let original =
                        table.cell(cell.row, cell.col).and_then(|c| c.text_block(*id)).ok_or_else(|| invalid("ordered cell body disappeared"))?;
                    if table.cell(cell.row, cell.col).is_some_and(|c| c.blocks.is_empty()) {
                        continue;
                    }
                    let frame = allocate()?;
                    children.push(Shape {
                        id: ShapeId(frame),
                        xfrm: Some(Xfrm::new(rect.x0, rect.y0, rect.width(), rect.height())),
                        text: Some(body.clone()),
                        fill: Some(deckcraft_model::Fill::None),
                        line: Some(deckcraft_model::Line::none()),
                        ..Default::default()
                    });
                    bodies.push(Body {
                        path: path.clone(),
                        row: cell.row,
                        col: cell.col,
                        block: *id,
                        frame,
                        cell: None,
                        style: BodyStyle::new(original, body)?,
                    });
                }
                deckcraft_text::table_content::BlockLayout::Table { id, transform, layout } => {
                    let original = table
                        .cell(cell.row, cell.col)
                        .and_then(|c| {
                            c.blocks.iter().find_map(|b| match b {
                                CellBlock::Table(t) if t.id == *id => Some(t),
                                _ => None,
                            })
                        })
                        .ok_or_else(|| invalid("nested table disappeared"))?;
                    let mut nested = Vec::new();
                    let mut child_path = path.clone();
                    child_path.0.push(TablePathStep { row: cell.row, col: cell.col, child: *id });
                    project_nested(ctx, &original.table, layout, &child_path, &mut nested, bodies, allocate, depth + 1)?;
                    let affine = transform.as_coeffs();
                    let w: f64 = original.table.cols.iter().sum();
                    let h = layout.height();
                    let group = allocate()?;
                    children.push(Shape {
                        id: ShapeId(group),
                        xfrm: Some(Xfrm::new(affine[4], affine[5], w * affine[0], h * affine[3])),
                        kind: ShapeKind::Group { children: nested, child: Xfrm::new(0., 0., w, h) },
                        ..Default::default()
                    });
                }
            }
        }
    }
    Ok(())
}

/// Recognized semantic envelopes are held unpublished until all core context exists.
pub(crate) fn capture(
    imp: &mut crate::read::Imp<'_>,
    part: &crate::read::Part,
    ids: &HashMap<u32, ShapeId>,
    root: &El,
    shape: &mut Shape,
) -> Result<(), PptxError> {
    let mut enriched = root.clone();
    for (key, value) in &part.ns {
        if !enriched.attrs.iter().any(|(name, _)| name == key) {
            enriched.attrs.push((key.clone(), value.clone()));
        }
    }
    let parsed = codec::read_record::<Record>(&enriched, "table")?;
    let Some(record) = parsed else {
        if root.children_named("extLst").flat_map(|list| list.elements()).any(|ext| ext.attr("uri") == Some(codec::URI)) {
            return Err(invalid("table extension namespace or owner was changed"));
        }
        return Ok(());
    };
    if imp.table_pending.len() >= 10_000 {
        return Err(invalid("table projection count exceeds limit"));
    }
    crate::notes_budget::Counter::new(MAX_BYTES).value(&record)?;
    if record.ids.len() > 20_000
        || record.bodies.len() > 100_000
        || record.cells.len() > 100_000
        || record.media.len() > 10_000
        || record.slides.len() > 10_000
        || record.prefix > 50_000
        || record.dependencies.len() > 50_000
    {
        return Err(invalid("table record collection limit"));
    }
    let bytes = root.to_xml().len();
    imp.table_bytes = imp
        .table_bytes
        .checked_add(bytes)
        .filter(|bytes| *bytes <= MAX_BYTES)
        .ok_or_else(|| invalid("aggregate table projection metadata exceeds byte limit"))?;
    let marker = format!("{MARKER}{}", imp.alloc());
    let mut used = HashSet::new();
    for id in &record.ids {
        if *id < 2 || !used.insert(*id) {
            return Err(invalid("invalid generated table shape identity"));
        }
    }
    shape.raw_ext = Some(marker.clone());
    let referenced: HashSet<_> = record.bodies.iter().map(|body| body.frame).collect();
    imp.table_pending.push(Pending {
        marker,
        part: crate::read::Part { name: part.name.clone(), rels: part.rels.clone(), ns: part.ns.clone(), inner_ns: part.inner_ns.clone() },
        core: root.clone(),
        ids: ids.iter().filter(|(file, _)| referenced.contains(*file)).map(|(file, id)| (*file, *id)).collect(),
        record,
    });
    Ok(())
}
fn enrich(root: &El) -> El {
    let mut root = root.clone();
    for (key, value) in [("xmlns:p", opc::NS_P), ("xmlns:a", opc::NS_A), ("xmlns:r", opc::NS_R)] {
        if !root.attrs.iter().any(|(name, _)| name == key) {
            root.attrs.push((key.into(), value.into()));
        }
    }
    root
}
fn marker(shape: &Shape) -> Option<&str> {
    shape.raw_ext.as_deref().filter(|value| value.starts_with(MARKER))
}

/// Restore atomically from current core text only, then validate the calculation graph.
/// This is also called after ordinary notes pages have supplied their own style contexts.
pub(crate) fn restore_all(imp: &mut crate::read::Imp<'_>, p: &mut Presentation) -> Result<(), PptxError> {
    if let Some(error) = imp.error.take() {
        return Err(error);
    }
    if imp.table_pending.is_empty() {
        return Ok(());
    }
    // The reader publishes its media vector at the end; regeneration needs the
    // already-read current core identities now, without copying any payload bytes.
    p.media = imp.media.clone();
    let mut replacements = HashMap::new();
    let mut current = HashMap::new();
    visit_owners(p, &mut |shape| {
        if let Some(marker) = marker(shape) {
            current.insert(marker.to_string(), shape.clone());
        }
    });
    let mut unresolved = Vec::new();
    for pending in std::mem::take(&mut imp.table_pending) {
        let Some(actual) = current.get(&pending.marker) else {
            unresolved.push(pending);
            continue;
        };
        let restored = restore(imp, p, &pending, actual)?;
        replacements.insert(pending.marker.clone(), restored);
    }
    apply_owners(p, &mut replacements)?;
    imp.table_pending = unresolved;
    deckcraft_model::table::validate_constants(p).map_err(invalid)?;
    Ok(())
}

fn restore(imp: &mut crate::read::Imp<'_>, p: &Presentation, pending: &Pending, actual: &Shape) -> Result<Shape, PptxError> {
    let record = &pending.record;
    record.counters.validate()?;
    if record.projection != codec::projection(&enrich(&pending.core))? {
        return Err(invalid("editable core table was changed; typed metadata is stale"));
    }
    let mut source = record.source.clone();
    if source.id != ShapeId(0) || source.raw_ext.is_some() || source.text.is_some() || source.diagram.is_some() {
        return Err(invalid("invalid source table envelope"));
    }
    let ShapeKind::Table(table) = &mut source.kind else {
        return Err(invalid("semantic envelope source is not a table"));
    };
    table.validate_content().map_err(invalid)?;
    validate_skeleton(table, 0)?;
    let mut body_keys = HashSet::new();
    for binding in &record.bodies {
        let key = serde_json::to_string(&(binding.path.clone(), binding.row, binding.col, binding.block)).map_err(invalid)?;
        if !body_keys.insert(key) {
            return Err(invalid("duplicate canonical table body binding"));
        }
        let id = pending.ids.get(&binding.frame).ok_or_else(|| invalid("table body frame was deleted"))?;
        let frame = deckcraft_model::find_shape(std::slice::from_ref(actual), *id).ok_or_else(|| invalid("table body frame is outside its owner"))?;
        let core = if let Some((row, col)) = binding.cell {
            let ShapeKind::Table(table) = &frame.kind else {
                return Err(invalid("table body frame changed kind"));
            };
            &table.cell(row, col).ok_or_else(|| invalid("table body cell was deleted"))?.text
        } else {
            frame.text.as_ref().ok_or_else(|| invalid("table continuation text was deleted"))?
        };
        let cell = table
            .at_path_mut(&binding.path)
            .map_err(invalid)?
            .cell_mut(binding.row, binding.col)
            .ok_or_else(|| invalid("native table body address is invalid"))?;
        let body = cell.text_block_mut(binding.block).ok_or_else(|| invalid("native ordered table body is missing"))?;
        if body.paragraphs.iter().flat_map(|p| &p.runs).any(|r| !r.text.is_empty()) {
            return Err(invalid("table skeleton contains a second text authority"));
        }
        *body = binding.style.restore(core)?;
    }
    let mut cell_keys = HashSet::new();
    for semantic in &record.cells {
        let key = serde_json::to_string(&(semantic.path.clone(), semantic.row, semantic.col)).map_err(invalid)?;
        if !cell_keys.insert(key) || (semantic.value.is_some() == semantic.formula.is_some()) {
            return Err(invalid("duplicate or competing typed table cell authority"));
        }
        let cell = table
            .at_path_mut(&semantic.path)
            .map_err(invalid)?
            .cell_mut(semantic.row, semantic.col)
            .ok_or_else(|| invalid("typed table cell was deleted"))?;
        if let Some(value) = &semantic.value {
            cell.data = Some(Arc::new(CellConstant::from_display(value.clone(), &cell.text).map_err(invalid)?));
        }
        if let Some(formula) = &semantic.formula {
            cell.formula = Some(Arc::new(
                CellFormula::from_profile_display(&formula.profile, formula.expression.clone(), formula.outcome.clone(), &cell.text)
                    .map_err(invalid)?,
            ));
        }
    }
    // Every text node of the source table must be bound exactly once unless it was
    // an originally empty block with no standard drawing text frame.
    let mut text_count = 0usize;
    table
        .visit_texts(&mut |body| {
            if body.paragraphs.iter().flat_map(|p| &p.runs).any(|r| !r.text.is_empty()) {
                text_count += 1;
            }
            Ok(())
        })
        .map_err(invalid)?;
    if text_count > record.bodies.len() {
        return Err(invalid("unbound native table text"));
    }
    let mut media_map = HashMap::new();
    let mut media_parts = HashMap::new();
    for binding in &record.media {
        if binding.wire == 0 || media_map.contains_key(&MediaId(binding.wire)) {
            return Err(invalid("duplicate table media wire identity"));
        }
        let relation = pending
            .part
            .rels
            .items
            .iter()
            .find(|r| {
                r.external == binding.external
                    && if r.external { r.target == binding.target } else { opc::norm(&r.target) == opc::norm(&binding.target) }
            })
            .ok_or_else(|| invalid("table media relationship was deleted or changed"))?;
        let id = if binding.external {
            imp.media.iter().find(|m| m.link.as_deref() == Some(relation.target.as_str())).map(|m| m.id)
        } else {
            imp.media_by_part.get(&opc::norm(&relation.target)).copied()
        }
        .ok_or_else(|| invalid("table media has no current core value"))?;
        let item = imp.media.iter().find(|item| item.id == id).ok_or_else(|| invalid("current table media is missing"))?;
        let hash = if let Some(hash) = imp.table_hashes.get(&id) {
            hash.clone()
        } else {
            let hash = codec::digest(&item.data);
            imp.table_hashes.insert(id, hash.clone());
            hash
        };
        if item.data.len() != binding.bytes || hash != binding.digest || (!binding.external && item.content_type != binding.content_type) {
            return Err(invalid("table media dependency was edited"));
        }
        if binding.external
            && let Some(item) = imp.media.iter_mut().find(|item| item.id == id)
        {
            item.content_type.clone_from(&binding.content_type);
        }
        media_map.insert(MediaId(binding.wire), id);
        if !binding.external {
            media_parts.insert(id, relation.target.clone());
        }
    }
    let mut missing = false;
    crate::notes_media::visit_mut::shapes(std::slice::from_mut(&mut source), &mut |id| {
        if let Some(mapped) = media_map.get(id) {
            *id = *mapped;
        } else {
            missing = true;
        }
    });
    if missing {
        return Err(invalid("unbound table media identity"));
    }
    let mut slide_map = HashMap::new();
    let mut slide_parts = HashMap::new();
    for binding in &record.slides {
        let id = imp.slide_by_part.get(&opc::norm(&binding.part)).copied().ok_or_else(|| invalid("table-linked slide was deleted"))?;
        if slide_map.insert(binding.wire, id.0).is_some() {
            return Err(invalid("duplicate table slide wire identity"));
        }
        slide_parts.insert(id, binding.part.clone());
    }
    crate::notes_projection::remap_slide_links(&mut source, &slide_map)?;
    let file_id = pending.core.find("cNvPr").and_then(|e| e.u32("id")).ok_or_else(|| invalid("table core owner has no shape identity"))?;
    source.id = ShapeId(file_id);
    let ctx = context(p, &pending.part.name)?;
    let mut ids = record.ids.iter();
    let mut allocate = || ids.next().copied().ok_or_else(|| invalid("table generated object set changed"));
    let (projection, _) = project(&ctx, &source, file_id, &mut allocate)?;
    if ids.next().is_some() {
        return Err(invalid("table generated object set has unused identities"));
    }
    if record.prefix > pending.part.rels.items.len() {
        return Err(invalid("table relationship prefix was deleted"));
    }
    let mut out = Out::new(&pending.part.name);
    out.rels.items =
        pending.part.rels.items.iter().take(record.prefix).map(|r| (r.id.clone(), r.ty.clone(), r.target_raw.clone(), r.external)).collect();
    // Media witnesses here have already been compared with the actual imported
    // payload above. Never use a stored dependency hash as regeneration authority.
    let mut seeds = media_seeds(&record.media);
    if let Some(part) = &record.counters.placeholder {
        let bytes = crate::png::transparent_1x1();
        seeds.insert(
            opc::norm(part),
            crate::notes_records::Dependency {
                part: part.clone(),
                content_type: "image/png".into(),
                bytes: bytes.len(),
                sha256: codec::digest(&bytes),
            },
        );
    }
    validate_dependencies(imp.pkg, &record.dependencies, &media_seeds(&record.media))?;
    let counters = &record.counters;
    let mut x = Exp {
        p,
        pkg: Default::default(),
        media_parts,
        n_image: counters.image,
        n_media: counters.media,
        n_chart: counters.chart,
        n_diagram: counters.diagram,
        n_tags: counters.tags,
        n_chartex: counters.chartex,
        n_kept: counters.kept,
        slide_parts,
        table_styles: Vec::new(),
        placeholder: counters.placeholder.clone(),
        error: None,
        table_hashes: Default::default(),
    };
    let mut map = shapes::IdMap::build(std::slice::from_ref(&projection));
    let mut cx = shapes::Ctx { ids: &mut map, written: Default::default() };
    let mut w = W::new();
    w.open("p:wrapper", A::new().a("xmlns:p", opc::NS_P).a("xmlns:a", opc::NS_A).a("xmlns:r", opc::NS_R));
    shapes::shape(&mut w, &mut x, &mut out, &mut cx, &projection, 0);
    w.close("p:wrapper");
    if let Some(error) = x.error.take() {
        return Err(error);
    }
    let doc = crate::xml::parse(&w.finish())?;
    let expected = doc.root.elements().next().ok_or_else(|| invalid("expected table projection is empty"))?;
    if codec::projection(&enrich(expected))? != codec::projection(&enrich(&pending.core))? {
        return Err(invalid("native table metadata does not reproduce the current editable core projection; it is stale"));
    }
    // Newly derived relationship targets must be current standard relationships.
    for (id, ty, target, external) in &out.rels.items {
        if !pending.part.rels.items.iter().any(|r| r.id == *id && r.ty == *ty && r.target_raw == *target && r.external == *external) {
            return Err(invalid("table projected relationship changed"));
        }
    }
    let expected_dependencies = projection_dependencies(&x, &out, record.prefix, &seeds)?;
    if expected_dependencies != record.dependencies {
        return Err(invalid("native table metadata does not reproduce the actual generated dependencies"));
    }
    source.id = actual.id;
    Ok(source)
}

fn validate_skeleton(table: &Table, depth: usize) -> Result<(), PptxError> {
    if depth > 16 {
        return Err(invalid("table skeleton nesting limit"));
    }
    for cell in table.rows.iter().flat_map(|r| &r.cells) {
        if cell.data.is_some() || cell.formula.is_some() {
            return Err(invalid("source skeleton contains a second typed cache authority"));
        }
        if cell.text.paragraphs.iter().flat_map(|p| &p.runs).any(|r| !r.text.is_empty()) {
            return Err(invalid("source skeleton contains canonical cell text"));
        }
        for block in &cell.blocks {
            match block {
                CellBlock::Text(block) => {
                    if block.body.paragraphs.iter().flat_map(|p| &p.runs).any(|r| !r.text.is_empty()) {
                        return Err(invalid("source skeleton contains canonical continuation text"));
                    }
                }
                CellBlock::Table(child) => validate_skeleton(&child.table, depth + 1)?,
            }
        }
    }
    Ok(())
}

fn visit_owners(p: &Presentation, visit: &mut impl FnMut(&Shape)) {
    for master in p.masters.iter().chain(p.notes_master.iter()).chain(p.handout_master.iter()).chain(p.notes_masters.iter().map(|n| &n.master)) {
        deckcraft_model::walk(&master.shapes, &mut |s, _| visit(s));
        for layout in &master.layouts {
            deckcraft_model::walk(&layout.shapes, &mut |s, _| visit(s));
        }
    }
    for slide in &p.slides {
        deckcraft_model::walk(&slide.shapes, &mut |s, _| visit(s));
        if let Some(page) = &slide.notes_page {
            deckcraft_model::walk(&page.shapes, &mut |s, _| visit(s));
        }
    }
}
fn apply_owners(p: &mut Presentation, replacements: &mut HashMap<String, Shape>) -> Result<(), PptxError> {
    fn apply(shapes: &mut [Shape], replacements: &HashMap<String, Shape>, depth: usize) -> Result<(), PptxError> {
        if depth > 32 {
            return Err(invalid("table restoration group depth limit"));
        }
        for shape in shapes {
            if let Some(next) = marker(shape).and_then(|m| replacements.get(m)) {
                *shape = next.clone();
            } else if let Some(children) = shape.children_mut() {
                apply(children, replacements, depth + 1)?;
            }
        }
        Ok(())
    }
    for master in p
        .masters
        .iter_mut()
        .chain(p.notes_master.iter_mut())
        .chain(p.handout_master.iter_mut())
        .chain(p.notes_masters.iter_mut().map(|n| &mut n.master))
    {
        let master = Arc::make_mut(master);
        apply(&mut master.shapes, replacements, 0)?;
        for layout in &mut master.layouts {
            apply(&mut layout.shapes, replacements, 0)?;
        }
    }
    for slide in &mut p.slides {
        let slide = Arc::make_mut(slide);
        apply(&mut slide.shapes, replacements, 0)?;
        if let Some(page) = &mut slide.notes_page {
            apply(&mut page.shapes, replacements, 0)?;
        }
    }
    Ok(())
}

/// Only relationships created by this projection belong to its generated closure.
/// Earlier unrelated tags/charts are preserved in the relationship prefix, but do
/// not become trusted seed payloads for fresh regeneration.
fn projection_dependencies(
    x: &Exp<'_>,
    out: &Out,
    prefix: usize,
    seeds: &HashMap<String, crate::notes_records::Dependency>,
) -> Result<Vec<crate::notes_records::Dependency>, PptxError> {
    let mut own = Out::new(&out.name);
    own.rels.items = out.rels.items.get(prefix..).ok_or_else(|| invalid("table relationship prefix is invalid"))?.to_vec();
    super::notes::dependency_closure(x, &own, Some(seeds))
}
fn media_seeds(media: &[Media]) -> HashMap<String, crate::notes_records::Dependency> {
    media
        .iter()
        .filter(|binding| !binding.external)
        .map(|binding| {
            (
                opc::norm(&binding.target),
                crate::notes_records::Dependency {
                    part: binding.target.clone(),
                    content_type: binding.content_type.clone(),
                    bytes: binding.bytes,
                    sha256: binding.digest.clone(),
                },
            )
        })
        .collect()
}
fn validate_dependencies(
    pkg: &opc::Package,
    dependencies: &[crate::notes_records::Dependency],
    seeds: &HashMap<String, crate::notes_records::Dependency>,
) -> Result<(), PptxError> {
    let mut seen = HashSet::new();
    let mut total = 0usize;
    for expected in dependencies {
        let key = opc::norm(&expected.part);
        if !seen.insert(key.clone()) {
            return Err(invalid("duplicate table generated dependency"));
        }
        total =
            total.checked_add(expected.bytes).filter(|n| *n <= 128 * 1024 * 1024).ok_or_else(|| invalid("table generated dependency byte limit"))?;
        let bytes = pkg.get(&expected.part).ok_or_else(|| invalid("table generated dependency was deleted"))?;
        if bytes.len() != expected.bytes || pkg.content_type(&expected.part).as_deref() != Some(expected.content_type.as_str()) {
            return Err(invalid("table generated dependency changed"));
        }
        // These seeds contain only live core media validated above. Generated
        // placeholder/tag/other side parts always hash the actual package bytes.
        let hash = if let Some(seed) = seeds.get(&key).filter(|seed| seed == &expected) { seed.sha256.clone() } else { codec::digest(bytes) };
        if hash != expected.sha256 {
            return Err(invalid("table generated dependency changed"));
        }
    }
    Ok(())
}
