//! DrawingML writing: colours, fills, lines, effects, transforms and text.

use deckcraft_geom::{Xfrm, pt_to_emu};
use deckcraft_model::style::{ColorBase, ColorRef, Compound, Dash, Effects, Fill, GradientShape, Line, LineCap, LineJoin, PictureFill, PictureMode};
use deckcraft_model::text::{
    Action, AutoFit, BodyProps, Bullet, Caps, Hyperlink, ListStyle, ParaProps, Paragraph, RunKind, RunProps, Spacing, Strike, TextBody,
};

use super::{Exp, Out};
use crate::xml::{A, W};

pub fn emu(pt: f64) -> i64 {
    pt_to_emu(pt).clamp(-27_273_042_316_900, 27_273_042_316_900)
}

/// Positive coordinate extent (ST_PositiveCoordinate).
pub fn emu_pos(pt: f64) -> i64 {
    emu(pt).max(0)
}

/// Fraction → 1/100 000.
pub fn pct(v: f64) -> i64 {
    if v.is_finite() { (v * 100_000.0).round().clamp(-2.0e9, 2.0e9) as i64 } else { 0 }
}

/// Degrees → 1/60 000 degree, normalised to 0..360.
pub fn ang(deg: f64) -> i64 {
    if !deg.is_finite() {
        return 0;
    }
    let d = deg.rem_euclid(360.0);
    ((d * 60_000.0).round() as i64).clamp(0, 21_599_999)
}

pub fn color(w: &mut W, c: &ColorRef) {
    let (tag, a) = match &c.base {
        ColorBase::Rgb { rgb } => ("a:srgbClr", A::new().a("val", rgb.hex())),
        ColorBase::Scheme { slot } => ("a:schemeClr", A::new().a("val", slot.xml_name())),
        ColorBase::Preset { name } => ("a:prstClr", A::new().a("val", if name.is_empty() { "black" } else { name })),
        ColorBase::System { name, last } => {
            ("a:sysClr", A::new().a("val", if name.is_empty() { "windowText" } else { name }).a("lastClr", last.hex()))
        }
    };
    let rgb_alpha = match &c.base {
        ColorBase::Rgb { rgb } if rgb.a < 255 && !c.mods.iter().any(|m| matches!(m, deckcraft_color::ColorTransform::Alpha(_))) => {
            Some((rgb.a as i64 * 100_000 / 255) as i32)
        }
        _ => None,
    };
    if c.mods.is_empty() && rgb_alpha.is_none() {
        w.empty(tag, a);
        return;
    }
    w.open(tag, a);
    for m in &c.mods {
        let name = format!("a:{}", m.xml_name());
        match m.value() {
            Some(v) => w.val(&name, v),
            None => w.empty0(&name),
        }
    }
    if let Some(al) = rgb_alpha {
        w.val("a:alpha", al);
    }
    w.close(tag);
}

pub fn solid(w: &mut W, c: &ColorRef) {
    w.open0("a:solidFill");
    color(w, c);
    w.close("a:solidFill");
}

