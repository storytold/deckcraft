//! `cargo xtask parity`: recompute the feature-parity summary in docs/parity-checklist.md from its row
//! table (each PowerPoint feature scored D / P / M, weighted by priority).

use std::path::Path;

const START: &str = "<!-- SUMMARY -->";
const ROWS: &str = "\n## Rows";

/// The cells of a table line that is a scored row: `Area | Feature | Priority | Status`, with an
/// optional fifth `Headless` cell (RFC-0052).
fn row_cells(l: &str) -> Option<Vec<&str>> {
    let cells: Vec<&str> = l.trim().trim_matches('|').split('|').map(str::trim).collect();
    let scored = matches!(cells.len(), 4 | 5) && matches!(cells[2], "P0" | "P1" | "P2") && matches!(cells[3], "D" | "P" | "M");
    scored.then_some(cells)
}

/// (area, feature, priority 0–2, value 0 / 0.5 / 1) for each table row.
pub fn rows(text: &str) -> Vec<(String, String, u8, f64)> {
    let mut out = Vec::new();
    for l in text.lines() {
        let Some(cells) = row_cells(l) else {
            continue;
        };
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

/// A row's RFC-0052 `headless:` cell: empty or `unknown`, `no (<reach>)` or `partial (<reach>)` with
/// a reach of `window`, `os` or `engine-gap`. The rows name no command ids, so nothing can check a
/// `yes`: it is refused until a row cites the engine command that does the feature.
pub fn headless_cell(cell: Option<&str>) -> Result<(&'static str, Option<&'static str>), String> {
    let c = cell.unwrap_or("").trim();
    if c.is_empty() || c == "unknown" {
        return Ok(("unknown", None));
    }
    let (value, rest) = c.split_once(' ').unwrap_or((c, ""));
    let value = match value {
        "no" => "no",
        "partial" => "partial",
        "yes" => return Err(format!("headless `{c}`: a row is `yes` only once it names the engine command id that does it (RFC-0052)")),
        _ => return Err(format!("headless `{c}` is not unknown, `no (<reach>)` or `partial (<reach>)`")),
    };
    let reach = match rest.trim().trim_start_matches('(').trim_end_matches(')') {
        "window" => "window",
        "os" => "os",
        "engine-gap" => "engine-gap",
        r => return Err(format!("headless `{c}`: reach `{r}` is not window, os or engine-gap")),
    };
    Ok((value, Some(reach)))
}

/// Every row's RFC-0052 value, or the first bad cell.
pub fn headless(text: &str) -> Result<Vec<(&'static str, Option<&'static str>)>, String> {
    text.lines().filter_map(row_cells).map(|c| headless_cell(c.get(4).copied()).map_err(|e| format!("{}: {e}", c[1]))).collect()
}

/// The RFC-0052 summary line.
pub fn headless_line(values: &[(&str, Option<&str>)]) -> String {
    let count = |v: &str| values.iter().filter(|h| h.0 == v).count();
    let no = |r: &str| values.iter().filter(|h| h.0 == "no" && h.1 == Some(r)).count();
    format!(
        "**Headless ([RFC-0052](https://github.com/storytold/craft-contrib/blob/main/rfcs/0052-headless-reach.md)): {} yes, {} no (window {}, os {}, engine-gap {}), {} partial, {} unknown.** \
         The rows name no command ids yet, so none can be `yes`. A row may add a fifth `Headless` cell, `no (<reach>)` or \
         `partial (<reach>)` with a reach of `window`, `os` or `engine-gap`; a row without one is `unknown`.\n",
        count("yes"),
        count("no"),
        no("window"),
        no("os"),
        no("engine-gap"),
        count("partial"),
        count("unknown")
    )
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
    let s = format!("{}\n{}", summary(&rows), headless_line(&headless(&text[b..])?));
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

    #[test]
    fn headless_cells_default_to_unknown_and_refuse_yes() {
        let t = "| A | x | P0 | D |\n| A | y | P1 | M | no (window) |\n| B | z | P2 | P | partial (engine-gap) |\n| B | w | P2 | D | unknown |\n";
        let v = headless(t).unwrap_or_default();
        assert_eq!(v, [("unknown", None), ("no", Some("window")), ("partial", Some("engine-gap")), ("unknown", None)]);
        assert_eq!(rows(t).len(), 4, "a fifth cell keeps the row scored");
        let line = headless_line(&v);
        assert!(line.contains("0 yes, 1 no (window 1, os 0, engine-gap 0), 1 partial, 2 unknown"), "{line}");
        for bad in ["yes", "yes (cmd:a.b)", "no", "no (desk)", "maybe (os)"] {
            assert!(headless(&format!("| A | x | P0 | D | {bad} |\n")).is_err(), "{bad}");
        }
    }
}
