//! Closed dependency profile for optional cache deletion. Unlisted grammar always refuses.
use super::{charge, refuse};
use crate::{
    Result,
    xml::{El, Node},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
struct Context {
    scope: &'static str,
    family: String,
    owner: String,
    common_named: bool,
}
pub(super) struct Profile<'a> {
    files: &'a BTreeMap<String, Vec<u8>>,
    owners: BTreeMap<String, String>,
    defs: BTreeSet<(String, String, String)>,
    parents: BTreeMap<(String, String, String), String>,
    metadata: usize,
}
fn ws(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
}
pub(super) fn integer(v: &str) -> bool {
    !v.is_empty() && v.len() <= 20 && v.bytes().all(|b| b.is_ascii_digit()) && v.parse::<u64>().is_ok()
}
fn ident(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 256
        && v.as_bytes().first().is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        && v.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}
/// Font declarations and their three script-slot references use opaque decoded strings,
/// independently of style/shape/resource identifiers. XML attribute normalization has
/// already happened in the parser; never trim, case-fold or normalize the key again.
fn font_name(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 256
        && v.chars().any(|c| !c.is_whitespace())
        && v.chars().all(|c| !c.is_control() && !matches!(c, '\u{fffe}' | '\u{ffff}'))
}
fn decimal(v: &str, signed: bool) -> Option<f64> {
    let body = if signed { v.strip_prefix('-').or_else(|| v.strip_prefix('+')).unwrap_or(v) } else { v };
    let (a, b) = body.split_once('.').map_or((body, None), |(a, b)| (a, Some(b)));
    if a.is_empty() || !a.bytes().all(|b| b.is_ascii_digit()) || b.is_some_and(|b| b.is_empty() || !b.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    v.parse::<f64>().ok().filter(|n| n.is_finite())
}
fn length(v: &str, signed: bool, positive: bool) -> bool {
    let Some(split) = v.len().checked_sub(2) else { return false };
    let Some((n, u)) = v.get(..split).zip(v.get(split..)) else { return false };
    let factor = match u {
        "pt" => 1.,
        "pc" => 12.,
        "in" => 72.,
        "cm" => 72. / 2.54,
        "mm" => 72. / 25.4,
        _ => return false,
    };
    decimal(n, signed).is_some_and(|n| (!positive || n > 0.) && (signed || n >= 0.) && (n * factor).abs() <= 10_000_000.)
}
fn percent(v: &str, zero: bool, max: f64) -> bool {
    v.strip_suffix('%').and_then(|s| decimal(s, false)).is_some_and(|n| if zero { n >= 0. && n <= max } else { n > 0. && n <= max })
}
/// Narrow intersection of the ODF percentage lexical types used by picture controls.
/// RGB neutrality is intentionally checked separately; only contrast permits a minus.
fn picture_percent(v: &str, signed: bool) -> bool {
    if v.len() > 64 {
        return false;
    }
    let Some(body) = v.strip_suffix('%') else {
        return false;
    };
    let body = if signed { body.strip_prefix('-').unwrap_or(body) } else { body };
    let (whole, fraction) = body.split_once('.').map_or((body, None), |(whole, fraction)| (whole, Some(fraction)));
    if fraction.is_some_and(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let small = (1..=2).contains(&whole.len()) && whole.bytes().all(|b| b.is_ascii_digit());
    let hundred = whole == "100" && fraction.is_none_or(|s| s.bytes().all(|b| b == b'0'));
    (small || hundred) && decimal(body, false).is_some_and(|n| (0.0..=100.0).contains(&n))
}
/// Identity-only intersection of ODF 1.2/1.3 clipShape and their XSL 1.0 clip rule.
/// Keep commas and canonical zero lengths; operand auto is deferred because of the
/// official clipShape pattern's grouping. No dimensions, floats or resources are needed.
fn identity_clip(v: &str) -> bool {
    if v.len() > 128 {
        return false;
    }
    if v == "auto" {
        return true;
    }
    let Some(body) = v.strip_prefix("rect(").and_then(|v| v.strip_suffix(')')) else {
        return false;
    };
    let mut operands = body.split(',');
    for _ in 0..4 {
        let Some(operand) = operands.next() else {
            return false;
        };
        if !matches!(operand.trim_matches(' '), "0cm" | "0mm" | "0in" | "0pt" | "0pc") {
            return false;
        }
    }
    operands.next().is_none()
}
fn color(v: &str) -> bool {
    v.len() == 7 && v.starts_with('#') && v.as_bytes().get(1..).is_some_and(|b| b.iter().all(u8::is_ascii_hexdigit))
}
fn family(v: &str) -> bool {
    let v = if let Some(inner) = v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        inner
    } else if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        inner
    } else {
        v
    };
    !v.is_empty()
        && v.len() <= 256
        && v.bytes().any(|b| b.is_ascii_alphabetic())
        && v.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'_' | b'.' | b'-'))
}
fn language(v: &str) -> bool {
    if v == "none" {
        return true;
    }
    if v.len() > 63 {
        return false;
    }
    let mut p = v.split('-');
    let first = p.next().unwrap_or("");
    (2..=8).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_alphabetic())
        && p.all(|s| (1..=8).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric()))
}
fn country(v: &str) -> bool {
    v == "none" || (v.len() == 2 && v.bytes().all(|b| b.is_ascii_alphabetic())) || (v.len() == 3 && v.bytes().all(|b| b.is_ascii_digit()))
}