/// Write a fill. `Fill::Background` (only meaningful on `p:sp`) writes nothing.
pub fn fill(w: &mut W, x: &mut Exp, o: &mut Out, f: &Fill) {
    match f {
        Fill::None => w.empty0("a:noFill"),
        Fill::Solid { color: c } => solid(w, c),
        Fill::Gradient(g) => {
            w.open("a:gradFill", A::new().a("rotWithShape", if g.rotate_with_shape { "1" } else { "0" }));
            w.open0("a:gsLst");
            let mut stops = g.stops.clone();
            if stops.is_empty() {
                stops.push(deckcraft_model::style::GradientStop { pos: 0.0, color: ColorRef::rgb(deckcraft_color::Rgba::WHITE) });
            }
            if stops.len() == 1 {
                let mut s = stops[0].clone();
                s.pos = 1.0;
                stops.push(s);
            }
            for s in &stops {
                w.open("a:gs", A::new().a("pos", pct(s.pos.clamp(0.0, 1.0))));
                color(w, &s.color);
                w.close("a:gs");
            }
            w.close("a:gsLst");
            match &g.shape {
                GradientShape::Linear { angle, scaled } => {
                    w.empty("a:lin", A::new().a("ang", ang(*angle)).a("scaled", if *scaled { "1" } else { "0" }))
                }
                GradientShape::Path { path, focus } => {
                    let p = match path.as_str() {
                        "rect" | "shape" | "circle" => path.as_str(),
                        _ => "circle",
                    };
                    w.open("a:path", A::new().a("path", p));
                    w.empty("a:fillToRect", A::new().a("l", pct(focus[0])).a("t", pct(focus[1])).a("r", pct(focus[2])).a("b", pct(focus[3])));
                    w.close("a:path");
                }
            }
            w.close("a:gradFill");
        }
        Fill::Picture(pf) => blip_fill(w, x, o, "a:blipFill", pf),
        Fill::Pattern(p) => {
            w.open("a:pattFill", A::new().a("prst", if p.preset.is_empty() { "pct5" } else { &p.preset }));
            w.open0("a:fgClr");
            color(w, &p.fg);
            w.close("a:fgClr");
            w.open0("a:bgClr");
            color(w, &p.bg);
            w.close("a:bgClr");
            w.close("a:pattFill");
        }
        Fill::Group => w.empty0("a:grpFill"),
        Fill::Background => {}
    }
}

/// `a:blipFill` / `p:blipFill`.
pub fn blip_fill(w: &mut W, x: &mut Exp, o: &mut Out, tag: &str, pf: &PictureFill) {
    let rid = x.media_rel(o, pf.media, "image").unwrap_or_else(|| x.placeholder_rel(o));
    w.open(tag, A::new().a("rotWithShape", "1"));
    let adj = &pf.adjust;
    let has_kids =
        pf.alpha.is_some() || adj.brightness != 0.0 || adj.contrast != 0.0 || adj.grayscale || adj.duotone.is_some() || adj.clear_color.is_some();
    let a = A::new().a("r:embed", &rid);
    if has_kids {
        w.open("a:blip", a);
        if let Some(c) = adj.clear_color {
            w.open0("a:clrChange");
            w.open0("a:clrFrom");
            color(w, &ColorRef::rgb(c));
            w.close("a:clrFrom");
            w.open0("a:clrTo");
            color(w, &ColorRef::rgb(c).with(deckcraft_color::ColorTransform::Alpha(0)));
            w.close("a:clrTo");
            w.close("a:clrChange");
        }
        if let Some(al) = pf.alpha {
            w.empty("a:alphaModFix", A::new().a("amt", pct(al.clamp(0.0, 1.0))));
        }
        if let Some((d, l)) = &adj.duotone {
            w.open0("a:duotone");
            color(w, d);
            color(w, l);
            w.close("a:duotone");
        }
        if adj.grayscale {
            w.empty0("a:grayscl");
        }
        if adj.brightness != 0.0 || adj.contrast != 0.0 {
            w.empty("a:lum", A::new().a("bright", pct(adj.brightness.clamp(-1.0, 1.0))).a("contrast", pct(adj.contrast.clamp(-1.0, 1.0))));
        }
        w.close("a:blip");
    } else {
        w.empty("a:blip", a);
    }
    let c = pf.crop;
    if c.iter().any(|v| *v != 0.0) {
        w.empty("a:srcRect", A::new().a("l", pct(c[0])).a("t", pct(c[1])).a("r", pct(c[2])).a("b", pct(c[3])));
    }
    match &pf.mode {
        PictureMode::Stretch { fill_rect: r } => {
            w.open0("a:stretch");
            if r.iter().any(|v| *v != 0.0) {
                w.empty("a:fillRect", A::new().a("l", pct(r[0])).a("t", pct(r[1])).a("r", pct(r[2])).a("b", pct(r[3])));
            } else {
                w.empty0("a:fillRect");
            }
            w.close("a:stretch");
        }
        PictureMode::Tile { tx, ty, sx, sy, flip, align } => {
            let flip = match flip.as_str() {
                "x" | "y" | "xy" => flip.as_str(),
                _ => "none",
            };
            let algn = match align.as_str() {
                "tl" | "t" | "tr" | "l" | "ctr" | "r" | "bl" | "b" | "br" => align.as_str(),
                _ => "tl",
            };
            w.empty("a:tile", A::new().a("tx", emu(*tx)).a("ty", emu(*ty)).a("sx", pct(*sx)).a("sy", pct(*sy)).a("flip", flip).a("algn", algn));
        }
    }
    w.close(tag);
}

