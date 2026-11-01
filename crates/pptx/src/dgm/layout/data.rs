//! The diagram's ordered data tree; presentation and transition points remain distinct.
use crate::xml::El;
use std::collections::{HashMap, HashSet};

pub(super) struct Data<'a> {
    pub points: Vec<&'a El>,
    pub root: usize,
    children: Vec<Vec<usize>>,
    parent: Vec<Option<usize>>,
    transitions: HashMap<usize, (Option<usize>, Option<usize>)>,
    endpoints: HashMap<usize, (usize, usize)>,
}

impl<'a> Data<'a> {
    pub fn new(root: &'a El) -> Option<Self> {
        let points: Vec<_> = root.child("ptLst")?.children_named("pt").take(16001).collect();
        if points.len() > 16000 {
            return None;
        }
        let ids: HashMap<_, _> = points.iter().enumerate().filter_map(|(i, p)| Some((p.attr("modelId")?, i))).collect();
        let doc = points.iter().position(|p| p.attr("type") == Some("doc"))?;
        let mut children = vec![vec![]; points.len()];
        let mut parent = vec![None; points.len()];
        let mut transitions = HashMap::new();
        let mut edges: Vec<_> = root.child("cxnLst")?.children_named("cxn").take(32001).collect();
        if edges.len() > 32000 {
            return None;
        }
        edges.retain(|c| c.attr("type").unwrap_or("parOf") == "parOf");
        edges.sort_by_key(|c| c.u32("srcOrd").unwrap_or(0));
        for edge in edges {
            let (Some(&a), Some(&b)) = (edge.attr("srcId").and_then(|id| ids.get(id)), edge.attr("destId").and_then(|id| ids.get(id))) else {
                continue;
            };
            if a == b || parent.get(b).copied().flatten().is_some() {
                continue;
            }
            if let Some(list) = children.get_mut(a) {
                list.push(b);
            }
            if let Some(p) = parent.get_mut(b) {
                *p = Some(a);
            }
            transitions.insert(
                b,
                (edge.attr("parTransId").and_then(|id| ids.get(id)).copied(), edge.attr("sibTransId").and_then(|id| ids.get(id)).copied()),
            );
        }
        let mut endpoints = HashMap::new();
        for (&child, &(par, sib)) in &transitions {
            if let Some(parent) = parent.get(child).copied().flatten() {
                if let Some(id) = par {
                    endpoints.insert(id, (parent, child));
                }
                if let Some(id) = sib
                    && let Some(siblings) = children.get(parent)
                    && let Some(pos) = siblings.iter().position(|i| *i == child)
                    && let Some(next) = siblings.get(pos + 1)
                {
                    endpoints.insert(id, (child, *next));
                }
            }
        }
        Some(Self { points, root: doc, children, parent, transitions, endpoints })
    }

    pub fn endpoints(&self, point: usize) -> Option<(usize, usize)> {
        self.endpoints.get(&point).copied()
    }

    fn descend(&self, context: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut seen = HashSet::from([context]);
        let mut stack: Vec<_> = self.children.get(context).into_iter().flatten().rev().copied().collect();
        while let Some(i) = stack.pop() {
            if !seen.insert(i) {
                continue;
            }
            out.push(i);
            stack.extend(self.children.get(i).into_iter().flatten().rev().copied());
        }
        out
    }

    pub fn depth(&self, context: usize) -> usize {
        let mut seen = HashSet::new();
        let mut next = Some(context);
        while let Some(i) = next {
            if !seen.insert(i) {
                break;
            }
            next = self.parent.get(i).copied().flatten();
        }
        seen.len().saturating_sub(1)
    }

