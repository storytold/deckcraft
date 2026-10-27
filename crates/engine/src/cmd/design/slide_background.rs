use super::*;

pub(super) fn background(s: &mut Session, p: &Value) -> Result<Value> {
    if bool_or(p, "copy", false) {
        return copy_slide_background(s);
    }
    let props = p.get("pictureProps").filter(|v| v.is_object()).cloned();
    let transparency = f64_param(p, "transparency");
    let reset = bool_or(p, "reset", false);
    let color = color_param(p, "color");
    let style = usize_param(p, "style");
    let op = if reset {
        BgOp::Set(None)
    } else if let Some(mut c) = color {
        if let Some(t) = transparency {
            set_solid_transparency(&mut c, t);
        }
        BgOp::Set(Some(Background::Fill { fill: Fill::solid(c) }))
    } else if let Some(i) = style {
        // Background styles: 3 theme bg fills × 4 colours (bg1, bg2, tx1, tx2).
        let slots =
            [deckcraft_color::SchemeSlot::Bg1, deckcraft_color::SchemeSlot::Bg2, deckcraft_color::SchemeSlot::Tx2, deckcraft_color::SchemeSlot::Tx1];
        let i = i.clamp(1, 12) - 1;
        BgOp::Set(Some(Background::Ref {
            idx: 1001 + (i / 4) as u32,
            color: deckcraft_model::ColorRef::scheme(slots.get(i % 4).copied().unwrap_or(deckcraft_color::SchemeSlot::Bg1)),
        }))
    } else if let Some(g) = p.get("gradient") {
        let mut g = crate::cmd::shape::gradient_param(g).ok_or_else(|| bad("design.background", "gradient needs two or more `stops`"))?;
        g.rotate_with_shape = false;
        BgOp::Set(Some(Background::Fill { fill: Fill::Gradient(g) }))
    } else if let Some(pt) = p.get("pattern") {
        BgOp::Set(Some(Background::Fill {
            fill: Fill::Pattern(deckcraft_model::style::PatternFill {
                preset: pt.get("preset").and_then(Value::as_str).unwrap_or("pct50").to_string(),
                fg: pt.get("fg").and_then(color_value).unwrap_or(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Accent1)),
                bg: pt.get("bg").and_then(color_value).unwrap_or(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Bg1)),
            }),
        }))
    } else if p.get("picture").is_some() {
        let (name, bytes) =
            crate::cmd::insert::media_bytes(&with_param(p, "data", p.get("picture").cloned().unwrap_or_default()), "design.background")?;
        let ct = crate::cmd::insert::content_type(&name, &bytes);
        let media = s.edit(|doc, _| doc.add_media(&name, ct, bytes).ok_or_else(|| bad("design.background", NO_ID_SPACE)))?;
        let mut pf = deckcraft_model::style::PictureFill { media, ..Default::default() };
        if let Some(pp) = &props {
            apply_picture_props(&mut pf, pp);
        }
        if let Some(t) = transparency {
            set_picture_transparency(&mut pf, t);
        }
        BgOp::Set(Some(Background::Fill { fill: Fill::Picture(pf) }))
    } else if props.is_some() || transparency.is_some() {
        BgOp::Adjust
    } else {
        return Err(bad(
            "design.background",
            "give `color`, `gradient`, `pattern`, `picture`, `style`, `reset`, `pictureProps`, `transparency` or `copy`",
        ));
    };
    let all = bool_or(p, "all", false);
    let index = usize_param(p, "index");
    if matches!(op, BgOp::Adjust) {
        let next = {
            let st = s.doc()?;
            let current = background_of(&st.doc, &st.selection, index);
            let picture = matches!(current, Some(Background::Fill { fill: Fill::Picture(_) }));
            let solid = matches!(current, Some(Background::Fill { fill: Fill::Solid { .. } }));
            if props.is_some() && !picture {
                return Err(bad("design.background", "picture options need a picture background"));
            }
            if transparency.is_some() && !picture && !solid {
                return Err(bad("design.background", "transparency needs a solid or picture background"));
            }
            let mut next = current.clone();
            if let Some(bg) = next.as_mut() {
                adjust_background(bg, props.as_ref(), transparency);
            }
            if next == current { None } else { Some(next) }
        };
        let Some(next) = next else { return ok() };
        s.edit(|doc, sel| {
            write_background(doc, sel, index, false, next);
            Ok(())
        })?;
        return ok();
    }
    let BgOp::Set(bg) = op else { return ok() };
    let unchanged = {
        let st = s.doc()?;
        backgrounds_already(&st.doc, &st.selection, index, all, &bg)
    };
    if unchanged {
        return ok();
    }
    s.edit(|doc, sel| {
        write_background(doc, sel, index, all, bg);
        Ok(())
    })?;
    ok()
}