pub fn line(w: &mut W, x: &mut Exp, o: &mut Out, tag: &str, l: &Line) {
    let a = A::new()
        .o("w", l.width.map(|v| emu(v.clamp(0.0, 1584.0))))
        .o(
            "cap",
            l.cap.map(|c| match c {
                LineCap::Round => "rnd",
                LineCap::Square => "sq",
                LineCap::Flat => "flat",
            }),
        )
        .o("cmpd", l.compound.map(Compound::xml));
    w.open(tag, a);
    if let Some(f) = &l.fill {
        match f {
            Fill::None | Fill::Solid { .. } | Fill::Gradient(_) | Fill::Pattern(_) => fill(w, x, o, f),
            _ => {}
        }
    }
    match &l.dash {
        Some(Dash::Preset { name }) => {
            let n = if Dash::PRESETS.contains(&name.as_str()) { name.as_str() } else { "solid" };
            w.val("a:prstDash", n);
        }
        Some(Dash::Custom { pattern }) if !pattern.is_empty() => {
            w.open0("a:custDash");
            for (d, s) in pattern.iter().take(64) {
                w.empty("a:ds", A::new().a("d", pct(*d).max(0)).a("sp", pct(*s).max(0)));
            }
            w.close("a:custDash");
        }
        _ => {}
    }
    match l.join {
        Some(LineJoin::Round) => w.empty0("a:round"),
        Some(LineJoin::Bevel) => w.empty0("a:bevel"),
        Some(LineJoin::Miter) => w.empty0("a:miter"),
        None => {}
    }
    for (t, e) in [("a:headEnd", &l.head), ("a:tailEnd", &l.tail)] {
        if let Some(e) = e {
            let kind = match e.kind.as_str() {
                "triangle" | "stealth" | "diamond" | "oval" | "arrow" => e.kind.as_str(),
                _ => "none",
            };
            let sz = |s: &str| match s {
                "sm" | "lg" => s.to_string(),
                _ => "med".to_string(),
            };
            w.empty(t, A::new().a("type", kind).a("w", sz(&e.w)).a("len", sz(&e.len)));
        }
    }
    w.close(tag);
}

fn shadow(w: &mut W, tag: &str, s: &deckcraft_model::style::Shadow) {
    let mut a = A::new().a("blurRad", emu(s.blur).max(0)).a("dist", emu(s.dist).max(0)).a("dir", ang(s.dir));
    if tag == "a:outerShdw" {
        if s.sx != 1.0 {
            a = a.a("sx", pct(s.sx));
        }
        if s.sy != 1.0 {
            a = a.a("sy", pct(s.sy));
        }
        if s.kx != 0.0 {
            a = a.a("kx", (s.kx.clamp(-89.0, 89.0) * 60_000.0).round() as i64);
        }
        if s.ky != 0.0 {
            a = a.a("ky", (s.ky.clamp(-89.0, 89.0) * 60_000.0).round() as i64);
        }
        let algn = match s.align.as_str() {
            "tl" | "t" | "tr" | "l" | "ctr" | "r" | "bl" | "b" | "br" => s.align.as_str(),
            _ => "b",
        };
        a = a.a("algn", algn).a("rotWithShape", if s.rotate_with_shape { "1" } else { "0" });
    }
    w.open(tag, a);
    color(w, &s.color);
    w.close(tag);
}

pub fn effects(w: &mut W, fx: &Effects) {
    w.open0("a:effectLst");
    if let Some(g) = &fx.glow {
        w.open("a:glow", A::new().a("rad", emu(g.radius).max(0)));
        color(w, &g.color);
        w.close("a:glow");
    }
    if let Some(s) = &fx.inner_shadow {
        shadow(w, "a:innerShdw", s);
    }
    if let Some(s) = &fx.outer_shadow {
        shadow(w, "a:outerShdw", s);
    }
    if let Some(r) = &fx.reflection {
        w.empty(
            "a:reflection",
            A::new()
                .a("blurRad", emu(r.blur).max(0))
                .a("stA", pct(r.start_alpha.clamp(0.0, 1.0)))
                .a("endA", pct(r.end_alpha.clamp(0.0, 1.0)))
                .a("endPos", pct(r.end_pos.clamp(0.0, 1.0)))
                .a("dist", emu(r.dist).max(0))
                .a("dir", 5_400_000)
                .a("sy", -100_000)
                .a("algn", "bl")
                .a("rotWithShape", "0"),
        );
    }
    if let Some(r) = fx.soft_edge {
        w.empty("a:softEdge", A::new().a("rad", emu(r).max(0)));
    }
    w.close("a:effectLst");
}

