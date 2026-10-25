//! PPTX import and export: Office Open XML PresentationML / DrawingML, written from the public
//! ECMA-376 standard.
//!
//! [`import`] reads a package leniently (unknown parts are skipped, malformed parts are logged and
//! ignored, every size and depth is bounded) into the DeckCraft model; [`export`] writes a valid
//! package that PowerPoint opens without repair.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod am3d;
mod cfb;
mod custgeom;
mod dgm;
mod maps;
mod opc;
mod png;
pub(crate) mod read;
mod tables;
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

/// Read a `.pptx` / `.potx` / `.ppsx` package.
pub fn import(bytes: &[u8]) -> Result<Presentation, PptxError> {
    read::import(bytes)
}

/// [`import`], plus the part names of slides the file lists but that couldn't be read and were
/// left out.
pub fn import_checked(bytes: &[u8]) -> Result<(Presentation, Vec<String>), PptxError> {
    read::import_checked(bytes)
}

/// Read the theme of a `.thmx` file, or of a `.pptx` / `.potx`'s first master, with its media.
pub fn import_theme(bytes: &[u8]) -> Result<(deckcraft_model::theme::Theme, Vec<deckcraft_model::MediaItem>), PptxError> {
    read::import_theme(bytes)
}

/// Write a `.pptx` package.
pub fn export(p: &Presentation) -> Result<Vec<u8>, PptxError> {
    write::export(p, write::CT_PRES)
}

/// Write a package for `ext`: `potx` (template) and `ppsx` (slide show) differ from `pptx` only in
/// the main part's content type.
pub fn export_as(p: &Presentation, ext: &str) -> Result<Vec<u8>, PptxError> {
    let ct = match ext {
        "potx" => write::CT_TEMPLATE,
        "ppsx" => write::CT_SHOW,
        _ => write::CT_PRES,
    };
    write::export(p, ct)
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
