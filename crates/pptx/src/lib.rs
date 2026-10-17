//! PPTX import and export: Office Open XML PresentationML / DrawingML, written from the public
//! ECMA-376 standard.
//!
//! [`import`] reads a package leniently (unknown parts are skipped, malformed parts are logged and
//! ignored, every size and depth is bounded) into the DeckCraft model; [`export`] writes a valid
//! package that PowerPoint opens without repair.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod cfb;
pub mod chat;
mod custgeom;
pub mod custom_ui;
mod dgm;
mod maps;
mod opc;
mod png;
pub(crate) mod read;
mod tables;
mod vba;
mod write;
mod xml;

/// The bounded XML reader the importer uses, for callers that read kept fragments (a shape's
/// 3-D settings) instead of scanning strings.
pub use xml::{Doc as XmlDoc, El as XmlEl, parse as parse_xml};

use deckcraft_model::Presentation;

/// Extension URI of the PowerPoint 2010 section list in `presentation.xml`.
pub(crate) const SECTION_EXT_URI: &str = "{521415D9-36F7-43E2-AB2F-B90AF26B5E84}";

/// Extension URI of the modern-comments anchor in a slide (names the threaded comment part).
pub(crate) const COMMENTS_EXT_URI: &str = "{6950BFC3-D8DA-4A85-94F7-54DA5524770B}";

#[derive(Debug, thiserror::Error)]
pub enum PptxError {
    #[error("not a PPTX file: {0}")]
    NotPptx(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("write failed: {0}")]
    Write(String),
    #[error("malformed XML: {0}")]
    Xml(String),
}

/// Does this look like an OOXML package (zip with `[Content_Types].xml`)?
///
/// The part is looked up in the zip's central directory, not in the leading bytes: writers may
/// store it anywhere in the archive (recent PowerPoint builds put it last, megabytes in).
pub fn sniff(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"PK") {
        return false;
    }
    zip::ZipArchive::new(std::io::Cursor::new(bytes)).map(|z| z.index_for_name("[Content_Types].xml").is_some()).unwrap_or(false)
}

/// Is this a password-protected presentation (an encrypted package inside a compound file)?
pub fn is_encrypted(bytes: &[u8]) -> bool {
    cfb::is_encrypted(bytes)
}

/// Read a `.pptx` / `.potx` / `.ppsx` package, or a macro-enabled `.pptm` / `.potm` / `.ppsm`
/// (whose VBA project is kept, never run, in [`Presentation::vba_project`]).
pub fn import(bytes: &[u8]) -> Result<Presentation, PptxError> {
    read::import(bytes)
}

/// Read the theme of a `.thmx` file, or of a `.pptx` / `.potx`'s first master, with its media.
pub fn import_theme(bytes: &[u8]) -> Result<(deckcraft_model::theme::Theme, Vec<deckcraft_model::MediaItem>), PptxError> {
    read::import_theme(bytes)
}

/// Write a `.pptx` package.
pub fn export(p: &Presentation) -> Result<Vec<u8>, PptxError> {
    write::export(p, PptxKind::Presentation)
}

/// Write a package of `kind`. Only the macro-enabled kinds carry the VBA project; the others
/// leave it (and the parts it relates to) out, as PowerPoint does.
pub fn export_as(p: &Presentation, kind: PptxKind) -> Result<Vec<u8>, PptxError> {
    write::export(p, kind)
}

/// Which kind of package `bytes` is, by its presentation part's content type. `None` for
/// anything that isn't a PresentationML package.
pub fn kind_of(bytes: &[u8]) -> Option<PptxKind> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut read = |name: &str| -> Option<Vec<u8>> {
        use std::io::Read;
        let f = zip.by_name(name).ok()?;
        let mut out = vec![];
        f.take(opc::MAX_XML_PART as u64).read_to_end(&mut out).ok()?;
        Some(out)
    };
    let root = xml::parse(&read("_rels/.rels")?).ok()?;
    let main = root
        .root
        .children_named("Relationship")
        .find(|r| r.attr("Type").is_some_and(|t| t.ends_with("/officeDocument")))
        .and_then(|r| r.attr("Target"))
        .map(|t| opc::resolve("", t))?;
    let types = xml::parse(&read("[Content_Types].xml")?).ok()?;
    let want = format!("/{}", opc::norm(&main));
    let ct =
        types.root.children_named("Override").find(|o| o.attr("PartName").is_some_and(|n| n.eq_ignore_ascii_case(&want)))?.attr("ContentType")?;
    ALL_KINDS.into_iter().find(|k| k.main_content_type().eq_ignore_ascii_case(ct))
}

const ALL_KINDS: [PptxKind; 6] =
    [PptxKind::Presentation, PptxKind::Template, PptxKind::Show, PptxKind::MacroPresentation, PptxKind::MacroTemplate, PptxKind::MacroShow];

/// Which kind of PresentationML package to write. The presentation part's content type differs
/// per kind (ECMA-376 Part 1 §13.3.6, MS-OFFMACRO2 for the macro-enabled ones), and PowerPoint
/// refuses a file whose content type doesn't match its extension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PptxKind {
    /// `.pptx`
    #[default]
    Presentation,
    /// `.potx`
    Template,
    /// `.ppsx`
    Show,
    /// `.pptm`: may carry a VBA project.
    MacroPresentation,
    /// `.potm`: may carry a VBA project.
    MacroTemplate,
    /// `.ppsm`: may carry a VBA project.
    MacroShow,
}

