//! Bounded active-dependency interpreter. Dead conditional branches are not visited.
use super::*;
use crate::table::formula::v2::{Area, Binary, Node as NodeV2};
use std::cmp::Ordering;
#[path = "table_formula_criteria.rs"]
mod criteria;

const MAX_EVAL_DEPTH: usize = 128;
const MAX_STRING: usize = 128;
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Number(Decimal),
    Boolean(bool),
    Text(String),
    Empty,
    Error(FormulaError),
    Area(Area),
}
impl Value {
    fn number(self) -> std::result::Result<Decimal, FormulaError> {
        match self {
            Self::Number(n) => Ok(n),
            Self::Boolean(b) => Ok(if b { Decimal::one() } else { Decimal::zero() }),
            Self::Empty => Ok(Decimal::zero()),
            Self::Error(e) => Err(e),
            _ => Err(FormulaError::InputNotNumeric),
        }
    }
    fn logical(self) -> std::result::Result<bool, FormulaError> {
        self.number().map(|n| !n.is_zero())
    }
    fn outcome(self) -> FormulaOutcome {
        match self {
            Self::Number(n) => FormulaOutcome::Number { lexical: n.canonical() },
            Self::Boolean(value) => FormulaOutcome::Boolean { value },
            Self::Text(value) => FormulaOutcome::String { value },
            Self::Empty => FormulaOutcome::Number { lexical: "0".into() },
            Self::Error(code) => FormulaOutcome::Error { code },
            Self::Area(_) => FormulaOutcome::Error { code: FormulaError::InputNotNumeric },
        }
    }
    fn string(self) -> std::result::Result<String, FormulaError> {
        match self {
            Self::Number(n) => Ok(n.canonical()),
            Self::Boolean(b) => Ok(if b { "TRUE" } else { "FALSE" }.into()),
            Self::Text(s) => Ok(s),
            Self::Empty => Ok(String::new()),
            Self::Error(e) => Err(e),
            Self::Area(_) => Err(FormulaError::InputNotNumeric),
        }
    }
}
fn value_error(error: FormulaError) -> Result<Value> {
    Ok(Value::Error(error))
}
fn numeric(result: std::result::Result<Decimal, FormulaError>) -> Value {
    match result {
        Ok(n) => Value::Number(n),
        Err(e) => Value::Error(e),
    }
}

pub(super) fn literal_state(cell: &super::super::Cell) -> Result<Option<String>> {
    let body = &cell.text;
    if body.paragraphs.is_empty() {
        return Ok(Some(String::new()));
    }
    if body.paragraphs.len() != 1 {
        return Ok(None);
    }
    let paragraph = body.paragraphs.first().ok_or_else(|| invalid("missing literal paragraph"))?;
    if paragraph.runs.is_empty() {
        return Ok(Some(String::new()));
    }
    if !ordinary_display(body) {
        return Ok(None);
    }
    Ok(Some(CellFormula::display_text(body)?))
}

