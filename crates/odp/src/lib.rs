//! Editable ODP interchange, based on OASIS OpenDocument 1.3 parts 2 and 3.
//! Native geometry, text, styles, tables, charts and media map to the presentation model.
//! Full ODF fidelity is incomplete; imports retain source bytes for recovery.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod annotation_metadata;
mod comments_codec;
mod comments_extension;
mod geometry;
mod loss_report;
mod notes_styles;
mod preserve;
mod read;
mod source_effects;
mod table;
mod table_number_styles;
mod write;
mod xml;

use std::io::{Cursor, Read};

pub use read::import;
pub use write::export;

pub const MIME: &str = "application/vnd.oasis.opendocument.presentation";
const MAX_PART: u64 = 32 * 1024 * 1024;
const MAX_TOTAL: u64 = 128 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("ODP: {0}")]
pub struct OdpError(pub String);

type Result<T> = std::result::Result<T, OdpError>;

/// Check newly diagnosed source-only effects before regenerating an older native document.
/// A saved warning is never treated as proof: the retained source is freshly imported.
pub fn validate_source_diagnostics(presentation: &deckcraft_model::Presentation) -> Result<()> {
    let mut sources = presentation.media.iter().filter(|media| media.content_type == "application/vnd.deckcraft.odp-source");
    let Some(source) = sources.next() else {
        if presentation.tags.iter().any(|(key, value)| {
            key == "odp:imported"
                || (key == "odp:warning"
                    && [source_effects::GAMMA_WARNING, source_effects::PROTECTION_WARNING, source_effects::CALCULATION_WARNING]
                        .contains(&value.as_str()))
        }) {
            return Err(OdpError("cannot verify missing retained ODP source diagnostics".into()));
        }
        return Ok(());
    };
    if sources.next().is_some() || source.link.is_some() || source.data.is_empty() {
        return Err(OdpError("cannot verify retained ODP source diagnostics".into()));
    }
    let baseline = import(&source.data)?;
    if baseline
        .tags
        .iter()
        .any(|(key, value)| key == "odp:warning" && (value.starts_with("ODF annotation") || value.starts_with("ODF comment extension")))
        && !presentation
            .tags
            .iter()
            .any(|(key, value)| key == "odp:warning" && (value.starts_with("ODF annotation") || value.starts_with("ODF comment extension")))
    {
        return Err(OdpError("retained ODF annotation diagnostics cannot be cleared before faithful interchange export".into()));
    }
    for (warning, feature) in [
        (source_effects::GAMMA_WARNING, "picture-gamma"),
        (source_effects::PROTECTION_WARNING, "frame-protection"),
        (source_effects::CALCULATION_WARNING, "table-calculation-settings"),
    ] {
        if baseline.tags.iter().any(|(key, value)| key == "odp:warning" && value == warning)
            && (!presentation.tags.iter().any(|(key, value)| key == "odp:warning" && value == warning)
                || !presentation.tags.iter().any(|(key, value)| key == "odp:imported" && value == "1"))
        {
            return Err(OdpError(format!(
                "retained ODP source requires the {feature} diagnostic; reopen the original source before interchange export"
            )));
        }
    }
    Ok(())
}

/// Recognise the package MIME type, regardless of filename or ZIP entry order.
pub fn sniff(bytes: &[u8]) -> bool {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes)) else { return false };
    part(&mut zip, "mimetype", &mut 0).is_ok_and(|b| b == MIME.as_bytes())
}

fn part(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, total: &mut u64) -> Result<Vec<u8>> {
    let file = zip.by_name(name).map_err(|e| OdpError(format!("{name}: {e}")))?;
    if file.size() > MAX_PART || total.saturating_add(file.size()) > MAX_TOTAL {
        return Err(OdpError("package exceeds the import size limit".into()));
    }
    let mut data = Vec::new();
    file.take(MAX_PART + 1).read_to_end(&mut data).map_err(|e| OdpError(e.to_string()))?;
    *total = total.saturating_add(data.len() as u64);
    if data.len() as u64 > MAX_PART || *total > MAX_TOTAL {
        return Err(OdpError("package exceeds the import size limit".into()));
    }
    Ok(data)
}
