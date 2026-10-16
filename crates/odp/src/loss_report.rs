//! Bounded first-slice semantic audit, independent of the XML namespace registry.
//!
//! Coverage v1: all paragraph/text property bags (including unused definitions), their
//! children, ruby, and the exact paragraph fill extension. Other ODF structures are
//! outside this audit; a clean report is not proof of whole-document losslessness.
use crate::xml::{El, ExpandedAttribute, ExpandedName, NS, Node};
use deckcraft_model::text::{LineHeight, Spacing};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const COVERAGE_VERSION: u16 = 1;
const MAX_ISSUES: usize = 128;
const MAX_REPORT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Content,
    Styles,
    Embedded,
    Metadata,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeatureCode {
    UnrepresentedParagraphProperty,
    UnrepresentedTextProperty,
    UnsupportedValue,
    InvalidValue,
    CompoundProperty,
    PropertyChild,
    PropertyContext,
    LineHeightContext,
    Ruby,
    Extension,
}

/// Locations contain no document strings or values. Embedded offsets are part-local;
/// this first report does not yet expose a persistent package provenance identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Issue {
    pub part: Part,
    pub node_start: usize,
    pub attribute: Option<usize>,
    pub feature_code: FeatureCode,
}

#[derive(Default, Debug)]
pub(crate) struct ImportReport {
    issues: Vec<Issue>,
    omitted: usize,
    blocking: bool,
    non_preservable: bool,
    line_height_context_required: bool,
}
impl ImportReport {
    pub fn audit(&mut self, part: Part, root: &El) {
        let mut chart_uses = ChartUses::default();
        chart_uses.collect(root);
        chart_uses.inherit();
        if chart_uses.limited {
            self.record(part, root, None, FeatureCode::PropertyContext);
        }
        self.walk(part, root, None, false, &chart_uses);
    }
    /// Complete the local datatype audit with exact paragraph-style inheritance.
    /// The caller supplies the same part-scoped registry used by its text reader.
    pub(crate) fn audit_line_heights(&mut self, part: Part, root: &El, styles: &crate::read::Styles<'_>, total: &mut u64) -> crate::Result<()> {
        let mut charge = |bytes: usize| {
            *total = total.saturating_add(bytes as u64);
            if *total > crate::MAX_TOTAL {
                return Err(crate::OdpError("paragraph line-height audit exceeds import budget".into()));
            }
            Ok(())
        };
        let mut required = line_height_context_in_tree(root, &mut charge)?;
        if !required {
            // A content style may inherit a new field from styles.xml without
            // spelling that field anywhere in the content part itself.
            for owner in styles.values() {
                required = line_height_context_in_tree(owner, &mut charge)?;
                if required {
                    break;
                }
            }
        }
        if !required {
            return Ok(());
        }
        self.line_height_context_required = true;
        self.line_height_owners(part, root, styles, &mut charge)
    }
    fn line_height_owners(
        &mut self,
        part: Part,
        el: &El,
        styles: &crate::read::Styles<'_>,
        charge: &mut impl FnMut(usize) -> crate::Result<()>,
    ) -> crate::Result<()> {
        charge(64)?;
        if (named(&el.expanded_name, "style", "style") || named(&el.expanded_name, "style", "default-style"))
            && el.attr("style:family") == Some("paragraph")
        {
            let name = el.attr("style:name");
            if named(&el.expanded_name, "style", "style") && name.is_none() {
                self.record(part, el, None, FeatureCode::LineHeightContext);
                return Ok(());
            }
            match paragraph_line_height(styles, name, &mut *charge) {
                Ok(Err(code)) => {
                    self.record(part, el, None, code);
                    self.record(part, el, None, FeatureCode::LineHeightContext);
                }
                Err(error) if error.0 == "paragraph line-height audit exceeds import budget" => return Err(error),
                Err(_) => self.record(part, el, None, FeatureCode::LineHeightContext),
                Ok(Ok(_)) => {}
            }
        }
        for child in el.elements() {
            self.line_height_owners(part, child, styles, charge)?;
        }
        Ok(())
    }
    pub fn has_blocking_loss(&self) -> bool {
        self.blocking
    }
    /// This aggregate is updated before report truncation. Package preservation
    /// still independently verifies passive XML, package ownership and every edit.
    pub fn preservation_eligible(&self) -> bool {
        self.blocking && !self.non_preservable && !self.line_height_context_required
    }
    pub fn summary(&self) -> &'static str {
        // Deliberately static: never copy private text, names, values or URI strings to tags.
        match COVERAGE_VERSION {
            1 => {
                "ODF semantic audit v1 found unrepresented paragraph/text properties or values; original source retained for recovery; edited export is blocked"
            }
            _ => "ODF semantic audit found unrepresented semantics; original source retained for recovery; edited export is blocked",
        }
    }
    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }
    pub fn omitted_issue_count(&self) -> usize {
        self.omitted
    }
    fn record(&mut self, part: Part, el: &El, attribute: Option<usize>, feature_code: FeatureCode) {
        self.blocking = true;
        self.non_preservable |= matches!(feature_code, FeatureCode::Ruby | FeatureCode::Extension | FeatureCode::LineHeightContext);
        let next_bytes = self.issues.len().saturating_add(1).saturating_mul(std::mem::size_of::<Issue>());
        if self.issues.len() >= MAX_ISSUES || next_bytes > MAX_REPORT_BYTES {
            self.omitted = self.omitted.saturating_add(1);
            return;
        }
        self.issues.push(Issue { part, node_start: el.source_start(), attribute, feature_code });
    }
    fn walk(&mut self, part: Part, el: &El, parent: Option<&El>, common_styles: bool, chart_uses: &ChartUses<'_>) {
        let common_styles = if named(&el.expanded_name, "office", "styles") {
            true
        } else if named(&el.expanded_name, "office", "automatic-styles") {
            false
        } else {
            common_styles
        };
        // Preserve the generic foreign-content safety net for every audited part,
        // including metadata. This is additive to explicit property semantics below;
        // registering a namespace cannot acknowledge a property bag field.
        if el.name.starts_with("unknown:")
            || (el.attrs.keys().any(|key| key.starts_with("unknown:") || key.starts_with("loext:"))
                && !crate::table_number_styles::supported_extension(el))
        {
            self.record(part, el, None, FeatureCode::Extension);
        }
        let new_line_height = has_line_height_context(el);
        self.line_height_context_required |= new_line_height;
        if new_line_height
            && !(named(&el.expanded_name, "style", "paragraph-properties")
                && parent.is_some_and(|owner| {
                    (named(&owner.expanded_name, "style", "style") || named(&owner.expanded_name, "style", "default-style"))
                        && owner.attr("style:family") == Some("paragraph")
                }))
        {
            self.record(part, el, None, FeatureCode::LineHeightContext);
        }
        if named(&el.expanded_name, "style", "paragraph-properties") {
            self.properties(part, el, parent, false, common_styles, chart_uses);
        } else if named(&el.expanded_name, "style", "text-properties") {
            self.properties(part, el, parent, true, common_styles, chart_uses);
        } else if named(&el.expanded_name, "text", "ruby") || named(&el.expanded_name, "style", "ruby-properties") {
            self.record(part, el, None, FeatureCode::Ruby);
        } else if el.expanded_name.namespace.as_deref() == uri("loext") && !parent.is_some_and(|p| el.supported_paragraph_fill(p)) {
            self.record(part, el, None, FeatureCode::Extension);
        }
        for child in el.elements() {
            self.walk(part, child, Some(el), common_styles, chart_uses);
        }
    }
    fn properties(&mut self, part: Part, el: &El, parent: Option<&El>, text: bool, common_styles: bool, chart_uses: &ChartUses<'_>) {
        let family = parent.and_then(|p| p.attr("style:family"));
        let style = parent.is_some_and(|p| named(&p.expanded_name, "style", "style") || named(&p.expanded_name, "style", "default-style"));
        let ordinary = style && family.is_some_and(|f| matches!(f, "paragraph" | "text" | "graphic" | "presentation" | "table-cell"));
        let bullet = text
            && parent.is_some_and(|p| {
                named(&p.expanded_name, "text", "list-level-style-bullet") || named(&p.expanded_name, "text", "list-level-style-number")
            });
        let chart = text && style && family == Some("chart");
        // Data-style character color is implemented by the number formatter;
        // other property fields remain independently audited as unknown context.
        let data_color =
            text && parent.is_some_and(|p| {
                matches!(
                    p.name.as_str(),
                    "number:number-style"
                        | "number:percentage-style"
                        | "number:currency-style"
                        | "number:date-style"
                        | "number:time-style"
                        | "number:boolean-style"
                        | "number:text-style"
                )
            }) && el.attrs.keys().all(|key| key == "fo:color")
                && el.elements().next().is_none();
        let chart_context = parent.and_then(|p| p.attr("style:name")).and_then(|name| chart_uses.uses.get(name)).copied().unwrap_or(0);
        if !(ordinary || bullet || chart || data_color) && (!el.attrs.is_empty() || el.elements().next().is_some()) {
            self.record(part, el, None, FeatureCode::PropertyContext);
        }
        for attr in &el.expanded_attrs {
            let Some(value) = el.attr(&attr.projected_key) else { continue };
            // ODF 1.3 §§20.284–285 permit script-size percentages only in
            // common styles, not automatic styles or default-style definitions.
            if text
                && (attr_is(attr, "style", "font-size-asian") || attr_is(attr, "style", "font-size-complex"))
                && value.trim().ends_with('%')
                && !(common_styles && parent.is_some_and(|p| named(&p.expanded_name, "style", "style")))
            {
                self.record(part, el, Some(attr.index), FeatureCode::PropertyContext);
                continue;
            }
            let disposition = if text {
                let unmapped_bullet =
                    bullet && !attr_is(attr, "fo", "font-family") && !attr_is(attr, "style", "font-name") && !attr_is(attr, "fo", "color");
                let chart_size = attr_is(attr, "fo", "font-size") && chart_context != 0 && chart_context & CHART_OTHER == 0;
                let chart_color = attr_is(attr, "fo", "color") && chart_context == CHART_ROOT;
                let unmapped_chart = chart && !(chart_size || chart_color);
                if unmapped_bullet || unmapped_chart {
                    Err(FeatureCode::UnrepresentedTextProperty)
                } else if chart && chart_size {
                    // Chart readers map absolute lengths, unlike ordinary text's relative
                    // size inheritance. A percentage must not inherit that acknowledgement.
                    positive_length(value)
                } else {
                    text_property(el, attr, value)
                }
            } else if family == Some("text") {
                Err(FeatureCode::PropertyContext)
            } else if family != Some("paragraph") && !attr_is(attr, "fo", "background-color") {
                // Shape/table inheritance maps paragraph backgrounds separately. Generic
                // property-map insertion does not represent all paragraph formatting.
                Err(FeatureCode::PropertyContext)
            } else if !matches!(part, Part::Content | Part::Styles)
                && (attr_is(attr, "style", "line-height-at-least")
                    || attr_is(attr, "style", "line-spacing")
                    || attr_is(attr, "fo", "line-height") && value == "normal")
            {
                // Other audit entry points have no paragraph-style registry supplied.
                Err(FeatureCode::PropertyContext)
            } else {
                paragraph_property(attr, value)
            };
            if let Err(code) = disposition {
                self.record(part, el, Some(attr.index), code);
                if attr_is(attr, "style", "line-height-at-least")
                    || attr_is(attr, "style", "line-spacing")
                    || attr_is(attr, "fo", "line-height") && value == "normal"
                {
                    self.record(part, el, Some(attr.index), FeatureCode::LineHeightContext);
                }
            }
        }
        for node in &el.children {
            match node {
                Node::El(child) if !text && family == Some("paragraph") && named(&child.expanded_name, "style", "tab-stops") => {
                    // Tabs currently resolve through named paragraph styles, not defaults.
                    if parent.is_some_and(|p| named(&p.expanded_name, "style", "default-style")) {
                        self.record(part, child, None, FeatureCode::PropertyContext);
                    }
                    self.tabs(part, child);
                }
                Node::El(child) => self.record(part, child, None, FeatureCode::PropertyChild),
                Node::Text(value) if !xml_space(value) => self.record(part, el, None, FeatureCode::PropertyChild),
                _ => {}
            }
        }
    }
    fn tabs(&mut self, part: Part, el: &El) {
        for attr in &el.expanded_attrs {
            self.record(part, el, Some(attr.index), FeatureCode::PropertyChild);
        }
        for node in &el.children {
            let Node::El(stop) = node else {
                if let Node::Text(value) = node
                    && !xml_space(value)
                {
                    self.record(part, el, None, FeatureCode::PropertyChild);
                }
                continue;
            };
            if !named(&stop.expanded_name, "style", "tab-stop") {
                self.record(part, stop, None, FeatureCode::PropertyChild);
                continue;
            }
            if stop.attr("style:position").and_then(crate::read::length).is_none_or(|n| n < 0.0) {
                self.record(part, stop, None, FeatureCode::InvalidValue);
            }
            for attr in &stop.expanded_attrs {
                let value = stop.attr(&attr.projected_key).unwrap_or("");
                let result = if attr_is(attr, "style", "position") {
                    length(value, false)
                } else if attr_is(attr, "style", "type") {
                    choice(value, &["left", "right", "center", "char"], &[])
                } else if attr_is(attr, "style", "char") {
                    if value == "." { Ok(()) } else { Err(FeatureCode::UnsupportedValue) }
                } else if attr_is(attr, "style", "leader-style") && value == "none" {
                    Ok(()) // Explicitly disabled decoration has no painted geometry.
                } else {
                    Err(FeatureCode::PropertyChild)
                };
                if let Err(code) = result {
                    self.record(part, stop, Some(attr.index), code);
                }
            }
            for node in &stop.children {
                if !matches!(node, Node::Text(value) if xml_space(value)) {
                    self.record(part, stop, None, FeatureCode::PropertyChild);
                }
            }
        }
    }
}

