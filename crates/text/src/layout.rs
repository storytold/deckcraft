use super::breaking::{break_lines, line_metrics, measure_str, tab_advance, tab_segment, tabs_of};
use super::run::{Deco, DecoLine, GlyphRun};
use super::shape::{SUB_SUPER, Upright, shape_para};
use super::style::{Opts, ParaShaped, spacing_pts};
use super::*;

#[cfg(test)]
thread_local! {
    /// How many times `layout_scaled` ran on this thread (tests count autofit's passes).
    pub(crate) static LAYOUT_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Lay out a shape's text body.
pub fn layout(ctx: &Ctx, shape: &Shape, body: &TextBody, opts: &Opts) -> TextLayout {
    let bp = resolve::body(ctx, shape);
    let autofit = bp.autofit.unwrap_or(AutoFit::None);
    // Shrink on overflow starts from the scale the file was saved with (`fontScale`,
    // `lnSpcReduction`), which is what PowerPoint draws; editing the text resets it.
    let (scale0, reduce0, no_saved_scale) = match (autofit, opts.no_shrink) {
        (AutoFit::Shrink { font_scale, line_reduction }, false) => (
            font_scale.filter(|s| s.is_finite()).map(|s| s.clamp(0.1, 1.0)).unwrap_or(1.0),
            if line_reduction.is_finite() { line_reduction.clamp(0.0, 0.2) } else { 0.0 },
            font_scale.is_none(),
        ),
        _ => (1.0, 0.0, false),
    };
    let mut l = layout_scaled(ctx, shape, body, &bp, opts, scale0, reduce0);
    // A scale PowerPoint saved is drawn as it is: PowerPoint fits the text again only when it
    // changes (and editing resets the saved scale), not when a file opens, even where our
    // metrics would call it overflowing (trailing empty paragraphs, substituted fonts).
    // A body whose file has no `fontScale` at all (`<a:normAutofit/>`) is the schema default
    // 100 % and is drawn as it is too, not refitted on open (#724).
    let saved = no_saved_scale || scale0 < 1.0 || reduce0 > 0.0;
    if let (AutoFit::Shrink { .. }, false) = (autofit, opts.no_shrink)
        && l.overflow
        && !saved
    {
        // Text that overflows even at the smallest scale gets that scale: every larger one
        // overflows too, so searching would only lay it out a dozen more times (seconds for a
        // body of tens of thousands of paragraphs, on every redraw of the slide).
        let smallest = layout_scaled(ctx, shape, body, &bp, opts, 0.1, 0.2);
        if smallest.overflow {
            return smallest;
        }
        // Binary search the largest scale that fits (steps like 90%, 80%… with line reduction).
        let (mut lo, mut hi) = (0.1_f64, scale0);
        let mut best = Some(smallest);
        for _ in 0..12 {
            let mid = (lo + hi) / 2.0;
            let red: f64 = ((1.0 - mid) * 0.8_f64).min(0.2).max(reduce0);
            let t = layout_scaled(ctx, shape, body, &bp, opts, mid, red);
            if t.overflow {
                hi = mid;
            } else {
                lo = mid;
                best = Some(t);
            }
        }
        if let Some(b) = best {
            l = b;
        }
    }
    l
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn layout_scaled(ctx: &Ctx, shape: &Shape, body: &TextBody, bp: &BodyProps, opts: &Opts, scale: f64, reduce: f64) -> TextLayout {
    #[cfg(test)]
    LAYOUT_PASSES.with(|n| n.set(n.get() + 1));
    let r = opts.rect;
    let vert = bp.vert.unwrap_or(TextDir::Horizontal);
    // East Asian vertical text: CJK stands upright in text turned a quarter turn; stacked text
    // stands every character upright, one under the other.
    let upright_text = match vert {
        TextDir::EaVertical => Upright::Cjk,
        TextDir::Stacked | TextDir::StackedRtl => Upright::All,
        _ => Upright::None,
    };
    // Mongolian vertical and stacked text: turned like vert, their lines run from left to right
    // (as PowerPoint draws both).
    let lines_ltr = matches!(vert, TextDir::MongolianVertical | TextDir::Stacked);
    let rotation = match vert {
        TextDir::Vertical | TextDir::EaVertical | TextDir::Stacked | TextDir::StackedRtl | TextDir::MongolianVertical => 90.0,
        TextDir::Vertical270 => 270.0,
        TextDir::Horizontal => 0.0,
    } + bp.rot.unwrap_or(0.0);
    // For vertical text, lay out in a box with swapped dimensions centred on the text rect.
    let r = if (rotation - 90.0).abs() < 1e-6 || (rotation - 270.0).abs() < 1e-6 {
        let c = r.center();
        Rect::new(c.x - r.height() / 2.0, c.y - r.width() / 2.0, c.x + r.height() / 2.0, c.y + r.width() / 2.0)
    } else {
        r
    };
    let inner = Rect::new(
        r.x0 + bp.inset_l.unwrap_or(7.2),
        r.y0 + bp.inset_t.unwrap_or(3.6),
        (r.x1 - bp.inset_r.unwrap_or(7.2)).max(r.x0 + bp.inset_l.unwrap_or(7.2)),
        (r.y1 - bp.inset_b.unwrap_or(3.6)).max(r.y0 + bp.inset_t.unwrap_or(3.6)),
    );
    // Text that follows a WordArt arch or circle is set on one line per paragraph.
    let follow = bp.warp.as_deref().is_some_and(deckcraft_geom::warp::is_follow);
    let wrap = bp.wrap.unwrap_or(true) && !follow && !matches!(bp.autofit, Some(AutoFit::Shape) if shape.text_box && !bp.wrap.unwrap_or(true));
    let ncols = bp.columns.unwrap_or(1).clamp(1, 16) as usize;
    let col_gap = bp.col_spacing.unwrap_or(0.0).max(0.0);
    let col_w = ((inner.width() - col_gap * (ncols as f64 - 1.0)) / ncols as f64).max(1.0);

    let mut out = TextLayout { inner, font_scale: scale, line_reduction: reduce, rotation, ..Default::default() };
    let mut y = 0.0;
    let mut numbering: Vec<u32> = vec![0; 9];
    let mut all_lines: Vec<(LineInfo, LinePlace, f64, Option<(String, f64, f64)>)> = vec![];
    let mut prev_after = 0.0;
    // How far the last line's full pitch reaches past the text (exact spacing; see below).
    let mut tail = 0.0;
    let first_last = bp.space_first_last.unwrap_or(false);
    let mut shaped_paras: Vec<ParaShaped> = Vec::with_capacity(body.paragraphs.len());
    for (pi, para) in body.paragraphs.iter().enumerate() {
        let lvl = para.level.min(8) as usize;
        let pp = resolve::para(ctx, shape, para);
        let number = if matches!(pp.bullet, Some(Bullet::AutoNum { .. })) && !para.is_empty() {
            if let Some(n) = numbering.get_mut(lvl) {
                *n += 1;
            }
            for deeper in numbering.iter_mut().skip(lvl + 1) {
                *deeper = 0;
            }
            numbering.get(lvl).copied()
        } else if matches!(pp.bullet, Some(Bullet::AutoNum { .. })) {
            // Preview the next number without consuming it: empty paragraphs do not advance
            // numbering in the saved document or affect later populated paragraphs.
            numbering.get(lvl).map(|n| n.saturating_add(1))
        } else {
            if !para.is_empty() {
                for n in numbering.iter_mut().skip(lvl) {
                    *n = 0;
                }
            }
            None
        };
        shaped_paras.push(shape_para(ctx, shape, body, pi, scale, opts, number, upright_text));
    }
    for (pi, p) in shaped_paras.iter().enumerate() {
        let props = &p.props;
        let margin = props.margin_left.unwrap_or(0.0).max(0.0);
        let indent = props.indent.unwrap_or(0.0);
        let mr = props.margin_right.unwrap_or(0.0).max(0.0);
        let bullet_w = p.bullet.as_ref().map(|(s, st)| measure_str(&st.face, st.size, s).0).unwrap_or(0.0);
        let first_x = if p.bullet.is_some() {
            let bx = (margin + indent).max(0.0);
            // A hanging bullet's text starts at the margin. When the hang reaches left of the box
            // (marL + indent < 0), PowerPoint pins the bullet to the box's edge and keeps the
            // hang: the first line's text starts at -indent (later lines stay at the margin).
            let stop = margin.max(-indent);
            if indent < 0.0 && bx + bullet_w <= stop { stop } else { bx + bullet_w + if indent >= 0.0 { 0.0 } else { 3.0 } }
        } else {
            (margin + indent).max(0.0)
        };
        let rest_x = margin;
        let tabs = tabs_of(props);
        let first_w = (col_w - first_x - mr).max(1.0);
        let rest_w = (col_w - rest_x - mr).max(1.0);
        let raw = break_lines(p, first_w, rest_w, wrap, &tabs, props.default_tab.unwrap_or(72.0), first_x, rest_x);
        // The first paragraph's space before counts only when the body says so.
        let before = if pi == 0 && !first_last { 0.0 } else { spacing_pts(props.space_before, p.lead_size) };
        y += before.max(0.0) + prev_after;
        let first_line_idx = all_lines.len();
        for (li, rl) in raw.iter().enumerate() {
            // Line metrics: max ascent/descent over the line's styles (or the lead style if empty).
            let (mut asc, mut desc, mut size) = (0.0f64, 0.0f64, 0.0f64);
            for c in p.cells.get(rl.start..rl.end.max(rl.start)).unwrap_or(&[]) {
                if let (Some(st), Some(face)) = (p.styles.get(c.style), p.faces.get(c.face)) {
                    let sz = st.size;
                    // The run's own fonts set the line; a fallback face (emoji, a symbol or a
                    // script the run's fonts lack) does not move the baseline in PowerPoint.
                    let own = Arc::ptr_eq(face, &st.face)
                        || [st.ea.as_deref(), st.cs.as_deref(), st.sym.as_deref()].into_iter().flatten().any(|f| face.family.eq_ignore_ascii_case(f));
                    let (a, d) = line_metrics(if own { face } else { &st.face }, sz);
                    asc = asc.max(a);
                    desc = desc.max(d);
                    size = size.max(sz);
                }
            }
            if size == 0.0 {
                // PowerPoint sizes an empty line by the mark that ends it: the `a:rPr` of the
                // `a:br` that closes it, or `a:endParaRPr` for the paragraph's last line -- not
                // by the paragraph's first run (#780). An empty *paragraph* already lands on
                // `end_props` through `lead`, and keeps the same height here.
                let brk = p.cells.get(rl.end).filter(|c| c.ch == '\u{b}').and_then(|c| p.styles.get(c.style));
                let (face, sz) = match brk {
                    Some(st) => (&st.face, st.size),
                    None if rl.end >= p.cells.len() => (&p.end_face, p.end_size),
                    // Anything else empty (a line of tabs alone) keeps the old measure.
                    None => (&p.lead_face, p.lead_size),
                };
                let (a, d) = line_metrics(face, sz);
                asc = a;
                desc = d;
                size = sz;
            }
            let natural = (asc + desc).max(size * 1.0);
            let lh = match props.line_spacing {
                // The reduction comes off the percentage (90 % less 20 % is 70 %), as PowerPoint
                // applies `lnSpcReduction`.
                Some(Spacing::Pct(f)) => natural * (f - reduce).max(f * 0.1).max(0.0),
                Some(Spacing::Pts(v)) => v * (1.0 - reduce),
                None => natural * (1.0 - reduce),
            };
            let ratio = if natural > 0.0 { lh / natural } else { 1.0 };
            let top = y;
            // Exact spacing (`spcPts`), as PowerPoint sets it: the baseline sits at 80 % of the
            // line pitch, and the text ends at the last line's descent rather than a full pitch.
            let exact = matches!(props.line_spacing, Some(Spacing::Pts(_)));
            let baseline = if exact { top + lh * 0.8 } else { top + asc * ratio };
            tail = if exact { (lh * 0.2 - desc).max(-lh) } else { 0.0 };
            y += lh;
            let x0 = if li == 0 { first_x } else { rest_x };
            let avail = (col_w - x0 - mr).max(0.0);
            let n = rl.end.saturating_sub(rl.start);
            let line_cells = p.cells.get(rl.start..rl.end).unwrap_or(&[]);
            // Logical advances; tabs measure from the start edge.
            let mut adv = Vec::with_capacity(n);
            let mut x = 0.0;
            // A line that breaks at a soft hyphen ends with a visible hyphen.
            let shy_end = li + 1 < raw.len() && line_cells.last().is_some_and(|c| c.ch == '\u{AD}');
            for (k, c) in line_cells.iter().enumerate() {
                let a = if shy_end && k + 1 == n {
                    p.faces.get(c.face).zip(p.styles.get(c.style)).map(|(f, st)| measure_str(f, st.size, "-").0).unwrap_or(0.0)
                } else if c.ch == '\t' {
                    tab_advance(x0 + x, &tabs, props.default_tab.unwrap_or(72.0), tab_segment(&p.cells, rl.start + k))
                } else {
                    c.adv
                };
                adv.push(a);
                x += a;
            }
            let width = x;
            let align = props.align.unwrap_or(Align::Left);
            let last_line = li + 1 == raw.len();
            let slack = (avail - width).max(0.0);
            // Justification: kashida (justLow, Arabic), spaces, or every character.
            let stretch = !last_line && wrap && slack > 0.0;
            let mut kashida = vec![0.0; n];
            let mut filled = false;
            if stretch && align == Align::JustLow {
                let ops = kashida_opportunities(p, rl.start, rl.end);
                if !ops.is_empty() {
                    let each = slack / ops.len() as f64;
                    for k in ops {
                        if let (Some(e), Some(a)) = (kashida.get_mut(k), adv.get_mut(k)) {
                            *e = each;
                            *a += each;
                        }
                    }
                    filled = true;
                }
            }
            if stretch && !filled && matches!(align, Align::Justify | Align::JustLow) {
                let spaces = line_cells.iter().filter(|c| c.ch == ' ').count();
                if spaces > 0 {
                    for (a, c) in adv.iter_mut().zip(line_cells) {
                        if c.ch == ' ' {
                            *a += slack / spaces as f64;
                        }
                    }
                    filled = true;
                }
            }
            if align == Align::Distributed && n > 1 {
                for a in adv.iter_mut().take(n - 1) {
                    *a += slack / (n - 1) as f64;
                }
                filled = true;
            }
            let line_w: f64 = adv.iter().sum();
            // The line box, in column coordinates: the start edge is the left one for LTR and the
            // right one (S = W - rIns) for RTL paragraphs.
            let (box_l, box_r) = if p.rtl { (mr, col_w - x0) } else { (x0, col_w - mr) };
            let left = if filled {
                box_l
            } else {
                match align {
                    Align::Left => box_l,
                    Align::Right => box_r - line_w,
                    Align::Center => box_l + (box_r - box_l - line_w) / 2.0,
                    Align::Distributed if n <= 1 => box_l + slack / 2.0,
                    // Justified lines that aren't stretched (the last one) sit at the start edge.
                    Align::Justify | Align::JustLow | Align::Distributed if p.rtl => box_r - line_w,
                    Align::Justify | Align::JustLow | Align::Distributed => box_l,
                }
            };
            // Distance from the box's start edge to the text (bullets move with centred or
            // end-aligned text).
            let content_shift = if p.rtl { box_r - (left + line_w) } else { left - box_l };
            // Rules L1 + L2: this line's visual order.
            let chars: Vec<char> = line_cells.iter().map(|c| c.ch).collect();
            let raw_levels: Vec<u8> = line_cells.iter().map(|c| c.level).collect();
            let levels = bidi::line_levels(&chars, &raw_levels, u8::from(p.rtl));
            let order = bidi::visual_order(&levels);
            let mut edges = vec![(left, left); n];
            let mut vx = left;
            for &k in &order {
                let a = adv.get(k).copied().unwrap_or(0.0);
                if let Some(e) = edges.get_mut(k) {
                    *e = (vx, vx + a);
                }
                vx += a;
            }
            let odd = |k: usize| levels.get(k).is_some_and(|l| l % 2 == 1);
            let mut caret_x: Vec<f64> = (0..n).map(|k| edges.get(k).map(|e| if odd(k) { e.1 } else { e.0 }).unwrap_or(left)).collect();
            caret_x.push(match n.checked_sub(1) {
                Some(k) => edges.get(k).map(|e| if odd(k) { e.0 } else { e.1 }).unwrap_or(left),
                None if p.rtl => left + line_w,
                None => left,
            });
            let bullet = if li == 0 {
                p.bullet.as_ref().map(|(s, _)| {
                    // Centred and end-aligned text carries its bullet along.
                    let end_aligned = if p.rtl { align == Align::Left } else { align == Align::Right };
                    let shifted = align == Align::Center || end_aligned;
                    let b = if shifted && p.cells.is_empty() {
                        content_shift + x0 - bullet_w
                    } else {
                        (margin + indent).max(0.0) + if shifted { content_shift } else { 0.0 }
                    };
                    // x_bullet = S - (marL + indent) for RTL: the bullet's start edge is its right.
                    let bx = if p.rtl { col_w - b - bullet_w } else { b };
                    (s.clone(), bx, baseline)
                })
            } else {
                None
            };
            out.content_width = out.content_width.max(width + x0 + mr);
            let boundary: Vec<bool> = line_cells.iter().map(|c| c.boundary).collect();
            all_lines.push((
                LineInfo {
                    para: pi,
                    start: rl.start,
                    end: rl.end,
                    top,
                    baseline,
                    bottom: y,
                    caret_x,
                    column: 0,
                    rtl: p.rtl,
                    levels,
                    edges,
                    boundary,
                    extent: (left, left + line_w),
                    start_offset: x0,
                },
                LinePlace { order, kashida },
                lh,
                bullet,
            ));
        }
        out.para_lines.push((first_line_idx, raw.len()));
        prev_after = spacing_pts(props.space_after, p.lead_size).max(0.0);
    }
    // The text ends at the last line's descent under exact spacing; the last paragraph's space
    // after counts only when the body says so.
    let total = (y - tail).max(0.0) + if first_last { prev_after } else { 0.0 };
    out.content_height = total;
    // Where column `c` starts: from the left, or from the right with `rtlCol` (PowerPoint puts the
    // first column on the right).
    let rtl_cols = bp.rtl_col.unwrap_or(false) && ncols > 1;
    let col_x = |c: usize| (if rtl_cols { ncols.saturating_sub(1 + c) } else { c }) as f64 * (col_w + col_gap);
    // Columns: distribute lines by height.
    let col_h = if ncols > 1 { inner.height() } else { f64::INFINITY };
    let mut col = 0usize;
    let mut col_y0 = 0.0;
    for (li, _, _, _) in all_lines.iter_mut() {
        if ncols > 1 && li.bottom - col_y0 > col_h + 0.01 && col + 1 < ncols {
            col += 1;
            col_y0 = li.top;
        }
        li.column = col;
        if col > 0 {
            let dy = col_y0;
            li.top -= dy;
            li.baseline -= dy;
            li.bottom -= dy;
        }
        let cx = col_x(col);
        if cx != 0.0 {
            li.shift_x(cx);
        }
    }
    let used_h = if ncols > 1 { all_lines.iter().map(|(l, ..)| l.bottom).fold(0.0, f64::max) } else { total };
    out.overflow = used_h > inner.height() + 0.5 || (ncols > 1 && all_lines.last().is_some_and(|(l, ..)| l.bottom > inner.height() + 0.5));
    let anchor = bp.anchor.unwrap_or(Anchor::Top);
    let dy = match anchor {
        Anchor::Top => 0.0,
        Anchor::Middle => (inner.height() - used_h) / 2.0,
        // PowerPoint draws justified and distributed anchoring at the bottom.
        Anchor::Bottom | Anchor::Justified | Anchor::Distributed => inner.height() - used_h,
    };
    // anchorCtr: centre the drawn text block on the box. Lines sit where their alignment put
    // them, so the origin is set so the block's centre lands on the box's centre — a left-edge
    // offset alone pushed right-aligned lines out of the box (#736). Empty lines carry no ink
    // and stay out of the block. Overflowing content centres symmetrically past both edges.
    // Not applied to vertical text (there the offset would run along the wrong axis) or
    // multi-column bodies (centring unimplemented for them).
    let centre_block = bp.anchor_ctr.unwrap_or(false) && ncols == 1 && matches!(vert, TextDir::Horizontal);
    let ox = if centre_block && !all_lines.is_empty() {
        let ink = all_lines.iter().filter(|(li, ..)| li.end > li.start);
        let (bmin, bmax) = ink.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), (li, ..)| (a.min(li.extent.0), b.max(li.extent.1)));
        if bmax >= bmin { inner.x0 + inner.width() / 2.0 - (bmin + bmax) / 2.0 } else { inner.x0 }
    } else {
        inner.x0
    };
    let oy = inner.y0 + dy;
    // Emit runs, in visual order.
    for (line, place, _, bullet) in all_lines.iter_mut() {
        line.top += oy;
        line.baseline += oy;
        line.bottom += oy;
        line.shift_x(ox);
        if lines_ltr {
            // The line's place mirrored across the box: the first line ends up on the left.
            let d = inner.y0 + inner.y1 - line.top - line.bottom;
            line.top += d;
            line.baseline += d;
            line.bottom += d;
        }
        let Some(p) = shaped_paras.get(line.para) else { continue };
        if let Some((s, bx, _)) = bullet.take()
            && let Some((_, st)) = p.bullet.as_ref()
        {
            let (_, g) = measure_str(&st.face, st.size, &s);
            let run = GlyphRun {
                face: st.face.clone(),
                size: st.size,
                color: st.color,
                glyphs: g.into_iter().map(|(gid, x)| (gid, ox + bx + x + col_x(line.column), line.baseline)).collect(),
                cells: vec![],
                fake_bold: false,
                fake_italic: false,
                outline: None,
                para: line.para,
                chars: (0, 0),
                link: false,
                alpha: st.alpha,
                gradient: None,
                effects: None,
                upright: false,
            };
            if body.paragraphs.get(line.para).is_some_and(|p| p.is_empty()) && opts.prompt_color.is_none() {
                out.empty_bullets.push(run);
            } else {
                out.runs.push(run);
            }
        }
        // Group visually consecutive cells by (style, face).
        let mut cur: Option<GlyphRun> = None;
        let mut cur_key = (usize::MAX, usize::MAX, false, false);
        for &k in &place.order {
            let ci = line.start + k;
            let Some(cell) = p.cells.get(ci) else { continue };
            let Some(st) = p.styles.get(cell.style) else { continue };
            let Some(&(cx, cx1)) = line.edges.get(k) else { continue };
            let base_y = line.baseline - st.baseline * st.size;
            let size = if st.baseline != 0.0 { st.size * SUB_SUPER } else { st.size };
            let size = if st.caps == Caps::Small && cell.ch.is_lowercase() { size * 0.8 } else { size };
            // Small capitals are drawn smaller than the capitals beside them: a run of their own.
            let key = (cell.style, cell.face, st.caps == Caps::Small && cell.ch.is_lowercase(), cell.upright);
            if key != cur_key {
                if let Some(r) = cur.take() {
                    out.runs.push(r);
                }
                let Some(face) = p.faces.get(cell.face) else { continue };
                cur = Some(GlyphRun {
                    face: face.clone(),
                    size,
                    color: st.color,
                    glyphs: vec![],
                    cells: vec![],
                    fake_bold: st.fake_bold,
                    fake_italic: st.fake_italic,
                    outline: st.outline,
                    para: line.para,
                    chars: (ci, ci + 1),
                    link: st.link,
                    alpha: st.alpha,
                    gradient: st.gradient.clone(),
                    effects: st.effects,
                    upright: cell.upright,
                });
                cur_key = key;
            }
            if let Some(r) = cur.as_mut() {
                r.chars = (r.chars.0.min(ci), r.chars.1.max(ci + 1));
                if cell.ch == '\u{AD}' && cx1 - cx > 0.01 {
                    // The soft hyphen a line breaks at is drawn as a hyphen.
                    if let Some(f) = p.faces.get(cell.face) {
                        r.glyphs.push((f.glyph_for('-'), cx, base_y));
                        r.cells.push((ci, cell.ch));
                    }
                } else if cell.head == ci && !cell.glyphs.is_empty() {
                    // The cluster's glyphs start at its leftmost cell; kashida (right-to-left
                    // only) fills the gap on the cluster's left with tatweel glyphs.
                    let cells_of =
                        (k..line.end.saturating_sub(line.start)).take_while(|&j| p.cells.get(line.start + j).is_some_and(|c| c.head == ci));
                    let (mut cl, mut kash) = (cx, 0.0);
                    for j in cells_of {
                        cl = cl.min(line.edges.get(j).map(|e| e.0).unwrap_or(cx));
                        kash += place.kashida.get(j).copied().unwrap_or(0.0);
                    }
                    if kash > 0.0 {
                        let face = p.faces.get(cell.face);
                        let tg = face.map(|f| f.glyph_for('\u{0640}')).unwrap_or(0);
                        let tw = face.map(|f| f.advance(tg) * size / f.upem.max(1.0)).unwrap_or(0.0);
                        if tg != 0 && tw > 0.0 {
                            let count = ((kash / tw).ceil() as usize).clamp(1, 256);
                            for i in 0..count {
                                r.glyphs.push((tg, cl + (i as f64 * tw).min((kash - tw).max(0.0)), base_y));
                                r.cells.push((ci, cell.ch));
                            }
                        }
                    }
                    for (gid, gx, gy) in &cell.glyphs {
                        r.glyphs.push((*gid, cl + kash + gx, base_y + gy));
                        r.cells.push((ci, cell.ch));
                    }
                }
            }
            let adv = cx1 - cx;
            if adv <= 0.0 {
                continue;
            }
            if let Some(hl) = st.highlight {
                out.decos.push(Deco {
                    rect: Rect::new(cx, line.top, cx1, line.bottom),
                    color: hl,
                    behind: true,
                    para: line.para,
                    line: DecoLine::Solid,
                });
            }
            if !cell.ch.is_whitespace() || st.underline.is_some() {
                if let Some((uc, u)) = st.underline {
                    // At the underline position and thickness of the run's own font.
                    let k = st.size / st.face.upem.max(1.0);
                    let (p, t) = (st.face.underline.0 * k, (st.face.underline.1 * k).max(0.5));
                    for (y0, y1, kind) in u.boxes(p, t) {
                        out.decos.push(Deco {
                            rect: Rect::new(cx, line.baseline + y0, cx1, line.baseline + y1),
                            color: uc,
                            behind: false,
                            para: line.para,
                            line: kind,
                        });
                    }
                }
                if let Some(sk) = st.strike {
                    // At the strikeout position and thickness of the run's own font.
                    let k = st.size / st.face.upem.max(1.0);
                    let t = (st.face.strikeout.1 * k).max(0.5);
                    let sy = line.baseline - st.face.strikeout.0 * k;
                    out.decos.push(Deco {
                        rect: Rect::new(cx, sy, cx1, sy + t),
                        color: st.color,
                        behind: false,
                        para: line.para,
                        line: DecoLine::Solid,
                    });
                    if sk == Strike::Double {
                        out.decos.push(Deco {
                            rect: Rect::new(cx, sy - t * 2.0, cx1, sy - t),
                            color: st.color,
                            behind: false,
                            para: line.para,
                            line: DecoLine::Solid,
                        });
                    }
                }
            }
        }
        if let Some(r) = cur.take() {
            out.runs.push(r);
        }
    }
    out.boundaries = shaped_paras
        .iter()
        .map(|p| {
            let mut b: Vec<bool> = p.cells.iter().map(|c| c.boundary).collect();
            b.push(true);
            if let Some(f) = b.first_mut() {
                *f = true;
            }
            b
        })
        .collect();
    out.lines = all_lines.into_iter().map(|(l, ..)| l).collect();
    // Merge adjacent decorations of the same colour on the same line.
    out.decos = merge_decos(std::mem::take(&mut out.decos));
    out
}

