//! Native whole-slide notes dependency closure. Captured owners are immutable snapshots;
//! names, numeric coincidences and equal destination resources never establish ownership.
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};

use deckcraft_model::{
    HeaderFooter, LayoutId, Master, MasterId, MediaId, MediaItem, Presentation, Shape, ShapeId, ShapeKind, Slide, SlideId,
    notes::{NotesFields, NotesMaster},
    text::{Action, Hyperlink, ListStyle, ParaProps, RunKind, RunProps, TextBody},
};
use serde::{Serialize, ser};

use crate::{EngineError, Result, media_refs};

const MAX_OWNERS: usize = 4096;
const MAX_SHAPES: usize = 100_000;
const MAX_IDS: usize = 2_000_000;
const MAX_MEDIA: usize = 10_000;
const MAX_DEST_MEDIA: usize = 100_000;
const MAX_ITEM_BYTES: usize = 32 * 1024 * 1024;
const MAX_MEDIA_BYTES: usize = 128 * 1024 * 1024;
const MAX_METADATA: usize = 32 * 1024 * 1024;
const MAX_DEST_METADATA: usize = 64 * 1024 * 1024;
const SOURCE_MIME: &str = "application/vnd.deckcraft.odp-source";

fn invalid(reason: impl fmt::Display) -> EngineError {
    EngineError::Other(format!("native notes transfer refused: {reason}"))
}

#[derive(Clone, Debug)]
pub(crate) struct NotesClosure {
    defaults: ListStyle,
    header_footer: HeaderFooter,
    slides: Vec<Arc<Slide>>,
    slide_index: HashMap<SlideId, usize>,
    layouts: HashMap<LayoutId, usize>,
    // Per captured slide, an index into owners. The full slide snapshot binds this reference.
    bindings: Vec<Option<usize>>,
    owners: Vec<NotesMaster>,
    regular: Vec<Arc<Master>>,
    media: Vec<MediaItem>,
    metadata: usize,
}

