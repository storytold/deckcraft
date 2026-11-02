//! Bounded interpreter for DrawingML diagram layout definitions (ISO/IEC 29500 §21.4).
//! Executes the document's layout tree rather than guessing a template from its display name.
mod constraints;
mod data;
mod geometry;
mod orientation;
mod rules;
#[cfg(test)]
mod tests;
use crate::xml::El;
use data::Data;
use deckcraft_geom::Xfrm;
use std::collections::HashMap;

const MAX_INSTANCES: usize = 10000;
const MAX_WORK: usize = 200000;

pub(crate) struct Placed<'a> {
    pub node_key: usize,
    pub shape: Option<&'a El>,
    pub points: Vec<&'a El>,
    pub point_levels: Vec<usize>,
    pub name: String,
    pub style: String,
    pub rect: Xfrm,
    /// A mapped text algorithm can own a different rectangle from its composite shape.
    pub text_rect: Option<Xfrm>,
    pub text_source: Option<usize>,
    pub text_enabled: bool,
    pub font: Option<f64>,
    pub secondary_font: Option<f64>,
    pub min_font: Option<f64>,
    pub margins: [f64; 4],
    pub text_rotation: f64,
    pub connector: bool,
    pub params: HashMap<String, String>,
    pub endpoints: Option<(usize, usize)>,
}

struct Instance<'a> {
    name: String,
    style: String,
    shape: Option<&'a El>,
    alg: String,
    params: HashMap<String, String>,
    points: Vec<usize>,
    point_levels: Vec<usize>,
    children: Vec<Instance<'a>>,
    constraints: Vec<&'a El>,
    rules: Vec<&'a El>,
    reverse: bool,
}
impl<'a> Instance<'a> {
    fn new(e: &El) -> Self {
        Self {
            name: e.attr("name").unwrap_or("").into(),
            style: e.attr("styleLbl").filter(|s| !s.is_empty()).unwrap_or("node1").into(),
            shape: None,
            alg: "composite".into(),
            params: HashMap::new(),
            points: vec![],
            point_levels: vec![],
            children: vec![],
            constraints: vec![],
            rules: vec![],
            reverse: e.attr("chOrder") == Some("t"),
        }
    }
    fn param(&self, key: &str, default: &str) -> String {
        self.params.get(key).cloned().unwrap_or_else(|| default.into())
    }
    fn number(&self, key: &str, default: f64) -> f64 {
        self.params.get(key).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(default)
    }
}

#[derive(Clone)]
struct Context {
    point: usize,
    position: usize,
    count: usize,
    vars: HashMap<String, String>,
}
struct Compiler<'a> {
    data: Data<'a>,
    refs: HashMap<String, &'a El>,
    instances: usize,
    work: usize,
}

/// Returns an error on exhausted budgets or an unknown algorithm; callers keep the source
/// object and its preview rather than silently substituting an unrelated layout.
pub(crate) fn evaluate<'a>(layout: &'a El, source: &'a El, frame: Xfrm) -> Result<Vec<Placed<'a>>, &'static str> {
    evaluate_measured(layout, source, frame, None)
}

/// Optional text measurement is supplied after the presentation's themes and embedded fonts
/// have been loaded. The interpreter stays independent of any rendering backend.
pub(crate) fn evaluate_measured<'a>(
    layout: &'a El,
    source: &'a El,
    frame: Xfrm,
    measure: Option<&mut rules::Measure<'_>>,
) -> Result<Vec<Placed<'a>>, &'static str> {
    if !frame.is_finite() || frame.w <= 0.0 || frame.h <= 0.0 {
        return Err("invalid diagram frame");
    }
    let data = Data::new(source).ok_or("invalid diagram data model")?;
    let root = layout.child("layoutNode").ok_or("missing diagram layout tree")?;
    let mut refs = HashMap::new();
    fn collect<'a>(e: &'a El, refs: &mut HashMap<String, &'a El>, depth: usize, work: &mut usize) {
        if depth > 64 || *work >= MAX_WORK {
            return;
        }
        *work += 1;
        if e.is("forEach")
            && let Some(name) = e.attr("name")
        {
            refs.insert(name.into(), e);
        }
        for child in e.elements() {
            collect(child, refs, depth + 1, work);
        }
    }
    let mut work = 0;
    collect(root, &mut refs, 0, &mut work);
    let context = Context { point: data.root, position: 1, count: 1, vars: HashMap::new() };
    let mut compiler = Compiler { data, refs, instances: 0, work };
    let tree = compiler.node(root, context, 0)?;
    if let Some(measure) = measure {
        return rules::search(&tree, frame, &compiler.data, measure);
    }
    let mut out = Vec::new();
    geometry::place(&tree, frame, &compiler.data, &mut out, 0)?;
    geometry::connect(&mut out, &compiler.data);
    Ok(out)
}