// A small transient borrowed-name index proves which chart text fields are used by
// the actual root/title readers. Names and source values never enter retained reports.
// Two256-entry maps, a256-entry set and a256-entry propagation list stay
// below the64KiB audit budget; unrelated graphic-only point styles are not indexed.
const MAX_CHART_STYLE_USES: usize = 256;
const CHART_ROOT: u8 = 1;
const CHART_TITLE: u8 = 2;
const CHART_OTHER: u8 = 4;
#[derive(Default)]
struct ChartUses<'a> {
    uses: BTreeMap<&'a str, u8>,
    parents: BTreeMap<&'a str, &'a str>,
    text_styles: BTreeSet<&'a str>,
    limited: bool,
}
impl<'a> ChartUses<'a> {
    fn add(&mut self, name: &'a str, flags: u8) {
        if let Some(usage) = self.uses.get_mut(name) {
            *usage |= flags;
        } else if self.uses.len() < MAX_CHART_STYLE_USES {
            self.uses.insert(name, flags);
        } else {
            self.limited = true;
        }
    }
    fn collect(&mut self, root: &'a El) {
        self.collect_definitions(root);
        self.collect_uses(root);
    }
    fn collect_definitions(&mut self, el: &'a El) {
        if named(&el.expanded_name, "style", "style")
            && el.attr("style:family") == Some("chart")
            && let Some(name) = el.attr("style:name")
        {
            if let Some(text) = el.child("style:text-properties")
                && (!text.attrs.is_empty() || text.elements().next().is_some())
            {
                if self.text_styles.len() < MAX_CHART_STYLE_USES {
                    self.text_styles.insert(name);
                } else {
                    self.limited = true;
                }
            }
            if let Some(parent) = el.attr("style:parent-style-name") {
                if self.parents.len() < MAX_CHART_STYLE_USES {
                    self.parents.insert(name, parent);
                } else {
                    self.limited = true;
                }
            }
        }
        for child in el.elements() {
            self.collect_definitions(child);
        }
    }
    fn collect_uses(&mut self, el: &'a El) {
        if let Some(name) = el.attr("chart:style-name")
            && (self.text_styles.contains(name) || self.parents.contains_key(name))
        {
            let flags = if named(&el.expanded_name, "chart", "chart") {
                CHART_ROOT
            } else if named(&el.expanded_name, "chart", "title") {
                CHART_TITLE
            } else {
                CHART_OTHER
            };
            self.add(name, flags);
        }
        for child in el.elements() {
            self.collect_uses(child);
        }
    }
    fn inherit(&mut self) {
        let seeds: Vec<_> = self.uses.iter().map(|(name, flags)| (*name, *flags)).collect();
        for (name, flags) in seeds {
            let mut name = name;
            for _ in 0..32 {
                let Some(parent) = self.parents.get(name).copied() else { break };
                self.add(parent, flags);
                name = parent;
            }
        }
    }
}