enum BgOp {
    Set(Option<Background>),
    Adjust,
}

fn background_of(doc: &Presentation, sel: &crate::Selection, index: Option<usize>) -> Option<Background> {
    match sel.target {
        Target::Master { master } => doc.masters.get(master).and_then(|m| m.background.clone()),
        Target::Layout { master, layout } => doc.masters.get(master).and_then(|m| m.layouts.get(layout)).and_then(|l| l.background.clone()),
        t @ (Target::Handout | Target::NotesMaster) => crate::page_master(doc, t).and_then(|m| m.background.clone()),
        Target::Slides => doc.slides.get(index.unwrap_or(sel.slide)).and_then(|sl| sl.background.clone()),
    }
}

fn used_master_indices(doc: &Presentation) -> BTreeSet<usize> {
    let fallback = (!doc.masters.is_empty()).then_some(0);
    doc.slides.iter().filter_map(|sl| doc.masters.iter().position(|m| m.layout(sl.layout).is_some()).or(fallback)).collect()
}

fn backgrounds_already(doc: &Presentation, sel: &crate::Selection, index: Option<usize>, all: bool, bg: &Option<Background>) -> bool {
    match sel.target {
        Target::Master { master } => doc.masters.get(master).is_some_and(|m| m.background == *bg),
        Target::Layout { master, layout } => doc.masters.get(master).and_then(|m| m.layouts.get(layout)).is_some_and(|l| l.background == *bg),
        t @ (Target::Handout | Target::NotesMaster) => crate::page_master(doc, t).is_some_and(|m| m.background == *bg),
        Target::Slides => {
            let list: Vec<usize> = if all { (0..doc.slides.len()).collect() } else { vec![index.unwrap_or(sel.slide)] };
            let slides_match = !list.is_empty() && list.iter().all(|i| doc.slides.get(*i).is_some_and(|sl| sl.background == *bg));
            if all && bg.is_some() {
                let masters = used_master_indices(doc);
                let masters_match = !masters.is_empty() && masters.iter().all(|&idx| doc.masters.get(idx).is_some_and(|m| m.background == *bg));
                let layouts_match = !masters.is_empty()
                    && masters.iter().all(|&idx| doc.masters.get(idx).is_some_and(|m| m.layouts.iter().all(|l| l.background.is_none())));
                slides_match && masters_match && layouts_match
            } else {
                slides_match
            }
        }
    }
}

fn write_background(doc: &mut Presentation, sel: &crate::Selection, index: Option<usize>, all: bool, bg: Option<Background>) {
    match sel.target {
        Target::Master { master } => {
            if let Some(m) = doc.masters.get_mut(master) {
                Arc::make_mut(m).background = bg;
            }
        }
        Target::Layout { master, layout } => {
            if let Some(l) = doc.masters.get_mut(master).and_then(|m| Arc::make_mut(m).layouts.get_mut(layout)) {
                l.background = bg;
            }
        }
        t @ (Target::Handout | Target::NotesMaster) => {
            if let Some(m) = crate::page_master_mut(doc, t) {
                Arc::make_mut(m).background = bg;
            }
        }
        Target::Slides => {
            if all && bg.is_some() {
                for idx in used_master_indices(doc) {
                    if let Some(m) = doc.masters.get_mut(idx) {
                        let m = Arc::make_mut(m);
                        m.background = bg.clone();
                        for l in &mut m.layouts {
                            l.background = None;
                        }
                    }
                }
            }
            let list: Vec<usize> = if all { (0..doc.slides.len()).collect() } else { vec![index.unwrap_or(sel.slide)] };
            for i in list {
                if let Some(sl) = doc.slides.get_mut(i) {
                    Arc::make_mut(sl).background = bg.clone();
                }
            }
        }
    }
}