// A nonallocating serializer counts all copied strings/vectors/fields and rejects nonfinite
// floats, including ones nested in paint/geometry/theme data. Topology is checked first.
#[derive(Debug)]
struct CountError(String);
impl fmt::Display for CountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for CountError {}
impl ser::Error for CountError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}
struct Counter {
    bytes: usize,
    limit: usize,
}
impl Counter {
    fn add(&mut self, amount: usize) -> std::result::Result<(), CountError> {
        self.bytes = self.bytes.checked_add(amount).ok_or_else(|| CountError("metadata accounting overflow".into()))?;
        if self.bytes > self.limit {
            return Err(CountError("metadata budget exceeded".into()));
        }
        Ok(())
    }
    fn value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        value.serialize(&mut *self).map_err(invalid)
    }
    fn vector<T>(&mut self, len: usize) -> Result<Vec<T>> {
        self.add(len.checked_mul(std::mem::size_of::<T>().max(32)).ok_or_else(|| invalid("index overflow"))?).map_err(invalid)?;
        let mut result = Vec::new();
        result.try_reserve_exact(len).map_err(|_| invalid("cannot allocate closure index"))?;
        Ok(result)
    }
}
macro_rules! scalar {
    ($($method:ident($ty:ty)),* $(,)?) => {$ (
        fn $method(self, _: $ty) -> std::result::Result<(), CountError> { self.add(16) }
    )*};
}
impl ser::Serializer for &mut Counter {
    type Ok = ();
    type Error = CountError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;
    scalar!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_char(char)
    );
    fn serialize_f32(self, value: f32) -> std::result::Result<(), CountError> {
        self.serialize_f64(f64::from(value))
    }
    fn serialize_f64(self, value: f64) -> std::result::Result<(), CountError> {
        if !value.is_finite() {
            return Err(CountError("nonfinite model value".into()));
        }
        self.add(16)
    }
    fn serialize_str(self, value: &str) -> std::result::Result<(), CountError> {
        self.add(value.len().saturating_add(32))
    }
    fn serialize_bytes(self, value: &[u8]) -> std::result::Result<(), CountError> {
        self.add(value.len().saturating_add(32))
    }
    fn serialize_none(self) -> std::result::Result<(), CountError> {
        self.add(8)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> std::result::Result<(), CountError> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> std::result::Result<(), CountError> {
        self.add(8)
    }
    fn serialize_unit_struct(self, _: &'static str) -> std::result::Result<(), CountError> {
        self.add(8)
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> std::result::Result<(), CountError> {
        self.add(16)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> std::result::Result<(), CountError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> std::result::Result<(), CountError> {
        self.add(16)?;
        value.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> std::result::Result<Self, CountError> {
        self.add(32)?;
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> std::result::Result<Self, CountError> {
        self.add(32)?;
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> std::result::Result<Self, CountError> {
        self.add(32)?;
        Ok(self)
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> std::result::Result<Self, CountError> {
        self.add(32)?;
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> std::result::Result<Self, CountError> {
        self.add(32)?;
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> std::result::Result<Self, CountError> {
        self.add(32)?;
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> std::result::Result<Self, CountError> {
        self.add(32)?;
        Ok(self)
    }
}
macro_rules! sequence {
    ($trait:ident, $method:ident) => {
        impl ser::$trait for &mut Counter {
            type Ok = ();
            type Error = CountError;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> std::result::Result<(), CountError> {
                self.add(16)?;
                value.serialize(&mut **self)
            }
            fn end(self) -> std::result::Result<(), CountError> {
                Ok(())
            }
        }
    };
}
sequence!(SerializeSeq, serialize_element);
sequence!(SerializeTuple, serialize_element);
sequence!(SerializeTupleStruct, serialize_field);
sequence!(SerializeTupleVariant, serialize_field);
impl ser::SerializeMap for &mut Counter {
    type Ok = ();
    type Error = CountError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, value: &T) -> std::result::Result<(), CountError> {
        self.add(16)?;
        value.serialize(&mut **self)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> std::result::Result<(), CountError> {
        value.serialize(&mut **self)
    }
    fn end(self) -> std::result::Result<(), CountError> {
        Ok(())
    }
}
macro_rules! structure {
    ($trait:ident) => {
        impl ser::$trait for &mut Counter {
            type Ok = ();
            type Error = CountError;
            fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, value: &T) -> std::result::Result<(), CountError> {
                self.add(32)?;
                value.serialize(&mut **self)
            }
            fn end(self) -> std::result::Result<(), CountError> {
                Ok(())
            }
        }
    };
}
structure!(SerializeStruct);
structure!(SerializeStructVariant);

fn native(source: &Presentation) -> Result<()> {
    if source.tags.len() > 100_000 || source.media.len() > MAX_DEST_MEDIA {
        return Err(invalid("document ownership index limit exceeded"));
    }
    if source.tags.iter().any(|(key, _)| key.starts_with("odp:")) || source.media.iter().any(|media| media.content_type == SOURCE_MIME) {
        return Err(invalid("ODP provenance or retained source attachments are outside the native notes subset"));
    }
    if !source.embedded_fonts.is_empty() {
        return Err(invalid("embedded fonts are outside the native notes subset"));
    }
    Ok(())
}
fn dependency_free_defaults(style: &ListStyle) -> Result<()> {
    let mut media = false;
    media_refs::visit::list(style, &mut |_| media = true);
    if media {
        return Err(invalid("document default text style must be dependency-free"));
    }
    Ok(())
}
fn tags(tags: &[(String, String)]) -> Result<()> {
    if tags.iter().any(|(key, _)| key.starts_with("odp:")) {
        return Err(invalid("selected owner carries ODP provenance"));
    }
    Ok(())
}
fn hyperlink(link: &Option<Hyperlink>) -> Result<()> {
    if let Some(link) = link
        && (!matches!(link.action, Action::Url { .. }) || link.zoom_return.is_some())
    {
        return Err(invalid("only URL hyperlinks are supported; internal navigation and program actions need a separate closure"));
    }
    Ok(())
}
fn run(props: &RunProps) -> Result<()> {
    hyperlink(&props.link)?;
    hyperlink(&props.hover)
}
fn para(props: &ParaProps) -> Result<()> {
    if props.line_height.is_some_and(|policy| !policy.valid()) || (props.line_height.is_some() && props.line_spacing.is_some()) {
        return Err(invalid("invalid or conflicting paragraph line-height policy"));
    }
    Ok(())
}
fn list(style: &ListStyle) -> Result<()> {
    for level in style.levels.iter().flatten() {
        para(&level.para)?;
        run(&level.run)?;
    }
    Ok(())
}
fn text(body: &TextBody) -> Result<()> {
    list(&body.list_style)?;
    for paragraph in &body.paragraphs {
        para(&paragraph.props)?;
        run(&paragraph.def_props)?;
        run(&paragraph.end_props)?;
        for item in &paragraph.runs {
            run(&item.props)?;
            match &item.kind {
                RunKind::Text | RunKind::Break => {}
                RunKind::Field { field } if matches!(field.as_str(), "slidenum" | "header" | "footer" | "notesdatetime") => {}
                _ => return Err(invalid("unsupported notes/text field or inline math in selected owner")),
            }
        }
    }
    Ok(())
}
fn tree(shapes: &[Shape], depth: usize, count: &mut usize, ids: &mut HashSet<ShapeId>) -> Result<()> {
    if depth > 64 {
        return Err(invalid("shape nesting exceeds 64"));
    }
    for shape in shapes {
        *count = count.checked_add(1).ok_or_else(|| invalid("shape count overflow"))?;
        if *count > MAX_SHAPES {
            return Err(invalid("selected shape count exceeds 100000"));
        }
        ids.try_reserve(1).map_err(|_| invalid("cannot allocate shape index"))?;
        if !ids.insert(shape.id) {
            return Err(invalid("duplicate shape identity inside one owner"));
        }
        if shape.raw_ext.is_some() || shape.diagram.is_some() || shape.effects.as_ref().is_some_and(|effects| effects.raw3d.is_some()) {
            return Err(invalid("raw extensions and diagram payloads are outside the native notes subset"));
        }
        match &shape.kind {
            ShapeKind::Shape | ShapeKind::Picture { .. } | ShapeKind::Connector { .. } => {}
            ShapeKind::Group { children, .. } => {
                if depth == 64 && !children.is_empty() {
                    return Err(invalid("shape nesting exceeds 64"));
                }
                if !children.is_empty() {
                    tree(children, depth + 1, count, ids)?;
                }
            }
            _ => return Err(invalid("selected owner supports only shapes, pictures, connectors and groups")),
        }
    }
    Ok(())
}
fn connectors(shapes: &[Shape], ids: &HashSet<ShapeId>) -> Result<()> {
    for shape in shapes {
        if let ShapeKind::Connector { start, end } = &shape.kind {
            for (id, _) in [start, end].into_iter().flatten() {
                if !ids.contains(id) {
                    return Err(invalid("connector endpoint is outside its owning shape tree"));
                }
            }
        }
        connectors(shape.children(), ids)?;
    }
    Ok(())
}
fn shape_profile(shapes: &[Shape]) -> Result<()> {
    for shape in shapes {
        tags(&shape.tags)?;
        hyperlink(&shape.click)?;
        hyperlink(&shape.hover)?;
        if let Some(body) = &shape.text {
            text(body)?;
        }
        shape_profile(shape.children())?;
    }
    Ok(())
}
fn owner_tree(shapes: &[Shape], count: &mut usize, budget: &mut Counter) -> Result<HashSet<ShapeId>> {
    let mut ids = HashSet::new();
    tree(shapes, 0, count, &mut ids)?;
    budget.value(shapes)?;
    shape_profile(shapes)?;
    budget.add(ids.len().checked_mul(96).ok_or_else(|| invalid("shape index overflow"))?).map_err(invalid)?;
    connectors(shapes, &ids)?;
    Ok(ids)
}
fn master(master: &Master, notes: bool, count: &mut usize, budget: &mut Counter) -> Result<()> {
    if master.raw_ext.is_some() || master.theme.raw_extra.is_some() {
        return Err(invalid("raw master or theme extensions are unsupported"));
    }
    budget.value(&master.theme)?;
    if master.theme.format.effects.iter().any(|effect| effect.raw3d.is_some()) {
        return Err(invalid("raw theme effects are unsupported"));
    }
    if notes && !master.layouts.is_empty() {
        return Err(invalid("notes-master layouts are outside the native notes subset"));
    }
    owner_tree(&master.shapes, count, budget)?;
    if notes {
        let mut bodies = 0usize;
        deckcraft_model::walk(&master.shapes, &mut |shape, _| bodies += usize::from(shape.ph_type() == Some(deckcraft_model::PhType::Body)));
        if bodies > 1 {
            return Err(invalid("notes master has multiple primary body frames"));
        }
    }
    budget.value(&master.title_style)?;
    budget.value(&master.body_style)?;
    budget.value(&master.other_style)?;
    list(&master.title_style)?;
    list(&master.body_style)?;
    list(&master.other_style)?;
    for layout in &master.layouts {
        if layout.raw_ext.is_some() {
            return Err(invalid("raw layout extensions are unsupported"));
        }
        owner_tree(&layout.shapes, count, budget)?;
    }
    budget.value(master)
}

/// Capture before cloning clipboard slides. No source presentation or file is needed afterward.
fn capture_native(source: &Presentation, selected: &[&Slide], preflight_bytes: usize) -> Result<Option<NotesClosure>> {
    if selected.len() > crate::transfer_policy::MAX_POLICY_PAGES {
        return Err(invalid("too many selected pages"));
    }
    if !selected.iter().any(|slide| slide.notes_page.is_some()) {
        return Ok(None);
    }
    if source.tags.len() <= 100_000 && source.tags.iter().any(|(key, _)| matches!(key.as_str(), "odp:warning" | "pptx:comment-warning")) {
        return Err(invalid("source has unattributed ODP loss; selected dependencies cannot yet be transferred safely"));
    }
    native(source)?;
    deckcraft_model::comments::validate(source, true).map_err(|e| invalid(e.to_string()))?;
    if source.notes_masters.len() + usize::from(source.notes_master.is_some()) > MAX_OWNERS {
        return Err(invalid("too many source notes owners"));
    }
    let mut budget = Counter { bytes: preflight_bytes, limit: MAX_METADATA };
    let template_stats = deckcraft_model::presentation_template::validate(source).map_err(|error| invalid(error.to_string()))?;
    budget.add(template_stats.logical_bytes.checked_mul(3).ok_or_else(|| invalid("template staging accounting overflow"))?).map_err(invalid)?;
    budget.value(&source.default_text_style)?;
    dependency_free_defaults(&source.default_text_style)?;
    budget.value(&source.header_footer)?;
    list(&source.default_text_style)?;
    if !deckcraft_model::notes::valid_size(source.notes_size) || !deckcraft_model::notes::valid_size(source.slide_size) {
        return Err(invalid("invalid source page size"));
    }
    let mut index = HashMap::new();
    index.try_reserve(source.notes_masters.len() + 1).map_err(|_| invalid("cannot allocate notes owner index"))?;
    budget.add((source.notes_masters.len() + 1).checked_mul(96).ok_or_else(|| invalid("owner index overflow"))?).map_err(invalid)?;
    for (at, owner) in source.notes_masters.iter().enumerate() {
        if index.insert(owner.master.id, at).is_some() {
            return Err(invalid("duplicate notes master identity"));
        }
    }
    let legacy = source.notes_masters.len();
    if let Some(master) = &source.notes_master
        && index.insert(master.id, legacy).is_some()
    {
        return Err(invalid("ambiguous legacy notes master identity"));
    }
    let mut owners = budget.vector::<NotesMaster>(selected.len().min(MAX_OWNERS))?;
    let mut bindings = budget.vector::<Option<usize>>(selected.len())?;
    let mut slides = budget.vector::<Arc<Slide>>(selected.len())?;
    let mut regular = budget.vector::<Arc<Master>>(source.masters.len().min(MAX_OWNERS))?;
    let mut copied = HashMap::new();
    copied.try_reserve(index.len()).map_err(|_| invalid("cannot allocate selected owner index"))?;
    if source.masters.len() > MAX_OWNERS {
        return Err(invalid("too many source regular masters"));
    }
    let mut regular_layouts = HashMap::new();
    let mut regular_master_ids = HashSet::new();
    regular_master_ids.try_reserve(source.masters.len()).map_err(|_| invalid("cannot allocate regular master index"))?;
    let mut layout_count = 0usize;
    for (at, owner) in source.masters.iter().enumerate() {
        if !regular_master_ids.insert(owner.id) {
            return Err(invalid("duplicate source regular master identity"));
        }
        for layout in &owner.layouts {
            layout_count = layout_count.checked_add(1).ok_or_else(|| invalid("regular layout count overflow"))?;
            if layout_count > MAX_SHAPES {
                return Err(invalid("too many source regular layouts"));
            }
            budget.add(96).map_err(invalid)?;
            regular_layouts.try_reserve(1).map_err(|_| invalid("cannot allocate regular layout index"))?;
            if regular_layouts.insert(layout.id, at).is_some() {
                return Err(invalid("ambiguous source regular layout identity"));
            }
        }
    }
    let mut regular_ids = HashSet::new();
    regular_ids.try_reserve(source.masters.len()).map_err(|_| invalid("cannot allocate selected regular index"))?;
    let mut slide_ids = HashSet::new();
    slide_ids.try_reserve(selected.len()).map_err(|_| invalid("cannot allocate selected page index"))?;
    let mut shape_count = 0usize;
    for slide in selected {
        if !slide_ids.insert(slide.id) {
            return Err(invalid("duplicate selected slide identity"));
        }
        budget.value(&slide.tags)?;
        tags(&slide.tags)?;
        if slide.raw_ext.is_some() || slide.transition.as_ref().is_some_and(|transition| transition.raw.is_some()) {
            return Err(invalid("raw slide extensions are outside the native notes subset"));
        }
        let shape_ids = owner_tree(&slide.shapes, &mut shape_count, &mut budget)?;
        budget.value(&slide.animations)?;
        for animation in &slide.animations {
            if !shape_ids.contains(&animation.shape) || animation.trigger.is_some_and(|id| !shape_ids.contains(&id)) {
                return Err(invalid("animation target or trigger is outside its slide"));
            }
        }
        budget.value(&slide.notes)?;
        text(&slide.notes)?;
        let binding = if let Some(page) = &slide.notes_page {
            owner_tree(&page.shapes, &mut shape_count, &mut budget)?;
            if page.size.is_some_and(|size| !deckcraft_model::notes::valid_size(size)) || !deckcraft_model::notes::valid_fields(&page.fields) {
                return Err(invalid("invalid notes page size or fields"));
            }
            let key = match page.master {
                Some(id) => *index.get(&id).ok_or_else(|| invalid("notes page uses a missing master"))?,
                None if source.notes_master.is_some() => legacy,
                None => return Err(invalid("structured notes require a real source notes master; no empty master is synthesized")),
            };
            let owner = if let Some(at) = copied.get(&key) {
                *at
            } else {
                let snapshot = if key == legacy {
                    NotesMaster {
                        master: source.notes_master.clone().ok_or_else(|| invalid("missing legacy master"))?,
                        size: source.notes_size,
                        fields: NotesFields::default(),
                    }
                } else {
                    let owner = source.notes_masters.get(key).ok_or_else(|| invalid("missing notes owner"))?;
                    budget.value(&owner.fields)?;
                    owner.clone()
                };
                if !deckcraft_model::notes::valid_size(snapshot.size) || !deckcraft_model::notes::valid_fields(&snapshot.fields) {
                    return Err(invalid("invalid notes master size or fields"));
                }
                master(&snapshot.master, true, &mut shape_count, &mut budget)?;
                budget.value(&snapshot.fields)?;
                let at = owners.len();
                owners.push(snapshot);
                copied.insert(key, at);
                at
            };
            let local = page
                .body_shape
                .map(|body| deckcraft_model::find_shape(&page.shapes, body).ok_or_else(|| invalid("notes body frame is missing")))
                .transpose()?;
            if let Some(frame) = local
                && (frame.text.is_some()
                    || !matches!(frame.kind, ShapeKind::Shape)
                    || frame.ph_type().is_some_and(|kind| kind != deckcraft_model::PhType::Body))
            {
                return Err(invalid("invalid primary notes body frame or duplicate canonical text"));
            }
            let mut bodies = 0usize;
            deckcraft_model::walk(&page.shapes, &mut |shape, _| bodies += usize::from(shape.ph_type() == Some(deckcraft_model::PhType::Body)));
            if bodies > 1 {
                return Err(invalid("notes page has multiple primary body frames"));
            }
            Some(owner)
        } else {
            None
        };
        // Reserve metadata for the proof snapshot, clipboard clone and eventual staging clone.
        let before = budget.bytes;
        budget.value(*slide)?;
        let payload = budget.bytes - before;
        budget.add(payload.checked_mul(2).ok_or_else(|| invalid("snapshot accounting overflow"))?).map_err(invalid)?;
        slides.push(Arc::new((*slide).clone()));
        bindings.push(binding);
        let at = *regular_layouts.get(&slide.layout).ok_or_else(|| invalid("selected slide uses a missing regular layout"))?;
        let owner = source.masters.get(at).ok_or_else(|| invalid("missing regular master"))?;
        if regular_ids.insert(owner.id) {
            if regular.len() >= MAX_OWNERS {
                return Err(invalid("too many regular masters"));
            }
            master(owner, false, &mut shape_count, &mut budget)?;
            regular.push(owner.clone());
        }
    }
    let mut referenced = HashSet::new();
    referenced.try_reserve(MAX_MEDIA).map_err(|_| invalid("cannot allocate media reference index"))?;
    let mut reference_error = None;
    let mut reference = |id: &MediaId| {
        if referenced.len() >= MAX_MEDIA && !referenced.contains(id) {
            reference_error = Some(invalid("too many selected embedded media items"));
        } else {
            referenced.insert(*id);
        }
    };
    for slide in selected {
        media_refs::visit::slide(slide, &mut reference);
    }
    for owner in &owners {
        media_refs::visit::master(&owner.master, &mut reference);
    }
    for owner in &regular {
        media_refs::visit::master(owner, &mut reference);
    }
    if let Some(error) = reference_error {
        return Err(error);
    }
    budget.add(referenced.len().checked_mul(128).ok_or_else(|| invalid("media index overflow"))?).map_err(invalid)?;
    let mut media = budget.vector::<MediaItem>(referenced.len())?;
    let mut media_ids = HashSet::new();
    media_ids.try_reserve(source.media.len()).map_err(|_| invalid("cannot allocate source media index"))?;
    budget.add(source.media.len().checked_mul(64).ok_or_else(|| invalid("source media index overflow"))?).map_err(invalid)?;
    let mut payload = 0usize;
    for item in &source.media {
        if !media_ids.insert(item.id) {
            return Err(invalid("duplicate source media identity"));
        }
        if !referenced.remove(&item.id) {
            continue;
        }
        if item.link.is_some() || item.data.is_empty() {
            return Err(invalid("selected media must have nonempty embedded bytes and no external link"));
        }
        if item.data.len() > MAX_ITEM_BYTES {
            return Err(invalid("selected embedded media exceeds 32 MiB"));
        }
        payload = payload.checked_add(item.data.len()).ok_or_else(|| invalid("media payload overflow"))?;
        if payload > MAX_MEDIA_BYTES {
            return Err(invalid("selected media payload exceeds 128 MiB"));
        }
        budget.value(&item.name)?;
        budget.value(&item.content_type)?;
        media.push(item.clone());
    }
    if !referenced.is_empty() {
        return Err(invalid("selected media reference is missing"));
    }
    let mut slide_index = HashMap::new();
    slide_index.try_reserve(slides.len()).map_err(|_| invalid("cannot allocate immutable page index"))?;
    for (at, slide) in slides.iter().enumerate() {
        slide_index.insert(slide.id, at);
    }
    let mut layouts = HashMap::new();
    for (at, owner) in regular.iter().enumerate() {
        layouts.try_reserve(owner.layouts.len()).map_err(|_| invalid("cannot allocate captured layout index"))?;
        for layout in &owner.layouts {
            layouts.insert(layout.id, at);
        }
    }
    budget.add((slide_index.len() + layouts.len()).checked_mul(96).ok_or_else(|| invalid("captured index overflow"))?).map_err(invalid)?;
    Ok(Some(NotesClosure {
        defaults: source.default_text_style.clone(),
        header_footer: source.header_footer.clone(),
        slides,
        slide_index,
        layouts,
        bindings,
        owners,
        regular,
        media,
        metadata: budget.bytes,
    }))
}

/// A validated same-document-only copy has no authority to cross an origin boundary.
#[derive(Debug, Default)]
pub(crate) struct Capture {
    pub(crate) closure: Option<NotesClosure>,
    pub(crate) same_document_only: Option<&'static str>,
}

fn comment_topology(comments: &[deckcraft_model::Comment], depth: usize, count: &mut usize) -> Result<()> {
    if depth > 64 {
        return Err(invalid("comment reply nesting exceeds 64"));
    }
    for comment in comments {
        *count = count.checked_add(1).ok_or_else(|| invalid("comment count overflow"))?;
        if *count > MAX_SHAPES {
            return Err(invalid("comment count exceeds 100000"));
        }
        if !comment.replies.is_empty() {
            comment_topology(&comment.replies, depth + 1, count)?;
        }
    }
    Ok(())
}
fn general_tree(shapes: &[Shape], depth: usize, count: &mut usize, ids: &mut HashSet<ShapeId>) -> Result<()> {
    if depth > 64 {
        return Err(invalid("shape nesting exceeds 64"));
    }
    for shape in shapes {
        *count = count.checked_add(1).ok_or_else(|| invalid("shape count overflow"))?;
        if *count > MAX_SHAPES {
            return Err(invalid("selected shape count exceeds 100000"));
        }
        ids.try_reserve(1).map_err(|_| invalid("cannot allocate general shape index"))?;
        if !ids.insert(shape.id) {
            return Err(invalid("duplicate shape identity inside one owner"));
        }
        if !shape.children().is_empty() {
            general_tree(shape.children(), depth + 1, count, ids)?;
        }
    }
    Ok(())
}
fn valid_text(body: &TextBody) -> Result<()> {
    for level in body.list_style.levels.iter().flatten() {
        para(&level.para)?;
    }
    for paragraph in &body.paragraphs {
        para(&paragraph.props)?;
    }
    Ok(())
}
fn general_shapes(shapes: &[Shape], count: &mut usize, budget: &mut Counter) -> Result<()> {
    let mut ids = HashSet::new();
    general_tree(shapes, 0, count, &mut ids)?;
    budget.value(shapes)?;
    budget.add(ids.len().checked_mul(96).ok_or_else(|| invalid("general shape index overflow"))?).map_err(invalid)?;
    connectors(shapes, &ids)?;
    fn valid(shapes: &[Shape]) -> Result<()> {
        for shape in shapes {
            if let Some(body) = &shape.text {
                valid_text(body)?;
            }
            if let ShapeKind::Table(table) = &shape.kind {
                for row in &table.rows {
                    for cell in &row.cells {
                        valid_text(&cell.text)?;
                    }
                }
            }
            valid(shape.children())?;
        }
        Ok(())
    }
    valid(shapes)
}
fn general_master(master: &Master, count: &mut usize, budget: &mut Counter) -> Result<()> {
    general_shapes(&master.shapes, count, budget)?;
    for layout in &master.layouts {
        general_shapes(&layout.shapes, count, budget)?;
    }
    budget.value(master)?;
    for style in [&master.title_style, &master.body_style, &master.other_style] {
        for level in style.levels.iter().flatten() {
            para(&level.para)?;
        }
    }
    Ok(())
}
fn link_reason(link: &Option<Hyperlink>) -> Option<&'static str> {
    link.as_ref()
        .filter(|link| !matches!(link.action, Action::Url { .. }) || link.zoom_return.is_some())
        .map(|_| "internal navigation and program actions need a separate notes dependency closure")
}
fn run_reason(props: &RunProps) -> Option<&'static str> {
    link_reason(&props.link).or_else(|| link_reason(&props.hover))
}
fn list_reason(style: &ListStyle) -> Option<&'static str> {
    style.levels.iter().flatten().find_map(|level| run_reason(&level.run))
}
fn text_reason(body: &TextBody) -> Option<&'static str> {
    list_reason(&body.list_style).or_else(|| {
        body.paragraphs.iter().find_map(|paragraph| {
            run_reason(&paragraph.def_props).or_else(|| run_reason(&paragraph.end_props)).or_else(|| {
                paragraph.runs.iter().find_map(|run| {
                    run_reason(&run.props).or(match &run.kind {
                        RunKind::Text | RunKind::Break => None,
                        RunKind::Field { field } if matches!(field.as_str(), "header" | "footer" | "notesdatetime" | "slidenum") => None,
                        _ => Some("unsupported notes/text field or inline math in selected owner"),
                    })
                })
            })
        })
    })
}
fn effects_reason(effects: &deckcraft_model::style::Effects) -> Option<&'static str> {
    if effects.raw3d.is_some() {
        return Some("raw 3-D effects are outside the native notes subset");
    }
    if effects.fill_overlay.as_ref().is_some_and(|overlay| {
        !matches!(overlay.fill, deckcraft_model::Fill::None | deckcraft_model::Fill::Solid { .. } | deckcraft_model::Fill::Gradient(_))
    }) {
        return Some("non-solid/gradient effect overlays are outside the native notes subset");
    }
    None
}
fn shapes_reason(shapes: &[Shape]) -> Option<&'static str> {
    for shape in shapes {
        if !matches!(shape.kind, ShapeKind::Shape | ShapeKind::Picture { .. } | ShapeKind::Connector { .. } | ShapeKind::Group { .. }) {
            return Some("selected owner supports only shapes, pictures, connectors and groups");
        }
        if shape.raw_ext.is_some() || shape.diagram.is_some() {
            return Some("raw extensions and diagram payloads are outside the native notes subset");
        }
        if shape.tags.iter().any(|(key, _)| key.starts_with("odp:")) {
            return Some("selected owner carries ODP provenance");
        }
        if let Some(reason) = shape
            .effects
            .as_ref()
            .and_then(effects_reason)
            .or_else(|| link_reason(&shape.click))
            .or_else(|| link_reason(&shape.hover))
            .or_else(|| shape.text.as_ref().and_then(text_reason))
            .or_else(|| shapes_reason(shape.children()))
        {
            return Some(reason);
        }
    }
    None
}
fn master_reason(master: &Master, notes: bool) -> Option<&'static str> {
    if master.raw_ext.is_some() || master.theme.raw_extra.is_some() {
        return Some("raw master or theme extensions are outside the native notes subset");
    }
    if notes && !master.layouts.is_empty() {
        return Some("notes-master layouts are outside the native notes subset");
    }
    shapes_reason(&master.shapes)
        .or_else(|| master.theme.format.effects.iter().find_map(effects_reason))
        .or_else(|| list_reason(&master.title_style))
        .or_else(|| list_reason(&master.body_style))
        .or_else(|| list_reason(&master.other_style))
        .or_else(|| {
            master.layouts.iter().find_map(|layout| {
                if layout.raw_ext.is_some() {
                    Some("raw layout extensions are outside the native notes subset")
                } else {
                    shapes_reason(&layout.shapes)
                }
            })
        })
}

/// General bounded validation precedes deciding whether the cross-document profile applies.
/// Unsupported native content remains copyable within its current UID, with no closure receipt.
pub(crate) fn capture(source: &Presentation, selected: &[&Slide]) -> Result<Capture> {
    if selected.len() > crate::transfer_policy::MAX_POLICY_PAGES {
        return Err(invalid("too many selected pages"));
    }
    if !selected.iter().any(|slide| slide.notes_page.is_some()) {
        return Ok(Capture::default());
    }
    if source.notes_masters.len() + usize::from(source.notes_master.is_some()) > MAX_OWNERS
        || source.masters.len() > MAX_OWNERS
        || source.media.len() > MAX_DEST_MEDIA
        || source.tags.len() > 100_000
    {
        return Err(invalid("document ownership index limit exceeded"));
    }
    let mut budget = Counter { bytes: 0, limit: MAX_METADATA / 2 };
    budget.value(&source.default_text_style)?;
    budget.value(&source.header_footer)?;
    budget.value(&source.tags)?;
    for level in source.default_text_style.levels.iter().flatten() {
        para(&level.para)?;
    }
    if !deckcraft_model::notes::valid_size(source.notes_size) || !deckcraft_model::notes::valid_size(source.slide_size) {
        return Err(invalid("invalid source page size"));
    }
    let mut reason =
        if source.tags.iter().any(|(key, _)| key.starts_with("odp:")) || source.media.iter().any(|media| media.content_type == SOURCE_MIME) {
            Some("ODP provenance or retained source attachments are outside the native notes subset")
        } else if !source.embedded_fonts.is_empty() {
            Some("embedded fonts are outside the native notes subset")
        } else {
            None
        };
    let mut default_media = false;
    media_refs::visit::list(&source.default_text_style, &mut |_| default_media = true);
    if default_media {
        reason.get_or_insert("document default text style must be dependency-free");
    }
    let default_fields = NotesFields::default();
    let mut notes = HashMap::new();
    notes.try_reserve(source.notes_masters.len() + 1).map_err(|_| invalid("cannot allocate general notes index"))?;
    budget.add((source.notes_masters.len() + 1).checked_mul(96).ok_or_else(|| invalid("notes index overflow"))?).map_err(invalid)?;
    for (at, owner) in source.notes_masters.iter().enumerate() {
        if notes.insert(owner.master.id, (at, owner.master.as_ref(), owner.size, &owner.fields)).is_some() {
            return Err(invalid("duplicate notes master identity"));
        }
    }
    let legacy = source.notes_masters.len();
    if let Some(owner) = &source.notes_master
        && notes.insert(owner.id, (legacy, owner.as_ref(), source.notes_size, &default_fields)).is_some()
    {
        return Err(invalid("ambiguous legacy notes master identity"));
    }
    let mut layouts = HashMap::new();
    let mut layout_count = 0usize;
    let mut master_ids = HashSet::new();
    master_ids.try_reserve(source.masters.len()).map_err(|_| invalid("cannot allocate general regular master index"))?;
    budget.add(source.masters.len().checked_mul(96).ok_or_else(|| invalid("regular master index overflow"))?).map_err(invalid)?;
    for owner in &source.masters {
        if !master_ids.insert(owner.id) {
            return Err(invalid("duplicate source regular master identity"));
        }
        for layout in &owner.layouts {
            layout_count = layout_count.checked_add(1).ok_or_else(|| invalid("layout count overflow"))?;
            if layout_count > MAX_SHAPES {
                return Err(invalid("too many source layouts"));
            }
            layouts.try_reserve(1).map_err(|_| invalid("cannot allocate general layout index"))?;
            budget.add(96).map_err(invalid)?;
            if layouts.insert(layout.id, owner.as_ref()).is_some() {
                return Err(invalid("ambiguous source regular layout identity"));
            }
        }
    }
    let mut shape_count = 0usize;
    let mut comment_count = 0usize;
    let mut scanned = HashSet::new();
    let mut regular = HashSet::new();
    let mut page_ids = HashSet::new();
    scanned.try_reserve(notes.len()).map_err(|_| invalid("cannot allocate selected general notes index"))?;
    regular.try_reserve(source.masters.len()).map_err(|_| invalid("cannot allocate selected general regular index"))?;
    page_ids.try_reserve(selected.len()).map_err(|_| invalid("cannot allocate selected general page index"))?;
    let mut refs = HashSet::new();
    refs.try_reserve(MAX_MEDIA).map_err(|_| invalid("cannot allocate general media index"))?;
    let mut reference_error = false;
    let mut reference = |id: &MediaId| {
        if refs.len() >= MAX_MEDIA && !refs.contains(id) {
            reference_error = true;
        } else {
            refs.insert(*id);
        }
    };
    for slide in selected {
        if !page_ids.insert(slide.id) {
            return Err(invalid("duplicate selected slide identity"));
        }
        comment_topology(&slide.comments, 0, &mut comment_count)?;
        general_shapes(&slide.shapes, &mut shape_count, &mut budget)?;
        if let Some(page) = &slide.notes_page {
            general_shapes(&page.shapes, &mut shape_count, &mut budget)?;
            if page.size.is_some_and(|size| !deckcraft_model::notes::valid_size(size)) || !deckcraft_model::notes::valid_fields(&page.fields) {
                return Err(invalid("invalid notes page size or fields"));
            }
            if let Some(body) = page.body_shape {
                let frame = deckcraft_model::find_shape(&page.shapes, body).ok_or_else(|| invalid("notes body frame is missing"))?;
                if frame.text.is_some()
                    || !matches!(frame.kind, ShapeKind::Shape)
                    || frame.ph_type().is_some_and(|kind| kind != deckcraft_model::PhType::Body)
                {
                    return Err(invalid("invalid primary notes body frame or duplicate canonical text"));
                }
            }
            let mut bodies = 0usize;
            deckcraft_model::walk(&page.shapes, &mut |shape, _| bodies += usize::from(shape.ph_type() == Some(deckcraft_model::PhType::Body)));
            if bodies > 1 {
                return Err(invalid("notes page has multiple primary body frames"));
            }
            let selected_owner = match page.master {
                Some(id) => Some(notes.get(&id).ok_or_else(|| invalid("notes page uses a missing master"))?),
                None => source.notes_master.as_ref().and_then(|master| notes.get(&master.id)),
            };
            if let Some((at, owner, size, fields)) = selected_owner {
                if scanned.insert(*at) {
                    general_master(owner, &mut shape_count, &mut budget)?;
                    budget.value(fields)?;
                    if !deckcraft_model::notes::valid_size(*size) || !deckcraft_model::notes::valid_fields(fields) {
                        return Err(invalid("invalid notes master size or fields"));
                    }
                    let mut bodies = 0usize;
                    deckcraft_model::walk(&owner.shapes, &mut |shape, _| {
                        bodies += usize::from(shape.ph_type() == Some(deckcraft_model::PhType::Body))
                    });
                    if bodies > 1 {
                        return Err(invalid("notes master has multiple primary body frames"));
                    }
                    if let Some(unsupported) = master_reason(owner, true) {
                        reason.get_or_insert(unsupported);
                    }
                    media_refs::visit::master(owner, &mut reference);
                }
            } else {
                reason.get_or_insert("structured notes require a real source notes master; no empty master is synthesized");
            }
        }
        budget.value(*slide)?;
        valid_text(&slide.notes)?;
        if slide.raw_ext.is_some() || slide.transition.as_ref().is_some_and(|transition| transition.raw.is_some()) {
            reason.get_or_insert("raw slide/transition extensions are outside the native notes subset");
        }
        if slide.tags.iter().any(|(key, _)| key.starts_with("odp:")) {
            reason.get_or_insert("selected owner carries ODP provenance");
        }
        if let Some(unsupported) = shapes_reason(&slide.shapes)
            .or_else(|| slide.notes_page.as_ref().and_then(|page| shapes_reason(&page.shapes)))
            .or_else(|| text_reason(&slide.notes))
        {
            reason.get_or_insert(unsupported);
        }
        media_refs::visit::slide(slide, &mut reference);
        let master = *layouts.get(&slide.layout).ok_or_else(|| invalid("selected slide uses a missing regular layout"))?;
        if regular.insert(master.id) {
            general_master(master, &mut shape_count, &mut budget)?;
            if let Some(unsupported) = master_reason(master, false) {
                reason.get_or_insert(unsupported);
            }
            media_refs::visit::master(master, &mut reference);
        }
    }
    if reference_error {
        return Err(invalid("too many selected embedded media items"));
    }
    let mut ids = HashSet::new();
    ids.try_reserve(source.media.len()).map_err(|_| invalid("cannot allocate general source media index"))?;
    let mut payload = 0usize;
    for item in &source.media {
        if !ids.insert(item.id) {
            return Err(invalid("duplicate source media identity"));
        }
        if !refs.remove(&item.id) {
            continue;
        }
        budget.value(item)?;
        if item.link.is_some() {
            reason.get_or_insert("linked media are outside the native embedded notes subset");
        } else if item.data.is_empty() {
            return Err(invalid("selected media must have nonempty embedded bytes and no external link"));
        }
        if item.data.len() > MAX_ITEM_BYTES {
            return Err(invalid("selected embedded media exceeds 32 MiB"));
        }
        payload = payload.checked_add(item.data.len()).ok_or_else(|| invalid("media payload overflow"))?;
        if payload > MAX_MEDIA_BYTES {
            return Err(invalid("selected media payload exceeds 128 MiB"));
        }
    }
    if !refs.is_empty() {
        return Err(invalid("selected media reference is missing"));
    }
    if let Some(reason) = reason {
        return Ok(Capture { closure: None, same_document_only: Some(reason) });
    }
    // The preflight allocated bounded indexes only. Charge that work to the retained proof too.
    let closure = capture_native(source, selected, budget.bytes)?.ok_or_else(|| invalid("missing native notes capture"))?;
    Ok(Capture { closure: Some(closure), same_document_only: None })
}

struct IdScan {
    count: usize,
    high: u32,
}
impl IdScan {
    fn id(&mut self, value: u32) -> Result<()> {
        self.count = self.count.checked_add(1).ok_or_else(|| invalid("destination ID count overflow"))?;
        if self.count > MAX_IDS {
            return Err(invalid("destination ID scan exceeds 2000000"));
        }
        self.high = self.high.max(value);
        Ok(())
    }
    fn shapes(&mut self, shapes: &[Shape], depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(invalid("destination shape nesting exceeds 64"));
        }
        for shape in shapes {
            self.id(shape.id.0)?;
            if depth == 64 && !shape.children().is_empty() {
                return Err(invalid("destination shape nesting exceeds 64"));
            }
            if !shape.children().is_empty() {
                self.shapes(shape.children(), depth + 1)?;
            }
        }
        Ok(())
    }
    fn master(&mut self, master: &Master) -> Result<()> {
        self.id(master.id.0)?;
        self.shapes(&master.shapes, 0)?;
        for layout in &master.layouts {
            self.id(layout.id.0)?;
            self.shapes(&layout.shapes, 0)?;
        }
        Ok(())
    }
}
fn destination(destination: &Presentation, closure: &NotesClosure) -> Result<u32> {
    native(destination)?;
    if destination.notes_masters.len() + usize::from(destination.notes_master.is_some()) > MAX_OWNERS {
        return Err(invalid("too many destination notes owners"));
    }
    // Admission runs before edit_resources clones owned metadata and once again on its
    // private candidate; each pass spends at most half the operation's 64 MiB scan budget.
    let mut budget = Counter { bytes: 0, limit: MAX_DEST_METADATA / 2 };
    budget.value(&destination.default_text_style)?;
    dependency_free_defaults(&destination.default_text_style)?;
    budget.value(&destination.header_footer)?;
    if destination.default_text_style != closure.defaults || destination.header_footer != closure.header_footer {
        return Err(invalid("document default text style and HeaderFooter must match exactly; global notes-default compatibility is required"));
    }
    if !deckcraft_model::notes::valid_size(destination.slide_size) || !deckcraft_model::notes::valid_size(destination.notes_size) {
        return Err(invalid("invalid destination page size"));
    }
    // Presentation's owned values are cloned by edit_resources; bound them before that call.
    budget.value(&destination.tags)?;
    budget.value(&destination.sections)?;
    budget.value(&destination.custom_shows)?;
    budget.value(&destination.show)?;
    budget.value(&destination.props)?;
    budget.value(&destination.table_styles)?;
    budget.value(&destination.kept_parts)?;
    budget.value(&destination.guides)?;
    budget.value(&destination.raw_ext)?;
    let mut scan = IdScan { count: 0, high: destination.next_id.max(1) };
    let mut comments = 0usize;
    for slide in &destination.slides {
        comment_topology(&slide.comments, 0, &mut comments)?;
        scan.id(slide.id.0)?;
        scan.shapes(&slide.shapes, 0)?;
        if let Some(page) = &slide.notes_page {
            scan.shapes(&page.shapes, 0)?;
        }
    }
    for master in destination.masters.iter().chain(destination.notes_master.iter()).chain(destination.handout_master.iter()) {
        scan.master(master)?;
    }
    let mut owner_ids = HashSet::new();
    owner_ids.try_reserve(destination.notes_masters.len() + 1).map_err(|_| invalid("cannot allocate destination owner index"))?;
    if let Some(master) = &destination.notes_master {
        owner_ids.insert(master.id);
    }
    for owner in &destination.notes_masters {
        if !owner_ids.insert(owner.master.id) {
            return Err(invalid("duplicate destination notes master identity"));
        }
        budget.value(&owner.fields)?;
        scan.master(&owner.master)?;
    }
    let mut media_ids = HashSet::new();
    media_ids.try_reserve(destination.media.len()).map_err(|_| invalid("cannot allocate destination media index"))?;
    for media in &destination.media {
        if !media_ids.insert(media.id) {
            return Err(invalid("duplicate destination media identity"));
        }
        scan.id(media.id.0)?;
        budget.value(&media.name)?;
        budget.value(&media.content_type)?;
        budget.value(&media.link)?;
    }
    for template in &destination.presentation_templates {
        scan.id(template.id.0)?;
        for slot in &template.slots {
            scan.id(slot.id.0)?;
        }
    }
    budget.value(destination)?;
    budget.add(scan.count.checked_mul(32).ok_or_else(|| invalid("destination scan accounting overflow"))?).map_err(invalid)?;
    Ok(scan.high)
}