/// Verify projected lookup keys against expanded identities before using static canonical keys.
pub(super) fn identity(el: &El) -> bool {
    fn valid(key: &str, ns: Option<&str>, local: &str) -> bool {
        let Some((prefix, name)) = key.split_once(':') else { return false };
        let uri = match prefix {
            "office" => "urn:oasis:names:tc:opendocument:xmlns:office:1.0",
            "style" => "urn:oasis:names:tc:opendocument:xmlns:style:1.0",
            "draw" => "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0",
            "text" => "urn:oasis:names:tc:opendocument:xmlns:text:1.0",
            "presentation" => "urn:oasis:names:tc:opendocument:xmlns:presentation:1.0",
            "svg" => "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0",
            "fo" => "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0",
            "meta" => "urn:oasis:names:tc:opendocument:xmlns:meta:1.0",
            "dc" => "http://purl.org/dc/elements/1.1/",
            "manifest" => "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0",
            "xlink" => "http://www.w3.org/1999/xlink",
            "xml" => "http://www.w3.org/XML/1998/namespace",
            _ => return false,
        };
        name == local && ns == Some(uri)
    }
    valid(&el.name, el.expanded_name.namespace.as_deref(), &el.expanded_name.local)
        && el.expanded_attrs.len() == el.attrs.len()
        && el.expanded_attrs.iter().all(|a| valid(&a.projected_key, a.name.namespace.as_deref(), &a.name.local))
}