struct Eval<'a, 'b> {
    table: &'a Table,
    entries: Vec<Entry<'a>>,
    states: Vec<u8>,
    results: Vec<Option<Value>>,
    budget: &'b mut FormulaBudget,
    authority: bool,
    active_legacy: usize,
}
impl Eval<'_, '_> {
    fn cell(&mut self, row: usize, col: usize, depth: usize) -> Result<Value> {
        self.budget.work(1)?;
        if depth > MAX_EVAL_DEPTH {
            return Err(invalid("v2 formula evaluation depth exceeds limit"));
        }
        let Some(cell) = self.table.cell(row, col) else {
            return value_error(FormulaError::InvalidReference);
        };
        // Covered physical cells have no scalar authority. In particular, stale
        // hidden rich text must not become a second value for the merged owner.
        // Legacy evaluation keeps its separate strict numeric boundary below.
        if cell.is_covered() {
            return Ok(Value::Empty);
        }
        if let Some(index) = find_entry(&self.entries, row, col) {
            return self.entry(index, depth + 1);
        }
        let state =
            if self.authority { cell.data.as_deref().map_or(CellConstantState::Absent, CellConstantState::Current) } else { cell.constant_state()? };
        match state {
            CellConstantState::Current(data) => Ok(match data.value() {
                CellValue::Number { lexical, .. } => numeric(Decimal::parse_v2(lexical).map_err(|_| FormulaError::NumericLimit)),
                CellValue::Boolean { value } => Value::Boolean(*value),
                CellValue::String { value } => {
                    if value.len() <= MAX_STRING {
                        Value::Text(value.clone())
                    } else {
                        Value::Error(FormulaError::NumericLimit)
                    }
                }
                CellValue::Date { lexical } => Value::Text(lexical.clone()),
                CellValue::Duration { lexical } => Value::Text(lexical.clone()),
            }),
            CellConstantState::Stale => value_error(FormulaError::InputNotNumeric),
            CellConstantState::Absent => match literal_state(cell)? {
                Some(text) if text.is_empty() => Ok(Value::Empty),
                Some(text) => {
                    self.budget.work(text.len())?;
                    Ok(Value::Text(text))
                }
                None => value_error(FormulaError::InputNotNumeric),
            },
        }
    }
    fn entry(&mut self, index: usize, depth: usize) -> Result<Value> {
        self.budget.work(1)?;
        if depth > MAX_EVAL_DEPTH {
            return Err(invalid("v2 formula active dependency depth exceeds limit"));
        }
        match self.states.get(index).copied() {
            Some(1) => {
                if self.active_legacy != 0 || self.entries.get(index).is_some_and(|entry| !entry.formula.is_v2()) {
                    return Err(invalid("formula dependency cycle"));
                }
                return value_error(FormulaError::Cycle);
            }
            Some(2) => {
                return self.results.get(index).cloned().flatten().ok_or_else(|| invalid("missing v2 formula result"));
            }
            Some(0) => {}
            _ => return Err(invalid("invalid v2 formula state")),
        }
        let entry = self.entries.get(index).ok_or_else(|| invalid("invalid formula entry"))?;
        let formula = entry.formula.clone();
        let (row, col) = (entry.row, entry.col);
        if !self.authority && !formula.matches_display(&self.table.cell(row, col).ok_or_else(|| invalid("missing formula cell"))?.text)? {
            return value_error(FormulaError::InputNotNumeric);
        }
        *self.states.get_mut(index).ok_or_else(|| invalid("missing v2 formula state"))? = 1;
        let value = if let Some(parsed) = &formula.parsed_v2 {
            self.node(parsed, parsed.root, depth + 1)?
        } else {
            self.active_legacy = self.active_legacy.checked_add(1).ok_or_else(|| invalid("legacy evaluation depth overflow"))?;
            let value = self.legacy(&formula, depth + 1);
            self.active_legacy = self.active_legacy.checked_sub(1).ok_or_else(|| invalid("legacy evaluation depth underflow"))?;
            value?
        };
        let value = match value {
            Value::Area(area) => {
                if area.first.row == area.last.row && area.first.col == area.last.col {
                    self.cell(area.first.row, area.first.col, depth + 1)?
                } else {
                    Value::Error(FormulaError::InputNotNumeric)
                }
            }
            v => v,
        };
        *self.states.get_mut(index).ok_or_else(|| invalid("missing v2 formula state"))? = 2;
        *self.results.get_mut(index).ok_or_else(|| invalid("missing v2 formula result"))? = Some(value.clone());
        Ok(value)
    }
    fn legacy(&mut self, formula: &CellFormula, depth: usize) -> Result<Value> {
        let mut values = Vec::new();
        values.try_reserve_exact(formula.parsed.nodes.len()).map_err(|_| invalid("cannot allocate legacy formula scratch"))?;
        for node in &formula.parsed.nodes {
            self.budget.work(1)?;
            let get = |i: usize| values.get(i).copied().ok_or_else(|| invalid("invalid legacy AST child"));
            let value = match *node {
                Node::Number(n) => Ok(n),
                Node::BrokenReference => Err(FormulaError::InvalidReference),
                Node::Reference(row, col) => {
                    if let Some(index) = find_entry(&self.entries, row, col) {
                        match self.entry(index, depth + 1)? {
                            Value::Number(n) => Decimal::parse(&n.canonical()).map_err(|_| FormulaError::NumericLimit),
                            Value::Error(_) => Err(FormulaError::DependencyError),
                            _ => Err(FormulaError::InputNotNumeric),
                        }
                    } else {
                        let Some(cell) = self.table.cell(row, col) else {
                            return Err(invalid("formula reference lies outside table grid"));
                        };
                        let state = if self.authority {
                            cell.data.as_deref().map_or(CellConstantState::Absent, CellConstantState::Current)
                        } else {
                            cell.constant_state()?
                        };
                        match state {
                            CellConstantState::Current(d) => match d.value() {
                                CellValue::Number { kind: CellNumberKind::Float, lexical } => {
                                    Decimal::parse(lexical).map_err(|_| FormulaError::NumericLimit)
                                }
                                _ => Err(FormulaError::InputNotNumeric),
                            },
                            _ => Err(FormulaError::InputNotNumeric),
                        }
                    }
                }
                Node::Positive(i) => get(i)?,
                Node::Negative(i) => get(i)?.and_then(|n: Decimal| n.neg().map_err(|_| FormulaError::NumericLimit)),
                Node::Add(a, b) | Node::Subtract(a, b) | Node::Multiply(a, b) => match (get(a)?, get(b)?) {
                    (Ok(a), Ok(b)) => match node {
                        Node::Add(..) => a.add(b),
                        Node::Subtract(..) => a.sub(b),
                        _ => a.mul(b),
                    }
                    .map_err(|_| FormulaError::NumericLimit),
                    (Err(e), _) | (_, Err(e)) => Err(e),
                },
            };
            values.push(value);
        }
        Ok(numeric(values.last().copied().ok_or_else(|| invalid("empty legacy formula"))?))
    }
    fn scalar(&mut self, parsed: &crate::table::formula::v2::Parsed, index: usize, depth: usize) -> Result<Value> {
        match self.node(parsed, index, depth)? {
            Value::Area(a) if a.first.row == a.last.row && a.first.col == a.last.col => self.cell(a.first.row, a.first.col, depth + 1),
            Value::Area(_) => value_error(FormulaError::InputNotNumeric),
            v => Ok(v),
        }
    }
    fn node(&mut self, parsed: &crate::table::formula::v2::Parsed, index: usize, depth: usize) -> Result<Value> {
        self.budget.work(1)?;
        if depth > MAX_EVAL_DEPTH {
            return Err(invalid("v2 formula evaluation nesting exceeds limit"));
        }
        let node = parsed.nodes.get(index).ok_or_else(|| invalid("invalid v2 AST index"))?;
        match node {
            NodeV2::Number(n) => Ok(Value::Number(*n)),
            NodeV2::Text(s) => Ok(Value::Text(s.clone())),
            NodeV2::Error(e) => value_error(*e),
            NodeV2::Reference(a) => Ok(Value::Area(*a)),
            NodeV2::Positive(i) => Ok(numeric(self.scalar(parsed, *i, depth + 1)?.number())),
            NodeV2::Negative(i) => {
                Ok(numeric(self.scalar(parsed, *i, depth + 1)?.number().and_then(|n| n.neg_v2().map_err(|_| FormulaError::NumericLimit))))
            }
            NodeV2::Percent(i) => {
                self.budget.numeric(65_536)?;
                Ok(numeric(
                    self.scalar(parsed, *i, depth + 1)?.number().and_then(|n| {
                        n.div(Decimal::parse_v2("100").map_err(|_| FormulaError::NumericLimit)?).map_err(|_| FormulaError::NumericLimit)
                    }),
                ))
            }
            NodeV2::Binary(op, a, b) => {
                let left = self.scalar(parsed, *a, depth + 1)?;
                let right = self.scalar(parsed, *b, depth + 1)?;
                self.binary(*op, left, right)
            }
            NodeV2::Call(name, args) => self.call(parsed, name, args, depth + 1),
        }
    }
    fn binary(&mut self, op: Binary, left: Value, right: Value) -> Result<Value> {
        if let Value::Error(e) = left {
            return value_error(e);
        }
        if let Value::Error(e) = right {
            return value_error(e);
        }
        if op == Binary::Concat {
            return Ok(match (left.string(), right.string()) {
                (Ok(mut a), Ok(b)) => {
                    if a.len().checked_add(b.len()).is_none_or(|n| n > MAX_STRING) {
                        Value::Error(FormulaError::NumericLimit)
                    } else {
                        a.push_str(&b);
                        Value::Text(a)
                    }
                }
                (Err(e), _) | (_, Err(e)) => Value::Error(e),
            });
        }
        if matches!(op, Binary::Equal | Binary::NotEqual | Binary::Less | Binary::LessEqual | Binary::Greater | Binary::GreaterEqual) {
            let ordering = match (&left, &right) {
                (Value::Number(a), Value::Number(b)) => Some(a.compare(*b)),
                (Value::Boolean(a), Value::Boolean(b)) => Some(a.cmp(b)),
                (Value::Text(a), Value::Text(b)) => Some(a.cmp(b)),
                (Value::Empty, Value::Empty) => Some(Ordering::Equal),
                _ => None,
            };
            return Ok(Value::Boolean(match op {
                Binary::Equal => ordering == Some(Ordering::Equal),
                Binary::NotEqual => ordering != Some(Ordering::Equal),
                Binary::Less => ordering == Some(Ordering::Less),
                Binary::LessEqual => matches!(ordering, Some(Ordering::Less | Ordering::Equal)),
                Binary::Greater => ordering == Some(Ordering::Greater),
                Binary::GreaterEqual => {
                    matches!(ordering, Some(Ordering::Greater | Ordering::Equal))
                }
                _ => false,
            }));
        }
        let (a, b) = match (left.number(), right.number()) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return value_error(e),
        };
        if matches!(op, Binary::Div | Binary::Pow) {
            self.budget.numeric(65_536)?;
        }
        let result = match op {
            Binary::Add => a.add_v2(b),
            Binary::Sub => a.sub_v2(b),
            Binary::Mul => a.mul_v2(b),
            Binary::Div => {
                if b.is_zero() {
                    return value_error(FormulaError::DivisionByZero);
                }
                a.div(b)
            }
            Binary::Pow => {
                if a.is_zero() && b.sign() < 0 {
                    return value_error(FormulaError::DivisionByZero);
                }
                if a.is_zero() && b.sign() > 0 {
                    return Ok(Value::Number(Decimal::zero()));
                }
                match b.integer() {
                    Ok(exponent) if (-4096..=4096).contains(&exponent) => a.pow_integer(exponent),
                    _ => {
                        if a.is_zero() && b.sign() > 0 {
                            return Ok(Value::Number(Decimal::zero()));
                        }
                        if b == Decimal::parse_v2("0.5")? {
                            a.sqrt()
                        } else {
                            return Ok(match a.pow_fractional(b, &mut |units| self.budget.numeric(units))? {
                                Some(n) => Value::Number(n),
                                None => Value::Error(FormulaError::NumericLimit),
                            });
                        }
                    }
                }
            }
            _ => return Err(invalid("invalid numeric operator")),
        };
        Ok(numeric(result.map_err(|_| FormulaError::NumericLimit)))
    }
    fn values(&mut self, parsed: &crate::table::formula::v2::Parsed, args: &[usize], depth: usize) -> Result<Vec<(Value, bool)>> {
        let mut out = Vec::new();
        for index in args {
            match self.node(parsed, *index, depth + 1)? {
                Value::Area(a) => {
                    let (r0, r1, c0, c1) =
                        (a.first.row.min(a.last.row), a.first.row.max(a.last.row), a.first.col.min(a.last.col), a.first.col.max(a.last.col));
                    if r1 >= self.table.n_rows() || c1 >= self.table.n_cols() {
                        out.try_reserve(1).map_err(|_| invalid("cannot allocate formula values"))?;
                        out.push((Value::Error(FormulaError::InvalidReference), true));
                        continue;
                    }
                    let count = (r1 - r0 + 1).checked_mul(c1 - c0 + 1).ok_or_else(|| invalid("formula range area overflow"))?;
                    self.budget.work(count)?;
                    if out.len().checked_add(count).is_none_or(|n| n > 100_000) {
                        return Err(invalid("formula aggregate expansion exceeds limit"));
                    }
                    out.try_reserve_exact(count).map_err(|_| invalid("cannot allocate formula range values"))?;
                    for row in r0..=r1 {
                        for col in c0..=c1 {
                            out.push((self.cell(row, col, depth + 1)?, true));
                        }
                    }
                }
                v => {
                    self.budget.work(1)?;
                    out.try_reserve(1).map_err(|_| invalid("cannot allocate formula values"))?;
                    out.push((v, false));
                }
            }
        }
        Ok(out)
    }
    fn call(&mut self, p: &crate::table::formula::v2::Parsed, name: &str, args: &[usize], depth: usize) -> Result<Value> {
        let arity = |minimum: usize, maximum: usize| args.len() >= minimum && args.len() <= maximum;
        let bad = FormulaError::InputNotNumeric;
        if matches!(name, "IF" | "IFERROR" | "IFNA") {
            if name == "IF" {
                if !arity(2, 3) {
                    return value_error(bad);
                }
                let condition = self.scalar(p, *args.first().ok_or_else(|| invalid("missing IF condition"))?, depth + 1)?.logical();
                let selected = match condition {
                    Ok(true) => args.get(1),
                    Ok(false) => args.get(2),
                    Err(e) => return value_error(e),
                };
                return match selected {
                    Some(i) => self.scalar(p, *i, depth + 1),
                    None => Ok(Value::Boolean(false)),
                };
            }
            if !arity(2, 2) {
                return value_error(bad);
            }
            let value = self.scalar(p, *args.first().ok_or_else(|| invalid("missing error handler value"))?, depth + 1)?;
            if matches!(&value,Value::Error(e)if name=="IFERROR"||*e==FormulaError::NotAvailable) {
                return self.scalar(p, *args.get(1).ok_or_else(|| invalid("missing fallback"))?, depth + 1);
            }
            return Ok(value);
        }
        if matches!(name, "TRUE" | "FALSE" | "NA") {
            if !arity(0, 0) {
                return value_error(bad);
            }
            return if name == "NA" { value_error(FormulaError::NotAvailable) } else { Ok(Value::Boolean(name == "TRUE")) };
        }
        if matches!(name, "SUM" | "PRODUCT" | "MIN" | "MAX" | "AVERAGE" | "COUNT" | "COUNTA" | "COUNTBLANK" | "AND" | "OR") {
            if (matches!(name, "AND" | "OR") && !arity(1, 32)) || (name == "COUNTBLANK" && !arity(1, 1)) {
                return value_error(bad);
            }
            if name == "COUNTBLANK"
                && !matches!(self.node(p, *args.first().ok_or_else(|| invalid("missing blank count range"))?, depth + 1)?, Value::Area(_))
            {
                return value_error(bad);
            }
            let values = self.values(p, args, depth)?;
            let mut count = 0usize;
            let mut result = Decimal::zero();
            let mut logical = name == "AND";
            for (value, range) in values {
                if name == "COUNTA" {
                    if !matches!(value, Value::Empty) {
                        count += 1;
                    }
                    continue;
                }
                if name == "COUNTBLANK" {
                    if matches!(&value, Value::Empty) || matches!(&value,Value::Text(s)if s.is_empty()) {
                        count += 1;
                    }
                    continue;
                }
                if matches!(name, "AND" | "OR") {
                    if range && matches!(value, Value::Text(_) | Value::Empty) {
                        continue;
                    }
                    match value.logical() {
                        Ok(v) => {
                            logical = if name == "AND" { logical && v } else { logical || v };
                            count += 1;
                        }
                        Err(e) => return value_error(e),
                    }
                    continue;
                }
                if range && matches!(value, Value::Text(_) | Value::Boolean(_) | Value::Empty) {
                    continue;
                }
                if name == "COUNT" {
                    if matches!(value, Value::Number(_)) || (!range && matches!(value, Value::Boolean(_) | Value::Empty)) {
                        count += 1;
                    }
                    continue;
                }
                let n = match value.number() {
                    Ok(n) => n,
                    Err(e) => return value_error(e),
                };
                let updated = match name {
                    "PRODUCT" => {
                        if count == 0 {
                            Ok(n)
                        } else {
                            result.mul_v2(n)
                        }
                    }
                    "MIN" => Ok(if count == 0 || n.compare(result) == Ordering::Less { n } else { result }),
                    "MAX" => Ok(if count == 0 || n.compare(result) == Ordering::Greater { n } else { result }),
                    _ => result.add_v2(n),
                };
                result = match updated {
                    Ok(n) => n,
                    Err(_) => return value_error(FormulaError::NumericLimit),
                };
                count += 1;
            }
            if matches!(name, "COUNT" | "COUNTA" | "COUNTBLANK") {
                return Ok(numeric(Decimal::parse_v2(&count.to_string()).map_err(|_| FormulaError::NumericLimit)));
            }
            if matches!(name, "AND" | "OR") {
                return if count == 0 { value_error(bad) } else { Ok(Value::Boolean(logical)) };
            }
            if name == "AVERAGE" {
                if count == 0 {
                    return value_error(FormulaError::DivisionByZero);
                }
                self.budget.numeric(65_536)?;
                result = match result.div(Decimal::parse_v2(&count.to_string())?) {
                    Ok(n) => n,
                    Err(_) => return value_error(FormulaError::NumericLimit),
                };
            }
            return Ok(Value::Number(result));
        }
        if matches!(name, "SUMIF" | "COUNTIF" | "AVERAGEIF") {
            return self.conditional(p, name, args, depth);
        }
        if matches!(name, "ISBLANK" | "ISNUMBER" | "ISTEXT" | "ISLOGICAL" | "ISERROR" | "ISERR" | "ISNA") {
            if !arity(1, 1) {
                return value_error(bad);
            }
            let v = self.scalar(p, *args.first().ok_or_else(|| invalid("missing predicate value"))?, depth + 1)?;
            return Ok(Value::Boolean(match name {
                "ISBLANK" => matches!(v, Value::Empty),
                "ISNUMBER" => matches!(v, Value::Number(_)),
                "ISTEXT" => matches!(v, Value::Text(_)),
                "ISLOGICAL" => matches!(v, Value::Boolean(_)),
                "ISERROR" => matches!(v, Value::Error(_)),
                "ISERR" => matches!(v,Value::Error(e)if e!=FormulaError::NotAvailable),
                "ISNA" => matches!(v, Value::Error(FormulaError::NotAvailable)),
                _ => false,
            }));
        }
        if name == "CONCATENATE" {
            let mut text = String::new();
            text.try_reserve_exact(MAX_STRING).map_err(|_| invalid("cannot allocate formula concatenation"))?;
            for index in args {
                let value = self.scalar(p, *index, depth + 1)?;
                let s = match value.string() {
                    Ok(s) => s,
                    Err(e) => return value_error(e),
                };
                if text.len().checked_add(s.len()).is_none_or(|n| n > MAX_STRING) {
                    return value_error(FormulaError::NumericLimit);
                }
                text.push_str(&s);
            }
            return Ok(Value::Text(text));
        }
        if matches!(name, "LEN" | "LEFT" | "RIGHT" | "MID") {
            return self.text_function(p, name, args, depth);
        }
        if matches!(name, "DATE" | "YEAR" | "MONTH" | "DAY" | "TIME" | "HOUR" | "MINUTE" | "SECOND") {
            return self.temporal(p, name, args, depth);
        }
        if !matches!(name, "ABS" | "SIGN" | "INT" | "TRUNC" | "ROUND" | "ROUNDUP" | "ROUNDDOWN" | "MOD" | "POWER" | "SQRT" | "NOT") {
            return value_error(FormulaError::UnknownName);
        }
        let optional = matches!(name, "TRUNC" | "ROUND" | "ROUNDUP" | "ROUNDDOWN");
        let pair = matches!(name, "MOD" | "POWER");
        if !(if optional {
            arity(1, 2)
        } else if pair {
            arity(2, 2)
        } else {
            arity(1, 1)
        }) {
            return value_error(bad);
        }
        let value = self.scalar(p, *args.first().ok_or_else(|| invalid("missing function operand"))?, depth + 1)?;
        if name == "NOT" {
            return Ok(match value.logical() {
                Ok(b) => Value::Boolean(!b),
                Err(e) => Value::Error(e),
            });
        }
        let n = match value.number() {
            Ok(n) => n,
            Err(e) => return value_error(e),
        };
        let second = if let Some(index) = args.get(1) {
            match self.scalar(p, *index, depth + 1)?.number() {
                Ok(n) => n,
                Err(e) => return value_error(e),
            }
        } else {
            Decimal::zero()
        };
        self.budget.numeric(if matches!(name, "POWER" | "SQRT" | "MOD") { 262_144 } else { 1024 })?;
        if name == "POWER" {
            return self.binary(Binary::Pow, Value::Number(n), Value::Number(second));
        }
        if name == "MOD" && second.is_zero() {
            return value_error(FormulaError::DivisionByZero);
        }
        let result = match name {
            "ABS" => n.abs(),
            "SIGN" => Decimal::parse_v2(&n.sign().to_string()),
            "INT" => n.round(0, 3),
            "SQRT" => n.sqrt(),
            "MOD" => n.modulo(second),
            "TRUNC" | "ROUND" | "ROUNDUP" | "ROUNDDOWN" => {
                let digits = match second.integer().ok().and_then(|n| i32::try_from(n).ok()) {
                    Some(n) => n,
                    None => return value_error(bad),
                };
                n.round(
                    digits,
                    match name {
                        "ROUND" => 0,
                        "ROUNDUP" => 1,
                        _ => 2,
                    },
                )
            }
            _ => return Err(invalid("invalid numeric function")),
        };
        Ok(numeric(result.map_err(|_| FormulaError::NumericLimit)))
    }
    fn text_function(&mut self, p: &crate::table::formula::v2::Parsed, name: &str, args: &[usize], depth: usize) -> Result<Value> {
        let valid = match name {
            "LEN" => args.len() == 1,
            "MID" => args.len() == 3,
            _ => matches!(args.len(), 1 | 2),
        };
        if !valid {
            return value_error(FormulaError::InputNotNumeric);
        }
        let text_value = self.scalar(p, *args.first().ok_or_else(|| invalid("missing text operand"))?, depth + 1)?;
        let text = match text_value {
            Value::Text(text) => text,
            Value::Error(error) => return value_error(error),
            _ => return value_error(FormulaError::InputNotNumeric),
        };
        self.budget.work(text.len())?;
        let length = text.chars().count();
        if name == "LEN" {
            return Ok(Value::Number(Decimal::parse_v2(&length.to_string())?));
        }
        let mut numbers = Vec::new();
        for i in args.iter().skip(1) {
            let number = match self.scalar(p, *i, depth + 1)?.number() {
                Ok(n) => n,
                Err(e) => return value_error(e),
            };
            match number.integer().ok().and_then(|n| usize::try_from(n).ok()) {
                Some(n) => numbers.push(n),
                None => return value_error(FormulaError::InputNotNumeric),
            }
        }
        let (skip, take) = match name {
            "LEFT" => (0, numbers.first().copied().unwrap_or(1)),
            "RIGHT" => {
                let count = numbers.first().copied().unwrap_or(1);
                (length.saturating_sub(count), count)
            }
            _ => {
                let start = numbers.first().copied().unwrap_or(0);
                if start == 0 {
                    return value_error(FormulaError::InputNotNumeric);
                }
                (start - 1, numbers.get(1).copied().unwrap_or(0))
            }
        };
        Ok(Value::Text(text.chars().skip(skip).take(take).collect()))
    }
    fn conditional(&mut self, p: &crate::table::formula::v2::Parsed, name: &str, args: &[usize], depth: usize) -> Result<Value> {
        if !(args.len() == 2 || (name != "COUNTIF" && args.len() == 3)) {
            return value_error(FormulaError::InputNotNumeric);
        }
        let Value::Area(area) = self.node(p, *args.first().ok_or_else(|| invalid("missing condition range"))?, depth + 1)? else {
            return value_error(FormulaError::InputNotNumeric);
        };
        let criterion = self.scalar(p, *args.get(1).ok_or_else(|| invalid("missing criterion"))?, depth + 1)?;
        if let Value::Error(e) = criterion {
            return value_error(e);
        }
        let result_area = if let Some(i) = args.get(2) {
            let Value::Area(a) = self.node(p, *i, depth + 1)? else {
                return value_error(FormulaError::InputNotNumeric);
            };
            a
        } else {
            area
        };
        let dimensions = |a: Area| (a.first.row.abs_diff(a.last.row), a.first.col.abs_diff(a.last.col));
        if dimensions(area) != dimensions(result_area) {
            return value_error(FormulaError::InputNotNumeric);
        }
        let (rows, cols) = dimensions(area);
        if area.first.row.max(area.last.row) >= self.table.n_rows()
            || area.first.col.max(area.last.col) >= self.table.n_cols()
            || result_area.first.row.max(result_area.last.row) >= self.table.n_rows()
            || result_area.first.col.max(result_area.last.col) >= self.table.n_cols()
        {
            return value_error(FormulaError::InvalidReference);
        }
        self.budget.work((rows + 1).checked_mul(cols + 1).ok_or_else(|| invalid("criterion range overflow"))?)?;
        let (operator, target) = if let Value::Text(s) = criterion {
            let (op, rest) = [
                ("<=", Binary::LessEqual),
                (">=", Binary::GreaterEqual),
                ("<>", Binary::NotEqual),
                ("<", Binary::Less),
                (">", Binary::Greater),
                ("=", Binary::Equal),
            ]
            .into_iter()
            .find_map(|(token, op)| s.strip_prefix(token).map(|rest| (op, rest)))
            .unwrap_or((Binary::Equal, s.as_str()));
            let target = match Decimal::parse_v2(rest) {
                Ok(n) => Value::Number(n),
                Err(_) => Value::Text(rest.into()),
            };
            (op, target)
        } else {
            (Binary::Equal, criterion)
        };
        let criterion = criteria::Criterion::new(operator, target)?;
        let mut sum = Decimal::zero();
        let mut count = 0usize;
        for r in 0..=rows {
            for c in 0..=cols {
                let value = self.cell(area.first.row.min(area.last.row) + r, area.first.col.min(area.last.col) + c, depth + 1)?;
                // Errors in tested cells are values to compare, not aggregate inputs.
                if criterion.matches(&value, self.budget)? {
                    if name == "COUNTIF" {
                        count += 1;
                        continue;
                    }
                    match self.cell(
                        result_area.first.row.min(result_area.last.row) + r,
                        result_area.first.col.min(result_area.last.col) + c,
                        depth + 1,
                    )? {
                        Value::Number(n) => {
                            sum = match sum.add_v2(n) {
                                Ok(n) => n,
                                Err(_) => return value_error(FormulaError::NumericLimit),
                            };
                            count += 1;
                        }
                        Value::Error(e) => return value_error(e),
                        _ => {}
                    }
                }
            }
        }
        if name == "COUNTIF" {
            return Ok(Value::Number(Decimal::parse_v2(&count.to_string())?));
        }
        if name == "AVERAGEIF" {
            if count == 0 {
                return value_error(FormulaError::DivisionByZero);
            }
            self.budget.numeric(65_536)?;
            sum = match sum.div(Decimal::parse_v2(&count.to_string())?) {
                Ok(n) => n,
                Err(_) => return value_error(FormulaError::NumericLimit),
            };
        }
        Ok(Value::Number(sum))
    }
    fn temporal(&mut self, p: &crate::table::formula::v2::Parsed, name: &str, args: &[usize], depth: usize) -> Result<Value> {
        let constructor = matches!(name, "DATE" | "TIME");
        if args.len() != if constructor { 3 } else { 1 } {
            return value_error(FormulaError::InputNotNumeric);
        }
        if constructor {
            let mut numbers = Vec::new();
            for index in args {
                match self.scalar(p, *index, depth + 1)?.number() {
                    Ok(n) => numbers.push(n),
                    Err(e) => return value_error(e),
                }
            }
            let a = *numbers.first().ok_or_else(|| invalid("missing temporal argument"))?;
            let b = *numbers.get(1).ok_or_else(|| invalid("missing temporal argument"))?;
            let c = *numbers.get(2).ok_or_else(|| invalid("missing temporal argument"))?;
            self.budget.numeric(65_536)?;
            let result = if name == "DATE" {
                crate::table::formula::calendar::date_serial(a, b, c)
            } else {
                a.mul_v2(Decimal::parse_v2("3600")?)
                    .and_then(|n| n.add_v2(b.mul_v2(Decimal::parse_v2("60")?)?))
                    .and_then(|n| n.add_v2(c))
                    .and_then(|n| n.div(Decimal::parse_v2("86400")?))
            };
            return Ok(numeric(result.map_err(|_| FormulaError::NumericLimit)));
        }
        let value = self.scalar(p, *args.first().ok_or_else(|| invalid("missing temporal operand"))?, depth + 1)?;
        let serial = match value {
            Value::Number(n) => n,
            Value::Text(text) => match crate::table::formula::calendar::serial_from_iso(&text) {
                Ok(n) => n,
                Err(_) => return value_error(FormulaError::InputNotNumeric),
            },
            Value::Error(e) => return value_error(e),
            _ => return value_error(FormulaError::InputNotNumeric),
        };
        self.budget.numeric(4096)?;
        if matches!(name, "YEAR" | "MONTH" | "DAY") {
            let parts = match crate::table::formula::calendar::date_parts(serial) {
                Ok(parts) => parts,
                Err(_) => return value_error(FormulaError::NumericLimit),
            };
            let n = match name {
                "YEAR" => parts.0,
                "MONTH" => parts.1,
                _ => parts.2,
            };
            return Ok(Value::Number(Decimal::parse_v2(&n.to_string())?));
        }
        let (hour, minute, second, fraction) = serial.clock_parts()?;
        let n = match name {
            "HOUR" => hour,
            "MINUTE" => minute,
            _ => {
                // OpenFormula's defining expression is ROUND(seconds-within-minute).
                // It can produce 60 near a minute boundary; preserve that expression
                // rather than silently truncating or changing the serial value.
                second + u32::from(fraction.as_bytes().first().is_some_and(|digit| *digit >= b'5'))
            }
        };
        Ok(Value::Number(Decimal::parse_v2(&n.to_string())?))
    }
}

