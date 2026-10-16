//! Resource styles and embedded media. Package paths are validated and never fetched.
use super::*;
use deckcraft_model::style::{Gradient, GradientShape, GradientStop, PatternFill, PictureAdjust, PictureMode};

pub(super) fn register<'a>(root: &'a El, styles: &mut Styles<'a>) {
    for (name, family) in [
        ("draw:gradient", "gradient"),
        ("svg:linearGradient", "gradient"),
        ("svg:radialGradient", "gradient"),
        ("draw:hatch", "hatch"),
        ("draw:fill-image", "fill-image"),
        ("draw:stroke-dash", "dash"),
    ] {
        let mut elements = Vec::new();
        root.descendants(name, &mut elements);
        for e in elements {
            if let Some(name) = e.attr("draw:name") {
                styles.insert((family.into(), name.into()), e);
            }
        }
    }
}

pub(super) fn expand(props: &mut Props, styles: &Styles<'_>) {
    for (key, family) in [("draw:fill-gradient-name", "gradient"), ("draw:fill-hatch-name", "hatch"), ("draw:fill-image-name", "fill-image")] {
        if let Some(resource) = props.get(key).and_then(|name| styles.get(&(family.into(), name.clone()))) {
            props.insert(format!("odp:{family}:element"), resource.name.clone());
            if family == "gradient" {
                let stops: Vec<_> = resource.elements().filter(|e| e.name == "svg:stop").collect();
                props.insert("odp:gradient:stop-count".into(), stops.len().to_string());
                for (i, stop) in stops.into_iter().take(4096).enumerate() {
                    for (key, value) in &stop.attrs {
                        props.insert(format!("odp:gradient:stop:{i}:{key}"), value.clone());
                    }
                }
            }
            for (key, value) in &resource.attrs {
                props.entry(format!("odp:{family}:{key}")).or_insert_with(|| value.clone());
            }
        }
    }
}

pub(super) fn fraction(s: &str) -> Option<f64> {
    s.strip_suffix('%').and_then(crate::geometry::number).map(|v| (v / 100.0).clamp(0.0, 1.0))
}

