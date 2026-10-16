//! Editable notes pages and their masters. Slide-image placeholders always show their owning
//! slide live; they are never cached pictures. The speaker-notes body has one canonical value
//! in `Slide::notes`, with an explicit frame identity in `NotesPage::body_shape`.
use std::sync::Arc;

use deckcraft_geom::Size;
use serde::{Deserialize, Serialize};

use crate::{Background, Master, MasterId, Shape, ShapeId, Slide};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum NotesDateTime {
    Fixed { text: String },
    Current { format: String, lang: Option<String> },
}

/// Resolved declaration values and visibility overrides. An absent value inherits, while
/// an explicitly empty string clears inherited text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NotesFields {
    pub header: Option<String>,
    pub footer: Option<String>,
    pub date_time: Option<NotesDateTime>,
    pub display_header: Option<bool>,
    pub display_footer: Option<bool>,
    pub display_date_time: Option<bool>,
    pub display_page_number: Option<bool>,
}

impl NotesFields {
    pub fn inherit(&mut self, parent: &Self) {
        if self.header.is_none() {
            self.header.clone_from(&parent.header);
        }
        if self.footer.is_none() {
            self.footer.clone_from(&parent.footer);
        }
        if self.date_time.is_none() {
            self.date_time.clone_from(&parent.date_time);
        }
        self.display_header = self.display_header.or(parent.display_header);
        self.display_footer = self.display_footer.or(parent.display_footer);
        self.display_date_time = self.display_date_time.or(parent.display_date_time);
        self.display_page_number = self.display_page_number.or(parent.display_page_number);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NotesMaster {
    pub master: Arc<Master>,
    pub size: Size,
    pub fields: NotesFields,
}

impl Default for NotesMaster {
    fn default() -> Self {
        Self { master: Arc::new(Master::default()), size: Size::new(540.0, 720.0), fields: NotesFields::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NotesPage {
    pub master: Option<MasterId>,
    pub size: Option<Size>,
    /// Drawing order is significant. A `SlideImage` placeholder shows the owning slide.
    pub shapes: Vec<Shape>,
    /// This frame has no stored text: readers, editors and exporters use `Slide::notes`.
    pub body_shape: Option<ShapeId>,
    pub background: Option<Background>,
    pub show_master_shapes: bool,
    pub fields: NotesFields,
}

impl Default for NotesPage {
    fn default() -> Self {
        Self {
            master: None,
            size: None,
            shapes: vec![],
            body_shape: None,
            background: None,
            show_master_shapes: true,
            fields: NotesFields::default(),
        }
    }
}

impl crate::Presentation {
    /// Materialize one local frame when editing an imported notes page without a local body.
    /// Master geometry remains inherited; the frame never receives a second copy of the text.
    pub fn ensure_notes_body(&mut self, index: usize) -> crate::Result<()> {
        let slide = self.slides.get(index).ok_or_else(|| crate::ModelError::NoSlide(index.to_string()))?;
        if let Some(error) = self.notes_page_error(slide) {
            return Err(crate::ModelError::Invalid(error.into()));
        }
        let Some(page) = &slide.notes_page else { return Ok(()) };
        if let Some(body) = page.body_shape {
            return if crate::find_shape(&page.shapes, body).is_some_and(|shape| shape.text.is_none()) {
                Ok(())
            } else {
                Err(crate::ModelError::Invalid("invalid canonical notes body frame".into()))
            };
        }
        let mut count = 0usize;
        crate::walk(&page.shapes, &mut |_, _| count = count.saturating_add(1));
        if count >= 100_000 {
            return Err(crate::ModelError::Invalid("notes page has too many shapes".into()));
        }
        let size = self.notes_page_size(slide);
        if !valid_size(size) {
            return Err(crate::ModelError::Invalid("invalid notes page size".into()));
        }
        let mut masters =
            self.notes_master_for(slide).into_iter().flat_map(|master| &master.shapes).filter(|shape| shape.ph_type() == Some(crate::PhType::Body));
        let inherited = masters.next().cloned();
        if masters.next().is_some() {
            return Err(crate::ModelError::Invalid("ambiguous notes master body frame".into()));
        }
        let mut frame = inherited.unwrap_or_else(|| Shape {
            ph: Some(crate::Placeholder { kind: crate::PhType::Body, ..Default::default() }),
            xfrm: Some(crate::Xfrm::new(size.width * 0.1, size.height * 0.53, size.width * 0.8, size.height * 0.36)),
            ..Default::default()
        });
        if self.notes_master_for(slide).is_some_and(|master| master.shapes.iter().any(|shape| shape.ph_type() == Some(crate::PhType::Body))) {
            frame.xfrm = None;
        }
        frame.text = None;
        let id = ShapeId(self.alloc_id().ok_or_else(|| crate::ModelError::Invalid("notes frame identity space exhausted".into()))?);
        frame.id = id;
        let slide = self.slides.get_mut(index).ok_or_else(|| crate::ModelError::NoSlide(index.to_string()))?;
        let page = Arc::make_mut(slide).notes_page.as_mut().ok_or_else(|| crate::ModelError::Invalid("notes page is missing".into()))?;
        page.shapes.push(frame);
        page.body_shape = Some(id);
        Ok(())
    }

    /// Validate notes references and bounded native structures before rendering/exporting them.
    pub fn notes_page_error(&self, slide: &Slide) -> Option<&'static str> {
        if let Some(id) = slide.notes_page.as_ref().and_then(|page| page.master)
            && self.notes_masters.iter().filter(|notes| notes.master.id == id).count()
                + usize::from(self.notes_master.as_ref().is_some_and(|master| master.id == id))
                > 1
        {
            return Some("notes page uses an ambiguous master identity");
        }
        if let Some(master) = self.notes_master_for(slide) {
            if let Some(error) = shape_error(&master.shapes) {
                return Some(error);
            }
            if let Some(notes) = self.notes_masters.iter().find(|notes| notes.master.id == master.id)
                && (!valid_size(notes.size) || !valid_fields(&notes.fields))
            {
                return Some("invalid notes master size or fields");
            }
        }
        let page = slide.notes_page.as_ref()?;
        if page.master.is_some() && self.notes_master_for(slide).is_none() {
            return Some("notes page uses a missing master");
        }
        let size = self.notes_page_size(slide);
        if !valid_size(size) {
            return Some("invalid notes page size");
        }
        if !valid_fields(&page.fields) {
            return Some("notes fields exceed limits");
        }
        if let Some(error) = shape_error(&page.shapes) {
            return Some(error);
        }
        if let Some(body) = page.body_shape {
            match crate::find_shape(&page.shapes, body) {
                None => return Some("notes body frame is missing"),
                Some(shape) if shape.text.is_some() => return Some("notes body frame duplicates canonical speaker notes"),
                Some(shape) if !matches!(shape.kind, crate::ShapeKind::Shape) => return Some("notes body must be a text frame"),
                Some(shape) if shape.ph_type().is_some_and(|kind| kind != crate::PhType::Body) => {
                    return Some("notes body has an incompatible placeholder role");
                }
                Some(_) => {}
            }
        }
        None
    }

    pub fn notes_master_for(&self, slide: &Slide) -> Option<&Master> {
        match slide.notes_page.as_ref().and_then(|page| page.master) {
            Some(id) => self
                .notes_masters
                .iter()
                .find(|master| master.master.id == id)
                .map(|master| master.master.as_ref())
                .or_else(|| self.notes_master.as_deref().filter(|master| master.id == id)),
            None => self.notes_master.as_deref(),
        }
    }

    pub fn notes_page_size(&self, slide: &Slide) -> Size {
        if let Some(size) = slide.notes_page.as_ref().and_then(|page| page.size) {
            return size;
        }
        let master =
            slide.notes_page.as_ref().and_then(|page| page.master).and_then(|id| self.notes_masters.iter().find(|master| master.master.id == id));
        master.map(|master| master.size).unwrap_or(self.notes_size)
    }

    pub fn notes_fields(&self, slide: &Slide) -> NotesFields {
        let mut fields = slide.notes_page.as_ref().map(|page| page.fields.clone()).unwrap_or_default();
        if let Some(master) =
            slide.notes_page.as_ref().and_then(|page| page.master).and_then(|id| self.notes_masters.iter().find(|master| master.master.id == id))
        {
            fields.inherit(&master.fields);
        }
        fields
    }
}

fn shape_error(shapes: &[Shape]) -> Option<&'static str> {
    let mut ids = std::collections::HashSet::new();
    let mut bodies = 0usize;
    let mut invalid = None;
    crate::walk(shapes, &mut |shape, depth| {
        bodies += usize::from(shape.ph_type() == Some(crate::PhType::Body));
        if bodies > 1 {
            invalid = Some("notes page has multiple primary body frames");
        }
        if ids.len() >= 100_000 {
            invalid = Some("notes page has too many shapes");
        } else if !ids.insert(shape.id) {
            invalid = Some("notes page has duplicate shape identities");
        }
        if depth >= 64 && !shape.children().is_empty() {
            invalid = Some("notes page shape nesting exceeds limit");
        }
    });
    invalid
}

pub fn valid_size(size: Size) -> bool {
    size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0 && size.width <= 5000.0 && size.height <= 5000.0
}

pub fn valid_fields(fields: &NotesFields) -> bool {
    fields.header.as_ref().is_none_or(|text| text.len() <= 64 * 1024)
        && fields.footer.as_ref().is_none_or(|text| text.len() <= 64 * 1024)
        && match &fields.date_time {
            Some(NotesDateTime::Fixed { text }) => text.len() <= 64 * 1024,
            Some(NotesDateTime::Current { format, lang }) => format.len() <= 128 && lang.as_ref().is_none_or(|lang| lang.len() <= 128),
            None => true,
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PhType, Placeholder, Presentation, TextBody, Xfrm};

    #[test]
    fn notes_fields_inherit_while_empty_text_and_false_visibility_override() {
        let parent = NotesFields { header: Some("Header".into()), footer: Some("Footer".into()), display_header: Some(true), ..Default::default() };
        let mut child = NotesFields { header: Some(String::new()), display_header: Some(false), ..Default::default() };
        child.inherit(&parent);
        assert_eq!(child.header.as_deref(), Some(""));
        assert_eq!(child.footer.as_deref(), Some("Footer"));
        assert_eq!(child.display_header, Some(false));
    }

    #[test]
    fn notes_master_identity_controls_size_and_body_stays_canonical() {
        let mut p = Presentation::default();
        let master = Master { id: MasterId(900), ..Default::default() };
        p.notes_masters.push(NotesMaster { master: Arc::new(master), size: Size::new(612.0, 792.0), ..Default::default() });
        let slide = Arc::make_mut(&mut p.slides[0]);
        slide.notes = TextBody::from_text("Editable notes");
        slide.notes_page = Some(NotesPage {
            master: Some(MasterId(900)),
            body_shape: Some(ShapeId(901)),
            shapes: vec![Shape {
                id: ShapeId(901),
                ph: Some(Placeholder { kind: PhType::Body, ..Default::default() }),
                xfrm: Some(Xfrm::new(50.0, 400.0, 500.0, 300.0)),
                ..Default::default()
            }],
            ..Default::default()
        });
        assert_eq!(p.notes_page_size(&p.slides[0]), Size::new(612.0, 792.0));
        assert!(p.notes_page_error(&p.slides[0]).is_none());
        p.fix_next_id();
        assert_eq!(p.alloc_id(), Some(902));
        let json = serde_json::to_vec(&p).unwrap();
        let restored: Presentation = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored.slides[0].notes.text(), "Editable notes");
        assert!(restored.notes_page_error(&restored.slides[0]).is_none());
        Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap().shapes[0].text = Some(TextBody::from_text("Stale copy"));
        assert_eq!(p.notes_page_error(&p.slides[0]), Some("notes body frame duplicates canonical speaker notes"));
    }

    #[test]
    fn missing_master_body_and_hostile_sizes_are_reported() {
        let mut p = Presentation::default();
        Arc::make_mut(&mut p.slides[0]).notes_page = Some(NotesPage { master: Some(MasterId(999)), ..Default::default() });
        assert_eq!(p.notes_page_error(&p.slides[0]), Some("notes page uses a missing master"));
        let page = Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap();
        page.master = None;
        page.size = Some(Size::new(f64::INFINITY, 100.0));
        assert_eq!(p.notes_page_error(&p.slides[0]), Some("invalid notes page size"));
        let page = Arc::make_mut(&mut p.slides[0]).notes_page.as_mut().unwrap();
        page.size = None;
        page.body_shape = Some(ShapeId(999));
        assert_eq!(p.notes_page_error(&p.slides[0]), Some("notes body frame is missing"));
    }
}
