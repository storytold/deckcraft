//! Editable presentation-template resources, independent of live layout inheritance.
//!
//! Native/core export/command boundaries validate these owners before publishing state.
use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::Hasher;
use std::mem::size_of;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{ModelError, Presentation, PresentationTemplateId, PresentationTemplateSlotId};

pub const MAX_TEMPLATES: usize = 10_000;
pub const MAX_SLOTS_PER_TEMPLATE: usize = 256;
pub const MAX_TOTAL_SLOTS: usize = 100_000;
pub const MAX_TEMPLATE_NAME_BYTES: usize = 8_192;
pub const MAX_TEMPLATE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TEMPLATE_COORDINATE: f64 = 10_000_000.0;
/// Independent bound for a direct model caller, matching the native value-profile ceiling.
pub const MAX_TEMPLATE_SLIDE_SCAN: usize = 2_000_000;
/// Logical scanned bytes/comparison work, not an instruction count or an RSS assertion.
pub const MAX_TEMPLATE_SCAN_WORK: usize = 64 * 1024 * 1024;

fn invalid(reason: &str) -> ModelError {
    ModelError::Invalid(format!("presentation template: {reason}"))
}

/// ODF role identity is separate from the legacy native placeholder compatibility matcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PresentationRole {
    #[serde(rename = "title")]
    Title,
    #[serde(rename = "outline")]
    Outline,
    #[serde(rename = "subtitle")]
    Subtitle,
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "graphic")]
    Graphic,
    #[serde(rename = "object")]
    Object,
    #[serde(rename = "chart")]
    Chart,
    #[serde(rename = "table")]
    Table,
    #[serde(rename = "orgchart")]
    OrgChart,
    #[serde(rename = "page")]
    Page,
    #[serde(rename = "notes")]
    Notes,
    #[serde(rename = "handout")]
    Handout,
    #[serde(rename = "header")]
    Header,
    #[serde(rename = "footer")]
    Footer,
    #[serde(rename = "date-time")]
    DateTime,
    #[serde(rename = "page-number")]
    PageNumber,
}

impl PresentationRole {
    pub const fn odf_token(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Outline => "outline",
            Self::Subtitle => "subtitle",
            Self::Text => "text",
            Self::Graphic => "graphic",
            Self::Object => "object",
            Self::Chart => "chart",
            Self::Table => "table",
            Self::OrgChart => "orgchart",
            Self::Page => "page",
            Self::Notes => "notes",
            Self::Handout => "handout",
            Self::Header => "header",
            Self::Footer => "footer",
            Self::DateTime => "date-time",
            Self::PageNumber => "page-number",
        }
    }

    pub fn from_odf(value: &str) -> Option<Self> {
        Some(match value {
            "title" => Self::Title,
            "outline" => Self::Outline,
            "subtitle" => Self::Subtitle,
            "text" => Self::Text,
            "graphic" => Self::Graphic,
            "object" => Self::Object,
            "chart" => Self::Chart,
            "table" => Self::Table,
            "orgchart" => Self::OrgChart,
            "page" => Self::Page,
            "notes" => Self::Notes,
            "handout" => Self::Handout,
            "header" => Self::Header,
            "footer" => Self::Footer,
            "date-time" => Self::DateTime,
            "page-number" => Self::PageNumber,
            _ => return None,
        })
    }
}

/// Axis-aligned template box in points. There is deliberately no rotation/shear/default.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TemplateBoxWire")]
pub struct PresentationTemplateBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateBoxWire {
    #[serde(deserialize_with = "coordinate")]
    x: f64,
    #[serde(deserialize_with = "coordinate")]
    y: f64,
    #[serde(deserialize_with = "coordinate")]
    w: f64,
    #[serde(deserialize_with = "coordinate")]
    h: f64,
}

impl TryFrom<TemplateBoxWire> for PresentationTemplateBox {
    type Error = ModelError;

    fn try_from(value: TemplateBoxWire) -> Result<Self, Self::Error> {
        let result = Self { x: value.x, y: value.y, w: value.w, h: value.h };
        result.validate()?;
        Ok(result)
    }
}

impl PresentationTemplateBox {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> crate::Result<Self> {
        let result = Self { x, y, w, h };
        result.validate()?;
        Ok(result)
    }

    /// Check operands before arithmetic; neither NaN nor infinity can pass comparisons.
    pub fn validate(&self) -> crate::Result<()> {
        if ![self.x, self.y, self.w, self.h].into_iter().all(bounded_coordinate) {
            return Err(invalid("coordinate is nonfinite or outside the supported range"));
        }
        if self.w <= 0.0 || self.h <= 0.0 {
            return Err(invalid("box width and height must be positive"));
        }
        if !bounded_coordinate(self.x + self.w) || !bounded_coordinate(self.y + self.h) {
            return Err(invalid("box edge is outside the supported range"));
        }
        Ok(())
    }
}