fn copy_slide_background(s: &mut Session) -> Result<Value> {
    let bg = {
        let st = s.doc()?;
        if !matches!(st.selection.target, Target::Slides) {
            return Err(bad("design.background", "copy applies to slides"));
        }
        let bg = st.doc.slides.get(st.selection.slide).and_then(|sl| sl.background.clone());
        let slides_match = st.doc.slides.iter().all(|sl| sl.background == bg);
        let masters_match = if bg.is_some() {
            let masters = used_master_indices(&st.doc);
            !masters.is_empty() && masters.iter().all(|&idx| st.doc.masters.get(idx).is_some_and(|m| m.background == bg))
        } else {
            true
        };
        if slides_match && masters_match {
            return ok();
        }
        bg
    };
    s.edit(|doc, sel| {
        write_background(doc, sel, None, true, bg);
        Ok(())
    })?;
    ok()
}

fn adjust_background(bg: &mut Background, props: Option<&Value>, transparency: Option<f64>) {
    match bg {
        Background::Fill { fill: Fill::Picture(pf) } => {
            if let Some(pp) = props {
                apply_picture_props(pf, pp);
            }
            if let Some(t) = transparency {
                set_picture_transparency(pf, t);
            }
        }
        Background::Fill { fill: Fill::Solid { color } } => {
            if let Some(t) = transparency {
                set_solid_transparency(color, t);
            }
        }
        _ => {}
    }
}

fn set_picture_transparency(pf: &mut deckcraft_model::style::PictureFill, t: f64) {
    let a = 1.0 - t.clamp(0.0, 1.0);
    pf.alpha = (a < 1.0).then_some(a);
}

fn set_solid_transparency(color: &mut deckcraft_model::ColorRef, t: f64) {
    color.mods.retain(|m| !matches!(m, deckcraft_color::ColorTransform::Alpha(_)));
    let t = t.clamp(0.0, 1.0);
    if t > 0.0 {
        color.mods.push(deckcraft_color::ColorTransform::Alpha(((1.0 - t) * 100_000.0).round() as i32));
    }
}

/// Picture-fill options for Format Background (tile, offsets, scale, alignment, mirror, stretch).
fn apply_picture_props(pf: &mut deckcraft_model::style::PictureFill, pp: &Value) {
    use deckcraft_model::style::PictureMode;
    let f = |k: &str| pp.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
    if let Some(up) = pp.get("rotateWithShape").and_then(Value::as_bool) {
        pf.upright = !up;
    }
    match pp.get("tile").and_then(Value::as_bool) {
        Some(true) if !matches!(pf.mode, PictureMode::Tile { .. }) => {
            pf.mode = PictureMode::Tile { tx: 0.0, ty: 0.0, sx: 1.0, sy: 1.0, flip: "none".into(), align: "tl".into() };
        }
        Some(false) if !matches!(pf.mode, PictureMode::Stretch { .. }) => pf.mode = PictureMode::default(),
        _ => {}
    }
    match &mut pf.mode {
        PictureMode::Tile { tx, ty, sx, sy, flip, align } => {
            if let Some(v) = f("offsetX") {
                *tx = v.clamp(-4000.0, 4000.0);
            }
            if let Some(v) = f("offsetY") {
                *ty = v.clamp(-4000.0, 4000.0);
            }
            if let Some(v) = f("scaleX") {
                *sx = v.clamp(0.01, 10.0);
            }
            if let Some(v) = f("scaleY") {
                *sy = v.clamp(0.01, 10.0);
            }
            if let Some(m) = pp.get("mirror").and_then(Value::as_str).filter(|m| matches!(*m, "none" | "x" | "y" | "xy")) {
                *flip = m.to_string();
            }
            if let Some(a) =
                pp.get("align").and_then(Value::as_str).filter(|a| matches!(*a, "tl" | "t" | "tr" | "l" | "ctr" | "r" | "bl" | "b" | "br"))
            {
                *align = a.to_string();
            }
        }
        PictureMode::Stretch { fill_rect } => {
            if let Some(arr) = pp.get("stretch").and_then(Value::as_array) {
                for (slot, v) in fill_rect.iter_mut().zip(arr) {
                    if let Some(v) = v.as_f64().filter(|v| v.is_finite()) {
                        *slot = v.clamp(-10.0, 0.99);
                    }
                }
            }
        }
    }
}

pub(super) fn hide_bg_graphics(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, sel| {
        if let Some(sl) = doc.slides.get_mut(sel.slide) {
            let sl = Arc::make_mut(sl);
            sl.show_master_shapes = !bool_param(p, "hide").unwrap_or(sl.show_master_shapes);
        }
        Ok(())
    })?;
    ok()
}