pub fn xfrm(w: &mut W, tag: &str, x: &Xfrm, child: Option<&Xfrm>) {
    let x = if x.is_finite() { *x } else { Xfrm::default() };
    let rot = ang(x.rot);
    w.open(tag, A::new().o("rot", (rot != 0).then_some(rot)).t("flipH", x.flip_h).t("flipV", x.flip_v));
    w.empty("a:off", A::new().a("x", emu(x.x)).a("y", emu(x.y)));
    w.empty("a:ext", A::new().a("cx", emu_pos(x.w)).a("cy", emu_pos(x.h)));
    if let Some(c) = child {
        let c = if c.is_finite() { *c } else { Xfrm::default() };
        w.empty("a:chOff", A::new().a("x", emu(c.x)).a("y", emu(c.y)));
        w.empty("a:chExt", A::new().a("cx", emu_pos(c.w)).a("cy", emu_pos(c.h)));
    }
    w.close(tag);
}

// ---------------------------------------------------------------------------------------------
// Text

pub fn hyperlink(w: &mut W, x: &mut Exp, o: &mut Out, tag: &str, h: &Hyperlink) {
    let (rid, action) = match &h.action {
        Action::Url { url } => {
            if url.is_empty() {
                return;
            }
            (o.rels.add("hyperlink", url, true), None)
        }
        Action::Slide { slide } => match x.slide_parts.get(slide) {
            Some(target) => {
                let rel = crate::opc::relative(&o.name, target);
                (o.rels.add("slide", &rel, false), Some("ppaction://hlinksldjump".to_string()))
            }
            None => return,
        },
        Action::NextSlide => (String::new(), Some("ppaction://hlinkshowjump?jump=nextslide".into())),
        Action::PreviousSlide => (String::new(), Some("ppaction://hlinkshowjump?jump=previousslide".into())),
        Action::FirstSlide => (String::new(), Some("ppaction://hlinkshowjump?jump=firstslide".into())),
        Action::LastSlide => (String::new(), Some("ppaction://hlinkshowjump?jump=lastslide".into())),
        Action::EndShow => (String::new(), Some("ppaction://hlinkshowjump?jump=endshow".into())),
        Action::LastViewed => (String::new(), Some("ppaction://hlinkshowjump?jump=lastslideviewed".into())),
        Action::CustomShow { name } => {
            let id = x.p.custom_shows.iter().position(|c| c.name == *name).unwrap_or(0);
            (String::new(), Some(format!("ppaction://customshow?id={id}&return=true")))
        }
        Action::Program { path } => {
            if path.is_empty() {
                return;
            }
            (o.rels.add("hyperlink", path, true), Some("ppaction://program".into()))
        }
        Action::PlayMedia => (String::new(), Some("ppaction://media".into())),
    };
    w.empty(
        tag,
        A::new()
            .a("r:id", rid)
            .o("action", action)
            .o("tooltip", (!h.tooltip.is_empty()).then_some(h.tooltip.as_str()))
            .t("highlightClick", h.highlight_click),
    );
}

