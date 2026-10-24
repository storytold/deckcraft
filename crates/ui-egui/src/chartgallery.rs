//! The chart-type picker of Insert ▸ Chart and Chart Design ▸ Change Chart Type: a tile per
//! type with a small sample chart drawn in code (our own shapes and colours), so you can see
//! what you are picking.

use deckcraft_model::chart::ChartType;
use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

use crate::theme::Tokens;

/// One element of a sample, in unit coordinates (0..1, y down) of the sample box. `series`
/// picks the colour.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Rect { min: [f32; 2], max: [f32; 2], series: usize },
    Line { pts: Vec<[f32; 2]>, series: usize },
    Poly { pts: Vec<[f32; 2]>, series: usize },
    Dot { at: [f32; 2], r: f32, series: usize },
}

impl Prim {
    pub fn points(&self) -> Vec<[f32; 2]> {
        match self {
            Prim::Rect { min, max, .. } => vec![*min, *max],
            Prim::Line { pts, .. } | Prim::Poly { pts, .. } => pts.clone(),
            Prim::Dot { at, .. } => vec![*at],
        }
    }
}

const SERIES_COLOURS: [Color32; 3] = [Color32::from_rgb(0x2E, 0x86, 0xC1), Color32::from_rgb(0xE6, 0x7E, 0x22), Color32::from_rgb(0x7D, 0x9F, 0x35)];

/// Bars across `n` categories of `vals[series][category]` (0..1), clustered, stacked or 100 %.
fn bars(vals: &[[f32; 3]], horizontal: bool, stack: Stack) -> Vec<Prim> {
    let cats = 3usize;
    let n = vals.len().max(1);
    let mut out = vec![];
    for c in 0..cats {
        let slot = 1.0 / cats as f32;
        let (c0, c1) = (c as f32 * slot + slot * 0.15, (c as f32 + 1.0) * slot - slot * 0.15);
        let total: f32 = vals.iter().map(|v| v[c]).sum::<f32>().max(1e-3);
        let mut base = 0.0f32;
        for (s, v) in vals.iter().enumerate() {
            let (lo, hi, a, b) = match stack {
                Stack::Clustered => {
                    let w = (c1 - c0) / n as f32;
                    (0.0, v[c], c0 + w * s as f32, c0 + w * (s as f32 + 1.0))
                }
                Stack::Stacked => (base, base + v[c] / 1.6, c0, c1),
                Stack::Percent => (base, base + v[c] / total, c0, c1),
            };
            base = hi;
            out.push(if horizontal {
                Prim::Rect { min: [lo, a], max: [hi, b], series: s }
            } else {
                Prim::Rect { min: [a, 1.0 - hi], max: [b, 1.0 - lo], series: s }
            });
        }
    }
    out
}

#[derive(Clone, Copy)]
enum Stack {
    Clustered,
    Stacked,
    Percent,
}

fn wedges(fracs: &[f32], inner: f32) -> Vec<Prim> {
    let (c, r) = ([0.5f32, 0.5f32], 0.48f32);
    let mut a0 = -std::f32::consts::FRAC_PI_2;
    let mut out = vec![];
    for (s, f) in fracs.iter().enumerate() {
        let a1 = a0 + f * std::f32::consts::TAU;
        let steps = ((f * 32.0).ceil() as usize).max(2);
        let arc =
            |rad: f32| (0..=steps).map(move |k| a0 + (a1 - a0) * k as f32 / steps as f32).map(move |a| [c[0] + rad * a.cos(), c[1] + rad * a.sin()]);
        let mut pts: Vec<[f32; 2]> = arc(r).collect();
        if inner > 0.0 {
            pts.extend(arc(r * inner).collect::<Vec<_>>().into_iter().rev());
        } else {
            pts.push(c);
        }
        out.push(Prim::Poly { pts, series: s % SERIES_COLOURS.len() });
        a0 = a1;
    }
    out
}

fn polyline(ys: &[f32]) -> Vec<[f32; 2]> {
    let n = ys.len().max(2) - 1;
    ys.iter().enumerate().map(|(i, y)| [i as f32 / n as f32, 1.0 - y]).collect()
}