fn xml_space(value: &str) -> bool {
    value.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
}
fn uri(prefix: &str) -> Option<&'static str> {
    NS.iter().find(|(p, _)| *p == prefix).map(|(_, uri)| *uri)
}
fn named(name: &ExpandedName, prefix: &str, local: &str) -> bool {
    name.local.as_ref() == local && name.namespace.as_deref() == uri(prefix)
}
fn attr_is(attr: &ExpandedAttribute, prefix: &str, local: &str) -> bool {
    named(&attr.name, prefix, local)
}
fn has_line_height_context(el: &El) -> bool {
    el.expanded_attrs.iter().any(|attr| {
        attr_is(attr, "style", "line-height-at-least")
            || attr_is(attr, "style", "line-spacing")
            || attr_is(attr, "fo", "line-height") && el.attr(&attr.projected_key) == Some("normal")
    })
}
fn line_height_context_in_tree(el: &El, charge: &mut impl FnMut(usize) -> crate::Result<()>) -> crate::Result<bool> {
    charge(64usize.saturating_add(el.expanded_attrs.len().saturating_mul(32)))?;
    let mut required = has_line_height_context(el);
    for child in el.elements() {
        required |= line_height_context_in_tree(child, charge)?;
    }
    Ok(required)
}
fn choice(value: &str, supported: &[&str], unsupported: &[&str]) -> std::result::Result<(), FeatureCode> {
    if supported.contains(&value) {
        Ok(())
    } else if unsupported.contains(&value) {
        Err(FeatureCode::UnsupportedValue)
    } else {
        Err(FeatureCode::InvalidValue)
    }
}
fn length(value: &str, signed: bool) -> std::result::Result<(), FeatureCode> {
    if crate::read::length(value).is_some_and(|n| n.is_finite() && (signed || n >= 0.0)) { Ok(()) } else { Err(FeatureCode::InvalidValue) }
}
fn positive_length(value: &str) -> std::result::Result<(), FeatureCode> {
    if crate::read::length(value).is_some_and(|n| n.is_finite() && n > 0.0) { Ok(()) } else { Err(FeatureCode::InvalidValue) }
}
fn color(value: &str) -> std::result::Result<(), FeatureCode> {
    if crate::read::color(value).is_some() { Ok(()) } else { Err(FeatureCode::InvalidValue) }
}
fn paragraph_property(attr: &ExpandedAttribute, value: &str) -> std::result::Result<(), FeatureCode> {
    let key = attr.projected_key.as_str();
    match key {
        "fo:text-align" => choice(value, &["left", "right", "center", "justify", "start", "end"], &[]),
        "style:writing-mode" => choice(value, &["lr-tb", "rl-tb"], &["tb-rl", "tb-lr", "lr", "rl", "tb", "page"]),
        "fo:margin-left" | "fo:margin-right" | "fo:text-indent" => length(value, true),
        "fo:margin-top" | "fo:margin-bottom" => length(value, false),
        "style:tab-stop-distance" => positive_length(value),
        "fo:line-height" => {
            if value == "normal" {
                Ok(())
            } else if let Some(percent) = value.strip_suffix('%') {
                if crate::geometry::number(percent).is_some_and(|n| n > 0.0) { Ok(()) } else { Err(FeatureCode::InvalidValue) }
            } else {
                positive_length(value)
            }
        }
        "style:line-height-at-least" => minimum_points(value).map(|_| ()),
        // Datatype acknowledgment only: the separate complete-owner audit proves
        // that this unsupported modifier is disabled by effective fo:line-height=normal.
        "style:line-spacing" => additional_points(value).map(|_| ()),
        "style:line-break" => choice(value, &["normal", "strict"], &[]),
        "style:punctuation-wrap" => choice(value, &["simple", "hanging"], &[]),
        "fo:background-color" => {
            if value == "transparent" {
                Ok(())
            } else {
                color(value)
            }
        }
        // These exact neutral values disable currently unrepresented painted area effects.
        "fo:padding" | "fo:padding-left" | "fo:padding-right" | "fo:padding-top" | "fo:padding-bottom" => {
            if crate::read::length(value) == Some(0.0) {
                Ok(())
            } else {
                Err(FeatureCode::UnrepresentedParagraphProperty)
            }
        }
        "fo:border" | "fo:border-left" | "fo:border-right" | "fo:border-top" | "fo:border-bottom" | "style:shadow" => {
            if value == "none" {
                Ok(())
            } else {
                Err(FeatureCode::UnrepresentedParagraphProperty)
            }
        }
        "style:join-border" if value == "false" => Ok(()),
        "style:background-transparency" if value == "0%" => Ok(()),
        _ => Err(FeatureCode::UnrepresentedParagraphProperty),
    }
}

