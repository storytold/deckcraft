//! Typed source-only effects, separate from the paragraph/text semantic audit.
//! Gamma proves bounded source-retention grammar/ownership, never native rendering.
//! Frame protection is detection-only: a blocking signal, never retention permission.
use crate::{
    OdpError, Result,
    xml::{El, Node},
};

const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";

pub(crate) const GAMMA_WARNING: &str =
    "ODF picture gamma has no native rendering mapping; source instructions are retained; edited export requires verified source preservation.";

pub(crate) const PROTECTION_WARNING: &str =
    "ODF frame protection is not fully represented natively; original source is retained and interchange export is blocked.";

pub(crate) const CALCULATION_WARNING: &str =
    "ODF calculation settings are outside the executable table-formula profile; original source is retained and interchange export is blocked.";

#[derive(Default)]
pub(crate) struct Report {
    gamma: bool,
    invalid_gamma: bool,
    source_protection: bool,
    calculation_settings: bool,
}

#[derive(Clone, Copy)]
enum Context {
    Other,
    ContentRoot,
    StylesRoot,
    Common,
    Automatic,
    GraphicOwner,
}

/// A bounded subset of ODF percent syntax, without numeric interpretation or rewriting.
pub(crate) fn gamma_percent(value: &str) -> bool {
    if value.len() > 64 {
        return false;
    }
    let Some(body) = value.strip_suffix('%') else { return false };
    let body = body.strip_prefix('-').unwrap_or(body);
    let (whole, fraction) = body.split_once('.').map_or((body, None), |(a, b)| (a, Some(b)));
    !whole.is_empty() && whole.bytes().all(|b| b.is_ascii_digit()) && fraction.is_none_or(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

fn named(el: &El, namespace: &str, local: &str) -> bool {
    el.expanded_name.namespace.as_deref() == Some(namespace) && el.expanded_name.local.as_ref() == local
}

fn charge(total: &mut u64, amount: usize) -> Result<()> {
    *total = total.checked_add(amount as u64).ok_or_else(|| OdpError("source-only effect scan exceeds total work budget".into()))?;
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("source-only effect scan exceeds total work budget".into()));
    }
    Ok(())
}

fn graphic_family(el: &El) -> bool {
    el.expanded_attrs.iter().any(|a| {
        a.name.namespace.as_deref() == Some(STYLE)
            && a.name.local.as_ref() == "family"
            && el.attrs.get(&a.projected_key).is_some_and(|v| matches!(v.as_str(), "graphic" | "presentation"))
    })
}

impl Report {
    pub(crate) fn scan(&mut self, part: &str, root: &El, total: &mut u64) -> Result<()> {
        let result = self.walk(part, root, Context::Other, total, 0);
        if result.is_err() {
            self.invalid_gamma = true;
        }
        result
    }

    pub(crate) fn has_gamma(&self) -> bool {
        self.gamma
    }

    pub(crate) fn has_protection(&self) -> bool {
        self.source_protection
    }

    pub(crate) fn has_calculation_settings(&self) -> bool {
        self.calculation_settings
    }

    pub(crate) fn preservation_eligible(&self) -> bool {
        self.gamma && !self.invalid_gamma
    }

    pub(crate) fn validate_preservation(&self) -> Result<()> {
        if self.invalid_gamma {
            return Err(OdpError(
                "source-preserving text export refused: source-only picture gamma is outside the bounded retention contract".into(),
            ));
        }
        Ok(())
    }