pub(super) fn special_fill(props: &Props) -> Result<Option<Fill>> {
    let get = |key: &str| props.get(&format!("odp:gradient:{key}")).map(String::as_str);
    match props.get("draw:fill").map(String::as_str) {
        Some("gradient") => {
            let opacity = props.get("draw:opacity").and_then(|s| fraction(s)).unwrap_or(1.0);
            if matches!(get("element"), Some("svg:linearGradient" | "svg:radialGradient")) {
                let count = get("stop-count").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                if count == 0 || count > 4096 {
                    return Err(OdpError("invalid SVG gradient stop count".into()));
                }
                let mut stops = Vec::with_capacity(count);
                let mut previous = 0.0;
                for i in 0..count {
                    let at = |key: &str| props.get(&format!("odp:gradient:stop:{i}:svg:{key}")).map(String::as_str);
                    let pos = at("offset")
                        .and_then(|v| fraction(v).or_else(|| crate::geometry::number(v)))
                        .filter(|v| (0.0..=1.0).contains(v) && *v >= previous)
                        .ok_or_else(|| OdpError("invalid gradient stop offset".into()))?;
                    previous = pos;
                    let mut c = at("stop-color").and_then(color).ok_or_else(|| OdpError("invalid gradient stop color".into()))?;
                    let alpha =
                        at("stop-opacity").and_then(|v| fraction(v).or_else(|| crate::geometry::number(v))).unwrap_or(1.0).clamp(0.0, 1.0) * opacity;
                    c = c.with(ColorTransform::Alpha((alpha * 100_000.0).round() as i32));
                    stops.push(GradientStop { pos, color: c });
                }
                if get("svg:spreadMethod").is_some_and(|v| v != "pad") {
                    return Err(OdpError("unsupported repeating or reflecting gradient spread".into()));
                }
                let coord = |key: &str, default: f64| -> Result<f64> {
                    get(key).map_or(Ok(default), |v| {
                        v.strip_suffix('%')
                            .and_then(crate::geometry::number)
                            .map(|v| v / 100.0)
                            .or_else(|| crate::geometry::number(v))
                            .ok_or_else(|| OdpError("invalid gradient coordinate".into()))
                    })
                };
                let w = props.get("odp:width").and_then(|v| crate::geometry::number(v)).unwrap_or(1.0);
                let h = props.get("odp:height").and_then(|v| crate::geometry::number(v)).unwrap_or(1.0);
                if w <= 0.0 || h <= 0.0 {
                    return Err(OdpError("invalid gradient bounds".into()));
                }
                let transform =
                    get("svg:gradientTransform").map(crate::geometry::svg_transform).transpose()?.unwrap_or(deckcraft_geom::Affine::IDENTITY);
                let physical = deckcraft_geom::Affine::scale_non_uniform(w, h) * transform;
                let determinant = physical.determinant();
                if !determinant.is_finite() || determinant.abs() < 1e-12 {
                    return Err(OdpError("unsupported singular gradient transform".into()));
                }
                let (shape, center, tile_scale) = if get("element") == Some("svg:linearGradient") {
                    let a = deckcraft_geom::Point::new(coord("svg:x1", 0.0)?, coord("svg:y1", 0.0)?);
                    let b = deckcraft_geom::Point::new(coord("svg:x2", 1.0)?, coord("svg:y2", 0.0)?);
                    let delta = b - a;
                    let len2 = delta.hypot2();
                    if len2 < 1e-20 {
                        return Err(OdpError("unsupported zero-length gradient".into()));
                    }
                    // SVG applies the affine to the entire gradient coordinate system.
                    // Its scalar field's normal therefore uses the inverse transpose,
                    // not the transformed endpoint vector (different under skew/nonuniform scale).
                    let [a0, b0, c0, d0, _, _] = physical.inverse().as_coeffs();
                    let nx = (a0 * delta.x + b0 * delta.y) / len2;
                    let ny = (c0 * delta.x + d0 * delta.y) / len2;
                    let norm = nx.hypot(ny);
                    let (dx, dy) = (nx / norm, ny / norm);
                    let scale = 1.0 / (norm * (w * dx.abs() + h * dy.abs()));
                    (GradientShape::Linear { angle: dy.atan2(dx).to_degrees(), scaled: false }, physical * a.midpoint(b), scale)
                } else {
                    let [a, b, c, d, _, _] = physical.as_coeffs();
                    let (sx, sy) = (a.hypot(b), c.hypot(d));
                    if (sx - sy).abs() > 1e-8 * sx.max(sy) || (a * c + b * d).abs() > 1e-8 * sx * sy {
                        return Err(OdpError("unsupported elliptical or sheared radial gradient".into()));
                    }
                    let cx = coord("svg:cx", 0.5)?;
                    let cy = coord("svg:cy", 0.5)?;
                    let radius = coord("svg:r", 0.5)? * sx;
                    if radius <= 0.0 {
                        return Err(OdpError("invalid radial gradient radius".into()));
                    }
                    let center = physical * deckcraft_geom::Point::new(cx, cy);
                    let mut focus = physical * deckcraft_geom::Point::new(coord("svg:fx", cx)?, coord("svg:fy", cy)?);
                    let offset = focus - center;
                    if offset.hypot() > radius * 0.999 * (1.0 + 1e-10) {
                        return Err(OdpError("unsupported radial focus at or outside the model's bounded circle".into()));
                    }
                    if offset.hypot() > radius * 0.999 {
                        focus = center + offset * (radius * 0.999 / offset.hypot());
                    }
                    let (fx, fy) = (focus.x / w, focus.y / h);
                    (GradientShape::Path { path: "circle".into(), focus: [fx, fy, 1.0 - fx, 1.0 - fy] }, center, radius * 2.0 / w.hypot(h))
                };
                let tile = [
                    center.x / w - tile_scale / 2.0,
                    center.y / h - tile_scale / 2.0,
                    1.0 - center.x / w - tile_scale / 2.0,
                    1.0 - center.y / h - tile_scale / 2.0,
                ];
                if !tile_scale.is_finite() || tile_scale <= 1e-9 || tile.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
                    return Err(OdpError("gradient geometry exceeds limits".into()));
                }
                return Ok(Some(Fill::Gradient(Gradient { shape, stops: odf_stops(stops), rotate_with_shape: true, tile })));
            }

            let endpoint = |key: &str, intensity: &str| -> Result<ColorRef> {
                let mut c = get(key).and_then(color).ok_or_else(|| OdpError("gradient has no valid endpoint color".into()))?;
                if let Some(v) = get(intensity).and_then(fraction) {
                    c = c.with(ColorTransform::LumMod((v * 100_000.0).round() as i32));
                }
                if opacity < 1.0 {
                    c = c.with(ColorTransform::Alpha((opacity * 100_000.0).round() as i32));
                }
                Ok(c)
            };
            let start = endpoint("draw:start-color", "draw:start-intensity")?;
            let end = endpoint("draw:end-color", "draw:end-intensity")?;
            let angle = get("draw:angle").and_then(crate::geometry::number).unwrap_or(0.0) / 10.0;
            let border = get("draw:border").and_then(fraction).unwrap_or(0.0);
            let cx = get("draw:cx").and_then(fraction).unwrap_or(0.5);
            let cy = get("draw:cy").and_then(fraction).unwrap_or(0.5);
            let style = get("draw:style").unwrap_or("linear");
            let (shape, stops) = match style {
                "linear" => (
                    GradientShape::Linear { angle: (90.0 - angle).rem_euclid(360.0), scaled: false },
                    vec![GradientStop { pos: border, color: start }, GradientStop { pos: 1.0, color: end }],
                ),
                "axial" => (
                    GradientShape::Linear { angle: (90.0 - angle).rem_euclid(360.0), scaled: false },
                    vec![
                        GradientStop { pos: 0.0, color: end.clone() },
                        GradientStop { pos: 0.5, color: start },
                        GradientStop { pos: 1.0, color: end },
                    ],
                ),
                "radial" | "ellipsoid" | "square" | "rectangular" => (
                    GradientShape::Path {
                        path: if matches!(style, "square" | "rectangular") { "rect" } else { "circle" }.into(),
                        focus: [cx, cy, 1.0 - cx, 1.0 - cy],
                    },
                    vec![GradientStop { pos: 0.0, color: end }, GradientStop { pos: 1.0 - border, color: start }],
                ),
                _ => return Err(OdpError(format!("unknown gradient style '{style}'"))),
            };
            Ok(Some(Fill::Gradient(Gradient { shape, stops: odf_stops(stops), rotate_with_shape: true, tile: [0.0; 4] })))
        }
        Some("hatch") => {
            let fg = props.get("odp:hatch:draw:color").and_then(|v| color(v)).unwrap_or_else(|| ColorRef::rgb(Rgba::BLACK));
            let bg = props.get("draw:fill-color").and_then(|v| color(v)).unwrap_or_else(|| ColorRef::rgb(Rgba::WHITE));
            let preset = match props.get("odp:hatch:draw:style").map(String::as_str) {
                Some("double" | "triple") => "diagCross",
                _ => "ltDnDiag",
            };
            Ok(Some(Fill::Pattern(PatternFill { preset: preset.into(), fg, bg })))
        }
        Some("bitmap") => Ok(None), // Resolved with package access, below.
        _ => Err(OdpError("unknown drawing fill".into())),
    }
}