pub fn body_pr(w: &mut W, b: &BodyProps) {
    let a = A::new()
        .o("rot", b.rot.map(|r| (r * 60_000.0).round().clamp(-21_600_000.0, 21_600_000.0) as i64))
        .o("vert", b.vert.map(|v| v.xml()))
        .o("wrap", b.wrap.map(|v| if v { "square" } else { "none" }))
        .o("lIns", b.inset_l.map(emu))
        .o("tIns", b.inset_t.map(emu))
        .o("rIns", b.inset_r.map(emu))
        .o("bIns", b.inset_b.map(emu))
        .o("numCol", b.columns.map(|c| c.clamp(1, 16)))
        .o("spcCol", b.col_spacing.map(|v| emu(v).max(0)))
        .b("rtlCol", b.rtl_col)
        .o("anchor", b.anchor.map(|a| a.xml()))
        .b("anchorCtr", b.anchor_ctr)
        .b("upright", b.upright);
    let warp = b.warp.as_deref().filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()));
    if warp.is_none() && b.autofit.is_none() {
        w.empty("a:bodyPr", a);
        return;
    }
    w.open("a:bodyPr", a);
    if let Some(p) = warp {
        w.open("a:prstTxWarp", A::new().a("prst", p));
        w.empty0("a:avLst");
        w.close("a:prstTxWarp");
    }
    match b.autofit {
        Some(AutoFit::None) => w.empty0("a:noAutofit"),
        Some(AutoFit::Shape) => w.empty0("a:spAutoFit"),
        Some(AutoFit::Shrink { font_scale, line_reduction }) => {
            let fs = pct(font_scale.clamp(0.01, 1.0));
            let lr = pct(line_reduction.clamp(0.0, 1.0));
            w.empty("a:normAutofit", A::new().o("fontScale", (fs != 100_000).then_some(fs)).o("lnSpcReduction", (lr != 0).then_some(lr)));
        }
        None => {}
    }
    w.close("a:bodyPr");
}

fn spacing(w: &mut W, tag: &str, s: &Spacing) {
    w.open0(tag);
    match s {
        Spacing::Pct(v) => w.val("a:spcPct", pct(v.clamp(0.0, 132.0))),
        Spacing::Pts(v) => w.val("a:spcPts", (v.clamp(0.0, 1584.0) * 100.0).round() as i64),
    }
    w.close(tag);
}

pub fn is_default_ppr(p: &ParaProps) -> bool {
    *p == ParaProps::default()
}

/// `a:pPr` / `a:lvlNpPr`.
pub fn ppr(w: &mut W, x: &mut Exp, o: &mut Out, tag: &str, level: Option<u8>, p: &ParaProps, def: Option<&RunProps>) {
    let a = A::new()
        .o("marL", p.margin_left.map(|v| emu(v.clamp(0.0, 4032.0))))
        .o("marR", p.margin_right.map(|v| emu(v.clamp(0.0, 4032.0))))
        .o("lvl", level.filter(|l| *l > 0).map(|l| l.min(8)))
        .o("indent", p.indent.map(|v| emu(v.clamp(-4032.0, 4032.0))))
        .o("algn", p.align.map(|a| a.xml()))
        .o("defTabSz", p.default_tab.map(|v| emu(v).max(0)))
        .b("rtl", p.rtl)
        .b("eaLnBrk", p.east_asian_line_break)
        .o("fontAlgn", p.font_align.as_deref().filter(|f| matches!(*f, "auto" | "t" | "ctr" | "base" | "b")));
    let has_kids = p.line_spacing.is_some()
        || p.space_before.is_some()
        || p.space_after.is_some()
        || p.bullet.is_some()
        || p.bullet_font.is_some()
        || p.bullet_color.is_some()
        || p.bullet_size.is_some()
        || p.tabs.is_some()
        || def.is_some();
    if !has_kids {
        w.empty(tag, a);
        return;
    }
    w.open(tag, a);
    if let Some(s) = &p.line_spacing {
        spacing(w, "a:lnSpc", s);
    }
    if let Some(s) = &p.space_before {
        spacing(w, "a:spcBef", s);
    }
    if let Some(s) = &p.space_after {
        spacing(w, "a:spcAft", s);
    }
    if let Some(c) = &p.bullet_color {
        w.open0("a:buClr");
        color(w, c);
        w.close("a:buClr");
    }
    if let Some(s) = p.bullet_size {
        w.val("a:buSzPct", pct(s.clamp(0.25, 4.0)));
    }
    if let Some(f) = &p.bullet_font {
        w.empty("a:buFont", A::new().a("typeface", f));
    }
    match &p.bullet {
        Some(Bullet::None) => w.empty0("a:buNone"),
        Some(Bullet::Char { char }) => w.empty("a:buChar", A::new().a("char", if char.is_empty() { "•" } else { char })),
        Some(Bullet::AutoNum { scheme, start_at }) => {
            let s = if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric()) { "arabicPeriod" } else { scheme };
            w.empty("a:buAutoNum", A::new().a("type", s).o("startAt", (*start_at != 1).then_some((*start_at).clamp(1, 32767))));
        }
        Some(Bullet::Picture { media }) => match x.media_rel(o, *media, "image") {
            Some(rid) => {
                w.open0("a:buBlip");
                w.empty("a:blip", A::new().a("r:embed", rid));
                w.close("a:buBlip");
            }
            None => w.empty("a:buChar", A::new().a("char", "•")),
        },
        None => {}
    }
    if let Some(t) = &p.tabs {
        w.open0("a:tabLst");
        for tab in t.iter().take(32) {
            let al = match tab.align.as_str() {
                "ctr" | "r" | "dec" => tab.align.as_str(),
                _ => "l",
            };
            w.empty("a:tab", A::new().a("pos", emu(tab.pos)).a("algn", al));
        }
        w.close("a:tabLst");
    }
    if let Some(d) = def {
        rpr(w, x, o, "a:defRPr", d);
    }
    w.close(tag);
}