    fn axis(&self, context: usize, axis: &str, pt_type: &str) -> Vec<usize> {
        let parent = self.parent.get(context).copied().flatten();
        let siblings = parent.and_then(|p| self.children.get(p)).cloned().unwrap_or_default();
        let position = siblings.iter().position(|i| *i == context).unwrap_or(0);
        if matches!(pt_type, "sibTrans" | "parTrans") {
            if axis == "self" && self.points.get(context).is_some_and(|p| p.attr("type") == Some(pt_type)) {
                return vec![context];
            }
            let transition = self.transitions.get(&context).and_then(|(par, sib)| if pt_type == "parTrans" { *par } else { *sib });
            let is_last = position + 1 >= siblings.len();
            return if axis == "followSib" && is_last { vec![] } else { transition.into_iter().collect() };
        }
        match axis {
            "self" => vec![context],
            "ch" => self.children.get(context).cloned().unwrap_or_default(),
            "des" => self.descend(context),
            "desOrSelf" => std::iter::once(context).chain(self.descend(context)).collect(),
            "par" => parent.into_iter().collect(),
            "root" => vec![self.root],
            "followSib" => siblings.into_iter().skip(position + 1).collect(),
            "precedSib" => siblings.into_iter().take(position).collect(),
            "ancst" | "ancstOrSelf" => {
                let mut next = if axis == "ancst" { parent } else { Some(context) };
                let mut out = Vec::new();
                let mut seen = HashSet::new();
                while let Some(i) = next {
                    if !seen.insert(i) {
                        break;
                    }
                    out.push(i);
                    next = self.parent.get(i).copied().flatten();
                }
                out
            }
            "follow" | "preced" => {
                let order = self.descend(self.root);
                let pos = order.iter().position(|i| *i == context).unwrap_or(0);
                if axis == "follow" { order.into_iter().skip(pos + 1).collect() } else { order.into_iter().take(pos).collect() }
            }
            _ => vec![],
        }
    }

    pub fn matches(&self, index: usize, kind: &str) -> bool {
        let Some(point) = self.points.get(index) else { return false };
        let ty = point.attr("type").unwrap_or("node");
        match kind {
            "all" | "" => true,
            "node" => ty == "node" || ty == "asst",
            "asst" => ty == "asst",
            "nonAsst" | "norm" => ty == "node",
            "nonNorm" => ty != "node",
            _ => ty == kind,
        }
    }

    #[cfg(test)]
    pub fn select(&self, selector: &El, context: usize) -> Vec<usize> {
        self.select_bounded(selector, context, &mut 0).unwrap_or_default()
    }

    pub fn select_bounded(&self, selector: &El, context: usize, work: &mut usize) -> Result<Vec<usize>, &'static str> {
        let axes: Vec<_> = selector.attr("axis").unwrap_or("self").split_whitespace().take(17).collect();
        if axes.len() > 16 {
            return Err("too many diagram selection stages");
        }
        let kinds: Vec<_> = selector.attr("ptType").unwrap_or("all").split_whitespace().take(16).collect();
        let starts: Vec<_> = selector.attr("st").unwrap_or("1").split_whitespace().take(16).collect();
        let counts: Vec<_> = selector.attr("cnt").unwrap_or("0").split_whitespace().take(16).collect();
        let steps: Vec<_> = selector.attr("step").unwrap_or("1").split_whitespace().take(16).collect();
        let mut values = vec![context];
        for (stage, axis) in axes.iter().enumerate() {
            fn at<'a>(v: &[&'a str], stage: usize, default: &'a str) -> &'a str {
                v.get(stage).copied().unwrap_or(default)
            }
            let kind = at(&kinds, stage, "all");
            let start = at(&starts, stage, "1").parse::<i32>().unwrap_or(1);
            let count = at(&counts, stage, "0").parse::<usize>().unwrap_or(0).min(16000);
            let step = at(&steps, stage, "1").parse::<i32>().unwrap_or(1);
            if step == 0 {
                return Ok(vec![]);
            }
            let mut next = Vec::new();
            for context in values {
                // Charge the worst-case traversal even when filtering yields no result.
                let cost = match *axis {
                    "self" | "root" | "par" => 1,
                    "ch" => self.children.get(context).map_or(0, Vec::len),
                    _ => self.points.len(),
                };
                *work = work.saturating_add(cost);
                if *work > super::MAX_WORK {
                    return Err("diagram selection budget exceeded");
                }
                let mut candidates: Vec<_> = self.axis(context, axis, kind).into_iter().filter(|i| self.matches(*i, kind)).collect();
                if step < 0 {
                    candidates.reverse();
                }
                let start = if start < 0 { candidates.len().saturating_sub(start.unsigned_abs() as usize) } else { start.saturating_sub(1) as usize };
                next.extend(candidates.into_iter().skip(start).step_by(step.unsigned_abs() as usize).take(if count == 0 { 16000 } else { count }));
                if next.len() > 16000 {
                    return Err("too many diagram selection results");
                }
            }
            values = next;
        }
        Ok(values)
    }
}