impl PptxKind {
    /// The kind a file extension (without the dot, any case) names.
    pub fn from_ext(ext: &str) -> Option<PptxKind> {
        match ext.to_ascii_lowercase().as_str() {
            "pptx" => Some(PptxKind::Presentation),
            "potx" => Some(PptxKind::Template),
            "ppsx" => Some(PptxKind::Show),
            "pptm" => Some(PptxKind::MacroPresentation),
            "potm" => Some(PptxKind::MacroTemplate),
            "ppsm" => Some(PptxKind::MacroShow),
            _ => None,
        }
    }

    /// The kind a path's extension names.
    pub fn from_path(path: &str) -> Option<PptxKind> {
        std::path::Path::new(path).extension().and_then(|e| e.to_str()).and_then(PptxKind::from_ext)
    }

    /// The extension (lower case, no dot).
    pub fn ext(self) -> &'static str {
        match self {
            PptxKind::Presentation => "pptx",
            PptxKind::Template => "potx",
            PptxKind::Show => "ppsx",
            PptxKind::MacroPresentation => "pptm",
            PptxKind::MacroTemplate => "potm",
            PptxKind::MacroShow => "ppsm",
        }
    }

    /// Content type of the presentation part.
    pub fn main_content_type(self) -> &'static str {
        match self {
            PptxKind::Presentation => "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
            PptxKind::Template => "application/vnd.openxmlformats-officedocument.presentationml.template.main+xml",
            PptxKind::Show => "application/vnd.openxmlformats-officedocument.presentationml.slideshow.main+xml",
            PptxKind::MacroPresentation => "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml",
            PptxKind::MacroTemplate => "application/vnd.ms-powerpoint.template.macroEnabled.main+xml",
            PptxKind::MacroShow => "application/vnd.ms-powerpoint.slideshow.macroEnabled.main+xml",
        }
    }

    /// Can this package hold macros?
    pub fn macros(self) -> bool {
        matches!(self, PptxKind::MacroPresentation | PptxKind::MacroTemplate | PptxKind::MacroShow)
    }

    /// The macro-enabled kind of the same family (a template stays a template, a show a show).
    pub fn with_macros(self) -> PptxKind {
        match self {
            PptxKind::Presentation | PptxKind::MacroPresentation => PptxKind::MacroPresentation,
            PptxKind::Template | PptxKind::MacroTemplate => PptxKind::MacroTemplate,
            PptxKind::Show | PptxKind::MacroShow => PptxKind::MacroShow,
        }
    }
}

/// Content type for a file name's extension.
pub(crate) fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "webp" => "image/webp",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "wmv" => "video/x-ms-wmv",
        "avi" => "video/x-msvideo",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "wma" => "audio/x-ms-wma",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "glb" => "model/gltf-binary",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "fntdata" => "application/x-fontdata",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod import_theme_tests {
    use std::io::{Read, Write};

    use deckcraft_model::Presentation;

    fn custom() -> Presentation {
        let mut p = Presentation::default();
        let m = std::sync::Arc::make_mut(&mut p.masters[0]);
        m.theme.name = "Harbour".into();
        m.theme.fonts.major.latin = "Lato".into();
        m.theme.fonts.minor.latin = "Source Sans 3".into();
        m.theme.colors.set(deckcraft_color::SchemeSlot::Accent1, deckcraft_color::Rgba::rgb(0x12, 0x34, 0x56));
        p
    }

    /// A `.thmx`: the deck's theme part as the package's main document.
    fn thmx(pptx: &[u8]) -> Vec<u8> {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(pptx)).unwrap();
        let mut theme = String::new();
        z.by_name("ppt/theme/theme1.xml").unwrap().read_to_string(&mut theme).unwrap();
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        out.start_file("[Content_Types].xml", o).unwrap();
        out.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/theme/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/></Types>"#).unwrap();
        out.start_file("_rels/.rels", o).unwrap();
        out.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="theme/theme/theme1.xml"/></Relationships>"#).unwrap();
        out.start_file("theme/theme/theme1.xml", o).unwrap();
        out.write_all(theme.as_bytes()).unwrap();
        out.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_the_theme_of_a_pptx_and_of_a_thmx() {
        let pptx = crate::export(&custom()).unwrap();
        for bytes in [pptx.clone(), thmx(&pptx)] {
            let (t, _) = crate::import_theme(&bytes).unwrap();
            assert_eq!(t.name, "Harbour");
            assert_eq!(t.fonts.major.latin, "Lato");
            assert_eq!(t.fonts.minor.latin, "Source Sans 3");
            assert_eq!(t.colors.get(deckcraft_color::SchemeSlot::Accent1), deckcraft_color::Rgba::rgb(0x12, 0x34, 0x56));
        }
    }

    #[test]
    fn non_theme_packages_are_errors() {
        assert!(crate::import_theme(b"not a zip").is_err());
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        out.start_file("[Content_Types].xml", zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(b"<Types/>").unwrap();
        assert!(crate::import_theme(&out.finish().unwrap().into_inner()).is_err());
    }
}