/// Single root integration hook: errors never use a numeric format.
fn display_outcome(cell: &super::super::Cell, outcome: &FormulaOutcome) -> Result<String> {
    match (&cell.number_format, outcome) {
        (Some(format), FormulaOutcome::Number { lexical }) => format.format_number(lexical),
        (Some(format), FormulaOutcome::Boolean { value }) => format.format_boolean(*value),
        (Some(format), FormulaOutcome::String { value }) => format.format_text(value),
        _ => outcome.plain_display_text(),
    }
}
pub(super) fn plan(table: &Table, budget: &mut FormulaBudget, authority: bool) -> Result<TableCalculationPlan> {
    validate_formula_grid(table)?;
    let mut entries = Vec::new();
    entries.try_reserve_exact(MAX_TABLE_FORMULAS).map_err(|_| invalid("cannot allocate v2 formula entries"))?;
    let mut updates = Vec::new();
    updates.try_reserve_exact(MAX_TABLE_FORMULAS).map_err(|_| invalid("cannot allocate v2 formula updates"))?;
    for (row, cells) in table.rows.iter().enumerate() {
        for (col, cell) in cells.cells.iter().enumerate() {
            if let Some(formula) = &cell.formula {
                if cell.data.is_some() {
                    return Err(invalid("cell cannot contain both constant data and formula"));
                }
                if entries.len() >= MAX_TABLE_FORMULAS {
                    return Err(invalid("table formula count exceeds limit"));
                }
                budget.formula(formula)?;
                entries.push(Entry { row, col, formula });
                if !authority && !formula.matches_display(&cell.text)? {
                    updates.push(Update { row, col, expected: formula.clone(), current: false, formula: None, replacement: None });
                }
            }
        }
    }
    let mut states = Vec::new();
    let mut results = Vec::new();
    states.try_reserve_exact(entries.len()).map_err(|_| invalid("cannot allocate v2 dependency states"))?;
    results.try_reserve_exact(entries.len()).map_err(|_| invalid("cannot allocate v2 dependency results"))?;
    states.resize(entries.len(), 0);
    results.resize(entries.len(), None);
    let mut eval = Eval { table, entries, states, results, budget, authority, active_legacy: 0 };
    for index in 0..eval.entries.len() {
        let outcome = eval.entry(index, 0)?.outcome();
        let entry = eval.entries.get(index).ok_or_else(|| invalid("missing evaluated entry"))?;
        let cell = table.cell(entry.row, entry.col).ok_or_else(|| invalid("missing evaluated cell"))?;
        let current = authority || entry.formula.matches_display(&cell.text)?;
        if !current {
            continue;
        }
        let replacement = if entry.formula.uses_formatted_display() { display_outcome(cell, &outcome)? } else { outcome.display_text()? };
        if !entry.formula.outcome().equivalent(&outcome)?
            || (entry.formula.uses_formatted_display() && entry.formula.display_baseline() != replacement)
        {
            if authority {
                return Err(invalid("formula cached outcome or formatted display differs from recomputation"));
            }
            let formula = Arc::new(entry.formula.with_outcome(outcome, &replacement_body(&cell.text, &replacement)?)?);
            updates.push(Update {
                row: entry.row,
                col: entry.col,
                expected: entry.formula.clone(),
                current: true,
                formula: Some(formula),
                replacement: Some(replacement),
            });
        }
    }
    // All physical cells are observed: lazy/range evaluation must not allow a plan to
    // be applied after a previously unvisited branch/range authority changes.
    let cells = table.n_rows().checked_mul(table.n_cols()).ok_or_else(|| invalid("formula observation count overflow"))?;
    eval.budget.work(cells)?;
    let mut observations = Vec::new();
    if !authority {
        observations.try_reserve_exact(cells).map_err(|_| invalid("cannot allocate formula observations"))?;
        for (row, cells) in table.rows.iter().enumerate() {
            for (col, cell) in cells.cells.iter().enumerate() {
                let current = if let Some(d) = &cell.data {
                    d.matches_display(&cell.text)?
                } else if let Some(f) = &cell.formula {
                    f.matches_display(&cell.text)?
                } else {
                    false
                };
                observations.push(Observation {
                    row,
                    col,
                    data: cell.data.clone(),
                    formula: cell.formula.clone(),
                    current,
                    format: Some(cell.number_format.clone()),
                    literal: if cell.data.is_none() && cell.formula.is_none() {
                        let literal = literal_state(cell)?;
                        eval.budget.work(literal.as_ref().map_or(0, String::len))?;
                        Some(literal)
                    } else {
                        None
                    },
                });
            }
        }
    }
    Ok(TableCalculationPlan {
        updates,
        observations,
        dimensions: Some((table.n_rows(), table.n_cols())),
        topology: Some(TableTopology::capture(table, eval.budget)?),
    })
}