/// The sample of chart type `k`.
pub fn sample(k: ChartType) -> Vec<Prim> {
    use ChartType as K;
    let v2 = [[0.55, 0.8, 0.4], [0.35, 0.6, 0.7]];
    match k {
        K::Column | K::Combo => bars(&v2, false, Stack::Clustered),
        K::StackedColumn => bars(&v2, false, Stack::Stacked),
        K::PercentColumn => bars(&v2, false, Stack::Percent),
        K::Bar => bars(&v2, true, Stack::Clustered),
        K::StackedBar => bars(&v2, true, Stack::Stacked),
        K::PercentBar => bars(&v2, true, Stack::Percent),
        K::Line | K::StackedLine => {
            vec![Prim::Line { pts: polyline(&[0.3, 0.6, 0.45, 0.85]), series: 0 }, Prim::Line { pts: polyline(&[0.15, 0.3, 0.55, 0.4]), series: 1 }]
        }
        K::LineMarkers => {
            let a = polyline(&[0.3, 0.6, 0.45, 0.85]);
            let mut v: Vec<Prim> = a.iter().map(|p| Prim::Dot { at: *p, r: 0.05, series: 0 }).collect();
            v.insert(0, Prim::Line { pts: a, series: 0 });
            v
        }
        K::Area | K::StackedArea => {
            let mut back = polyline(&[0.5, 0.8, 0.6, 0.9]);
            back.extend([[1.0, 1.0], [0.0, 1.0]]);
            let mut front = polyline(&[0.2, 0.4, 0.3, 0.5]);
            front.extend([[1.0, 1.0], [0.0, 1.0]]);
            vec![Prim::Poly { pts: back, series: 0 }, Prim::Poly { pts: front, series: 1 }]
        }
        K::Pie => wedges(&[0.45, 0.3, 0.25], 0.0),
        K::Doughnut | K::Sunburst => wedges(&[0.45, 0.3, 0.25], 0.5),
        K::Scatter => [[0.15, 0.7], [0.3, 0.55], [0.45, 0.6], [0.6, 0.35], [0.75, 0.3], [0.88, 0.15]]
            .iter()
            .enumerate()
            .map(|(i, p)| Prim::Dot { at: *p, r: 0.05, series: i % 2 })
            .collect(),
        K::Bubble => [([0.25, 0.6], 0.12), ([0.55, 0.4], 0.18), ([0.8, 0.7], 0.09)]
            .iter()
            .enumerate()
            .map(|(i, (p, r))| Prim::Dot { at: *p, r: *r, series: i })
            .collect(),
        K::Radar | K::FilledRadar => {
            let ring = |rs: &[f32]| -> Vec<[f32; 2]> {
                rs.iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / rs.len() as f32;
                        [0.5 + 0.48 * r * a.cos(), 0.5 + 0.48 * r * a.sin()]
                    })
                    .collect()
            };
            let a = ring(&[0.9, 0.6, 0.8, 0.5, 0.7]);
            let b = ring(&[0.5, 0.9, 0.4, 0.8, 0.45]);
            if k == K::FilledRadar {
                vec![Prim::Poly { pts: a, series: 0 }, Prim::Poly { pts: b, series: 1 }]
            } else {
                let close = |mut p: Vec<[f32; 2]>| {
                    if let Some(f) = p.first().copied() {
                        p.push(f);
                    }
                    p
                };
                vec![Prim::Line { pts: close(a), series: 0 }, Prim::Line { pts: close(b), series: 1 }]
            }
        }
        K::Waterfall => {
            // Start, two rises, a fall, the total.
            let steps = [(0.0, 0.4, 0), (0.4, 0.6, 2), (0.6, 0.75, 2), (0.75, 0.55, 1), (0.0, 0.55, 0)];
            steps
                .iter()
                .enumerate()
                .map(|(i, (a, b, s)): (usize, &(f32, f32, usize))| {
                    let x0 = i as f32 / 5.0 + 0.03;
                    Prim::Rect { min: [x0, 1.0 - a.max(*b)], max: [x0 + 0.14, 1.0 - a.min(*b)], series: *s }
                })
                .collect()
        }
        K::Funnel => [0.95, 0.75, 0.55, 0.35]
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let y = i as f32 * 0.25;
                Prim::Rect { min: [0.5 - w / 2.0, y + 0.02], max: [0.5 + w / 2.0, y + 0.23], series: 0 }
            })
            .collect(),
        K::Histogram | K::Pareto => {
            let hs = [0.3, 0.75, 0.95, 0.6, 0.25];
            let mut v: Vec<Prim> = hs
                .iter()
                .enumerate()
                .map(|(i, h)| Prim::Rect { min: [i as f32 / 5.0, 1.0 - h], max: [(i + 1) as f32 / 5.0 - 0.01, 1.0], series: 0 })
                .collect();
            if k == K::Pareto {
                v.push(Prim::Line { pts: polyline(&[0.35, 0.6, 0.8, 0.93, 1.0]), series: 1 });
            }
            v
        }
        K::Treemap | K::RegionMap => vec![
            Prim::Rect { min: [0.0, 0.0], max: [0.58, 1.0], series: 0 },
            Prim::Rect { min: [0.6, 0.0], max: [1.0, 0.58], series: 1 },
            Prim::Rect { min: [0.6, 0.6], max: [0.79, 1.0], series: 2 },
            Prim::Rect { min: [0.81, 0.6], max: [1.0, 1.0], series: 1 },
        ],
        K::BoxWhisker | K::Stock => {
            let boxes = [(0.2, 0.35, 0.65, 0.8), (0.5, 0.15, 0.45, 0.6), (0.8, 0.3, 0.55, 0.9)];
            let mut v = vec![];
            for (x, lo, hi, whisk) in boxes {
                v.push(Prim::Line { pts: vec![[x, 1.0 - whisk], [x, 1.0 - lo + 0.1]], series: 0 });
                v.push(Prim::Rect { min: [x - 0.08, 1.0 - hi], max: [x + 0.08, 1.0 - lo], series: if k == K::Stock { 1 } else { 0 } });
            }
            v
        }
        K::Surface => {
            let mut v = vec![];
            for (s, ys) in [[0.2, 0.45, 0.3, 0.5], [0.45, 0.7, 0.55, 0.8]].iter().enumerate() {
                let mut pts = polyline(ys);
                pts.extend([[1.0, 1.0], [0.0, 1.0]]);
                v.push(Prim::Poly { pts, series: s });
            }
            v
        }
    }
}