type LineSetting = (Option<Spacing>, Option<LineHeight>);

fn minimum_points(value: &str) -> std::result::Result<f64, FeatureCode> {
    crate::read::length(value).filter(|points| LineHeight::Minimum { points: *points }.valid()).ok_or(FeatureCode::InvalidValue)
}
fn additional_points(value: &str) -> std::result::Result<f64, FeatureCode> {
    crate::read::length(value).filter(|points| points.is_finite() && points.abs() <= 1_000_000.0).ok_or(FeatureCode::InvalidValue)
}

/// ODF 1.3 §§20.204,20.317–318. Only normal disables both modifiers.
/// The cited definitions do not establish evaluation order for other combinations.
pub(crate) fn line_height(base: Option<&str>, minimum: Option<&str>, additional: Option<&str>) -> std::result::Result<LineSetting, FeatureCode> {
    let minimum = minimum.map(minimum_points).transpose()?;
    additional.map(additional_points).transpose()?;
    if base == Some("normal") {
        return Ok((None, Some(LineHeight::Normal)));
    }
    if minimum.is_none() && additional.is_none() {
        // Preserve the pre-policy reader's recoverable numeric-only behavior.
        // Local semantic audit still rejects malformed/non-positive base values;
        // this fallback never acknowledges or suppresses that source loss.
        let spacing = base.and_then(|value| {
            value
                .strip_suffix('%')
                .and_then(crate::geometry::number)
                .map(|n| Spacing::Pct(n / 100.0))
                .or_else(|| crate::read::length(value).map(Spacing::Pts))
        });
        return Ok((spacing, None));
    }
    let spacing = base
        .map(|value| {
            if let Some(percent) = value.strip_suffix('%') {
                crate::geometry::number(percent).filter(|n| *n > 0.0).map(|n| Spacing::Pct(n / 100.0)).ok_or(FeatureCode::InvalidValue)
            } else {
                crate::read::length(value).filter(|n| *n > 0.0).map(Spacing::Pts).ok_or(FeatureCode::InvalidValue)
            }
        })
        .transpose()?;
    if spacing.is_some() && (minimum.is_some() || additional.is_some()) || minimum.is_some() && additional.is_some() {
        return Err(FeatureCode::CompoundProperty);
    }
    if additional.is_some() {
        return Err(FeatureCode::UnsupportedValue);
    }
    Ok((spacing, minimum.map(|points| LineHeight::Minimum { points })))
}

