//! A bounded interpreter for the public ODF enhanced geometry expression grammar.
use crate::{OdpError, Result, xml::El};
use std::{cell::Cell, collections::BTreeMap};

pub(super) struct Formulas<'a> {
    fuel: Cell<usize>,
    equations: BTreeMap<&'a str, &'a str>,
    modifiers: Vec<f64>,
    w: f64,
    h: f64,
    bounds: [f64; 4],
    has_fill: bool,
    has_stroke: bool,
    stretch: [f64; 2],
}
impl<'a> Formulas<'a> {
    pub(super) fn new(el: &'a El, w: f64, h: f64, bounds: [f64; 4], has_fill: bool, has_stroke: bool) -> Result<Self> {
        let mut equations = BTreeMap::new();
        for e in el.elements().filter(|e| e.name == "draw:equation") {
            if equations.len() >= 256 {
                return Err(OdpError("too many geometry equations".into()));
            }
            if let (Some(name), Some(formula)) = (e.attr("draw:name"), e.attr("draw:formula"))
                && equations.insert(name, formula).is_some()
            {
                return Err(OdpError("duplicate geometry equation name".into()));
            }
        }
        Ok(Self {
            fuel: Cell::new(100_000),
            equations,
            modifiers: el.attr("draw:modifiers").map(super::numbers).transpose()?.unwrap_or_default(),
            w,
            h,
            bounds,
            has_fill,
            has_stroke,
            stretch: [
                el.attr("draw:path-stretchpoint-x")
                    .map(|s| super::number(s).ok_or_else(|| OdpError("invalid horizontal stretch point".into())))
                    .transpose()?
                    .unwrap_or(0.0),
                el.attr("draw:path-stretchpoint-y")
                    .map(|s| super::number(s).ok_or_else(|| OdpError("invalid vertical stretch point".into())))
                    .transpose()?
                    .unwrap_or(0.0),
            ],
        })
    }
    fn value(&self, name: &str, depth: usize) -> Result<f64> {
        self.tick()?;
        if depth > 32 {
            return Err(OdpError("cyclic or excessive geometry formula".into()));
        }
        let value = if let Some(name) = name.strip_prefix('?') {
            let formula = self.equations.get(name).ok_or_else(|| OdpError("unknown geometry equation".into()))?;
            let mut p = Parser { s: formula, formulas: self, depth: depth + 1, steps: 0, evaluate: true };
            let n = p.expression(0)?;
            p.space();
            if !p.s.is_empty() {
                return Err(OdpError("invalid geometry formula suffix".into()));
            }
            n
        } else if let Some(index) = name.strip_prefix('$') {
            let index = index.parse::<usize>().map_err(|_| OdpError("invalid geometry modifier".into()))?;
            self.modifiers.get(index).copied().ok_or_else(|| OdpError("missing geometry modifier".into()))?
        } else {
            let [x, y, w, h] = self.bounds;
            match name {
                "left" => x,
                "top" => y,
                "right" => x + w,
                "bottom" => y + h,
                "width" => w,
                "height" => h,
                "logwidth" => self.w * 2540.0 / 72.0,
                "logheight" => self.h * 2540.0 / 72.0,
                "pi" => std::f64::consts::PI,
                "hasfill" => f64::from(self.has_fill),
                "hasstroke" => f64::from(self.has_stroke),
                "xstretch" => self.stretch[0],
                "ystretch" => self.stretch[1],
                _ => return Err(OdpError(format!("unknown geometry variable '{name}'"))),
            }
        };
        if !value.is_finite() || value.abs() > 1.0e12 {
            return Err(OdpError("non-finite or excessive geometry result".into()));
        }
        Ok(value)
    }
    fn tick(&self) -> Result<()> {
        let remaining = self.fuel.get().checked_sub(1).ok_or_else(|| OdpError("geometry formula operation budget exceeded".into()))?;
        self.fuel.set(remaining);
        Ok(())
    }
    pub(super) fn substitute(&self, path: &str) -> Result<String> {
        let mut out = String::with_capacity(path.len());
        let mut rest = path;
        let mut count = 0usize;
        while let Some(ch) = rest.chars().next() {
            if matches!(ch, '?' | '$') {
                let end = rest
                    .char_indices()
                    .skip(1)
                    // ODF names are not restricted to ASCII. Keep this bounded
                    // subset consistent with Parser::atom, including UTF-8 boundaries.
                    .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(1);
                let token = rest.get(..end).ok_or_else(|| OdpError("invalid geometry reference".into()))?;
                out.push(' ');
                out.push_str(&self.value(token, 0)?.to_string());
                out.push(' ');
                rest = rest.get(end..).ok_or_else(|| OdpError("invalid geometry reference".into()))?;
                count += 1;
                if count > 4096 || out.len() > 256 * 1024 {
                    return Err(OdpError("expanded geometry exceeds limit".into()));
                }
            } else {
                out.push(ch);
                rest = rest.get(ch.len_utf8()..).ok_or_else(|| OdpError("invalid geometry path".into()))?;
            }
        }
        Ok(out)
    }
}
struct Parser<'s, 'a, 'f> {
    s: &'s str,
    formulas: &'f Formulas<'a>,
    depth: usize,
    steps: usize,
    evaluate: bool,
}
impl Parser<'_, '_, '_> {
    fn space(&mut self) {
        self.s = self.s.trim_start();
    }
    fn expression(&mut self, min: u8) -> Result<f64> {
        self.formulas.tick()?;
        self.steps += 1;
        if self.steps > 512 || self.depth > 32 {
            return Err(OdpError("geometry formula complexity exceeds limit".into()));
        }
        self.depth += 1;
        self.space();
        let mut left = if self.s.starts_with('-') {
            self.s = &self.s[1..];
            -self.expression(3)?
        } else if self.s.starts_with('+') {
            self.s = &self.s[1..];
            self.expression(3)?
        } else if self.s.starts_with('(') {
            self.s = &self.s[1..];
            let n = self.expression(0)?;
            self.close()?;
            n
        } else {
            self.atom()?
        };
        loop {
            self.space();
            let op = self.s.chars().next().unwrap_or('\0');
            let prec = match op {
                '+' | '-' => 1,
                '*' | '/' => 2,
                _ => 0,
            };
            if prec == 0 || prec < min {
                break;
            }
            self.s = &self.s[1..];
            let right = self.expression(prec + 1)?;
            left = match op {
                '+' => left + right,
                '-' => left - right,
                '*' => left * right,
                _ => left / right,
            };
        }
        self.depth -= 1;
        if !self.evaluate {
            return Ok(0.0);
        }
        if !left.is_finite() || left.abs() > 1.0e12 {
            return Err(OdpError("non-finite or excessive geometry formula result".into()));
        }
        Ok(left)
    }
    fn close(&mut self) -> Result<()> {
        self.space();
        if !self.s.starts_with(')') {
            return Err(OdpError("unclosed geometry formula".into()));
        }
        self.s = &self.s[1..];
        Ok(())
    }
    fn atom(&mut self) -> Result<f64> {
        self.space();
        if self.s.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
            let mut end = 0;
            let mut exp = false;
            for (i, c) in self.s.char_indices() {
                if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || (exp && (c == '+' || c == '-')) {
                    end = i + c.len_utf8();
                    exp = c == 'e' || c == 'E';
                } else {
                    break;
                }
            }
            let (token, after) = self.s.split_at(end);
            self.s = after;
            return super::number(token).ok_or_else(|| OdpError("invalid formula number".into()));
        }
        let end = self
            .s
            .char_indices()
            .take_while(|(i, c)| c.is_alphanumeric() || *c == '_' || (*i == 0 && matches!(c, '?' | '$')))
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        if end == 0 {
            return Err(OdpError("invalid geometry formula token".into()));
        }
        let (name, after) = self.s.split_at(end);
        self.s = after;
        self.space();
        if !self.s.starts_with('(') {
            return if self.evaluate { self.formulas.value(name, self.depth) } else { Ok(0.0) };
        }
        self.s = &self.s[1..];
        let mut args = Vec::new();
        loop {
            if args.len() >= 3 {
                return Err(OdpError("too many geometry function arguments".into()));
            }
            // ODF §19.171 if() evaluates only the selected branch. Parse both
            // for syntax/budgets, but do not evaluate an unused singular/cyclic branch.
            let evaluate = self.evaluate;
            if name == "if" && !args.is_empty() {
                let condition = args.first().copied().unwrap_or(0.0) > 0.0;
                self.evaluate &= if args.len() == 1 { condition } else { !condition };
            }
            let argument = self.expression(0);
            self.evaluate = evaluate;
            args.push(argument?);
            self.space();
            if self.s.starts_with(',') {
                self.s = &self.s[1..];
            } else {
                break;
            }
        }
        self.close()?;
        if !self.evaluate {
            return Ok(0.0);
        }
        let value = match (name, args.as_slice()) {
            ("abs", [a]) => a.abs(),
            ("sqrt", [a]) => a.sqrt(),
            ("sin", [a]) => a.sin(),
            ("cos", [a]) => a.cos(),
            ("tan", [a]) => a.tan(),
            ("atan", [a]) => a.atan(),
            ("atan2", [a, b]) => a.atan2(*b),
            ("min", [a, b]) => a.min(*b),
            ("max", [a, b]) => a.max(*b),
            ("if", [test, a, b]) => {
                if *test > 0.0 {
                    *a
                } else {
                    *b
                }
            }
            _ => return Err(OdpError(format!("unsupported geometry function '{name}'"))),
        };
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn unicode_substitution_retains_its_reference_limit_beyond_the_xml_boundary() {
        let geometry = crate::xml::parse(
            "<draw:enhanced-geometry xmlns:draw='urn:oasis:names:tc:opendocument:xmlns:drawing:1.0'><draw:equation draw:name='幅' draw:formula='75'/></draw:enhanced-geometry>"
                .as_bytes(),
        )
        .unwrap();
        let formulas = Formulas::new(&geometry, 200.0, 100.0, [0.0, 0.0, 100.0, 100.0], true, false).unwrap();
        // XML attributes currently stop at 8192 bytes, before 4096 Unicode
        // references can reach this layer. Exercise its independent safeguard
        // directly, retaining the same bounded public-standard equation fixture.
        assert!(formulas.substitute(&"?幅 ".repeat(4096)).is_ok());
        let error = formulas.substitute(&"?幅 ".repeat(4097)).unwrap_err();
        assert_eq!(error.0, "expanded geometry exceeds limit");
    }
}