pub(crate) fn preflight<'a>(destination: &Presentation, closure: &NotesClosure, slides: impl IntoIterator<Item = &'a Slide>) -> Result<()> {
    destination_check(destination, closure, slides).map(|_| ())
}
fn destination_check<'a>(destination_doc: &Presentation, closure: &NotesClosure, slides: impl IntoIterator<Item = &'a Slide>) -> Result<u32> {
    let high = destination(destination_doc, closure)?;
    // This proof cannot authorize arbitrary clipboard edits, including owner/media changes.
    let mut count = 0usize;
    let mut seen = HashSet::new();
    seen.try_reserve(closure.slides.len()).map_err(|_| invalid("cannot allocate actual page index"))?;
    for slide in slides {
        count = count.checked_add(1).ok_or_else(|| invalid("page count overflow"))?;
        if count > crate::transfer_policy::MAX_POLICY_PAGES {
            return Err(invalid("too many selected pages"));
        }
        if !seen.insert(slide.id) {
            return Err(invalid("actual clipboard batch has duplicate source slide identities"));
        }
        let captured = closure
            .slide_index
            .get(&slide.id)
            .and_then(|at| closure.slides.get(*at))
            .ok_or_else(|| invalid("slide was not captured by notes closure"))?;
        if captured.as_ref() != slide {
            return Err(invalid("clipboard slide changed after notes closure capture"));
        }
    }
    Ok(high)
}

