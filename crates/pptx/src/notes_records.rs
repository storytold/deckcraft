//! Closed typed records shared by the notes reader and writer.
use std::sync::Arc;

use deckcraft_geom::Size;
use deckcraft_model::{
    Master,
    notes::{NotesMaster, NotesPage},
};
use serde::{Deserialize, Serialize};

use crate::notes_projection::BodyStyle;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Dependency {
    pub part: String,
    pub content_type: String,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MediaBinding {
    pub original: u32,
    pub relation: String,
    pub name: String,
    pub content_type: String,
    pub external: Option<String>,
    pub original_link: Option<String>,
    pub dependency: Option<Dependency>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SlideBinding {
    pub original: u32,
    pub part: String,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TableSource {
    pub master: u32,
    /// Notes-master layouts have no DrawingML notes-layout part. They are
    /// unused, typed native resources, never a projected table authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<u32>,
    pub shape: u32,
    pub projected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Resources {
    pub legacy: Option<Arc<Master>>,
    pub named: Vec<NotesMaster>,
    pub size: Size,
    pub slides: Vec<SlideBinding>,
    pub media: Vec<MediaBinding>,
    pub table_sources: Vec<TableSource>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MasterRecord {
    pub projection: String,
    pub relationships: String,
    pub theme: Dependency,
    pub resources: Resources,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PageRecord {
    pub projection: String,
    pub relationships: String,
    pub resources: String,
    pub original_slide: u32,
    pub body_file_id: u32,
    pub original: Option<NotesPage>,
    pub body_style: BodyStyle,
    pub theme: Dependency,
    pub theme_relationships: String,
    pub counters: Counters,
    pub tables: Vec<crate::write::table_projection::notes::Binding>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Counters {
    pub image: u32,
    pub media: u32,
    pub chart: u32,
    pub diagram: u32,
    pub tags: u32,
    pub chartex: u32,
    pub kept: u32,
    pub placeholder: Option<String>,
}

impl Counters {
    pub(crate) fn validate(&self) -> Result<(), crate::PptxError> {
        if [self.image, self.media, self.chart, self.diagram, self.tags, self.chartex, self.kept].iter().any(|value| *value > 50_000) {
            return Err(crate::notes_codec::invalid("notes projection counter exceeds package limits"));
        }
        if self
            .placeholder
            .as_ref()
            .is_some_and(|part| part.len() > 256 || !part.starts_with("ppt/media/image") || !part.ends_with(".png") || part.contains(".."))
        {
            return Err(crate::notes_codec::invalid("invalid notes placeholder part identity"));
        }
        Ok(())
    }
}
