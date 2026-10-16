//! Native ODF style resources, derived from OASIS OpenDocument 1.3 part 3.
use super::*;
use deckcraft_model::style::{Compound, GradientShape, LineCap, LineJoin, PictureAdjust, PictureFill, PictureMode};

pub(super) fn finite(values: &[f64]) -> Result<()> {
    if values.iter().all(|v| v.is_finite() && v.abs() <= 1_000_000.0) { Ok(()) } else { Err(OdpError("non-finite or excessive style value".into())) }
}
impl Writer<'_> {
    pub(super) fn image_path(&mut self, id: MediaId) -> Result<String> {
        let media = self.pres.media.iter().find(|m| m.id == id).ok_or_else(|| OdpError("missing media".into()))?;
        if let Some(link) = &media.link {
            return Ok(esc(link));
        }
        if media.data.is_empty() {
            return Err(OdpError("empty embedded media".into()));
        }
        let ext = match media.content_type.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/svg+xml" => "svg",
            "image/webp" => "webp",
            "image/bmp" => "bmp",
            "image/tiff" => "tif",
            "audio/mpeg" => "mp3",
            "audio/wav" | "audio/x-wav" => "wav",
            "audio/ogg" => "ogg",
            "audio/mp4" => "m4a",
            "video/mp4" => "mp4",
            "video/webm" => "webm",
            "video/quicktime" => "mov",
            _ => return Err(OdpError(format!("unsupported media type '{}'", media.content_type))),
        };
        Ok(self.images.entry(id).or_insert_with(|| format!("Pictures/media{}.{ext}", id.0)).clone())
    }
    pub(super) fn fill_attrs(&mut self, ctx: &Ctx<'_>, fill: Option<&Fill>, ph: Option<Rgba>, dimensions: (f64, f64)) -> Result<String> {
        match fill {
            None | Some(Fill::None) => Ok("draw:fill=\"none\"".into()),
            Some(Fill::Solid { color }) => {
                let c = ctx.color(color, ph);
                Ok(format!("draw:fill=\"solid\" draw:fill-color=\"#{}\" draw:opacity=\"{}%\"", c.hex(), f64::from(c.a) * 100.0 / 255.0))
            }
            Some(Fill::Gradient(g)) => {
                let (w, h) = dimensions;
                finite(&[w, h])?;
                if w <= 0.0 || h <= 0.0 {
                    return Err(OdpError("gradient needs positive fill dimensions".into()));
                }
                if g.stops.is_empty() || g.stops.len() > 4096 {
                    return Err(OdpError("gradient has invalid stop count".into()));
                }
                finite(&g.tile)?;
                if !g.rotate_with_shape {
                    return Err(OdpError("slide-aligned gradients cannot be represented faithfully in ODP yet".into()));
                }
                let [l, t, r, b] = g.tile;
                let (tw, th) = (w * (1.0 - l - r), h * (1.0 - t - b));
                // The renderer falls back to the shape bounds for inverted tile rectangles.
                let (cx, cy, tw, th) =
                    if tw <= 1e-9 || th <= 1e-9 { (w / 2.0, h / 2.0, w, h) } else { ((l * w + w - r * w) / 2.0, (t * h + h - b * h) / 2.0, tw, th) };
                finite(&[cx, cy, tw, th])?;
                self.next += 1;
                let name = format!("Gradient{}", self.next);
                let (tag, attrs) = match &g.shape {
                    GradientShape::Linear { angle, .. } => {
                        finite(&[*angle])?;
                        let a = angle.to_radians();
                        let dx = a.cos();
                        let dy = a.sin();
                        let extent = (tw * dx.abs() + th * dy.abs()) / 2.0;
                        (
                            "svg:linearGradient",
                            format!(
                                "svg:x1=\"{}%\" svg:y1=\"{}%\" svg:x2=\"{}%\" svg:y2=\"{}%\" svg:gradientTransform=\"scale(1 {})\"",
                                (cx - dx * extent) / w * 100.0,
                                (cy - dy * extent) / w * 100.0,
                                (cx + dx * extent) / w * 100.0,
                                (cy + dy * extent) / w * 100.0,
                                w / h
                            ),
                        )
                    }
                    GradientShape::Path { path, focus } => {
                        finite(focus)?;
                        if path != "circle" {
                            return Err(OdpError(format!("'{path}' path gradients cannot be represented faithfully in ODP yet")));
                        }
                        let fx = (focus[0] + 1.0 - focus[2]) / 2.0;
                        let fy = (focus[1] + 1.0 - focus[3]) / 2.0;
                        let ratio = w / h;
                        let radius = tw.hypot(th) / (2.0 * w);
                        let (focus_x, focus_y) = (fx * w - cx, fy * h - cy);
                        let distance = focus_x.hypot(focus_y);
                        let (fx, fy) = if distance > radius * w * 0.999 {
                            let k = radius * w * 0.999 / distance;
                            ((cx + focus_x * k) / w, (cy + focus_y * k) / h)
                        } else {
                            (fx, fy)
                        };
                        (
                            "svg:radialGradient",
                            format!(
                                "svg:cx=\"{}%\" svg:cy=\"{}%\" svg:fx=\"{}%\" svg:fy=\"{}%\" svg:r=\"{}%\" svg:gradientTransform=\"scale(1 {ratio})\"",
                                cx / w * 100.0,
                                cy / w * 100.0,
                                fx * 100.0,
                                fy * 100.0 / ratio,
                                radius * 100.0
                            ),
                        )
                    }
                };
                self.definitions.push_str(&format!("<{tag} draw:name=\"{name}\" svg:gradientUnits=\"objectBoundingBox\" {attrs}>"));
                let mut previous = -1.0;
                let mut resolved = Vec::with_capacity(g.stops.len());
                for stop in &g.stops {
                    finite(&[stop.pos])?;
                    if !(0.0..=1.0).contains(&stop.pos) || stop.pos < previous {
                        return Err(OdpError("gradient stops must be ordered fractions".into()));
                    }
                    previous = stop.pos;
                    resolved.push((stop.pos, ctx.color(&stop.color, ph)));
                }
                // Use the renderer's exact native ramp, including alpha. Only a
                // two-stop native gradient expands (to 17), within the 4096 budget.
                for (position, c) in deckcraft_render::gradient_stops(&resolved) {
                    self.definitions.push_str(&format!(
                        "<svg:stop svg:offset=\"{}\" svg:stop-color=\"#{}\" svg:stop-opacity=\"{}\"/>",
                        position,
                        c.hex(),
                        f64::from(c.a) / 255.0
                    ));
                }
                self.definitions.push_str(&format!("</{tag}>"));
                Ok(format!("draw:fill=\"gradient\" draw:fill-gradient-name=\"{name}\""))
            }
            Some(Fill::Picture(p)) => {
                if p.crop != [0.0; 4] || p.adjust != PictureAdjust::default() || p.upright {
                    return Err(OdpError("cropped, adjusted or upright picture fills cannot be represented faithfully in ODP yet".into()));
                }
                let path = self.image_path(p.media)?;
                self.next += 1;
                let name = format!("Bitmap{}", self.next);
                self.definitions.push_str(&format!("<draw:fill-image draw:name=\"{name}\" xlink:href=\"{path}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>"));
                let mode: String = match &p.mode {
                    PictureMode::Stretch { fill_rect } if *fill_rect == [0.0; 4] => "style:repeat=\"stretch\"".into(),
                    PictureMode::Tile { tx, ty, sx, sy, flip, align } if flip.is_empty() => {
                        finite(&[*tx, *ty, *sx, *sy])?;
                        let media = self.pres.media(p.media).ok_or_else(|| OdpError("missing bitmap tile image".into()))?;
                        let (pw, ph) = image_size(&media.data).ok_or_else(|| OdpError("bitmap tile intrinsic dimensions are unavailable".into()))?;
                        // The renderer defines model tile scale at 96dpi. Explicit ODF
                        // logical dimensions retain this appearance even with embedded DPI.
                        let (tw, th) = (pw * 0.75 * sx, ph * 0.75 * sy);
                        finite(&[tw, th])?;
                        if tw <= 1e-9 || th <= 1e-9 {
                            return Err(OdpError("bitmap tile dimensions must be positive".into()));
                        }
                        let align = match align.as_str() {
                            "" | "tl" => "top-left",
                            "t" => "top",
                            "tr" => "top-right",
                            "l" => "left",
                            "ctr" => "center",
                            "r" => "right",
                            "bl" => "bottom-left",
                            "b" => "bottom",
                            "br" => "bottom-right",
                            _ => return Err(OdpError("unsupported bitmap tile alignment".into())),
                        };
                        let (ox, oy) = (tx / tw * 100.0, ty / tw * 100.0);
                        finite(&[ox, oy])?;
                        format!(
                            "style:repeat=\"repeat\" draw:fill-image-width=\"{tw}pt\" draw:fill-image-height=\"{th}pt\" draw:fill-image-ref-point=\"{align}\" draw:fill-image-ref-point-x=\"{ox}%\" draw:fill-image-ref-point-y=\"{oy}%\""
                        )
                    }
                    _ => return Err(OdpError("positioned or flipped picture fill tiles cannot be represented faithfully in ODP yet".into())),
                };
                let alpha = p.alpha.unwrap_or(1.0);
                finite(&[alpha])?;
                Ok(format!("draw:fill=\"bitmap\" draw:fill-image-name=\"{name}\" draw:opacity=\"{}%\" {mode}", alpha.clamp(0.0, 1.0) * 100.0))
            }
            Some(Fill::Pattern(_)) => Err(OdpError("pattern fills cannot be represented faithfully in ODP yet".into())),
            Some(Fill::Group | Fill::Background) => Err(OdpError("unresolved group or background fill".into())),
        }
    }
    pub(super) fn picture_attrs(&self, p: &PictureFill) -> Result<String> {
        finite(&p.crop)?;
        finite(&[p.adjust.brightness, p.adjust.contrast, p.alpha.unwrap_or(1.0)])?;
        let mut clip = String::new();
        if p.crop != [0.0; 4] {
            let media = self.pres.media.iter().find(|m| m.id == p.media).ok_or_else(|| OdpError("missing cropped picture".into()))?;
            let (w, h) = image_size(&media.data).ok_or_else(|| OdpError("cropped picture intrinsic dimensions are unavailable".into()))?;
            if p.crop[0] + p.crop[2] >= 1.0 || p.crop[1] + p.crop[3] >= 1.0 {
                return Err(OdpError("picture crop removes entire image".into()));
            }
            let (dx, dy) = deckcraft_render::image_dpi(&media.data).unwrap_or((96.0, 96.0));
            clip = format!(
                " fo:clip=\"rect({}pt, {}pt, {}pt, {}pt)\"",
                p.crop[1] * h * 72.0 / dy,
                p.crop[2] * w * 72.0 / dx,
                p.crop[3] * h * 72.0 / dy,
                p.crop[0] * w * 72.0 / dx
            );
        }
        if p.adjust.duotone.is_some()
            || p.adjust.clear_color.is_some()
            || p.adjust.sharpen != 0.0
            || p.adjust.saturation.is_some_and(|v| v != 1.0)
            || p.adjust.alpha_inv
            || p.upright
            || p.adjust.bilevel.is_some_and(|v| v != 0.5)
        {
            return Err(OdpError("this picture adjustment cannot be represented faithfully in ODP yet".into()));
        }
        Ok(format!(
            "{clip} draw:luminance=\"{}%\" draw:contrast=\"{}%\" draw:image-opacity=\"{}%\" draw:color-mode=\"{}\"",
            p.adjust.brightness * 100.0,
            p.adjust.contrast * 100.0,
            p.alpha.unwrap_or(1.0).clamp(0.0, 1.0) * 100.0,
            if p.adjust.grayscale {
                "greyscale"
            } else if p.adjust.bilevel.is_some() {
                "mono"
            } else {
                "standard"
            }
        ))
    }
    pub(super) fn effect_attrs(&self, ctx: &Ctx<'_>, shape: &Shape) -> Result<String> {
        let (effects, ph) = resolve::effects(ctx, shape);
        let Some(e) = effects else { return Ok(String::new()) };
        if e.inner_shadow.is_some()
            || e.glow.is_some()
            || e.soft_edge.is_some()
            || e.reflection.is_some()
            || e.bevel.is_some()
            || e.fill_overlay.is_some()
            || e.raw3d.is_some()
        {
            return Err(OdpError("this shape effect cannot be represented faithfully in ODP yet".into()));
        }
        let Some(s) = e.outer_shadow else { return Ok(String::new()) };
        finite(&[s.dist, s.dir, s.blur, s.sx, s.sy, s.kx, s.ky])?;
        if s.blur != 0.0 || s.inner || s.sx != 1.0 || s.sy != 1.0 || s.kx != 0.0 || s.ky != 0.0 {
            return Err(OdpError("blurred or perspective shadows cannot be represented faithfully in ODP 1.3".into()));
        }
        let c = ctx.color(&s.color, ph);
        let angle = s.dir.to_radians();
        Ok(format!(
            "draw:shadow=\"visible\" draw:shadow-color=\"#{}\" draw:shadow-opacity=\"{}%\" draw:shadow-offset-x=\"{}pt\" draw:shadow-offset-y=\"{}pt\"",
            c.hex(),
            f64::from(c.a) * 100.0 / 255.0,
            s.dist * angle.cos(),
            s.dist * angle.sin()
        ))
    }
    pub(super) fn line_attrs(&mut self, ctx: &Ctx<'_>, line: &Line, ph: Option<Rgba>) -> Result<String> {
        let width = line.width.unwrap_or(1.0);
        finite(&[width])?;
        if width < 0.0 {
            return Err(OdpError("negative line width".into()));
        }
        let c = match &line.fill {
            None | Some(Fill::None) => return Ok("draw:stroke=\"none\"".into()),
            Some(Fill::Solid { color }) => ctx.color(color, ph),
            _ => return Err(OdpError("gradient or picture outlines cannot be represented faithfully in ODP".into())),
        };
        if line.compound.is_some_and(|v| v != Compound::Single) {
            return Err(OdpError("compound outlines cannot be represented faithfully in ODP".into()));
        }
        let mut out = format!(
            "draw:stroke=\"solid\" svg:stroke-color=\"#{}\" svg:stroke-width=\"{width}pt\" svg:stroke-opacity=\"{}%\"",
            c.hex(),
            f64::from(c.a) * 100.0 / 255.0
        );
        if let Some(cap) = line.cap {
            out.push_str(&format!(
                " svg:stroke-linecap=\"{}\"",
                match cap {
                    LineCap::Round => "round",
                    LineCap::Square => "square",
                    _ => "butt",
                }
            ));
        }
        if let Some(join) = line.join {
            out.push_str(&format!(
                " draw:stroke-linejoin=\"{}\"",
                match join {
                    LineJoin::Round => "round",
                    LineJoin::Bevel => "bevel",
                    _ => "miter",
                }
            ));
        }
        if let Some(dash) = &line.dash {
            let p = dash.pattern();
            if !p.is_empty() {
                finite(&p)?;
                if p.len() > 6 || p.chunks(2).any(|pair| pair.get(1) != p.get(1)) {
                    return Err(OdpError("custom outline dash pattern cannot be represented faithfully in ODP".into()));
                }
                self.next += 1;
                let name = format!("Dash{}", self.next);
                let first = p.first().copied().unwrap_or(1.0);
                let distance = p.get(1).copied().unwrap_or(1.0);
                let mut extra = String::new();
                if p.len() > 2 {
                    extra = format!(" draw:dots2=\"{}\" draw:dots2-length=\"{}pt\"", p.len() / 2 - 1, p.get(2).copied().unwrap_or(1.0) * width);
                }
                self.definitions.push_str(&format!("<draw:stroke-dash draw:name=\"{name}\" draw:style=\"rect\" draw:dots1=\"1\" draw:dots1-length=\"{}pt\" draw:distance=\"{}pt\"{extra}/>",first*width,distance*width));
                out = out.replacen("draw:stroke=\"solid\"", &format!("draw:stroke=\"dash\" draw:stroke-dash=\"{name}\""), 1);
            }
        }
        // ODF §16.42.8 markers scale a glyph and place it using a 0.3/0.5
        // bound offset. The model's arrow tips/lengths use different placement;
        // emitting an approximate glyph silently changes visible geometry.
        if [&line.head, &line.tail].into_iter().flatten().any(|end| end.kind != "none") {
            return Err(OdpError("line markers cannot be represented faithfully in ODP yet".into()));
        }
        Ok(out)
    }
}

