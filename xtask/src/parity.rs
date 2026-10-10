//! `cargo xtask parity`: recompute the feature-parity summary in docs/parity-checklist.md from its row
//! table (each PowerPoint feature scored D / P / M, weighted by priority).

use std::path::Path;

const START: &str = "<!-- SUMMARY -->";
const ROWS: &str = "\n## Rows";

/// (area, feature, priority 0–2, value 0 / 0.5 / 1) for each table row.
pub fn rows(text: &str) -> Vec<(String, String, u8, f64)> {
    let mut out = Vec::new();
    for l in text.lines() {
        let cells: Vec<&str> = l.trim().trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() != 4 {
            continue;
        }
        let pri = match cells[2] {
            "P0" => 0,
            "P1" => 1,
            "P2" => 2,
            _ => continue,
        };
        let v = match cells[3] {
            "D" => 1.0,
            "P" => 0.5,
            "M" => 0.0,
            _ => continue,
        };
        out.push((cells[0].to_string(), cells[1].to_string(), pri, v));
    }
    out
}

fn weight(p: u8) -> f64 {
    [3.0, 2.0, 1.0][p as usize]
}

pub fn summary(rows: &[(String, String, u8, f64)]) -> String {
    let pct = |rs: &[&(String, String, u8, f64)]| {
        let tot: f64 = rs.iter().map(|r| weight(r.2)).sum();
        if tot == 0.0 { 0.0 } else { 100.0 * rs.iter().map(|r| weight(r.2) * r.3).sum::<f64>() / tot }
    };
    let all: Vec<_> = rows.iter().collect();
    let mut s = format!(
        "**Weighted breadth parity: {:.0}%** over {} features.\n\n| Priority | Features | Done | Partial | Missing | Parity |\n|---|---|---|---|---|---|\n",
        pct(&all),
        rows.len()
    );
    for p in 0..3u8 {
        let rr: Vec<_> = rows.iter().filter(|r| r.2 == p).collect();
        let n = |v: f64| rr.iter().filter(|r| r.3 == v).count();
        let unweighted = if rr.is_empty() { 0.0 } else { 100.0 * rr.iter().map(|r| r.3).sum::<f64>() / rr.len() as f64 };
        s += &format!("| P{p} | {} | {} | {} | {} | {unweighted:.0}% |\n", rr.len(), n(1.0), n(0.5), n(0.0));
    }
    s += "\n| Area | Parity |\n|---|---|\n";
    let mut areas: Vec<&str> = Vec::new();
    for r in rows {
        if !areas.contains(&r.0.as_str()) {
            areas.push(&r.0);
        }
    }
    for a in areas {
        let rr: Vec<_> = rows.iter().filter(|r| r.0 == a).collect();
        s += &format!("| {a} | {:.0}% |\n", pct(&rr));
    }
    let open: Vec<&str> = rows.iter().filter(|r| r.2 == 0 && r.3 < 1.0).map(|r| r.1.as_str()).collect();
    s += &format!("\nOpen P0 items: {}.\n", open.join(", "));
    s
}

pub fn run(root: &Path) -> Result<(), String> {
    let path = root.join("docs/parity-checklist.md");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (Some(a), Some(b)) = (text.find(START), text.find(ROWS)) else {
        return Err("docs/parity-checklist.md needs a `<!-- SUMMARY -->` marker before `## Rows`".into());
    };
    let rows = rows(&text[b..]);
    let s = summary(&rows);
    let new = format!("{}{START}\n{s}{}", &text[..a], &text[b..]);
    std::fs::write(&path, new).map_err(|e| e.to_string())?;
    print!("{s}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_rows() {
        let t = "| Area | Feature | Priority | Status |\n|---|---|---|---|\n| A | x | P0 | D |\n| A | y | P2 | M |\n| B | z | P1 | P |\n";
        let r = rows(t);
        assert_eq!(r.len(), 3);
        let s = summary(&r);
        // (3·1 + 1·0 + 2·0.5) / 6 = 67%
        assert!(s.contains("**Weighted breadth parity: 67%**"), "{s}");
        assert!(s.contains("Open P0 items: ."));
    }
}
