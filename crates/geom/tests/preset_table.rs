//! Every preset against ECMA-376's presetShapeDefinitions.xml, evaluated by
//! `fixtures/make_preset_table.py`: default adjust values and the text rectangle at three sizes.

use deckcraft_geom::preset::{build, defaults, info};

#[test]
fn presets_follow_preset_shape_definitions() {
    let table = include_str!("fixtures/preset_table.txt");
    let sizes = [(200.0, 100.0), (100.0, 200.0), (144.0, 144.0)];
    let mut bad = vec![];
    let mut checked = 0;
    for line in table.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split(" | ").collect();
        let name = f[0];
        // Connectors the gallery doesn't list still build, with the standard's defaults.
        if info(name).is_none() && !name.contains("Connector") {
            bad.push(format!("{name}: not in the catalogue"));
            continue;
        }
        let have = defaults(name);
        checked += 1;
        let want: Vec<f64> = f[1].split_whitespace().map(|v| v.parse().unwrap()).collect();
        // The standard lists stretch factors (star5's hf/vf…) after the adjust handles; the
        // catalogue keeps handles only, so compare what both have.
        for (k, (a, b)) in have.iter().zip(&want).enumerate() {
            if (a - b).abs() > 0.5 {
                bad.push(format!("{name}: adj{} default {a} (standard {b})", k + 1));
            }
        }
        if have.len() > want.len() {
            bad.push(format!("{name}: {} defaults (standard {})", have.len(), want.len()));
        }
        for ((w, h), r) in sizes.iter().zip(f[2].split(" ; ")) {
            let r: Vec<f64> = r.split_whitespace().map(|v| v.parse().unwrap()).collect();
            let g = build(name, *w, *h, &[]).unwrap_or_else(|| panic!("{name} builds"));
            let t = g.text_rect;
            let got = [t.x0, t.y0, t.x1, t.y1];
            if got.iter().zip(&r).any(|(a, b)| (a - b).abs() > 0.6) {
                bad.push(format!("{name} {w}x{h}: text rect {got:.1?} (standard {r:.1?})"));
            }
        }
    }
    assert!(checked > 180, "only {checked} presets checked");
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.join("\n"));
}