// Read only bounded format headers; no pixel allocation is needed for ODF clipping lengths.
fn image_size(b: &[u8]) -> Option<(f64, f64)> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes(b.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(b.get(20..24)?.try_into().ok()?);
        return Some((f64::from(w), f64::from(h)));
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Some((f64::from(u16::from_le_bytes(b.get(6..8)?.try_into().ok()?)), f64::from(u16::from_le_bytes(b.get(8..10)?.try_into().ok()?))));
    }
    if b.starts_with(&[0xff, 0xd8]) {
        let mut i = 2usize;
        for _ in 0..65536 {
            if *b.get(i)? != 0xff {
                return None;
            }
            i = i.checked_add(1)?;
            let marker = *b.get(i)?;
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
            let len = usize::from(u16::from_be_bytes(b.get(i..i.checked_add(2)?)?.try_into().ok()?));
            if len < 2 {
                return None;
            }
            if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
                let h = u16::from_be_bytes(b.get(i.checked_add(3)?..i.checked_add(5)?)?.try_into().ok()?);
                let w = u16::from_be_bytes(b.get(i.checked_add(5)?..i.checked_add(7)?)?.try_into().ok()?);
                return Some((f64::from(w), f64::from(h)));
            }
            i = i.checked_add(len)?;
        }
    }
    None
}