fn fresh(destination: &mut Presentation) -> Result<u32> {
    destination.alloc_id().ok_or_else(|| invalid("identity space exhausted"))
}
fn reid_tree(destination: &mut Presentation, shapes: &mut [Shape]) -> Result<HashMap<ShapeId, ShapeId>> {
    fn assign(destination: &mut Presentation, shapes: &mut [Shape], map: &mut HashMap<ShapeId, ShapeId>) -> Result<()> {
        for shape in shapes {
            let old = shape.id;
            shape.id = ShapeId(fresh(destination)?);
            map.insert(old, shape.id);
            if let Some(children) = shape.children_mut() {
                assign(destination, children, map)?;
            }
        }
        Ok(())
    }
    fn endpoints(shapes: &mut [Shape], map: &HashMap<ShapeId, ShapeId>) -> Result<()> {
        for shape in shapes {
            if let ShapeKind::Connector { start, end } = &mut shape.kind {
                for (id, _) in [start, end].into_iter().flatten() {
                    *id = *map.get(id).ok_or_else(|| invalid("connector endpoint missing from local map"))?;
                }
            }
            if let Some(children) = shape.children_mut() {
                endpoints(children, map)?;
            }
        }
        Ok(())
    }
    let mut count = 0usize;
    deckcraft_model::walk(shapes, &mut |_, _| count += 1);
    let mut map = HashMap::new();
    map.try_reserve(count).map_err(|_| invalid("cannot allocate local shape map"))?;
    assign(destination, shapes, &mut map)?;
    endpoints(shapes, &map)?;
    Ok(map)
}
fn rewrite_media(map: &HashMap<MediaId, MediaId>) -> impl FnMut(&mut MediaId) + '_ {
    |id| {
        if let Some(fresh) = map.get(id) {
            *id = *fresh;
        }
    }
}
fn match_layout(destination: &Presentation, name: &str, kind: deckcraft_model::LayoutType) -> LayoutId {
    let all = || destination.masters.iter().flat_map(|master| &master.layouts);
    all()
        .find(|layout| !name.is_empty() && layout.name.eq_ignore_ascii_case(name))
        .or_else(|| all().find(|layout| layout.kind == kind))
        .or_else(|| all().next())
        .map(|layout| layout.id)
        .unwrap_or_default()
}