fn merge_decos(v: Vec<Deco>) -> Vec<Deco> {
    let mut out: Vec<Deco> = Vec::with_capacity(v.len());
    for d in v {
        // Lines drawn together (a double underline, a strikethrough) interleave: look a few back.
        if let Some(last) = out.iter_mut().rev().take(4).find(|last| {
            last.color == d.color
                && last.behind == d.behind
                && last.line == d.line
                && (last.rect.y0 - d.rect.y0).abs() < 0.01
                && (last.rect.y1 - d.rect.y1).abs() < 0.01
                && (last.rect.x1 - d.rect.x0).abs() < 0.5
        }) {
            last.rect.x1 = d.rect.x1;
            continue;
        }
        out.push(d);
    }
    out
}

/// How a line's cells are placed: visual order (indices from the line start) and kashida widths
/// (the boxes, with justification, are in [`LineInfo::edges`]).
struct LinePlace {
    order: Vec<usize>,
    kashida: Vec<f64>,
}

/// Kashida opportunities on a line (indices from the line start), at most one per word (its last
/// join, where elongation reads most naturally): the last cell of a right-to-left Arabic grapheme
/// that joins the next letter, in a face with a tatweel glyph, when the two aren't one ligature.
fn kashida_opportunities(p: &ParaShaped, start: usize, end: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let mut word_has = false;
    for i in start..end {
        let Some(c) = p.cells.get(i) else { continue };
        if c.ch.is_whitespace() {
            word_has = false;
            continue;
        }
        if !c.boundary || c.level % 2 == 0 || !joins_following(c.ch) {
            continue;
        }
        // The next grapheme on the line.
        let Some(nx) = (i + 1..end).find(|&j| p.cells.get(j).is_some_and(|d| d.boundary)) else { continue };
        let Some(d) = p.cells.get(nx) else { continue };
        let same_cluster = d.head == c.head || d.head != nx;
        let tatweel = p.faces.get(c.face).is_some_and(|f| f.glyph_for('\u{0640}') != 0);
        if joins_preceding(d.ch) && d.face == c.face && !same_cluster && tatweel {
            // A later join in the same word replaces the earlier one.
            if word_has {
                out.pop();
            }
            out.push(nx - 1 - start);
            word_has = true;
        }
    }
    out
}