/// SVG 1.1 §11.7.1 (referenced by ODF §16.42.2–3) interpolates in sRGB.
/// Three model stops disable the renderer's special native two-color ramp.
/// Input is already bounded to 4096; this only changes 2 stops to 3.
fn odf_stops(mut stops: Vec<GradientStop>) -> Vec<GradientStop> {
    if let [a, b] = stops.as_slice()
        && a.pos <= 1e-6
        && b.pos >= 1.0 - 1e-6
    {
        // ODF endpoints here are RGB; an inert scheme avoids allocating a theme.
        let scheme = ColorScheme { name: String::new(), colors: [Rgba::BLACK; 12] };
        let (ca, cb) = (a.color.resolve(&scheme, None), b.color.resolve(&scheme, None));
        let channel = |a: u8, b: u8| (u16::from(a) + u16::from(b)).div_ceil(2) as u8;
        let midpoint = GradientStop {
            pos: (a.pos + b.pos) / 2.0,
            color: ColorRef::rgb(Rgba::rgb(channel(ca.r, cb.r), channel(ca.g, cb.g), channel(ca.b, cb.b)))
                .with(ColorTransform::Alpha(((a.color.alpha() + b.color.alpha()) * 50_000.0).round() as i32)),
        };
        stops.insert(1, midpoint);
    }
    stops
}