/// Stage into the edit_resources private document. Caller publishes only after template staging
/// also succeeds. Ordinary slide artwork is independent of each page/master's local ID map.
pub(crate) fn remap(destination: &mut Presentation, closure: &NotesClosure, slides: &mut [Slide], keep_regular: bool) -> Result<()> {
    let high = destination_check(destination, closure, slides.iter())?;
    deckcraft_model::comments::normalize(destination).map_err(|e| invalid(e.to_string()))?;
    let high = high.max(destination.next_id);
    let mut budget = Counter { bytes: closure.metadata, limit: MAX_METADATA };
    let mut selected = budget.vector::<(usize, Option<usize>)>(slides.len())?;
    let mut selected_owners = HashSet::new();
    selected_owners.try_reserve(closure.owners.len()).map_err(|_| invalid("cannot allocate selected notes index"))?;
    let mut regular = HashSet::new();
    regular.try_reserve(closure.regular.len()).map_err(|_| invalid("cannot allocate regular owner index"))?;
    let mut references = HashSet::new();
    references.try_reserve(closure.media.len()).map_err(|_| invalid("cannot allocate media map"))?;
    let mut reference = |id: &MediaId| {
        references.insert(*id);
    };
    for slide in slides.iter() {
        let at = *closure.slide_index.get(&slide.id).ok_or_else(|| invalid("uncaptured slide"))?;
        let binding = closure.bindings.get(at).copied().flatten();
        selected.push((at, binding));
        if let Some(owner) = binding {
            selected_owners.insert(owner);
        }
        media_refs::visit::slide(slide, &mut reference);
        if keep_regular {
            let at = *closure.layouts.get(&slide.layout).ok_or_else(|| invalid("uncaptured regular layout"))?;
            regular.insert(at);
        }
    }
    for at in &selected_owners {
        media_refs::visit::master(&closure.owners.get(*at).ok_or_else(|| invalid("uncaptured notes owner"))?.master, &mut reference);
    }
    for at in &regular {
        media_refs::visit::master(closure.regular.get(*at).ok_or_else(|| invalid("uncaptured regular owner"))?, &mut reference);
    }
    if (destination.notes_masters.len() + usize::from(destination.notes_master.is_some()))
        .checked_add(selected_owners.len())
        .is_none_or(|count| count > MAX_OWNERS)
    {
        return Err(invalid("resulting notes owner count exceeds 4096"));
    }
    if destination.media.len().checked_add(references.len()).is_none_or(|count| count > MAX_DEST_MEDIA) {
        return Err(invalid("resulting media count exceeds 100000"));
    }
    let mut needed = slides
        .len()
        .checked_add(selected_owners.len())
        .and_then(|count| count.checked_add(references.len()))
        .ok_or_else(|| invalid("ID reservation overflow"))?;
    let mut count_shapes = |shapes: &[Shape]| -> Result<()> {
        let mut overflow = false;
        deckcraft_model::walk(shapes, &mut |_, _| {
            if let Some(next) = needed.checked_add(1) {
                needed = next;
            } else {
                overflow = true;
            }
        });
        if overflow {
            return Err(invalid("shape ID reservation overflow"));
        }
        Ok(())
    };
    for slide in slides.iter() {
        count_shapes(&slide.shapes)?;
        if let Some(page) = &slide.notes_page {
            count_shapes(&page.shapes)?;
        }
    }
    for at in &selected_owners {
        count_shapes(&closure.owners.get(*at).ok_or_else(|| invalid("missing selected owner"))?.master.shapes)?;
    }
    for at in &regular {
        let master = closure.regular.get(*at).ok_or_else(|| invalid("missing selected regular owner"))?;
        count_shapes(&master.shapes)?;
        for layout in &master.layouts {
            count_shapes(&layout.shapes)?;
        }
    }
    for at in &regular {
        needed = needed
            .checked_add(1 + closure.regular.get(*at).ok_or_else(|| invalid("missing regular owner"))?.layouts.len())
            .ok_or_else(|| invalid("ID reservation overflow"))?;
    }
    let amount = u32::try_from(needed).map_err(|_| invalid("ID reservation overflow"))?;
    high.checked_add(amount).ok_or_else(|| invalid("identity space exhausted before staging"))?;
    budget.add(needed.checked_mul(96).ok_or_else(|| invalid("staging index overflow"))?).map_err(invalid)?;
    destination.next_id = high;
    destination.notes_masters.try_reserve(selected_owners.len()).map_err(|_| invalid("cannot allocate destination notes owners"))?;
    destination.media.try_reserve(references.len()).map_err(|_| invalid("cannot allocate destination media"))?;
    destination.masters.try_reserve(regular.len()).map_err(|_| invalid("cannot allocate destination regular masters"))?;
    let mut media_map = HashMap::new();
    media_map.try_reserve(references.len()).map_err(|_| invalid("cannot allocate media rewrite map"))?;
    // Iterate capture order for deterministic IDs. Every item gets a fresh ID, regardless of bytes.
    for media in &closure.media {
        if !references.remove(&media.id) {
            continue;
        }
        let mut item = media.clone();
        item.id = MediaId(fresh(destination)?);
        media_map.insert(media.id, item.id);
        destination.media.push(item);
    }
    if !references.is_empty() {
        return Err(invalid("media lies outside captured closure"));
    }
    let mut owner_map = HashMap::new();
    owner_map.try_reserve(selected_owners.len()).map_err(|_| invalid("cannot allocate notes rewrite map"))?;
    for (at, owner) in closure.owners.iter().enumerate() {
        if !selected_owners.contains(&at) {
            continue;
        }
        let mut copy = owner.clone();
        let mut master = owner.master.as_ref().clone();
        master.id = MasterId(fresh(destination)?);
        reid_tree(destination, &mut master.shapes)?;
        media_refs::visit_mut::master(&mut master, &mut rewrite_media(&media_map));
        owner_map.insert(at, master.id);
        copy.master = Arc::new(master);
        destination.notes_masters.push(copy);
    }
    let mut layout_map = HashMap::new();
    layout_map.try_reserve(needed).map_err(|_| invalid("cannot allocate regular layout map"))?;
    for (at, owner) in closure.regular.iter().enumerate() {
        if !regular.contains(&at) {
            continue;
        }
        let mut master = owner.as_ref().clone();
        master.id = MasterId(fresh(destination)?);
        reid_tree(destination, &mut master.shapes)?;
        for layout in &mut master.layouts {
            let old = layout.id;
            layout.id = LayoutId(fresh(destination)?);
            layout_map.insert(old, layout.id);
            reid_tree(destination, &mut layout.shapes)?;
        }
        media_refs::visit_mut::master(&mut master, &mut rewrite_media(&media_map));
        destination.masters.push(Arc::new(master));
    }
    let mut comment_context = crate::comment_transfer::Context::default();
    let mut comment_slides = std::collections::BTreeMap::new();
    for slide in slides.iter_mut() {
        let old = slide.id;
        slide.id = SlideId(fresh(destination)?);
        comment_slides.insert(old, slide.id);
    }
    for (slide, (_, binding)) in slides.iter_mut().zip(selected) {
        let shapes = reid_tree(destination, &mut slide.shapes)?;
        for animation in &mut slide.animations {
            animation.shape = *shapes.get(&animation.shape).ok_or_else(|| invalid("animation target is outside its slide"))?;
            animation.trigger =
                animation.trigger.map(|id| shapes.get(&id).copied().ok_or_else(|| invalid("animation trigger is outside its slide"))).transpose()?;
        }
        if let Some(page) = &mut slide.notes_page {
            let local = reid_tree(destination, &mut page.shapes)?;
            page.body_shape =
                page.body_shape.map(|id| local.get(&id).copied().ok_or_else(|| invalid("notes body missing from local map"))).transpose()?;
            page.master = Some(
                *owner_map
                    .get(&binding.ok_or_else(|| invalid("notes owner binding missing"))?)
                    .ok_or_else(|| invalid("notes owner was not staged"))?,
            );
        }
        let mut comments = std::mem::take(&mut slide.comments);
        media_refs::visit_mut::slide(slide, &mut rewrite_media(&media_map));
        let comment_shapes = shapes.iter().map(|(a, b)| (*a, *b)).collect();
        let comment_media = crate::comment_transfer::identity_media(&comments, &media_map);
        crate::comment_transfer::remap(destination, &mut comments, &comment_shapes, &comment_media, &comment_slides, &mut comment_context)?;
        slide.comments = comments;
        slide.layout = if keep_regular {
            *layout_map.get(&slide.layout).ok_or_else(|| invalid("regular layout was not staged"))?
        } else {
            let source = closure
                .layouts
                .get(&slide.layout)
                .and_then(|at| closure.regular.get(*at))
                .and_then(|master| master.layout(slide.layout))
                .ok_or_else(|| invalid("regular layout was not captured"))?;
            match_layout(destination, &source.name, source.kind)
        };
    }
    Ok(())
}