/// Paint `prims` into `r` (with a little margin).
pub fn paint(painter: &egui::Painter, r: Rect, prims: &[Prim]) {
    let r = r.shrink(4.0);
    let at = |p: [f32; 2]| -> Pos2 { pos2(r.min.x + p[0] * r.width(), r.min.y + p[1] * r.height()) };
    let col = |s: usize| SERIES_COLOURS[s % SERIES_COLOURS.len()];
    for p in prims {
        match p {
            Prim::Rect { min, max, series } => {
                painter.rect_filled(Rect::from_two_pos(at(*min), at(*max)), CornerRadius::ZERO, col(*series));
            }
            Prim::Line { pts, series } => {
                painter.add(Shape::line(pts.iter().map(|p| at(*p)).collect(), Stroke::new(1.8, col(*series))));
            }
            Prim::Poly { pts, series } => {
                // Fans from the first point: fine for the convex-enough samples drawn here.
                let ps: Vec<Pos2> = pts.iter().map(|p| at(*p)).collect();
                let c = col(*series);
                painter.add(Shape::Path(egui::epaint::PathShape {
                    points: ps,
                    closed: true,
                    fill: c.gamma_multiply(0.85),
                    stroke: Stroke::new(0.5, c).into(),
                }));
            }
            Prim::Dot { at: p, r: rad, series } => {
                painter.circle_filled(at(*p), rad * r.width().min(r.height()), col(*series));
            }
        }
    }
}

/// A grid of chart-type tiles (sample above, name below); returns the type clicked.
pub fn picker(ui: &mut Ui, kinds: &[ChartType], selected: Option<ChartType>) -> Option<ChartType> {
    let t = Tokens::get(ui.ctx());
    let tile = vec2(96.0, 84.0);
    let mut out = None;
    for row in kinds.chunks(4) {
        ui.horizontal(|ui| {
            for k in row {
                let (rect, resp) = ui.allocate_exact_size(tile, Sense::click());
                let painter = ui.painter_at(rect);
                let sel = selected == Some(*k);
                if sel || resp.hovered() {
                    painter.rect_filled(rect, CornerRadius::same(4), if sel { t.selected } else { t.hover });
                }
                let sample_box = Rect::from_min_size(rect.min + vec2(18.0, 6.0), vec2(tile.x - 36.0, 52.0));
                painter.rect_filled(sample_box, CornerRadius::same(2), Color32::WHITE);
                paint(&painter, sample_box, &sample(*k));
                painter.text(pos2(rect.center().x, rect.max.y - 12.0), egui::Align2::CENTER_CENTER, k.label(), crate::theme::font(10.5), t.text);
                if sel {
                    painter.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
                }
                if resp.on_hover_text(k.label()).clicked() {
                    out = Some(*k);
                }
            }
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [ChartType; 28] = [
        ChartType::Column,
        ChartType::StackedColumn,
        ChartType::PercentColumn,
        ChartType::Bar,
        ChartType::StackedBar,
        ChartType::PercentBar,
        ChartType::Line,
        ChartType::LineMarkers,
        ChartType::StackedLine,
        ChartType::Area,
        ChartType::StackedArea,
        ChartType::Pie,
        ChartType::Doughnut,
        ChartType::Scatter,
        ChartType::Bubble,
        ChartType::Radar,
        ChartType::FilledRadar,
        ChartType::Stock,
        ChartType::Surface,
        ChartType::Histogram,
        ChartType::Pareto,
        ChartType::BoxWhisker,
        ChartType::Waterfall,
        ChartType::Funnel,
        ChartType::Treemap,
        ChartType::RegionMap,
        ChartType::Sunburst,
        ChartType::Combo,
    ];

    #[test]
    fn every_chart_type_has_a_sample_inside_its_box() {
        for k in ALL {
            let s = sample(k);
            assert!(!s.is_empty(), "{k:?}");
            for p in s.iter().flat_map(|p| p.points()) {
                assert!(p.iter().all(|v| v.is_finite() && (-1e-4..=1.0001).contains(v)), "{k:?}: {p:?}");
            }
        }
    }

    #[test]
    fn the_main_types_look_different_from_each_other() {
        let main = ChartType::MAIN;
        for (i, a) in main.iter().enumerate() {
            for b in main.iter().skip(i + 1) {
                assert_ne!(sample(*a), sample(*b), "{a:?} and {b:?} share a sample");
            }
        }
    }
}