    fn walk(&mut self, part: &str, el: &El, parent: Context, total: &mut u64, depth: usize) -> Result<()> {
        charge(total, 128)?;
        if depth > 64 {
            return Err(OdpError("source-only effect scan exceeds nesting limit".into()));
        }
        if named(el, TABLE, "calculation-settings") {
            self.calculation_settings = true;
        }
        charge(
            total,
            el.name.len().saturating_add(el.expanded_name.local.len()).saturating_add(el.expanded_name.namespace.as_deref().map_or(0, str::len)),
        )?;
        for (key, value) in &el.attrs {
            charge(total, 64usize.saturating_add(key.len()).saturating_add(value.len()))?;
        }
        for a in &el.expanded_attrs {
            charge(
                total,
                64usize
                    .saturating_add(a.projected_key.len())
                    .saturating_add(a.name.local.len())
                    .saturating_add(a.name.namespace.as_deref().map_or(0, str::len)),
            )?;
        }
        // Precharge text and child inspection before testing the property leaf or recursing.
        for child in &el.children {
            charge(total, 32)?;
            if let Node::Text(text) = child {
                charge(total, text.len())?;
            }
        }
        let leaf = el.children.iter().all(|node| matches!(node, Node::Text(t) if t.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))));
        let admitted_bag = matches!(parent, Context::GraphicOwner) && named(el, STYLE, "graphic-properties") && leaf;
        let mut gamma_attributes = 0usize;
        let mut protection_attributes = 0usize;
        for a in &el.expanded_attrs {
            if a.name.namespace.as_deref() == Some(STYLE) && a.name.local.as_ref() == "protect" {
                protection_attributes = protection_attributes.saturating_add(1);
                // Detection only: this flag never grants preservation eligibility.
                if protection_attributes > 1 || !admitted_bag || el.attrs.get(&a.projected_key).is_none_or(|v| v != "none") {
                    self.source_protection = true;
                }
            }
            if a.name.namespace.as_deref() == Some(DRAW) && a.name.local.as_ref() == "gamma" {
                // Aggregate before any optional detail recording; no source values are stored.
                self.gamma = true;
                gamma_attributes = gamma_attributes.saturating_add(1);
                if gamma_attributes > 1 || !admitted_bag || !el.attrs.get(&a.projected_key).is_some_and(|v| gamma_percent(v)) {
                    self.invalid_gamma = true;
                }
            }
        }
        let context = if depth == 0 {
            if part == "content.xml" && named(el, OFFICE, "document-content") {
                Context::ContentRoot
            } else if part == "styles.xml" && named(el, OFFICE, "document-styles") {
                Context::StylesRoot
            } else {
                Context::Other
            }
        } else {
            match parent {
                Context::ContentRoot | Context::StylesRoot if named(el, OFFICE, "automatic-styles") => Context::Automatic,
                Context::StylesRoot if named(el, OFFICE, "styles") => Context::Common,
                Context::Common | Context::Automatic if named(el, STYLE, "style") && graphic_family(el) => Context::GraphicOwner,
                Context::Common if named(el, STYLE, "default-style") && graphic_family(el) => Context::GraphicOwner,
                _ => Context::Other,
            }
        };
        for child in el.elements() {
            self.walk(part, child, context, total, depth + 1)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    const NS: &str = "xmlns:o='urn:oasis:names:tc:opendocument:xmlns:office:1.0' xmlns:s='urn:oasis:names:tc:opendocument:xmlns:style:1.0' xmlns:d='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0'";
    fn document(root: &str, scope: &str, owner: &str, family: &str, value: &str) -> El {
        crate::xml::parse(format!("<o:{root} {NS}><o:{scope}><s:{owner} s:family='{family}'><s:graphic-properties d:gamma='{value}'/></s:{owner}></o:{scope}></o:{root}>").as_bytes()).unwrap()
    }
    #[test]
    fn protection_detection_is_complete_and_does_not_change_gamma_eligibility() {
        for value in ["position", "size", "content", "position size", "none content", "", "None", "none ", "false", "file:///protect"] {
            let source = format!(
                "<o:document-content {NS}><o:automatic-styles><s:style s:family='graphic'><s:graphic-properties s:protect='{value}' d:gamma='125.5%'/></s:style></o:automatic-styles></o:document-content>"
            );
            let root = crate::xml::parse(source.as_bytes()).unwrap();
            let mut report = Report::default();
            report.scan("content.xml", &root, &mut 0).unwrap();
            assert!(report.has_protection());
            assert!(
                report.has_gamma() && report.preservation_eligible(),
                "gamma evidence stays independent; protection warning is never allowlisted"
            );
            report.validate_preservation().unwrap();
        }
    }
    #[test]
    fn only_exact_none_on_an_admitted_graphic_leaf_is_exempt_from_protection_warning() {
        for (part, root_name, scope, owner) in [
            ("content.xml", "document-content", "automatic-styles", "style"),
            ("styles.xml", "document-styles", "automatic-styles", "style"),
            ("styles.xml", "document-styles", "styles", "style"),
            ("styles.xml", "document-styles", "styles", "default-style"),
        ] {
            for family in ["graphic", "presentation"] {
                let source = format!(
                    "<o:{root_name} {NS}><o:{scope}><s:{owner} s:family='{family}'><s:graphic-properties s:protect='n&#111;ne'/></s:{owner}></o:{scope}></o:{root_name}>"
                );
                let root = crate::xml::parse(source.as_bytes()).unwrap();
                let mut report = Report::default();
                report.scan(part, &root, &mut 0).unwrap();
                assert!(!report.has_protection());
                assert!(!report.has_gamma() && !report.preservation_eligible());
                report.validate_preservation().unwrap();
            }
        }
        for source in [
            format!("<o:document-content {NS} s:protect='none'/>"),
            format!(
                "<o:document-content {NS}><o:automatic-styles><s:default-style s:family='graphic'><s:graphic-properties s:protect='none'/></s:default-style></o:automatic-styles></o:document-content>"
            ),
            format!(
                "<o:document-content {NS}><o:automatic-styles><s:style s:family='paragraph'><s:graphic-properties s:protect='none'/></s:style></o:automatic-styles></o:document-content>"
            ),
            format!(
                "<o:document-content {NS}><o:automatic-styles><s:style s:family='graphic'><s:graphic-properties s:protect='none'>x</s:graphic-properties></s:style></o:automatic-styles></o:document-content>"
            ),
        ] {
            let mut report = Report::default();
            report.scan("content.xml", &crate::xml::parse(source.as_bytes()).unwrap(), &mut 0).unwrap();
            assert!(report.has_protection());
        }
    }
    #[test]
    fn unused_wrong_part_and_late_protection_cannot_be_hidden_by_none_or_report_aggregation() {
        let mut body = String::new();
        for _ in 0..129 {
            body.push_str("<s:style s:family='graphic'><s:graphic-properties s:protect='none'/></s:style>");
        }
        body.push_str("<s:style s:family='graphic'><s:graphic-properties s:protect='position'/></s:style>");
        let root =
            crate::xml::parse(format!("<o:document-content {NS}><o:automatic-styles>{body}</o:automatic-styles></o:document-content>").as_bytes())
                .unwrap();
        let mut report = Report::default();
        report.scan("content.xml", &root, &mut 0).unwrap();
        assert!(report.has_protection());
        let none = crate::xml::parse(format!("<o:document-content {NS}><o:automatic-styles><s:style s:family='graphic'><s:graphic-properties s:protect='none'/></s:style></o:automatic-styles></o:document-content>").as_bytes()).unwrap();
        report.scan("content.xml", &none, &mut 0).unwrap();
        assert!(report.has_protection());
        let mut other_part = Report::default();
        other_part.scan("settings.xml", &none, &mut 0).unwrap();
        assert!(other_part.has_protection());
    }
    #[test]
    fn gamma_is_bounded_source_percent_syntax_without_numeric_domain_or_normalization() {
        for value in ["0%", "100%", "10000%", "0000%", "0.125%", "-0%", "-100%", "-10000.25%"] {
            assert!(gamma_percent(value), "{value}");
        }
        assert!(gamma_percent(&format!("{}%", "9".repeat(63))));
        assert!(gamma_percent(&format!("-{}%", "9".repeat(62))));
        for value in [
            "",
            "%",
            "-%",
            "+1%",
            "--1%",
            "1e2%",
            "NaN%",
            "inf%",
            "1",
            ".5%",
            "1.%",
            "1.2.3%",
            "1%%",
            "1% ",
            " 1%",
            "1%\t",
            "1% 2%",
            "１%",
            "file:///gamma",
        ] {
            assert!(!gamma_percent(value), "{value}");
        }
        assert!(!gamma_percent(&format!("{}%", "9".repeat(64))));
    }
    #[test]
    fn all_admitted_direct_scopes_and_families_are_typed_and_absence_is_distinct() {
        let empty = crate::xml::parse(format!("<o:document-content {NS}/>").as_bytes()).unwrap();
        let mut absent = Report::default();
        absent.scan("content.xml", &empty, &mut 0).unwrap();
        assert!(!absent.has_gamma());
        assert!(!absent.preservation_eligible());
        absent.validate_preservation().unwrap();
        for (part, root, scope) in [
            ("content.xml", "document-content", "automatic-styles"),
            ("styles.xml", "document-styles", "styles"),
            ("styles.xml", "document-styles", "automatic-styles"),
        ] {
            for family in ["graphic", "presentation"] {
                let mut report = Report::default();
                report.scan(part, &document(root, scope, "style", family, "-10000%"), &mut 0).unwrap();
                assert!(report.has_gamma() && report.preservation_eligible());
                report.validate_preservation().unwrap();
            }
        }
        let mut report = Report::default();
        report.scan("styles.xml", &document("document-styles", "styles", "default-style", "graphic", "100%"), &mut 0).unwrap();
        assert!(report.preservation_eligible());
    }
    #[test]
    fn invalid_gamma_never_hides_behind_valid_gamma_or_a_part_label() {
        let good = document("document-content", "automatic-styles", "style", "graphic", "100%");
        let bad = document("document-styles", "automatic-styles", "default-style", "graphic", "100%");
        for invalid_first in [false, true] {
            let mut report = Report::default();
            let mut total = 0;
            if invalid_first {
                report.scan("styles.xml", &bad, &mut total).unwrap();
            }
            report.scan("content.xml", &good, &mut total).unwrap();
            if !invalid_first {
                report.scan("styles.xml", &bad, &mut total).unwrap();
            }
            assert!(report.has_gamma());
            assert!(!report.preservation_eligible());
            assert!(report.validate_preservation().is_err());
        }
        for part in ["styles.xml", "meta.xml", "settings.xml", "embedded"] {
            let mut report = Report::default();
            report.scan(part, &good, &mut 0).unwrap();
            assert!(report.has_gamma() && !report.preservation_eligible());
        }
    }
    #[test]
    fn complete_expanded_scan_finds_wrong_owners_and_nonleaf_bags() {
        for body in [
            "<o:automatic-styles d:gamma='100%'/>",
            "<o:automatic-styles><s:style s:family='paragraph'><s:graphic-properties d:gamma='100%'/></s:style></o:automatic-styles>",
            "<o:automatic-styles><s:style s:family='graphic'><s:graphic-properties d:gamma='100%'><s:graphic-properties/></s:graphic-properties></s:style></o:automatic-styles>",
            "<o:automatic-styles><s:style s:family='graphic'><s:graphic-properties d:gamma='100%'>x</s:graphic-properties></s:style></o:automatic-styles>",
            "<o:automatic-styles><s:style s:family='graphic'><s:graphic-properties d:gamma='100%'/></s:style><s:style s:family='graphic'><s:graphic-properties d:gamma='bad'/></s:style></o:automatic-styles>",
        ] {
            let root = crate::xml::parse(format!("<o:document-content {NS}>{body}</o:document-content>").as_bytes()).unwrap();
            let mut report = Report::default();
            report.scan("content.xml", &root, &mut 0).unwrap();
            assert!(report.has_gamma() && !report.preservation_eligible());
        }
        for attributes in ["gamma='100%'", "xmlns:d='urn:deckcraft:test:foreign' d:gamma='100%'"] {
            let root =
                crate::xml::parse(format!("<o:document-content {NS}><o:automatic-styles {attributes}/></o:document-content>").as_bytes()).unwrap();
            let mut report = Report::default();
            report.scan("content.xml", &root, &mut 0).unwrap();
            assert!(!report.has_gamma(), "foreign/unqualified names are handled by independent guards");
        }
    }
    #[test]
    fn exhausted_traversal_budget_fails_closed_without_diagnostic_content() {
        let root = document("document-content", "automatic-styles", "style", "graphic", "100%");
        let mut report = Report::default();
        report.scan("content.xml", &root, &mut 0).unwrap();
        let mut exhausted = crate::MAX_TOTAL;
        let error = report.scan("content.xml", &root, &mut exhausted).unwrap_err().to_string();
        assert!(error.ends_with("source-only effect scan exceeds total work budget"));
        assert!(!report.preservation_eligible());
        assert!(report.validate_preservation().is_err());
    }
}