pub(super) fn path(href: &str) -> Result<String> {
    // Percent escapes may spell a space in a valid package member. Decode only UTF-8 bytes.
    let mut decoded = Vec::with_capacity(href.len());
    let mut iter = href.bytes();
    while let Some(b) = iter.next() {
        if b == b'%' {
            let a = iter.next().and_then(|n| (n as char).to_digit(16));
            let b = iter.next().and_then(|n| (n as char).to_digit(16));
            let (Some(a), Some(b)) = (a, b) else {
                return Err(OdpError("invalid package path escape".into()));
            };
            decoded.push((a * 16 + b) as u8);
        } else {
            decoded.push(b);
        }
    }
    let path = String::from_utf8(decoded).map_err(|_| OdpError("invalid package path".into()))?;
    let path = path.strip_prefix("./").unwrap_or(&path);
    if path.is_empty() || path.contains([':', '\\', '\0', '#', '?']) || path.starts_with('/') || path.split('/').any(|s| matches!(s, ".." | "")) {
        return Err(OdpError("external or unsafe package resource path".into()));
    }
    Ok(path.into())
}

pub(super) fn media(
    href: &str,
    hint: Option<&str>,
    p: &mut Presentation,
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    total: &mut u64,
    cache: &mut BTreeMap<String, MediaId>,
) -> Result<MediaId> {
    let path = path(href)?;
    if let Some(id) = cache.get(&path) {
        return Ok(*id);
    }
    let data = part(zip, &path, total)?;
    let mime = match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "m4a" => "audio/mp4",
        "mov" => "video/quicktime",
        _ => hint.unwrap_or("application/octet-stream"),
    };
    let id = MediaId(super::id(p)?);
    p.media.push(MediaItem { id, name: path.clone(), content_type: mime.into(), data: Arc::new(data), link: None });
    cache.insert(path, id);
    Ok(id)
}

