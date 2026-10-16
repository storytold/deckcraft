//! Deterministic whole-cell wildcard criteria. Unicode scalar, case sensitive;
//! no regular expression engine, locale lookup, recursion or backtracking.
use super::{Binary, Decimal, FormulaBudget, Result, Value, invalid};
use std::cmp::Ordering;

#[derive(Clone, Copy)]
enum Token {
    Literal(char),
    One,
    Many,
}
pub(super) struct Criterion {
    operator: Binary,
    target: Value,
    pattern: Option<Vec<Token>>,
}
impl Criterion {
    pub fn new(operator: Binary, target: Value) -> Result<Self> {
        let target = if matches!(target, Value::Empty) { Value::Number(Decimal::zero()) } else { target };
        let pattern = if matches!(operator, Binary::Equal | Binary::NotEqual) {
            if let Value::Text(text) = &target {
                let mut out = Vec::new();
                out.try_reserve_exact(128).map_err(|_| invalid("cannot allocate bounded wildcard criterion"))?;
                let mut chars = text.chars();
                while let Some(c) = chars.next() {
                    let token = match c {
                        '*' => Token::Many,
                        '?' => Token::One,
                        '~' => Token::Literal(chars.next().unwrap_or('~')),
                        _ => Token::Literal(c),
                    };
                    if out.len() >= 128 {
                        return Err(invalid("wildcard criterion exceeds limit"));
                    }
                    out.push(token);
                }
                Some(out)
            } else {
                None
            }
        } else {
            None
        };
        Ok(Self { operator, target, pattern })
    }
    pub fn matches(&self, value: &Value, budget: &mut FormulaBudget) -> Result<bool> {
        if let Some(pattern) = &self.pattern {
            let matched = match value {
                Value::Empty => matches!(&self.target,Value::Text(text)if text.is_empty()),
                Value::Text(text) => wildcard(pattern, text, budget)?,
                _ => false,
            };
            return Ok(if self.operator == Binary::NotEqual { !matched } else { matched });
        }
        let ordering = match (value, &self.target) {
            (Value::Number(a), Value::Number(b)) => Some(a.compare(*b)),
            (Value::Boolean(a), Value::Boolean(b)) => Some(a.cmp(b)),
            (Value::Text(a), Value::Text(b)) => {
                budget.work(a.len().saturating_add(b.len()))?;
                Some(a.cmp(b))
            }
            _ => None,
        };
        Ok(match self.operator {
            Binary::Equal => ordering == Some(Ordering::Equal),
            Binary::NotEqual => ordering != Some(Ordering::Equal),
            Binary::Less => ordering == Some(Ordering::Less),
            Binary::LessEqual => matches!(ordering, Some(Ordering::Less | Ordering::Equal)),
            Binary::Greater => ordering == Some(Ordering::Greater),
            Binary::GreaterEqual => matches!(ordering, Some(Ordering::Greater | Ordering::Equal)),
            _ => false,
        })
    }
}
fn wildcard(pattern: &[Token], text: &str, budget: &mut FormulaBudget) -> Result<bool> {
    if text.len() > 128 {
        return Err(invalid("wildcard input exceeds formula text limit"));
    }
    let mut chars = Vec::new();
    chars.try_reserve_exact(128).map_err(|_| invalid("cannot allocate wildcard input"))?;
    chars.extend(text.chars());
    let width = chars.len().checked_add(1).ok_or_else(|| invalid("wildcard width overflow"))?;
    budget.work(
        width
            .checked_mul(pattern.len().checked_add(1).ok_or_else(|| invalid("wildcard pattern overflow"))?)
            .ok_or_else(|| invalid("wildcard work overflow"))?,
    )?;
    let mut previous = [false; 129];
    *previous.first_mut().ok_or_else(|| invalid("wildcard row missing"))? = true;
    for token in pattern {
        let mut next = [false; 129];
        if matches!(token, Token::Many) {
            *next.first_mut().ok_or_else(|| invalid("wildcard row missing"))? = previous.first().copied().unwrap_or(false);
        }
        for at in 1..width {
            let before = previous.get(at - 1).copied().ok_or_else(|| invalid("wildcard state missing"))?;
            let matched = match token {
                Token::Literal(c) => before && chars.get(at - 1) == Some(c),
                Token::One => before,
                Token::Many => {
                    previous.get(at).copied().ok_or_else(|| invalid("wildcard state missing"))?
                        || next.get(at - 1).copied().ok_or_else(|| invalid("wildcard state missing"))?
                }
            };
            *next.get_mut(at).ok_or_else(|| invalid("wildcard state missing"))? = matched;
        }
        previous = next;
    }
    previous.get(chars.len()).copied().ok_or_else(|| invalid("wildcard result missing"))
}