impl<'a> Compiler<'a> {
    fn tick(&mut self, depth: usize) -> Result<(), &'static str> {
        self.work += 1;
        if depth > 64 || self.work > MAX_WORK { Err("diagram layout budget exceeded") } else { Ok(()) }
    }
    fn node(&mut self, e: &'a El, mut context: Context, depth: usize) -> Result<Instance<'a>, &'static str> {
        self.tick(depth)?;
        self.instances += 1;
        if self.instances > MAX_INSTANCES {
            return Err("too many diagram layout nodes");
        }
        if let Some(variables) = self.data.points.get(context.point).and_then(|p| p.child("prSet")).and_then(|p| p.child("presLayoutVars")) {
            for v in variables.elements().take(128) {
                if let Some(value) = v.attr("val") {
                    context.vars.insert(v.local().into(), value.into());
                }
            }
        }
        let mut node = Instance::new(e);
        node.points.push(context.point);
        self.body(e, &mut node, &mut context, depth + 1)?;
        for &point in &node.points {
            let level = self.data.depth(point);
            self.work = self.work.saturating_add(level + 1);
            self.tick(depth)?;
            node.point_levels.push(level);
        }
        let min = node.point_levels.iter().copied().min().unwrap_or(0);
        for level in &mut node.point_levels {
            *level -= min;
        }
        Ok(node)
    }
    fn body(&mut self, e: &'a El, node: &mut Instance<'a>, context: &mut Context, depth: usize) -> Result<(), &'static str> {
        self.tick(depth)?;
        for child in e.elements() {
            self.tick(depth)?;
            match child.local() {
                "alg" => {
                    let name = child.attr("type").unwrap_or("composite");
                    if !["lin", "cycle", "snake", "hierRoot", "hierChild", "composite", "sp", "tx", "conn", "pyra"].contains(&name) {
                        return Err("unsupported diagram algorithm");
                    }
                    node.alg = name.into();
                    for p in child.children_named("param").take(128) {
                        if let (Some(k), Some(v)) = (p.attr("type"), p.attr("val")) {
                            node.params.insert(k.into(), v.into());
                        }
                    }
                }
                "shape" => node.shape = Some(child),
                "presOf" => {
                    let selected = if child.has_attr("axis") { self.data.select_bounded(child, context.point, &mut self.work)? } else { vec![] };
                    self.work = self.work.saturating_add(selected.len());
                    self.tick(depth)?;
                    node.points = selected;
                }
                "varLst" => {
                    for var in child.elements().take(128) {
                        if let Some(value) = var.attr("val") {
                            context.vars.insert(var.local().into(), value.into());
                        }
                    }
                }
                "constrLst" => node.constraints.extend(child.children_named("constr").take(512)),
                "ruleLst" => node.rules.extend(child.children_named("rule").take(512)),
                "layoutNode" => node.children.push(self.node(child, context.clone(), depth + 1)?),
                "forEach" => {
                    let definition = if let Some(name) = child.attr("ref").filter(|s| !s.is_empty()) {
                        self.refs.get(name).copied().ok_or("unknown diagram iterator reference")?
                    } else {
                        child
                    };
                    let selector = if child.has_attr("axis") { child } else { definition };
                    let points = self.data.select_bounded(selector, context.point, &mut self.work)?;
                    let count = points.len();
                    self.work = self.work.saturating_add(count);
                    self.tick(depth)?;
                    for (i, point) in points.into_iter().enumerate() {
                        let mut nested = Context { point, position: i + 1, count, vars: context.vars.clone() };
                        self.body(definition, node, &mut nested, depth + 1)?;
                    }
                }
                "choose" => {
                    for branch in child.elements() {
                        if branch.is("else") || (branch.is("if") && self.condition(branch, context)?) {
                            self.body(branch, node, &mut context.clone(), depth + 1)?;
                            break;
                        }
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }
    fn condition(&mut self, e: &El, context: &Context) -> Result<bool, &'static str> {
        let actual = match e.attr("func").unwrap_or("cnt") {
            "pos" => context.position.to_string(),
            "revPos" => (context.count + 1 - context.position).to_string(),
            "cnt" => self.data.select_bounded(e, context.point, &mut self.work)?.len().to_string(),
            "depth" => self.data.depth(context.point).to_string(),
            "maxDepth" => {
                self.data.select_bounded(e, context.point, &mut self.work)?.iter().map(|i| self.data.depth(*i)).max().unwrap_or(0).to_string()
            }
            "posEven" => usize::from(context.position.is_multiple_of(2)).to_string(),
            "posOdd" => usize::from(!context.position.is_multiple_of(2)).to_string(),
            "var" => context.vars.get(e.attr("arg").unwrap_or("")).cloned().unwrap_or_default(),
            _ => return Ok(false),
        };
        let expected = e.attr("val").unwrap_or("");
        let ordering = match (actual.parse::<f64>(), expected.parse::<f64>()) {
            (Ok(a), Ok(b)) => a.partial_cmp(&b),
            _ => Some(actual.as_str().cmp(expected)),
        };
        use std::cmp::Ordering::{Equal, Greater, Less};
        Ok(match e.attr("op").unwrap_or("equ") {
            "equ" => ordering == Some(Equal),
            "neq" => ordering != Some(Equal),
            "gt" => ordering == Some(Greater),
            "lt" => ordering == Some(Less),
            "gte" => matches!(ordering, Some(Equal | Greater)),
            "lte" => matches!(ordering, Some(Equal | Less)),
            _ => false,
        })
    }
}