#[derive(Default)]
struct LineValues<'a> {
    base: Option<&'a str>,
    minimum: Option<&'a str>,
    additional: Option<&'a str>,
}
impl<'a> LineValues<'a> {
    fn inherit(&mut self, style: &'a El, charge: &mut impl FnMut(usize) -> crate::Result<()>) -> crate::Result<()> {
        let mut found = false;
        for bag in style.elements() {
            charge(64)?;
            if !named(&bag.expanded_name, "style", "paragraph-properties") {
                continue;
            }
            if found {
                return Err(crate::OdpError("duplicate ODF paragraph line-height property elements".into()));
            }
            found = true;
            for (target, key) in
                [(&mut self.base, "fo:line-height"), (&mut self.minimum, "style:line-height-at-least"), (&mut self.additional, "style:line-spacing")]
            {
                if target.is_none() {
                    let value = bag.attr(key);
                    charge(value.map_or(32, |value| value.len().saturating_add(32)))?;
                    *target = value;
                }
            }
        }
        Ok(())
    }
}

/// Resolve only actual paragraph property bags; generic properties() also flattens
/// other bags and must not mint permission for misplaced same-spelling attributes.
pub(crate) fn paragraph_line_height(
    styles: &crate::read::Styles<'_>,
    name: Option<&str>,
    charge: impl FnMut(usize) -> crate::Result<()>,
) -> crate::Result<std::result::Result<LineSetting, FeatureCode>> {
    resolve_line_height(styles, name, charge, true)
}

/// Notes ownership reconstruction supplies a deliberately truncated local registry.
/// Missing ancestry can stop there; the separate source audit uses strict full registries.
pub(crate) fn paragraph_line_height_for_read(
    styles: &crate::read::Styles<'_>,
    name: Option<&str>,
    charge: impl FnMut(usize) -> crate::Result<()>,
) -> crate::Result<std::result::Result<LineSetting, FeatureCode>> {
    resolve_line_height(styles, name, charge, false)
}

/// The native atomic policy cannot retain a dormant local modifier independently
/// of shared Normal. Keep current inheritance accurate and report that source-only
/// ownership boundary instead of activating or pinning a reconstructed local policy.
pub(crate) fn notes_line_height_depends_on_shared_normal(
    styles: &crate::read::Styles<'_>,
    name: Option<&str>,
    common: &str,
    mut charge: impl FnMut(usize) -> crate::Result<()>,
) -> crate::Result<bool> {
    let full = paragraph_line_height(styles, name, &mut charge)?;
    if full == Err(FeatureCode::InvalidValue) {
        return Err(crate::OdpError("invalid ODF notes line-height dependency".into()));
    }
    if full != Ok((None, Some(LineHeight::Normal))) {
        return Ok(false);
    }
    let mut values = LineValues::default();
    let mut seen = Vec::new();
    let mut current = name;
    while let Some(name) = current {
        charge(256usize.saturating_add(name.len().saturating_mul(seen.len().saturating_add(24))).saturating_add(common.len()))?;
        if name == common {
            break;
        }
        if seen.len() >= 32 || seen.contains(&name) {
            return Err(crate::OdpError("cyclic or excessive ODF notes line-height ownership".into()));
        }
        seen.push(name);
        let owner = styles
            .get(&(String::from("paragraph"), String::from(name)))
            .ok_or_else(|| crate::OdpError("missing ODF notes line-height owner".into()))?;
        values.inherit(owner, &mut charge)?;
        current = owner.attr("style:parent-style-name").filter(|name| !name.is_empty());
    }
    if current != Some(common) {
        return Err(crate::OdpError("unproved ODF notes line-height ownership boundary".into()));
    }
    if values.base.is_some() || values.minimum.is_none() && values.additional.is_none() {
        return Ok(false);
    }
    if paragraph_line_height(styles, Some(common), &mut charge)? != Ok((None, Some(LineHeight::Normal))) {
        return Err(crate::OdpError("inconsistent ODF shared notes line-height policy".into()));
    }
    Ok(true)
}
fn resolve_line_height(
    styles: &crate::read::Styles<'_>,
    name: Option<&str>,
    mut charge: impl FnMut(usize) -> crate::Result<()>,
    strict: bool,
) -> crate::Result<std::result::Result<LineSetting, FeatureCode>> {
    let mut values = LineValues::default();
    let mut current = name;
    let mut seen = Vec::new();
    while let Some(name) = current {
        // Charge bounded borrowed comparisons and the temporary typed lookup key
        // before either the key clone or the next chain entry is allocated.
        charge(256usize.saturating_add(name.len().saturating_mul(seen.len().saturating_add(24))))?;
        if seen.len() >= 32 || seen.contains(&name) {
            return Err(crate::OdpError("cyclic or excessive ODF paragraph line-height inheritance".into()));
        }
        seen.push(name);
        let Some(style) = styles.get(&(String::from("paragraph"), String::from(name))) else {
            if !strict {
                break;
            }
            return Err(crate::OdpError("missing or wrong-family ODF paragraph line-height style".into()));
        };
        if !named(&style.expanded_name, "style", "style") || style.attr("style:family") != Some("paragraph") {
            return Err(crate::OdpError("missing or wrong-family ODF paragraph line-height style".into()));
        }
        values.inherit(style, &mut charge)?;
        current = style.attr("style:parent-style-name").filter(|name| !name.is_empty());
    }
    charge(256)?;
    if let Some(default) = styles.get(&(String::from("paragraph"), String::new())) {
        if !named(&default.expanded_name, "style", "default-style") || default.attr("style:family") != Some("paragraph") {
            return Err(crate::OdpError("invalid ODF paragraph default line-height style".into()));
        }
        values.inherit(default, &mut charge)?;
    }
    Ok(line_height(values.base, values.minimum, values.additional))
}