pub(super) fn picture(media: MediaId, props: &Props, p: &Presentation) -> PictureFill {
    let mut fill = PictureFill {
        media,
        alpha: props.get("draw:image-opacity").or_else(|| props.get("draw:opacity")).and_then(|s| fraction(s)),
        ..Default::default()
    };
    fill.adjust = PictureAdjust {
        brightness: props
            .get("draw:luminance")
            .and_then(|v| v.strip_suffix('%'))
            .and_then(crate::geometry::number)
            .map(|v| (v / 100.0).clamp(-1.0, 1.0))
            .unwrap_or(0.0),
        contrast: props
            .get("draw:contrast")
            .and_then(|v| v.strip_suffix('%'))
            .and_then(crate::geometry::number)
            .map(|v| (v / 100.0).clamp(-1.0, 1.0))
            .unwrap_or(0.0),
        grayscale: props.get("draw:color-mode").is_some_and(|v| v == "greyscale"),
        bilevel: props.get("draw:color-mode").filter(|v| *v == "mono").map(|_| 0.5),
        ..Default::default()
    };
    if let Some(clip) = props.get("fo:clip").and_then(|v| v.strip_prefix("rect(")).and_then(|v| v.strip_suffix(')')) {
        let values: Vec<_> = clip.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).collect();
        if let [top, right, bottom, left] = values.as_slice()
            && let Some(data) = p.media.iter().find(|m| m.id == media).map(|m| m.data.as_slice())
            && let Some((w, h)) = image_size(data)
        {
            // ODF clip edges are distances clipped off each edge, at 96 dpi.
            let (dx, dy) = deckcraft_render::image_dpi(data).unwrap_or((96.0, 96.0));
            fill.crop = [
                length(left).unwrap_or(0.0) / (w * 72.0 / dx),
                length(top).unwrap_or(0.0) / (h * 72.0 / dy),
                length(right).unwrap_or(0.0) / (w * 72.0 / dx),
                length(bottom).unwrap_or(0.0) / (h * 72.0 / dy),
            ];
        }
    }
    fill
}

fn image_size(bytes: &[u8]) -> Option<(f64, f64)> {
    // Header-only inspection; no image decoding or input-derived allocation.
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
        if w > 0 && h > 0 {
            return Some((w as f64, h as f64));
        }
    }
    if bytes.starts_with(b"GIF") {
        let w = u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?);
        let h = u16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?);
        if w > 0 && h > 0 {
            return Some((w as f64, h as f64));
        }
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        let mut i = 2usize;
        for _ in 0..65536 {
            if *bytes.get(i)? != 0xff {
                return None;
            }
            i = i.checked_add(1)?;
            let marker = *bytes.get(i)?;
            i = i.checked_add(1)?;
            if marker == 0xff {
                continue;
            }
            if marker == 0xd9 || marker == 0xda {
                return None;
            }
            if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
                continue;
            }
            let len = usize::from(u16::from_be_bytes(bytes.get(i..i.checked_add(2)?)?.try_into().ok()?));
            if len < 2 {
                return None;
            }
            if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
                let h = u16::from_be_bytes(bytes.get(i.checked_add(3)?..i.checked_add(5)?)?.try_into().ok()?);
                let w = u16::from_be_bytes(bytes.get(i.checked_add(5)?..i.checked_add(7)?)?.try_into().ok()?);
                return (w > 0 && h > 0).then_some((f64::from(w), f64::from(h)));
            }
            i = i.checked_add(len)?;
        }
    }
    None
}

pub(super) fn resolve_fill(
    props: &Props,
    p: &mut Presentation,
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    total: &mut u64,
    cache: &mut BTreeMap<String, MediaId>,
    w: f64,
    h: f64,
) -> Result<Option<Fill>> {
    let fill = if props.get("draw:fill").is_some_and(|v| v == "bitmap") {
        let href = props.get("odp:fill-image:xlink:href").ok_or_else(|| OdpError("bitmap fill has no package image".into()))?;
        let id = media(href, None, p, zip, total, cache)?;
        let mut pic = picture(id, props, p);
        if props.get("style:repeat").is_some_and(|v| v == "repeat") {
            match bitmap_tile(props, p.media(id)) {
                Ok(mode) => pic.mode = mode,
                Err(error) if error.0.starts_with("unsupported ") => {
                    super::extras::warn(p, &format!("{}; original fill retained in source recovery", error.0));
                    return Ok(Some(Fill::None));
                }
                Err(error) => return Err(error),
            }
        } else if props.get("style:repeat").is_some_and(|v| v != "stretch") {
            super::extras::warn(p, "Unsupported nonrepeating bitmap fill retained in source recovery");
            return Ok(Some(Fill::None));
        }
        Ok(Some(Fill::Picture(pic)))
    } else {
        let mut bounded = props.clone();
        bounded.insert("odp:width".into(), w.to_string());
        bounded.insert("odp:height".into(), h.to_string());
        match super::fill(&bounded) {
            Err(error) if error.0.starts_with("unsupported ") => {
                super::extras::warn(p, &format!("{}; original fill retained in source recovery", error.0));
                Ok(Some(Fill::None))
            }
            result => result,
        }
    }?;
    *total = total.saturating_add(fill.as_ref().map(fill_weight).unwrap_or(0) as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("expanded fill resources exceed total size limit".into()));
    }
    Ok(fill)
}

