//! Macro projects: the VBA project of a `.pptm` / `.potm` / `.ppsm` (MS-OFFMACRO2) and the parts
//! it relates to (signatures, VBA data…), carried byte for byte. Never parsed, never run: only the
//! relationships and content types are read.

use std::sync::Arc;

use deckcraft_model::{Presentation, VbaPart};

use crate::opc::{self, CT_VBA_PROJECT, MAX_VBA_PART, MAX_VBA_RELATED, Package, PackageOut, RT_VBA_PROJECT, Rels, RelsOut, VBA_PROJECT_PART};

/// Can a part the project relates to be written under `name`? Only plain names inside `ppt/`
/// (letters, digits, `.`, `_`, `-`), never a relationship part, the presentation or the project.
fn usable_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("ppt/") else { return false };
    let lower = name.to_ascii_lowercase();
    !rest.is_empty()
        && name.len() <= 260
        && rest.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.eq_ignore_ascii_case("_rels"))
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'/' | b'.' | b'_' | b'-'))
        && !lower.ends_with(".rels")
        && lower != "ppt/presentation.xml"
        && !name.eq_ignore_ascii_case(VBA_PROJECT_PART)
}

/// A content type we can write as is.
fn usable_content_type(ct: &str) -> bool {
    !ct.is_empty() && ct.len() <= 255 && ct.contains('/') && ct.bytes().all(|c| c.is_ascii_graphic())
}

/// A relationship type we can write as is.
fn usable_rel_type(ty: &str) -> bool {
    ty.starts_with("http") && ty.len() <= 1024 && ty.bytes().all(|c| c.is_ascii_graphic())
}

/// Keep the VBA project the presentation part relates to, and the parts it relates to.
/// Anything missing, oversized or odd is logged and left out; the deck itself always opens.
pub fn read(pkg: &Package, pres_rels: &Rels, p: &mut Presentation) {
    let Some(rel) = pres_rels.items.iter().find(|r| !r.external && (r.ty == RT_VBA_PROJECT || r.kind == "vbaProject")) else { return };
    let Some(bytes) = pkg.get(&rel.target) else {
        log::warn!("pptx: the VBA project {} is missing; the presentation opens without macros", rel.target);
        return;
    };
    if bytes.is_empty() {
        return;
    }
    if bytes.len() > MAX_VBA_PART {
        log::warn!("pptx: the VBA project {} is too large ({} bytes); left out", rel.target, bytes.len());
        return;
    }
    p.vba_project = Some(Arc::new(bytes.to_vec()));
    for r in pkg.rels(&rel.target).items.iter().filter(|r| !r.external) {
        // Part names compare case-insensitively (OPC); keep the spelling the zip has.
        let name = pkg.original_name(&r.target).unwrap_or(&r.target).trim_start_matches('/').to_string();
        if !usable_name(&name) || name.eq_ignore_ascii_case(&rel.target) || !usable_rel_type(&r.ty) {
            log::warn!("pptx: not carrying VBA-related part {name:?}");
            continue;
        }
        if p.vba_parts.iter().any(|v| v.name.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let Some(data) = pkg.get(&name) else {
            log::warn!("pptx: VBA-related part {name} is missing; keeping the project without it");
            continue;
        };
        if data.len() > MAX_VBA_PART {
            log::warn!("pptx: VBA-related part {name} is too large; left out");
            continue;
        }
        if p.vba_parts.len() >= MAX_VBA_RELATED {
            log::warn!("pptx: more than {MAX_VBA_RELATED} VBA-related parts; the rest are dropped");
            break;
        }
        let content_type = pkg.content_type(&name).filter(|c| usable_content_type(c)).unwrap_or_else(|| "application/octet-stream".into());
        p.vba_parts.push(VbaPart { rel_type: r.ty.clone(), name, content_type, data: Arc::new(data.to_vec()) });
    }
}

/// Write the VBA project at `ppt/vbaProject.bin` with the parts it relates to and add its
/// relationship to `pres_rels`. Call after every other `ppt/` part is in `pkg`: a related part
/// never replaces one the writer produced.
pub fn write(p: &Presentation, pkg: &mut PackageOut, pres_rels: &mut RelsOut) {
    let Some(project) = p.vba_project.as_ref().filter(|b| !b.is_empty()) else { return };
    let mut rels = RelsOut::default();
    for part in p.vba_parts.iter().take(MAX_VBA_RELATED) {
        if !usable_name(&part.name) || !usable_rel_type(&part.rel_type) || part.data.len() > MAX_VBA_PART || pkg.has(&part.name) {
            log::warn!("pptx: not writing VBA-related part {:?}", part.name);
            continue;
        }
        let ct = if usable_content_type(&part.content_type) { part.content_type.as_str() } else { "application/octet-stream" };
        rels.add_new(&part.rel_type, &opc::relative(VBA_PROJECT_PART, &part.name), false);
        pkg.binary_override(&part.name, ct, part.data.to_vec());
    }
    pres_rels.add(RT_VBA_PROJECT, &opc::relative("ppt/presentation.xml", VBA_PROJECT_PART), false);
    pkg.binary_override(VBA_PROJECT_PART, CT_VBA_PROJECT, project.to_vec());
    pkg.rels(VBA_PROJECT_PART, &rels);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(usable_name("ppt/vbaProjectSignature.bin"));
        assert!(usable_name("ppt/sub/vbaData.xml"));
        for bad in [
            "ppt/presentation.xml",
            "PPT/vbaProject.bin",
            "ppt/vbaProject.bin",
            "ppt/_rels/presentation.xml.rels",
            "ppt/../x.bin",
            "ppt//x.bin",
            "docProps/core.xml",
            "[Content_Types].xml",
            "ppt/a:b.bin",
            "ppt/",
        ] {
            assert!(!usable_name(bad), "{bad}");
        }
    }
}