impl Table {
    pub fn set_cell_formula_v2(&mut self, row: usize, col: usize, expression: &str) -> Result<bool> {
        self.set_cell_formula_v2_with_budget(row, col, expression, &mut FormulaBudget::default())
    }
    pub fn set_cell_formula_v2_with_budget(&mut self, row: usize, col: usize, expression: &str, budget: &mut FormulaBudget) -> Result<bool> {
        let parsed = crate::table::formula::v2::Parsed::parse(expression)?;
        budget.work(expression.len().saturating_add(parsed.nodes.len()))?;
        validate_formula_grid(self)?;
        let cell = self.cell(row, col).ok_or_else(|| invalid("formula cell coordinate outside table"))?;
        // Accept surrounding merged geometry without broadening assignment to
        // covered cells or merged anchors.
        if cell.is_covered() || cell.grid_span != 1 || cell.row_span != 1 {
            return Err(invalid("formula assignment requires an unmerged visible cell"));
        }
        if cell.formula.as_ref().is_some_and(|f| f.is_v2() && f.expression() == expression)
            && matches!(cell.formula_state()?, CellFormulaState::Current(_))
        {
            self.validate_formulas_with_budget(budget)?;
            return Ok(false);
        }
        let empty =
            cell.text.paragraphs.is_empty() || (cell.text.paragraphs.len() == 1 && cell.text.paragraphs.first().is_some_and(|p| p.runs.is_empty()));
        if !ordinary_display(&cell.text) && !empty {
            return Err(invalid("v2 formula assignment requires ordinary display text"));
        }
        let original = cell.clone();
        let result = (|| {
            let target = self.cell_mut(row, col).ok_or_else(|| invalid("missing formula target"))?;
            if target.text.paragraphs.is_empty() {
                target.text.paragraphs.push(Paragraph::default());
            }
            if let Some(p) = target.text.paragraphs.first_mut()
                && p.runs.is_empty()
            {
                p.runs.push(Run::new(""));
            }
            replace_display(&mut target.text, "0".into());
            target.data = None;
            target.formula =
                Some(Arc::new(CellFormula::from_display_v2(expression.into(), FormulaOutcome::Number { lexical: "0".into() }, &target.text)?));
            let plan = self.plan_recalculation_with_budget(budget)?;
            let changed = !plan.is_empty() || self.cell(row, col).is_some_and(|c| c != &original);
            plan.apply(self)?;
            Ok(changed)
        })();
        if result.is_err()
            && let Some(cell) = self.cell_mut(row, col)
        {
            *cell = original;
        }
        result
    }
}