fn text_property(el: &El, attr: &ExpandedAttribute, value: &str) -> std::result::Result<(), FeatureCode> {
    match attr.projected_key.as_str() {
        "fo:font-family" | "style:font-family-asian" | "style:font-family-complex" => {
            // The reader deliberately selects one CSS family. A fallback list is not retained.
            if !value.trim().is_empty() && !value.contains(',') { Ok(()) } else { Err(FeatureCode::UnsupportedValue) }
        }
        "style:font-name" | "style:font-name-asian" | "style:font-name-complex" => {
            // styled_run resolves declared font faces through the shared style registry;
            // the original name is the fallback family. Cross-part reference validation
            // is outside this local-property audit.
            if !value.trim().is_empty() { Ok(()) } else { Err(FeatureCode::InvalidValue) }
        }
        "fo:font-size" | "style:font-size-asian" | "style:font-size-complex" => {
            if let Some(percent) = value.strip_suffix('%') {
                // apply_run resolves relative sizes against named/default/parent runs.
                // Exact resulting size and unresolved inheritance are reader concerns.
                if crate::geometry::number(percent).is_some_and(|n| n > 0.0) { Ok(()) } else { Err(FeatureCode::InvalidValue) }
            } else if crate::read::length(value).is_some_and(|n| n > 10000.0 || attr.projected_key != "fo:font-size" && !(1.0..=4000.0).contains(&n))
            {
                Err(FeatureCode::UnsupportedValue)
            } else {
                positive_length(value)
            }
        }
        "fo:font-weight" | "style:font-weight-asian" | "style:font-weight-complex" => {
            choice(value, &["normal", "bold", "400", "700"], &["100", "200", "300", "500", "600", "800", "900", "bolder", "lighter"])
        }
        "fo:font-style" | "style:font-style-asian" | "style:font-style-complex" => choice(value, &["normal", "italic"], &["oblique"]),
        "fo:color" | "style:text-underline-color" => {
            if value == "font-color" && attr_is(attr, "style", "text-underline-color") {
                // None inherits a prior custom underline color; it cannot express
                // an explicit switch back to following the run's text color.
                Err(FeatureCode::CompoundProperty)
            } else {
                color(value)
            }
        }
        "fo:background-color" => {
            if value == "transparent" {
                Ok(()) // Stored as an explicit zero-alpha highlight, distinct from inheritance.
            } else {
                color(value)
            }
        }
        "fo:letter-spacing" => {
            if value == "normal" {
                Ok(())
            } else {
                length(value, true)
            }
        }
        "fo:font-variant" if value == "normal" && !matches!(el.attr("fo:text-transform"), Some("none" | "uppercase")) => {
            // run_props returns None here, so inheritance can restore small caps.
            Err(FeatureCode::CompoundProperty)
        }
        "fo:font-variant" => choice(value, &["normal", "small-caps"], &[]),
        "fo:text-transform" => choice(value, &["none", "uppercase"], &["lowercase", "capitalize"]),
        "fo:language" | "fo:country" | "style:language-asian" | "style:country-asian" | "style:language-complex" | "style:country-complex" => {
            if !value.is_empty() && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                Ok(())
            } else {
                Err(FeatureCode::InvalidValue)
            }
        }
        "style:letter-kerning" => choice(value, &["true", "false"], &[]),
        "style:text-position" => text_position(value),
        "style:text-underline-style" | "style:text-underline-type" | "style:text-underline-width" | "style:text-underline-mode" => underline(el),
        "style:text-line-through-style" | "style:text-line-through-type" => {
            let style = el.attr("style:text-line-through-style");
            let kind = el.attr("style:text-line-through-type");
            if !matches!(style, Some("none" | "solid"))
                || !(matches!(kind, None | Some("single" | "double")) || style == Some("none") && kind == Some("none"))
            {
                Err(FeatureCode::CompoundProperty)
            } else {
                Ok(())
            }
        }
        // Explicitly disabled transformations have no effect; enabled forms need new semantics.
        "fo:hyphenate" if value == "false" => Ok(()),
        "style:text-combine" if value == "none" => Ok(()),
        "style:text-scale" if value == "100%" => Ok(()),
        _ => Err(FeatureCode::UnrepresentedTextProperty),
    }
}
/// Shared with the text reader's compatibility warnings so equivalent percent
/// spellings are accepted and no secondary path approves unsupported script sizing.
pub(crate) fn supported_text_position(value: &str) -> bool {
    text_position(value).is_ok()
}
fn text_position(value: &str) -> std::result::Result<(), FeatureCode> {
    let mut fields = value.split_whitespace();
    let offset = fields.next().unwrap_or("");
    let size = fields.next();
    if fields.next().is_some() {
        return Err(FeatureCode::CompoundProperty);
    }
    let baseline = match offset {
        "super" => Some(0.3),
        "sub" => Some(-0.25),
        _ => offset.strip_suffix('%').and_then(crate::geometry::number),
    }
    .ok_or(FeatureCode::InvalidValue)?;
    // ODF 1.3 §20.384 leaves omitted script size to the consumer. For an
    // explicit size, native text layout scales nonzero baselines to66%,
    // and does not scale zero baselines. Do not acknowledge other values.
    if let Some(size) = size {
        let percent = size.strip_suffix('%').and_then(crate::geometry::number).filter(|n| *n > 0.0).ok_or(FeatureCode::InvalidValue)?;
        let represented = if baseline == 0.0 { 100.0 } else { 66.0 };
        if percent != represented {
            return Err(FeatureCode::CompoundProperty);
        }
    }
    Ok(())
}