fn bounded_coordinate(value: f64) -> bool {
    value.is_finite() && value.abs() <= MAX_TEMPLATE_COORDINATE
}

fn coordinate<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    struct Coordinate;
    impl Visitor<'_> for Coordinate {
        type Value = f64;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            // Do not use "f64" or "a number": format's legacy numeric-null repair
            // intentionally recognizes those older expectation prefixes.
            formatter.write_str("a bounded presentation-template coordinate")
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<f64, E> {
            if bounded_coordinate(value) { Ok(value) } else { Err(E::custom("presentation-template coordinate is nonfinite or out of range")) }
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<f64, E> {
            if !(-10_000_000..=10_000_000).contains(&value) {
                return Err(E::custom("presentation-template coordinate is out of range"));
            }
            // Every admitted integer is exactly representable as f64.
            Ok(value as f64)
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<f64, E> {
            if value > 10_000_000 {
                return Err(E::custom("presentation-template coordinate is out of range"));
            }
            Ok(value as f64)
        }
    }
    // Default unit/none/string/bool/sequence/map visitors reject their types. Null
    // therefore never becomes 0, including through a buffered serde deserializer.
    deserializer.deserialize_any(Coordinate)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresentationTemplateSlot {
    pub id: PresentationTemplateSlotId,
    pub role: PresentationRole,
    pub bounds: PresentationTemplateBox,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresentationTemplate {
    pub id: PresentationTemplateId,
    /// Unique resource NCName. This is independent of the user-facing display label.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Ordered creation/repositioning template, never live resolver-parent shapes.
    pub slots: Vec<PresentationTemplateSlot>,
}

/// XML 1.0 Fifth Edition NameStartChar/NameChar, excluding the colon separator.
/// Primary source: https://www.w3.org/TR/xml/#NT-NameStartChar .
/// This helper also applies the template resource-name byte limit.
pub fn valid_ncname(value: &str) -> bool {
    fn start(character: char) -> bool {
        matches!(character,
            'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}' |
            '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}' |
            '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' |
            '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}' |
            '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' |
            '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' |
            '\u{10000}'..='\u{EFFFF}')
    }
    if value.is_empty() || value.len() > MAX_TEMPLATE_NAME_BYTES {
        return false;
    }
    let mut characters = value.chars();
    characters.next().is_some_and(start)
        && characters.all(|character| {
            start(character)
                || matches!(character,
                    '-' | '.' | '0'..='9' | '\u{B7}' |
                    '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
        })
}

fn valid_xml_text(value: &str) -> bool {
    value.chars().all(|character| {
        matches!(character,
            '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' |
            '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TemplateValidation {
    pub template_count: usize,
    pub slot_count: usize,
    /// Contribution from both typed template IDs and owner-local slot IDs.
    /// Zero for no resources; root folds this into its existing next_id high-water mark.
    pub max_id: u32,
    /// Logical field bytes plus accounted validation scratch, not allocator/RSS bytes.
    pub logical_bytes: usize,
    pub scan_work: usize,
}

impl TemplateValidation {
    fn bytes(&mut self, amount: usize) -> crate::Result<()> {
        self.logical_bytes = self.logical_bytes.checked_add(amount).ok_or_else(|| invalid("byte budget overflow"))?;
        if self.logical_bytes > MAX_TEMPLATE_BYTES {
            return Err(invalid("resource byte budget exceeded"));
        }
        Ok(())
    }

    fn work(&mut self, amount: usize) -> crate::Result<()> {
        self.scan_work = self.scan_work.checked_add(amount).ok_or_else(|| invalid("scan budget overflow"))?;
        if self.scan_work > MAX_TEMPLATE_SCAN_WORK {
            return Err(invalid("resource scan budget exceeded"));
        }
        Ok(())
    }
}

fn multiplied(count: usize, width: usize) -> crate::Result<usize> {
    count.checked_mul(width).ok_or_else(|| invalid("resource accounting overflow"))
}

#[derive(Clone, Copy)]
struct NameEntry<'a> {
    hash: u64,
    name: &'a str,
}

/// Read-only validator for native loading, commands and every core export entry point.
/// This never matches/deduplicates resources by role, value or legacy layout fallback.
/// The native parser's own byte/value profile must run before deserializing allocations.
pub fn validate(presentation: &Presentation) -> crate::Result<TemplateValidation> {
    let resources = &presentation.presentation_templates;
    if resources.len() > MAX_TEMPLATES || presentation.slides.len() > MAX_TEMPLATE_SLIDE_SCAN {
        return Err(invalid("too many resources or page references"));
    }
    let mut stats = TemplateValidation { template_count: resources.len(), ..TemplateValidation::default() };
    stats.bytes(multiplied(resources.len(), size_of::<PresentationTemplate>())?)?;
    stats.bytes(multiplied(presentation.slides.len(), size_of::<Option<PresentationTemplateId>>())?)?;
    // Account all ID/name index storage before reserving it. Keys borrow existing strings.
    stats.bytes(multiplied(resources.len(), size_of::<PresentationTemplateId>())?)?;
    stats.bytes(multiplied(resources.len(), size_of::<NameEntry<'_>>())?)?;
    stats.work(multiplied(resources.len(), 32)?)?;
    stats.work(presentation.slides.len())?;
    let mut largest_owner = 0usize;
    for template in resources {
        if template.slots.len() > MAX_SLOTS_PER_TEMPLATE {
            return Err(invalid("too many slots in one template"));
        }
        stats.slot_count = stats.slot_count.checked_add(template.slots.len()).ok_or_else(|| invalid("slot count overflow"))?;
        if stats.slot_count > MAX_TOTAL_SLOTS {
            return Err(invalid("too many template slots"));
        }
        largest_owner = largest_owner.max(template.slots.len());
        stats.bytes(multiplied(template.slots.len(), size_of::<PresentationTemplateSlot>())?)?;
        stats.work(multiplied(template.slots.len(), 16)?)?;
        stats.max_id = stats.max_id.max(template.id.0);
        stats.bytes(template.name.len())?;
        stats.work(template.name.len())?;
        if !valid_ncname(&template.name) {
            return Err(invalid("resource name must be a bounded XML NCName"));
        }
        if let Some(label) = &template.display_name {
            if label.len() > MAX_TEMPLATE_NAME_BYTES {
                return Err(invalid("display label is too long"));
            }
            stats.bytes(label.len())?;
            stats.work(label.len())?;
            if !valid_xml_text(label) {
                return Err(invalid("display label contains a non-XML character"));
            }
        }
        for slot in &template.slots {
            stats.max_id = stats.max_id.max(slot.id.0);
            slot.bounds.validate()?;
        }
    }
    stats.bytes(multiplied(largest_owner, size_of::<PresentationTemplateSlotId>())?)?;
    // A second linear pass hashes names; equality work below is separately charged even
    // for adversarial hash collisions. The hash is an optimization, not identity proof.
    let mut ids = Vec::new();
    ids.try_reserve_exact(resources.len()).map_err(|_| invalid("cannot allocate template ID index"))?;
    let mut names = Vec::new();
    names.try_reserve_exact(resources.len()).map_err(|_| invalid("cannot allocate template name index"))?;
    let mut owner_slots = Vec::new();
    owner_slots.try_reserve_exact(largest_owner).map_err(|_| invalid("cannot allocate slot ID index"))?;
    for template in resources {
        ids.push(template.id);
        stats.work(template.name.len())?;
        let mut hash = DefaultHasher::new();
        hash.write(template.name.as_bytes());
        names.push(NameEntry { hash: hash.finish(), name: &template.name });
        owner_slots.clear();
        owner_slots.extend(template.slots.iter().map(|slot| slot.id));
        if repeated(&mut owner_slots) {
            return Err(invalid("duplicate slot ID within one template owner"));
        }
    }
    if repeated(&mut ids) {
        return Err(invalid("duplicate template ID"));
    }
    // Sorting compares only fixed-width hashes. Long common string prefixes cannot make
    // all sort comparisons expensive; exact same-hash comparisons have a fallible budget.
    names.sort_unstable_by_key(|entry| entry.hash);
    let mut start = 0usize;
    while let Some(first) = names.get(start) {
        let mut end = start.checked_add(1).ok_or_else(|| invalid("name scan overflow"))?;
        while names.get(end).is_some_and(|entry| entry.hash == first.hash) {
            end = end.checked_add(1).ok_or_else(|| invalid("name scan overflow"))?;
        }
        let group = names.get(start..end).ok_or_else(|| invalid("invalid name index range"))?;
        for (index, entry) in group.iter().enumerate() {
            let previous = group.get(..index).ok_or_else(|| invalid("invalid name comparison range"))?;
            for other in previous {
                let bytes = entry.name.len().min(other.name.len()).checked_add(1).ok_or_else(|| invalid("name comparison overflow"))?;
                stats.work(bytes)?;
                if entry.name == other.name {
                    return Err(invalid("duplicate template resource name"));
                }
            }
        }
        start = end;
    }
    for slide in &presentation.slides {
        if let Some(id) = slide.presentation_template {
            // <= 10,000 sorted IDs: at most 14 binary-search comparisons plus accounting.
            stats.work(16)?;
            if ids.binary_search(&id).is_err() {
                return Err(invalid("page selects a missing presentation template"));
            }
        }
    }
    Ok(stats)
}

fn repeated<T: Ord>(values: &mut [T]) -> bool {
    values.sort_unstable();
    values.windows(2).any(|pair| match (pair.first(), pair.get(1)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    })
}