impl<'a> Profile<'a> {
    pub(super) fn validate(files: &'a BTreeMap<String, Vec<u8>>, docs: &BTreeMap<String, El>, manifest: &El, budget: &mut usize) -> Result<()> {
        let mut p = Self { files, owners: BTreeMap::new(), defs: BTreeSet::new(), parents: BTreeMap::new(), metadata: 0 };
        if !identity(manifest)
            || manifest.name != "manifest:manifest"
            || manifest.attrs.len() != 1
            || !matches!(manifest.attr("manifest:version"), Some("1.2" | "1.3"))
        {
            return Err(refuse("cache manifest profile"));
        }
        let manifest_version = manifest.attr("manifest:version").ok_or_else(|| refuse("cache manifest version"))?;
        let mut root_owner_version = None;
        for node in &manifest.children {
            if let Node::Text(t) = node
                && !ws(t)
            {
                return Err(refuse("cache manifest text"));
            }
        }
        for entry in manifest.elements() {
            charge(budget, 128)?;
            if !identity(entry) || entry.name != "manifest:file-entry" || entry.children.iter().any(|n| !matches!(n,Node::Text(t) if ws(t))) {
                return Err(refuse("cache manifest owner profile"));
            }
            let path = entry.attr("manifest:full-path").ok_or_else(|| refuse("cache owner path"))?;
            let mime = entry.attr("manifest:media-type").ok_or_else(|| refuse("cache owner MIME"))?;
            match entry.attr("manifest:version") {
                None if entry.attrs.len() == 2 => {}
                Some(version) if path == "/" && entry.attrs.len() == 3 && matches!(version, "1.2" | "1.3") && version == manifest_version => {
                    root_owner_version = Some(version);
                }
                _ => return Err(refuse("cache manifest owner profile")),
            }
            if path == "/" && mime != crate::MIME {
                return Err(refuse("cache root owner MIME"));
            }
            p.index_charge(path.len() + mime.len() + 64, budget)?;
            if p.owners.insert(path.into(), mime.into()).is_some() {
                return Err(refuse("duplicate cache owner"));
            }
        }
        for verify in [false, true] {
            for (part, doc) in docs {
                let expected = match part.as_str() {
                    "content.xml" => "office:document-content",
                    "styles.xml" => "office:document-styles",
                    "meta.xml" => "office:document-meta",
                    "settings.xml" => "office:document-settings",
                    _ => return Err(refuse("cache XML inventory")),
                };
                if doc.name != expected || root_owner_version.is_some_and(|version| doc.attr("office:version") != Some(version)) {
                    return Err(refuse("cache part root"));
                }
                let ctx = Context { owner: format!("{part}:global"), ..Context::default() };
                p.walk(doc, "", part, ctx, verify, budget, 0)?;
            }
        }
        for key in p.parents.keys() {
            let mut next = key.clone();
            let mut seen = BTreeSet::new();
            for _ in 0..=p.parents.len() {
                charge(budget, 1024)?;
                if !seen.insert(next.clone()) {
                    return Err(refuse("cache style inheritance cycle"));
                }
                let Some(parent) = p.parents.get(&next) else { break };
                next = ("common".into(), next.1.clone(), parent.clone());
            }
        }
        Ok(())
    }
    fn index_charge(&mut self, n: usize, budget: &mut usize) -> Result<()> {
        self.metadata = self.metadata.checked_add(n).ok_or_else(|| refuse("cache index budget"))?;
        if self.metadata > 8 * 1024 * 1024 {
            return Err(refuse("cache index budget"));
        }
        charge(budget, n)
    }
    fn define(&mut self, scope: &str, kind: &str, value: &str, verify: bool, budget: &mut usize) -> Result<()> {
        if !ident(value) {
            return Err(refuse("cache identifier grammar"));
        }
        self.define_key(scope, kind, value, verify, budget)
    }
    fn define_font(&mut self, part: &str, value: &str, verify: bool, budget: &mut usize) -> Result<()> {
        if !font_name(value) {
            return Err(refuse("cache font name grammar"));
        }
        self.define_key(part, "font", value, verify, budget)
    }
    // Callers validate their own typed key grammar before using the shared bounded index.
    fn define_key(&mut self, scope: &str, kind: &str, value: &str, verify: bool, budget: &mut usize) -> Result<()> {
        if verify {
            return Ok(());
        }
        if self.defs.len() >= 4096 {
            return Err(refuse("cache definition count"));
        }
        self.index_charge(scope.len() + kind.len() + value.len() + 96, budget)?;
        if !self.defs.insert((scope.into(), kind.into(), value.into())) {
            return Err(refuse("cache ambiguous definition"));
        }
        Ok(())
    }
    fn reference(&self, scopes: &[&str], kind: &str, value: &str, verify: bool) -> Result<()> {
        if !ident(value) || (verify && !scopes.iter().any(|scope| self.defs.contains(&(String::from(*scope), kind.into(), value.into())))) {
            return Err(refuse("cache unresolved typed reference"));
        }
        Ok(())
    }
    fn reference_font(&self, part: &str, value: &str, verify: bool) -> Result<()> {
        // At most two exact lookups of <=256-byte keys; walk's per-attribute charge also
        // covers these transient bounded key copies, just like the generic reference path.
        if !font_name(value)
            || (verify && ![part, "styles.xml"].iter().any(|scope| self.defs.contains(&(String::from(*scope), "font".into(), value.into()))))
        {
            return Err(refuse("cache unresolved font reference"));
        }
        Ok(())
    }
    fn part_ref(&self, v: &str) -> Result<()> {
        let path = v.strip_prefix("./").unwrap_or(v);
        if path.is_empty()
            || path.bytes().any(|b| b <= 32 || matches!(b, b':' | b'?' | b'#' | b'%' | b'\\'))
            || path.split('/').any(|p| p.is_empty() || p == "." || p == "..")
            || path == "Thumbnails/thumbnail.png"
        {
            return Err(refuse("cache unsafe part reference"));
        }
        let bytes = self.files.get(path).ok_or_else(|| refuse("cache missing raster"))?;
        let mime = self.owners.get(path).ok_or_else(|| refuse("cache raster owner"))?;
        if !super::super::raster(path, mime, bytes) {
            return Err(refuse("cache non-raster reference"));
        }
        Ok(())
    }
    fn walk(&mut self, el: &El, parent: &str, part: &str, mut ctx: Context, verify: bool, budget: &mut usize, depth: usize) -> Result<()> {
        // This also covers transient bounded context/key copies during both index passes.
        charge(budget, 512usize.saturating_add(el.attrs.len().saturating_mul(1024)))?;
        if depth > 64 || !identity(el) {
            return Err(refuse("cache XML identity/depth"));
        }
        let n = el.name.as_str();
        if !child(parent, n, part, &ctx.family) {
            return Err(refuse("cache unclassified XML context"));
        }
        if n == "meta:creation-date" {
            super::super::metadata::creation(el)?;
        }
        if n == "office:styles" {
            ctx.scope = "common";
        }
        if n == "office:automatic-styles" {
            ctx.scope = if part == "content.xml" { "content-auto" } else { "styles-auto" };
        }
        if matches!(n, "draw:page" | "style:master-page") {
            ctx.owner = format!("{part}:{}", el.source_start());
        }
        if matches!(n, "style:style" | "style:default-style") {
            ctx.family = el.attr("style:family").unwrap_or("").into();
            if !matches!(ctx.family.as_str(), "paragraph" | "text" | "graphic" | "presentation" | "drawing-page") {
                return Err(refuse("cache style family"));
            }
            ctx.common_named = ctx.scope == "common" && n == "style:style";
            if n == "style:style" {
                let name = el.attr("style:name").ok_or_else(|| refuse("cache style name"))?;
                self.define(ctx.scope, &ctx.family, name, verify, budget)?;
                if !verify && let Some(parent) = el.attr("style:parent-style-name") {
                    self.index_charge(name.len() + parent.len() + ctx.family.len() + 128, budget)?;
                    self.parents.insert((ctx.scope.into(), ctx.family.clone(), name.into()), parent.into());
                }
            } else {
                if el.attr("style:name").is_some() {
                    return Err(refuse("cache named default style"));
                }
                self.define(ctx.scope, &format!("default-{}", ctx.family), "__default", verify, budget)?;
            }
        }
        if n == "style:page-layout" {
            ctx.family = "page-layout".into();
        }
        if n == "draw:layer-set" {
            self.define(&ctx.owner, "layer-set", "__set", verify, budget)?;
        }
        let mut ids = 0;
        let mut style_refs = 0;
        for (key, v) in &el.attrs {
            let ok = match n {
                "office:document-content" | "office:document-styles" | "office:document-meta" | "office:document-settings" => {
                    key == "office:version" && matches!(v.as_str(), "1.2" | "1.3")
                }
                "style:style" | "style:default-style" => match key.as_str() {
                    "style:family" | "style:name" => true,
                    "style:display-name" => true,
                    "style:parent-style-name" => {
                        self.reference(&["common"], &ctx.family, v, verify)?;
                        true
                    }
                    _ => false,
                },
                "style:page-layout" if key == "style:name" => {
                    self.define("styles", "layout", v, verify, budget)?;
                    true
                }
                "style:master-page" => match key.as_str() {
                    "style:name" => {
                        self.define("styles", "master", v, verify, budget)?;
                        true
                    }
                    "style:page-layout-name" => {
                        self.reference(&["styles"], "layout", v, verify)?;
                        true
                    }
                    "draw:style-name" => {
                        self.reference(&["styles-auto", "common"], "drawing-page", v, verify)?;
                        true
                    }
                    _ => false,
                },
                "draw:page" => match key.as_str() {
                    "draw:name" => true,
                    "draw:id" | "xml:id" => {
                        ids += 1;
                        self.define(part, "id", v, verify, budget)?;
                        true
                    }
                    "draw:master-page-name" => {
                        self.reference(&["styles"], "master", v, verify)?;
                        true
                    }
                    "draw:style-name" => {
                        self.reference(&["content-auto", "common"], "drawing-page", v, verify)?;
                        true
                    }
                    _ => false,
                },
                "draw:frame" | "draw:rect" | "draw:ellipse" | "draw:circle" | "draw:g" => match key.as_str() {
                    "draw:name" => true,
                    "draw:id" | "xml:id" => {
                        ids += 1;
                        self.define(part, "id", v, verify, budget)?;
                        true
                    }
                    "svg:x" | "svg:y" => length(v, true, false),
                    "svg:width" | "svg:height" => length(v, false, true),
                    "draw:style-name" | "presentation:style-name" => {
                        style_refs += 1;
                        self.reference(
                            &[if part == "content.xml" { "content-auto" } else { "styles-auto" }, "common"],
                            if key == "draw:style-name" { "graphic" } else { "presentation" },
                            v,
                            verify,
                        )?;
                        true
                    }
                    "draw:layer" => {
                        let local = self.defs.contains(&(ctx.owner.clone(), "layer-set".into(), "__set".into()));
                        let owner = if local { ctx.owner.clone() } else { format!("{part}:global") };
                        self.reference(&[&owner], "layer", v, verify)?;
                        true
                    }
                    _ => false,
                },
                "text:p" | "text:span" => match key.as_str() {
                    "text:style-name" => {
                        self.reference(
                            &[if part == "content.xml" { "content-auto" } else { "styles-auto" }, "common"],
                            if n == "text:p" { "paragraph" } else { "text" },
                            v,
                            verify,
                        )?;
                        true
                    }
                    "xml:id" => {
                        self.define(part, "id", v, verify, budget)?;
                        true
                    }
                    _ => false,
                },
                "text:s" => key == "text:c" && integer(v) && v.parse::<u64>().is_ok_and(|n| (1..=100000).contains(&n)),
                "draw:image" | "draw:fill-image" => match key.as_str() {
                    "draw:name" if n == "draw:fill-image" => {
                        self.define("common", "fill", v, verify, budget)?;
                        true
                    }
                    "xlink:href" => {
                        self.part_ref(v)?;
                        true
                    }
                    "xlink:type" => v == "simple",
                    "xlink:show" => v == "embed",
                    "xlink:actuate" => v == "onLoad",
                    _ => false,
                },
                "draw:layer" => match key.as_str() {
                    "draw:name" => {
                        self.define(&ctx.owner, "layer", v, verify, budget)?;
                        true
                    }
                    "draw:display" => v == "none",
                    _ => false,
                },
                "style:font-face" => match key.as_str() {
                    "style:name" => {
                        self.define_font(part, v, verify, budget)?;
                        true
                    }
                    "svg:font-family" => family(v),
                    "style:font-family-generic" => matches!(v.as_str(), "roman" | "swiss" | "modern" | "decorative" | "script" | "system"),
                    "style:font-pitch" => matches!(v.as_str(), "fixed" | "variable"),
                    _ => false,
                },
                "meta:document-statistic" => super::stat_attr(key).is_some() && integer(v),
                "style:text-properties"
                | "style:paragraph-properties"
                | "style:graphic-properties"
                | "style:drawing-page-properties"
                | "style:page-layout-properties" => self.property(n, key, v, &ctx, part, verify)?,
                _ => false,
            };
            if !ok {
                return Err(refuse("cache unclassified attribute/value"));
            }
        }
        if ids > 1 || style_refs > 1 {
            return Err(refuse("cache ambiguous shape identity/style"));
        }
        if matches!(n, "office:document-content" | "office:document-styles" | "office:document-meta" | "office:document-settings")
            && el.attr("office:version").is_none()
        {
            return Err(refuse("cache document version"));
        }
        for required in match n {
            "style:page-layout" | "style:master-page" | "style:font-face" => &["style:name"][..],
            "draw:fill-image" => &["draw:name", "xlink:href"][..],
            "draw:image" => &["xlink:href"][..],
            "draw:layer" => &["draw:name"][..],
            _ => &[],
        } {
            if el.attr(required).is_none() {
                return Err(refuse("cache required attribute"));
            }
        }
        if matches!(n, "style:graphic-properties" | "style:drawing-page-properties")
            && el.attr("draw:fill") == Some("bitmap")
            && el.attr("draw:fill-image-name").is_none()
        {
            return Err(refuse("cache bitmap resource"));
        }
        let literal = matches!(
            n,
            "text:p"
                | "text:span"
                | "dc:title"
                | "dc:subject"
                | "dc:description"
                | "dc:creator"
                | "meta:initial-creator"
                | "meta:keyword"
                | "meta:generator"
                | "meta:editing-cycles"
                | "meta:creation-date"
        );
        let mut singleton = BTreeSet::new();
        let mut count = 0usize;
        let mut number = String::new();
        for node in &el.children {
            match node {
                Node::Text(t) => {
                    if !literal && !ws(t) {
                        return Err(refuse("cache nonliteral text"));
                    }
                    if n == "meta:editing-cycles" {
                        charge(budget, t.len())?;
                        number.push_str(t);
                    }
                }
                Node::El(e) => {
                    count += 1;
                    if n == "office:document-content" && e.name == "office:scripts" && count != 1 {
                        return Err(refuse("cache scripts root order"));
                    }
                    if matches!(
                        e.name.as_str(),
                        "office:font-face-decls"
                            | "office:scripts"
                            | "office:styles"
                            | "office:automatic-styles"
                            | "office:master-styles"
                            | "office:body"
                            | "office:presentation"
                            | "office:meta"
                            | "office:settings"
                            | "draw:layer-set"
                            | "style:text-properties"
                            | "style:paragraph-properties"
                            | "style:graphic-properties"
                            | "style:drawing-page-properties"
                            | "style:page-layout-properties"
                            | "meta:creation-date"
                    ) && !singleton.insert(&e.name)
                    {
                        return Err(refuse("cache duplicate singleton"));
                    }
                    self.walk(e, n, part, ctx.clone(), verify, budget, depth + 1)?;
                }
            }
        }
        if n == "draw:frame" && count != 1 || n == "meta:editing-cycles" && !integer(&number) {
            return Err(refuse("cache leaf/frame cardinality"));
        }
        if n == "office:document-content" && !singleton.contains(&"office:body".to_string())
            || n == "office:body" && count != 1
            || n == "office:document-meta" && count != 1
            || n == "office:document-settings" && count > 1
        {
            return Err(refuse("cache root cardinality"));
        }
        Ok(())
    }
    fn property(&self, bag: &str, k: &str, v: &str, c: &Context, part: &str, verify: bool) -> Result<bool> {
        Ok(match bag {
            "style:paragraph-properties" => match k {
                "fo:text-align" => matches!(v, "left" | "right" | "center" | "justify" | "start" | "end"),
                "fo:text-align-last" => v == "center",
                "fo:keep-with-next" | "fo:keep-together" => matches!(v, "auto" | "always"),
                "fo:margin-left" | "fo:margin-right" | "fo:text-indent" => length(v, true, false),
                "fo:margin-top" | "fo:margin-bottom" | "style:line-spacing" => length(v, false, false),
                "fo:line-height" => length(v, false, true) || percent(v, false, 10000.),
                "fo:background-color" => color(v) || v == "transparent",
                "style:writing-mode" => matches!(v, "lr-tb" | "rl-tb"),
                _ => false,
            },
            "style:text-properties" => match k {
                "fo:font-size" | "style:font-size-asian" | "style:font-size-complex" => {
                    length(v, false, true) || (c.common_named && percent(v, false, 10000.))
                }
                "fo:font-weight" | "style:font-weight-asian" | "style:font-weight-complex" => matches!(v, "normal" | "bold" | "400" | "700"),
                "fo:font-style" | "style:font-style-asian" | "style:font-style-complex" => matches!(v, "normal" | "italic" | "oblique"),
                "fo:text-transform" => matches!(v, "none" | "uppercase" | "lowercase" | "capitalize"),
                "fo:color" => color(v),
                "fo:background-color" => color(v) || v == "transparent",
                "style:font-name" | "style:font-name-asian" | "style:font-name-complex" => {
                    self.reference_font(part, v, verify)?;
                    true
                }
                "fo:font-family" | "style:font-family-asian" | "style:font-family-complex" => family(v),
                "fo:language" | "style:language-asian" | "style:language-complex" => language(v),
                "fo:country" | "style:country-asian" | "style:country-complex" => country(v),
                _ => false,
            },
            "style:graphic-properties" | "style:drawing-page-properties" => match k {
                // Unsupported channel offsets and mirroring are admitted only as identities.
                "draw:blue" | "draw:green" | "draw:red" if bag == "style:graphic-properties" => v == "0%",
                "style:mirror" if bag == "style:graphic-properties" => v == "none",
                "fo:clip" if bag == "style:graphic-properties" => identity_clip(v),
                "draw:color-mode" if bag == "style:graphic-properties" => {
                    matches!(v, "standard" | "greyscale" | "mono")
                }
                // Luminance is unsigned in ODF 1.2; use the 1.2/1.3 intersection here.
                "draw:image-opacity" | "draw:luminance" if bag == "style:graphic-properties" => picture_percent(v, false),
                "draw:contrast" if bag == "style:graphic-properties" => picture_percent(v, true),
                "draw:gamma" if bag == "style:graphic-properties" => crate::source_effects::gamma_percent(v),
                "draw:auto-grow-height" | "style:shrink-to-fit" if bag == "style:graphic-properties" => matches!(v, "true" | "false"),
                // The standard's true value stretches text; it is not the native shrink mode.
                "draw:fit-to-size" if bag == "style:graphic-properties" => v == "false",
                "fo:wrap-option" if bag == "style:graphic-properties" => matches!(v, "wrap" | "no-wrap"),
                "fo:min-height" | "fo:min-width" if bag == "style:graphic-properties" => length(v, false, false),
                "presentation:background-objects-visible"
                | "presentation:background-visible"
                | "presentation:display-date-time"
                | "presentation:display-footer"
                | "presentation:display-header"
                | "presentation:display-page-number"
                    if bag == "style:drawing-page-properties" =>
                {
                    matches!(v, "true" | "false")
                }
                "draw:fill" => matches!(v, "none" | "solid" | "bitmap"),
                "draw:fill-color" => color(v),
                "draw:opacity" => percent(v, true, 100.),
                "draw:fill-image-name" if bag == "style:graphic-properties" => {
                    self.reference(&["common"], "fill", v, verify)?;
                    true
                }
                "draw:stroke" if bag == "style:graphic-properties" => matches!(v, "none" | "solid"),
                "svg:stroke-color" if bag == "style:graphic-properties" => color(v),
                "svg:stroke-width" if bag == "style:graphic-properties" => length(v, false, false),
                "fo:padding" | "fo:padding-left" | "fo:padding-right" | "fo:padding-top" | "fo:padding-bottom"
                    if bag == "style:graphic-properties" =>
                {
                    length(v, false, false)
                }
                "draw:textarea-vertical-align" if bag == "style:graphic-properties" => matches!(v, "top" | "middle" | "bottom" | "justify"),
                _ => false,
            },
            "style:page-layout-properties" => match k {
                "fo:page-width" | "fo:page-height" => length(v, false, true),
                "fo:margin" | "fo:margin-left" | "fo:margin-right" | "fo:margin-top" | "fo:margin-bottom" => length(v, false, false),
                "style:print-orientation" => matches!(v, "portrait" | "landscape"),
                _ => false,
            },
            _ => false,
        })
    }
}
fn child(p: &str, n: &str, part: &str, family: &str) -> bool {
    let artwork = matches!(n, "draw:frame" | "draw:rect" | "draw:ellipse" | "draw:circle" | "draw:g");
    match p {
        "" => matches!(n, "office:document-content" | "office:document-styles" | "office:document-meta" | "office:document-settings"),
        "office:document-content" => {
            matches!(n, "office:font-face-decls" | "office:automatic-styles" | "office:body") || (part == "content.xml" && n == "office:scripts")
        }
        "office:document-styles" => matches!(n, "office:font-face-decls" | "office:styles" | "office:automatic-styles" | "office:master-styles"),
        "office:document-meta" => n == "office:meta",
        "office:document-settings" => n == "office:settings",
        "office:body" => n == "office:presentation",
        "office:presentation" => matches!(n, "draw:page" | "draw:layer-set"),
        "office:master-styles" => matches!(n, "style:master-page" | "draw:layer-set"),
        "office:font-face-decls" => n == "style:font-face",
        "office:styles" => matches!(n, "style:style" | "style:default-style" | "draw:fill-image"),
        "office:automatic-styles" => n == "style:style" || (part == "styles.xml" && n == "style:page-layout"),
        "draw:page" | "style:master-page" => artwork || n == "draw:layer-set",
        "draw:g" => artwork,
        "draw:frame" => matches!(n, "draw:text-box" | "draw:image"),
        "draw:rect" | "draw:ellipse" | "draw:circle" | "draw:text-box" => n == "text:p",
        "text:p" | "text:span" => matches!(n, "text:span" | "text:s" | "text:tab" | "text:line-break"),
        "draw:layer-set" => n == "draw:layer",
        "office:meta" => {
            matches!(
                n,
                "dc:title"
                    | "dc:subject"
                    | "dc:description"
                    | "dc:creator"
                    | "meta:initial-creator"
                    | "meta:keyword"
                    | "meta:generator"
                    | "meta:editing-cycles"
                    | "meta:document-statistic"
            ) || (part == "meta.xml" && n == "meta:creation-date")
        }
        "style:page-layout" => n == "style:page-layout-properties",
        "style:style" | "style:default-style" => match family {
            "text" => n == "style:text-properties",
            "paragraph" => matches!(n, "style:paragraph-properties" | "style:text-properties"),
            "graphic" | "presentation" => matches!(n, "style:graphic-properties" | "style:paragraph-properties" | "style:text-properties"),
            "drawing-page" => n == "style:drawing-page-properties",
            _ => false,
        },
        _ => false,
    }
}