#[cfg(test)]
mod admission_arithmetic_tests {
    use super::*;
    #[test]
    fn exact_counter_limit_and_checked_overflow_do_not_allocate_a_json_payload() {
        let mut count = Counter { bytes: 0, limit: 32 };
        assert!(count.add(32).is_ok());
        assert!(count.add(1).is_err());
        let mut count = Counter { bytes: usize::MAX, limit: usize::MAX };
        assert!(count.add(1).is_err());
        let mut count = Counter { bytes: 0, limit: MAX_METADATA };
        assert!(count.value(&f64::NAN).is_err());
        assert!(count.value(&f64::INFINITY).is_err());
    }
    #[test]
    fn destination_identifier_and_selected_shape_caps_are_inclusive() {
        let mut scan = IdScan { count: MAX_IDS - 1, high: 1 };
        assert!(scan.id(2).is_ok());
        assert!(scan.id(3).is_err());
        let shape = Shape { id: ShapeId(10), ..Default::default() };
        let mut count = MAX_SHAPES - 1;
        let mut ids = HashSet::new();
        assert!(tree(std::slice::from_ref(&shape), 0, &mut count, &mut ids).is_ok());
        assert!(tree(&[Shape { id: ShapeId(11), ..Default::default() }], 0, &mut count, &mut ids).is_err());
        let mut scan = IdScan { count: usize::MAX, high: 1 };
        assert!(scan.id(1).is_err());
    }
}