fn underline(el: &El) -> std::result::Result<(), FeatureCode> {
    let style = el.attr("style:text-underline-style");
    let kind = el.attr("style:text-underline-type").unwrap_or("single");
    let width = el.attr("style:text-underline-width").unwrap_or("auto");
    let mode = el.attr("style:text-underline-mode").unwrap_or("continuous");
    let represented = matches!(style, Some("none" | "solid" | "dotted" | "dash" | "long-dash" | "dot-dash" | "dot-dot-dash" | "wave"))
        && matches!(kind, "single" | "double")
        && matches!(width, "auto" | "bold")
        && matches!(mode, "continuous" | "skip-white-space")
        && (kind != "double" || matches!(style, Some("solid" | "wave")) && width == "auto")
        && (mode != "skip-white-space" || style == Some("solid") && kind == "single" && width == "auto");
    if represented { Ok(()) } else { Err(FeatureCode::CompoundProperty) }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn audit(properties: &str) -> ImportReport {
        let xml = format!(
            "<style:style xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' style:family='paragraph'>{properties}</style:style>"
        );
        let root = crate::xml::parse(xml.as_bytes()).unwrap();
        let mut report = ImportReport::default();
        report.audit(Part::Content, &root);
        report
    }
    #[test]
    fn report_saturation_remains_blocking_without_private_source_values() {
        let properties = "<style:paragraph-properties fo:text-align-last='PRIVATE Ω VALUE'/>".repeat(500);
        let report = audit(&properties);
        assert!(report.has_blocking_loss());
        assert_eq!(report.issues().len(), 128);
        assert_eq!(report.omitted_issue_count(), 372);
        assert!(report.issues().iter().all(|issue| issue.feature_code == FeatureCode::UnrepresentedParagraphProperty));
        assert!(std::mem::size_of_val(report.issues()) < MAX_REPORT_BYTES);
        assert!(!format!("{report:?}").contains("PRIVATE"));
        assert!(!report.summary().contains('Ω'));
    }
    #[test]
    fn preservation_eligibility_survives_report_truncation() {
        let mut report = audit(&"<style:paragraph-properties fo:text-align-last='center'/>".repeat(500));
        assert!(report.preservation_eligible());
        let root = crate::xml::parse(b"<text:ruby xmlns:text='urn:oasis:names:tc:opendocument:xmlns:text:1.0'/>").unwrap();
        report.audit(Part::Styles, &root);
        assert!(!report.preservation_eligible());
        assert_eq!(report.issues().len(), 128);
        assert!(report.issues().iter().all(|issue| issue.feature_code != FeatureCode::Ruby));
        let empty = ImportReport::default();
        assert!(!empty.preservation_eligible());
        let mut foreign = audit("<style:paragraph-properties fo:text-align-last='center'/>");
        foreign.audit(Part::Metadata, &crate::xml::parse(b"<x:unknown xmlns:x='urn:original:unknown'/>").unwrap());
        assert!(!foreign.preservation_eligible());
    }
    #[test]
    fn valid_known_properties_do_not_clear_prior_loss() {
        let mut report = audit("<style:paragraph-properties fo:text-align-last='center'/>");
        let root = crate::xml::parse(b"<r/>").unwrap();
        for part in [Part::Styles, Part::Embedded, Part::Metadata] {
            report.audit(part, &root);
        }
        assert!(report.has_blocking_loss());
        assert_eq!(report.issues()[0].part, Part::Content);
        assert!(report.issues()[0].node_start > 0);
        assert!(report.issues()[0].attribute.is_some());
    }
    #[test]
    fn omissions_values_children_and_compounds_have_distinct_codes() {
        for (properties, code) in [
            ("<style:paragraph-properties fo:text-align-last='center'/>", FeatureCode::UnrepresentedParagraphProperty),
            ("<style:paragraph-properties style:writing-mode='tb-rl'/>", FeatureCode::UnsupportedValue),
            ("<style:paragraph-properties style:writing-mode='banana'/>", FeatureCode::InvalidValue),
            ("<style:paragraph-properties fo:margin-left='NaNpt'/>", FeatureCode::InvalidValue),
            ("<style:paragraph-properties><style:drop-cap/></style:paragraph-properties>", FeatureCode::PropertyChild),
            ("<style:text-properties style:text-underline-style='dotted' style:text-underline-type='double'/>", FeatureCode::CompoundProperty),
        ] {
            let report = audit(properties);
            assert!(report.issues().iter().any(|issue| issue.feature_code == code), "{properties}: {report:?}");
        }
    }
    #[test]
    fn chart_text_fields_require_actual_reader_usage_context() {
        for (element, field, expected_clean) in [
            ("chart", "fo:font-size='18pt' fo:color='#123456'", true),
            ("title", "fo:font-size='18pt'", true),
            ("title", "fo:color='#123456'", false),
            ("axis", "fo:font-size='18pt'", false),
            ("legend", "fo:color='#123456'", false),
        ] {
            let xml = format!(
                "<r xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0' xmlns:chart='urn:oasis:names:tc:opendocument:xmlns:chart:1.0'><style:style style:name='C' style:family='chart'><style:text-properties {field}/></style:style><chart:{element} chart:style-name='C'/></r>"
            );
            let root = crate::xml::parse(xml.as_bytes()).unwrap();
            let mut uses = ChartUses::default();
            uses.collect(&root);
            uses.inherit();
            let mut report = ImportReport::default();
            // The synthetic no-namespace container is deliberately outside this check.
            for el in root.elements() {
                report.walk(Part::Embedded, el, Some(&root), false, &uses);
            }
            assert_eq!(!report.has_blocking_loss(), expected_clean, "{element}/{field}: {report:?}");
        }
    }
    #[test]
    fn chart_context_index_saturation_is_permanently_blocking() {
        let uses = (0..300).map(|i| format!("<style:style style:name='C{i}' style:family='chart'><style:text-properties fo:font-size='18pt'/></style:style><chart:chart chart:style-name='C{i}'/>")).collect::<String>();
        let xml = format!(
            "<office:document-content xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:chart='urn:oasis:names:tc:opendocument:xmlns:chart:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'>{uses}</office:document-content>"
        );
        let root = crate::xml::parse(xml.as_bytes()).unwrap();
        let mut report = ImportReport::default();
        report.audit(Part::Content, &root);
        assert!(report.has_blocking_loss());
        assert!(report.issues().iter().any(|issue| issue.feature_code == FeatureCode::PropertyContext));
    }

    #[test]
    fn ordinary_supported_bags_are_clean() {
        let report = audit(
            "<style:paragraph-properties fo:text-align='center' fo:line-height='120%' fo:background-color='#123456'><style:tab-stops><style:tab-stop style:position='18pt' style:type='char' style:char='.'/></style:tab-stops></style:paragraph-properties><style:text-properties fo:font-family='Liberation Sans' fo:font-size='18pt' fo:color='#123456' style:text-underline-style='solid' style:text-underline-type='single' style:text-underline-width='auto' style:text-underline-mode='continuous'/>",
        );
        assert!(!report.has_blocking_loss(), "{report:?}");
    }

    #[test]
    fn line_height_datatypes_and_effective_combinations_are_distinct() {
        assert_eq!(line_height(None, Some("0pt"), None), Ok((None, Some(LineHeight::Minimum { points: 0.0 }))));
        assert_eq!(line_height(Some("normal"), Some("24pt"), Some("-2pt")), Ok((None, Some(LineHeight::Normal))));
        assert_eq!(line_height(None, None, Some("-2pt")), Err(FeatureCode::UnsupportedValue));
        assert_eq!(line_height(Some("120%"), Some("24pt"), None), Err(FeatureCode::CompoundProperty));
        assert_eq!(line_height(None, Some("24pt"), Some("2pt")), Err(FeatureCode::CompoundProperty));
        for (minimum, additional) in [(Some("14000in"), None), (Some("-1pt"), None), (None, Some("NaNpt"))] {
            assert_eq!(line_height(Some("normal"), minimum, additional), Err(FeatureCode::InvalidValue));
        }
    }

    #[test]
    fn complete_owner_audit_finds_inherited_compounds_and_charges_before_lookup() {
        let root = crate::xml::parse(b"<office:styles xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'><style:style style:name='Base' style:family='paragraph'><style:paragraph-properties style:line-height-at-least='24pt'/></style:style><style:style style:name='Child' style:family='paragraph' style:parent-style-name='Base'><style:paragraph-properties fo:line-height='120%'/></style:style></office:styles>").unwrap();
        let styles = root.elements().map(|style| ((String::from("paragraph"), style.attr("style:name").unwrap().into()), style)).collect();
        let mut report = ImportReport::default();
        report.audit(Part::Styles, &root);
        assert!(!report.has_blocking_loss(), "local datatypes alone do not prove inherited combinations");
        report.audit_line_heights(Part::Styles, &root, &styles, &mut 0).unwrap();
        assert!(report.issues().iter().any(|issue| issue.feature_code == FeatureCode::CompoundProperty));
        let mut total = crate::MAX_TOTAL;
        assert!(ImportReport::default().audit_line_heights(Part::Styles, &root, &styles, &mut total).is_err());
    }

    #[test]
    fn unproved_new_policy_context_never_borrows_another_passive_propertys_preservation_permission() {
        let old = audit("<style:paragraph-properties fo:line-height='120%' fo:keep-with-next='always'/>");
        assert!(old.preservation_eligible());
        let clean = audit("<style:paragraph-properties style:line-height-at-least='24pt'/>");
        assert!(!clean.has_blocking_loss(), "native typed minimum remains locally represented");
        for properties in [
            "<style:paragraph-properties style:line-height-at-least='24pt' fo:keep-with-next='always'/>",
            "<style:paragraph-properties fo:line-height='normal' fo:keep-with-next='always'/>",
            "<style:graphic-properties style:line-height-at-least='24pt'/>",
        ] {
            let report = audit(properties);
            assert!(report.has_blocking_loss());
            assert!(!report.preservation_eligible());
        }
        assert!(
            audit("<style:graphic-properties style:line-height-at-least='24pt'/>")
                .issues()
                .iter()
                .any(|issue| issue.feature_code == FeatureCode::LineHeightContext)
        );
    }

    #[test]
    fn notes_dormant_modifier_detection_uses_presence_and_propagates_its_budget() {
        let root = crate::xml::parse(b"<office:styles xmlns:office='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:style='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:fo='urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0'><style:style style:name='Shared' style:family='paragraph'><style:paragraph-properties fo:line-height='normal'/></style:style><style:style style:name='Local' style:family='paragraph' style:parent-style-name='Shared'><style:paragraph-properties style:line-height-at-least='31pt'/></style:style><style:style style:name='LocalNormal' style:family='paragraph' style:parent-style-name='Shared'><style:paragraph-properties fo:line-height='normal' style:line-height-at-least='31pt'/></style:style><style:style style:name='Inherited' style:family='paragraph' style:parent-style-name='Shared'/><style:style style:name='Base' style:family='paragraph'><style:paragraph-properties style:line-height-at-least='24pt'/></style:style><style:style style:name='Same' style:family='paragraph' style:parent-style-name='Base'><style:paragraph-properties style:line-height-at-least='24pt'/></style:style></office:styles>").unwrap();
        let styles = root.elements().map(|style| ((String::from("paragraph"), style.attr("style:name").unwrap().into()), style)).collect();
        assert!(notes_line_height_depends_on_shared_normal(&styles, Some("Local"), "Shared", |_| Ok(())).unwrap());
        for name in ["LocalNormal", "Inherited", "Shared"] {
            assert!(!notes_line_height_depends_on_shared_normal(&styles, Some(name), "Shared", |_| Ok(())).unwrap());
        }
        assert!(!notes_line_height_depends_on_shared_normal(&styles, Some("Same"), "Base", |_| Ok(())).unwrap());
        assert!(
            notes_line_height_depends_on_shared_normal(&styles, Some("Local"), "Shared", |_| Err(crate::OdpError("notes budget".into()))).is_err()
        );
    }
}