fn fill_weight(fill: &Fill) -> usize {
    let color_weight = |color: &ColorRef| color.mods.len().saturating_mul(std::mem::size_of::<ColorTransform>());
    let extra = match fill {
        Fill::Solid { color } => color_weight(color),
        Fill::Gradient(g) => g
            .stops
            .iter()
            .fold(0usize, |sum, stop| sum.saturating_add(std::mem::size_of::<GradientStop>()).saturating_add(color_weight(&stop.color)))
            .saturating_add(match &g.shape {
                GradientShape::Path { path, .. } => path.len(),
                _ => 0,
            }),
        Fill::Pattern(p) => p.preset.len().saturating_add(color_weight(&p.fg)).saturating_add(color_weight(&p.bg)),
        Fill::Picture(p) => match &p.mode {
            PictureMode::Tile { flip, align, .. } => flip.len().saturating_add(align.len()),
            _ => 0,
        },
        _ => 0,
    };
    std::mem::size_of::<Fill>().saturating_add(extra)
}

/// ODF §20.123–128: tile size, alignment then movement. Movement in both
/// directions is expressed relative to tile width (including the vertical offset).
fn bitmap_tile(props: &Props, media: Option<&MediaItem>) -> Result<PictureMode> {
    if props.get("draw:tile-repeat-offset").is_some_and(|v| v.split_whitespace().next().is_none_or(|n| n != "0%")) {
        return Err(OdpError("unsupported staggered bitmap tiles".into()));
    }
    let media = media.ok_or_else(|| OdpError("missing tile image".into()))?;
    let (pw, ph) = image_size(&media.data).ok_or_else(|| OdpError("unsupported bitmap tile intrinsic dimensions".into()))?;
    let (dx, dy) = deckcraft_render::image_dpi(&media.data).unwrap_or((96.0, 96.0));
    // Resource svg:width/height are estimates overridden by the actual image
    // (ODF §19.575.11/19.234). Only explicit fill-image size overrides it.
    let logical = |axis: &str, fallback: f64| -> Result<f64> {
        let size = props.get(&format!("draw:fill-image-{axis}"));
        size.map_or(Ok(fallback), |v| {
            length(v)
                .filter(|n| *n > 0.0 && *n <= 1e6)
                .ok_or_else(|| OdpError("unsupported bitmap tile size; requires a positive absolute length".into()))
        })
    };
    let tw = logical("width", pw * 72.0 / dx)?;
    let th = logical("height", ph * 72.0 / dy)?;
    let offset = |axis: &str| -> Result<f64> {
        props.get(&format!("draw:fill-image-ref-point-{axis}")).map_or(Ok(0.0), |v| {
            v.strip_suffix('%')
                .and_then(crate::geometry::number)
                .map(|v| v / 100.0 * tw)
                .filter(|v| v.is_finite() && v.abs() <= 1e6)
                .ok_or_else(|| OdpError("invalid tile offset".into()))
        })
    };
    let align = match props.get("draw:fill-image-ref-point").map(String::as_str).unwrap_or("top-left") {
        "top-left" => "tl",
        "top" => "t",
        "top-right" => "tr",
        "left" => "l",
        "center" => "ctr",
        "right" => "r",
        "bottom-left" => "bl",
        "bottom" => "b",
        "bottom-right" => "br",
        _ => return Err(OdpError("invalid bitmap tile alignment".into())),
    };
    let (sx, sy) = (tw / (pw * 0.75), th / (ph * 0.75));
    if ![tw, th, sx, sy].iter().all(|v| v.is_finite() && *v > 1e-9 && *v <= 1e6) {
        return Err(OdpError("bitmap tile dimensions exceed limits".into()));
    }
    Ok(PictureMode::Tile { tx: offset("x")?, ty: offset("y")?, sx, sy, flip: String::new(), align: align.into() })
}

