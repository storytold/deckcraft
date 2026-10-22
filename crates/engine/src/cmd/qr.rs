//! Insert ▸ QR Code (#154): a QR code for a link or any text, drawn as vector modules on a
//! white square so it stays sharp at any size and in PPTX and PDF.

use deckcraft_geom::Xfrm;
use deckcraft_model::style::Line;
use deckcraft_model::text::{Action, Hyperlink};
use deckcraft_model::{ColorRef, CustomPath, Fill, Geom, Rgba, Shape, ShapeId, ShapeKind};
use qrcode::{EcLevel, QrCode};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

/// Light modules around the code, as the QR standard asks.
const QUIET: usize = 4;
/// Longest text we encode (a version 40 code holds about 2,900 bytes at level L).
const MAX_TEXT: usize = 2900;

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "insert.qrCode",
        "QR Code…",
        ["Insert", "Text"],
        None,
        "{text: string (a link or any text), rect?: [x,y,w,h], level?: L|M|Q|H (error correction, default M), link?: bool (click opens the link in a show, default true for URLs)}",
        has_slide,
        insert_qr
    )]
}

/// The dark modules of `text`'s QR code as one path, a rectangle per run of dark modules in a
/// row, in a square of `width + 2 × QUIET` units; and that size.
pub fn qr_path(text: &str, level: EcLevel) -> Option<(String, usize)> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), level).ok()?;
    let n = code.width();
    let colors = code.to_colors();
    let mut d = String::new();
    for y in 0..n {
        let mut x = 0;
        while x < n {
            if colors.get(y * n + x) != Some(&qrcode::Color::Dark) {
                x += 1;
                continue;
            }
            let start = x;
            while x < n && colors.get(y * n + x) == Some(&qrcode::Color::Dark) {
                x += 1;
            }
            let (x0, x1, y0) = (start + QUIET, x + QUIET, y + QUIET);
            d.push_str(&format!("M {x0} {y0} L {x1} {y0} L {x1} {} L {x0} {} Z ", y0 + 1, y0 + 1));
        }
    }
    Some((d.trim_end().to_string(), n + 2 * QUIET))
}

fn insert_qr(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| bad("insert.qrCode", "missing `text`"))?;
    if text.len() > MAX_TEXT {
        return Err(bad("insert.qrCode", format!("text is {} bytes; a QR code holds at most {MAX_TEXT}", text.len())));
    }
    let level = match str_param(p, "level").unwrap_or("M") {
        "L" | "l" => EcLevel::L,
        "Q" | "q" => EcLevel::Q,
        "H" | "h" => EcLevel::H,
        _ => EcLevel::M,
    };
    let (d, units) = qr_path(text, level).ok_or_else(|| bad("insert.qrCode", "this text doesn't fit a QR code at that error correction level"))?;
    let rect = rect_param(p, "rect").unwrap_or_else(|| {
        let size = s.active().map(|d| d.doc.slide_size).unwrap_or(deckcraft_model::defaults::WIDE);
        let side = 144.0;
        Xfrm::new((size.width - side) / 2.0, (size.height - side) / 2.0, side, side)
    });
    let is_url = ["http://", "https://", "mailto:"].iter().any(|pre| text.to_ascii_lowercase().starts_with(pre));
    let link = p.get("link").and_then(Value::as_bool).unwrap_or(is_url);
    let child = Xfrm::new(0.0, 0.0, rect.w, rect.h);
    let solid = |c: Rgba| Some(Fill::solid(ColorRef::rgb(c)));
    let background =
        Shape { name: "QR Background".into(), xfrm: Some(child), fill: solid(Rgba::WHITE), line: Some(Line::none()), ..Default::default() };
    let modules = Shape {
        name: "QR Modules".into(),
        xfrm: Some(child),
        geom: Geom::Custom { paths: vec![CustomPath { w: units as f64, h: units as f64, d, stroke: false, ..Default::default() }] },
        fill: solid(Rgba::rgb(0, 0, 0)),
        line: Some(Line::none()),
        ..Default::default()
    };
    let group = Shape {
        xfrm: Some(rect),
        kind: ShapeKind::Group { children: vec![background, modules], child },
        name: String::new(),
        descr: format!("QR code: {text}"),
        click: link.then(|| Hyperlink {
            action: Action::Url { url: text.to_string() },
            tooltip: String::new(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
        }),
        ..Default::default()
    };
    let id = s.edit(|doc, sel| {
        let mut g = group;
        g.id = ShapeId(doc.alloc_id().ok_or_else(|| bad("insert.qrCode", NO_ID_SPACE))?);
        if let ShapeKind::Group { children, .. } = &mut g.kind {
            for c in children {
                c.id = ShapeId(doc.alloc_id().ok_or_else(|| bad("insert.qrCode", NO_ID_SPACE))?);
            }
        }
        let n = super::current_shapes(doc, sel).map(|v| v.len()).unwrap_or(0) + 1;
        g.name = format!("QR Code {n}");
        let id = g.id;
        crate::shapes_mut(doc, sel).ok_or_else(|| bad("insert.qrCode", "no slide"))?.push(g);
        sel.shapes = vec![id];
        sel.text = None;
        Ok(id)
    })?;
    Ok(json!({"id": id, "modules": units - 2 * QUIET}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_path_draws_the_finder_patterns() {
        let (d, units) = qr_path("https://github.com/storytold/deckcraft", EcLevel::M).unwrap();
        // Version 3 (29 modules) plus the quiet zone.
        assert_eq!(units, 29 + 2 * QUIET);
        // The top-left finder pattern's first row is 7 dark modules in one run.
        assert!(d.starts_with("M 4 4 L 11 4 L 11 5 L 4 5 Z"), "{}", &d[..60]);
        assert!(qr_path(&"x".repeat(5000), EcLevel::H).is_none());
    }

    #[test]
    fn insert_qr_code_adds_a_linked_group() {
        let mut s = Session::with_new();
        let r = s.execute("insert.qrCode", &json!({"text": "https://example.com"})).unwrap();
        let st = s.doc().unwrap();
        let slide = st.current_slide().unwrap();
        let g = slide.shapes.iter().find(|sh| Some(sh.id.0 as u64) == r["id"].as_u64()).unwrap();
        assert!(g.name.starts_with("QR Code"));
        assert!(matches!(g.click.as_ref().map(|h| &h.action), Some(Action::Url { url }) if url == "https://example.com"));
        assert_eq!(g.children().len(), 2);
        assert!(matches!(g.children()[1].geom, Geom::Custom { .. }));
        let mut ids: Vec<u32> = std::iter::once(g.id.0).chain(g.children().iter().map(|c| c.id.0)).collect();
        ids.dedup();
        assert_eq!(ids.len(), 3);
        // Plain text isn't linked; empty or oversized text is refused; undo removes the code.
        s.execute("insert.qrCode", &json!({"text": "Room 204"})).unwrap();
        let last = s.doc().unwrap().current_slide().unwrap().shapes.last().cloned().unwrap();
        assert!(last.click.is_none());
        assert!(s.execute("insert.qrCode", &json!({"text": "  "})).is_err());
        assert!(s.execute("insert.qrCode", &json!({"text": "x".repeat(4000)})).is_err());
        let before = s.doc().unwrap().current_slide().unwrap().shapes.len();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().current_slide().unwrap().shapes.len(), before - 1);
        // It survives PPTX.
        let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
        let back = deckcraft_pptx::import(&bytes).unwrap();
        let found = back.slides.iter().flat_map(|sl| sl.shapes.iter()).any(|sh| sh.name.starts_with("QR Code") && sh.children().len() == 2);
        assert!(found);
    }
}