fn typeface(w: &mut W, tag: &str, f: &Option<String>) {
    if let Some(f) = f {
        w.empty(tag, A::new().a("typeface", f));
    }
}

/// `a:rPr`, `a:defRPr`, `a:endParaRPr`.
pub fn rpr(w: &mut W, x: &mut Exp, o: &mut Out, tag: &str, r: &RunProps) {
    let a = A::new()
        .o("lang", r.lang.as_deref().filter(|l| !l.is_empty()))
        .o("sz", r.size.map(|s| (s.clamp(1.0, 4000.0) * 100.0).round() as i64))
        .b("b", r.bold)
        .b("i", r.italic)
        .o("u", r.underline.as_deref().filter(|u| !u.is_empty() && u.chars().all(|c| c.is_ascii_alphanumeric())))
        .o(
            "strike",
            r.strike.map(|s| match s {
                Strike::None => "noStrike",
                Strike::Single => "sngStrike",
                Strike::Double => "dblStrike",
            }),
        )
        .o("kern", r.kern.map(|k| (k.clamp(0.0, 4000.0) * 100.0).round() as i64))
        .o(
            "cap",
            r.caps.map(|c| match c {
                Caps::None => "none",
                Caps::Small => "small",
                Caps::All => "all",
            }),
        )
        .o("spc", r.spacing.map(|s| (s.clamp(-4000.0, 4000.0) * 100.0).round() as i64))
        .o("baseline", r.baseline.map(|b| pct(b.clamp(-10.0, 10.0))))
        .b("noProof", r.no_proof);
    let has_kids = r.outline.is_some()
        || r.fill.is_some()
        || r.shadow.is_some()
        || r.glow.is_some()
        || r.highlight.is_some()
        || r.underline_color.is_some()
        || r.font.is_some()
        || r.font_ea.is_some()
        || r.font_cs.is_some()
        || r.font_sym.is_some()
        || r.link.is_some();
    if !has_kids {
        w.empty(tag, a);
        return;
    }
    w.open(tag, a);
    if let Some(l) = &r.outline {
        line(w, x, o, "a:ln", l);
    }
    if let Some(f) = &r.fill {
        match f {
            Fill::Background => {}
            f => fill(w, x, o, f),
        }
    }
    if r.shadow.is_some() || r.glow.is_some() {
        effects(w, &Effects { outer_shadow: r.shadow.clone(), glow: r.glow.clone(), ..Default::default() });
    }
    if let Some(h) = &r.highlight {
        w.open0("a:highlight");
        color(w, h);
        w.close("a:highlight");
    }
    if let Some(c) = &r.underline_color {
        w.open0("a:uFill");
        solid(w, c);
        w.close("a:uFill");
    }
    typeface(w, "a:latin", &r.font);
    typeface(w, "a:ea", &r.font_ea);
    typeface(w, "a:cs", &r.font_cs);
    typeface(w, "a:sym", &r.font_sym);
    if let Some(h) = &r.link {
        hyperlink(w, x, o, "a:hlinkClick", h);
    }
    w.close(tag);
}