pub(super) fn outline(props: &Props, styles: &Styles<'_>) -> Result<Line> {
    use deckcraft_model::style::{Dash, LineCap, LineJoin};
    if props.get("draw:stroke").is_none_or(|v| v == "none") {
        return Ok(Line::none());
    }
    let mut c = props.get("svg:stroke-color").and_then(|s| color(s)).unwrap_or_else(|| ColorRef::rgb(Rgba::BLACK));
    if let Some(alpha) = props.get("svg:stroke-opacity").and_then(|s| fraction(s)) {
        c = c.with(ColorTransform::Alpha((alpha * 100_000.0).round() as i32));
    }
    let width = props.get("svg:stroke-width").and_then(|s| length(s)).unwrap_or(1.0).max(0.0);
    let mut line = Line::solid(c, width);
    line.cap = props.get("svg:stroke-linecap").map(|v| match v.as_str() {
        "round" => LineCap::Round,
        "square" => LineCap::Square,
        _ => LineCap::Flat,
    });
    line.join = props.get("draw:stroke-linejoin").map(|v| match v.as_str() {
        "round" => LineJoin::Round,
        "bevel" => LineJoin::Bevel,
        _ => LineJoin::Miter,
    });
    if let Some(dash) = props.get("draw:stroke-dash").and_then(|name| styles.get(&("dash".into(), name.clone()))) {
        let dim = |key: &str, default: f64| dash.attr(key).and_then(|s| length(s).or_else(|| fraction(s).map(|f| f * width))).unwrap_or(default);
        let distance = dim("draw:distance", width * 3.0);
        let mut pattern = Vec::new();
        for (count, length_key) in [("draw:dots1", "draw:dots1-length"), ("draw:dots2", "draw:dots2-length")] {
            let n = dash.attr(count).unwrap_or("0").parse::<usize>().map_err(|_| OdpError("invalid stroke dash count".into()))?;
            if n > 128 || pattern.len() + n > 128 {
                return Err(OdpError("stroke dash count exceeds limit".into()));
            }
            pattern.extend(std::iter::repeat_n((dim(length_key, width) / width.max(0.001), distance / width.max(0.001)), n));
        }
        if !pattern.is_empty() {
            line.dash = Some(Dash::Custom { pattern });
        }
    }
    Ok(line)
}

pub(super) fn effects(props: &Props) -> Option<Effects> {
    use deckcraft_model::style::Shadow;
    if props.get("draw:shadow").is_none_or(|v| v != "visible") {
        return None;
    }
    let x = props.get("draw:shadow-offset-x").and_then(|s| length(s)).unwrap_or(0.0);
    let y = props.get("draw:shadow-offset-y").and_then(|s| length(s)).unwrap_or(0.0);
    let mut c = props.get("draw:shadow-color").and_then(|s| color(s)).unwrap_or_else(|| ColorRef::rgb(Rgba::BLACK));
    if let Some(alpha) = props.get("draw:shadow-opacity").and_then(|s| fraction(s)) {
        c = c.with(ColorTransform::Alpha((alpha * 100_000.0).round() as i32));
    }
    Some(Effects {
        outer_shadow: Some(Shadow {
            color: c,
            blur: 0.0,
            dist: x.hypot(y),
            dir: y.atan2(x).to_degrees(),
            inner: false,
            sx: 1.0,
            sy: 1.0,
            kx: 0.0,
            ky: 0.0,
            align: String::new(),
            rotate_with_shape: true,
        }),
        ..Default::default()
    })
}