/// `a:lstStyle`-like element (`a:lstStyle`, `p:titleStyle`, `p:defaultTextStyle`…).
pub fn list_style(w: &mut W, x: &mut Exp, o: &mut Out, tag: &str, ls: &ListStyle) {
    if ls.is_empty() {
        w.empty0(tag);
        return;
    }
    w.open0(tag);
    for (i, l) in ls.levels.iter().enumerate().take(9) {
        if let Some(l) = l {
            let t = format!("a:lvl{}pPr", i + 1);
            let def = (l.run != RunProps::default()).then_some(&l.run);
            ppr(w, x, o, &t, None, &l.para, def);
        }
    }
    w.close(tag);
}

/// Write `<tag>` text body (`p:txBody`, `a:txBody`). `math` selects the OMML form of equations.
pub fn text_body(w: &mut W, x: &mut Exp, o: &mut Out, tag: &str, tb: &TextBody, math: bool) {
    text_body_mode(w, x, o, tag, tb, if math { MathMode::Direct } else { MathMode::Text });
}

/// Table cells: equations sit inside a paragraph-level `mc:AlternateContent` (OMML for readers
/// that know it, plain text otherwise), since the cell cannot be wrapped as a whole.
pub fn text_body_cell(w: &mut W, x: &mut Exp, o: &mut Out, tag: &str, tb: &TextBody) {
    text_body_mode(w, x, o, tag, tb, MathMode::Alternate);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MathMode {
    Text,
    Direct,
    Alternate,
}

fn text_body_mode(w: &mut W, x: &mut Exp, o: &mut Out, tag: &str, tb: &TextBody, math: MathMode) {
    w.open0(tag);
    body_pr(w, &tb.body);
    list_style(w, x, o, "a:lstStyle", &tb.list_style);
    if tb.paragraphs.is_empty() {
        w.empty0("a:p");
    }
    for (pi, p) in tb.paragraphs.iter().enumerate() {
        paragraph_mode(w, x, o, p, math, pi);
    }
    w.close(tag);
}

fn run_text(w: &mut W, x: &mut Exp, o: &mut Out, text: &str, props: &RunProps) {
    for (i, piece) in text.split(['\n', '\u{b}']).enumerate() {
        if i > 0 {
            w.open0("a:br");
            rpr(w, x, o, "a:rPr", props);
            w.close("a:br");
        }
        if piece.is_empty() && i > 0 {
            continue;
        }
        w.open0("a:r");
        rpr(w, x, o, "a:rPr", props);
        w.elt("a:t", piece);
        w.close("a:r");
    }
}

fn paragraph_mode(w: &mut W, x: &mut Exp, o: &mut Out, p: &Paragraph, math: MathMode, index: usize) {
    w.open0("a:p");
    if p.level > 0 || !is_default_ppr(&p.props) {
        ppr(w, x, o, "a:pPr", Some(p.level), &p.props, None);
    }
    for (ri, r) in p.runs.iter().enumerate() {
        match &r.kind {
            RunKind::Text => run_text(w, x, o, &r.text, &r.props),
            RunKind::Break => {
                w.open0("a:br");
                rpr(w, x, o, "a:rPr", &r.props);
                w.close("a:br");
            }
            RunKind::Field { field } => {
                let id = crate::tables::guid(&format!("{}:{index}:{ri}:{field}", o.name));
                let ty = (!field.is_empty() && field.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then_some(field.as_str());
                w.open("a:fld", A::new().a("id", id).o("type", ty));
                rpr(w, x, o, "a:rPr", &r.props);
                w.elt("a:t", &r.text);
                w.close("a:fld");
            }
            RunKind::Math { omml } => {
                if math != MathMode::Text && !omml.is_empty() {
                    let alt = math == MathMode::Alternate;
                    if alt {
                        w.open("mc:AlternateContent", A::new().a("xmlns:mc", crate::opc::NS_MC));
                        w.open("mc:Choice", A::new().a("xmlns:a14", crate::opc::NS_A14).a("Requires", "a14"));
                    }
                    w.open("a14:m", A::new().a("xmlns:a14", crate::opc::NS_A14));
                    if r.props == RunProps::default() {
                        w.raw(omml);
                    } else {
                        let mut f = W::frag();
                        rpr(&mut f, x, o, "a:rPr", &r.props);
                        w.raw(&with_run_props(omml, &f.s));
                    }
                    w.close("a14:m");
                    if alt {
                        w.close("mc:Choice");
                        w.open0("mc:Fallback");
                        run_text(w, x, o, &r.text, &r.props);
                        w.close("mc:Fallback");
                        w.close("mc:AlternateContent");
                    }
                } else {
                    run_text(w, x, o, &r.text, &r.props);
                }
            }
        }
    }
    if p.end_props != RunProps::default() {
        rpr(w, x, o, "a:endParaRPr", &p.end_props);
    }
    w.close("a:p");
}

/// Give every `m:r` of `omml` that has no `a:rPr` the run properties `rpr_xml` (PowerPoint keeps an
/// equation's size, font and colour in an `a:rPr` inside each math run, after `m:rPr`).
fn with_run_props(omml: &str, rpr_xml: &str) -> String {
    let mut out = String::with_capacity(omml.len() + rpr_xml.len());
    let mut rest = omml;
    while let Some(i) = rest.find("<m:r") {
        let (head, tail) = rest.split_at(i);
        out.push_str(head);
        let after = tail.get(4..).unwrap_or("");
        let is_run = after.starts_with('>') || after.starts_with(' ');
        let Some(open_end) = tail.find('>').filter(|_| is_run) else {
            // `<m:rPr`, `<m:rad` and friends, or a truncated tag: copy the opener and go on.
            out.push_str("<m:r");
            rest = after;
            continue;
        };
        if tail.get(..open_end).is_some_and(|t| t.ends_with('/')) {
            out.push_str(tail.get(..=open_end).unwrap_or(""));
            rest = tail.get(open_end + 1..).unwrap_or("");
            continue;
        }
        out.push_str(tail.get(..=open_end).unwrap_or(""));
        let mut body = tail.get(open_end + 1..).unwrap_or("");
        let end = body.find("</m:r>").unwrap_or(body.len());
        if body.get(..end).is_some_and(|b| b.contains("<a:rPr")) {
            rest = body;
            continue;
        }
        // Skip a leading `m:rPr` so the properties follow it.
        if body.starts_with("<m:rPr/>") {
            out.push_str("<m:rPr/>");
            body = body.get(8..).unwrap_or("");
        } else if body.starts_with("<m:rPr>")
            && let Some(c) = body.find("</m:rPr>")
        {
            out.push_str(body.get(..c + 8).unwrap_or(""));
            body = body.get(c + 8..).unwrap_or("");
        }
        out.push_str(rpr_xml);
        rest = body;
    }
    out.push_str(rest);
    out
}

pub fn has_math(tb: &TextBody) -> bool {
    tb.paragraphs.iter().flat_map(|p| &p.runs).any(|r| matches!(&r.kind, RunKind::Math { omml } if !omml.is_empty()))
}

#[cfg(test)]
mod math_props_tests {
    use super::with_run_props;

    #[test]
    fn props_go_after_m_rpr_and_only_where_missing() {
        let p = "<a:rPr sz=\"2800\"/>";
        let got = with_run_props(
            "<m:oMath><m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>x</m:t></m:r><m:r><m:t>y</m:t></m:r><m:rad><m:radPr/></m:rad></m:oMath>",
            p,
        );
        assert_eq!(
            got,
            "<m:oMath><m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><a:rPr sz=\"2800\"/><m:t>x</m:t></m:r><m:r><a:rPr sz=\"2800\"/><m:t>y</m:t></m:r><m:rad><m:radPr/></m:rad></m:oMath>"
        );
        // Already has properties: untouched. Hostile / truncated input does not panic.
        let own = "<m:r><a:rPr sz=\"100\"/><m:t>x</m:t></m:r>";
        assert_eq!(with_run_props(own, p), own);
        for t in ["<m:r", "<m:r ", "<m:r>", "<m:r/>", "<m:rPr>", "<m:r><m:rPr>", "", "<m:r x=\"\u{e9}"] {
            let _ = with_run_props(t, p);
        }
    }
}
