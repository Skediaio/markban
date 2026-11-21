//! Exact decimal numbers for the card `order` key.

use std::cmp::Ordering;

/// A decimal number `-?[0-9]+(\.[0-9]+)?`, compared exactly (no floating point).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Decimal {
    negative: bool,
    /// Integer digits without leading zeros (empty for zero).
    int: String,
    /// Fraction digits without trailing zeros.
    frac: String,
}

impl Decimal {
    /// Parses the spec's decimal syntax. Returns `None` for anything else
    /// (`1.`, `.5`, `+1`, `1e3`, `NaN`, empty, surrounding spaces).
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (int, frac) = match digits.split_once('.') {
            Some((i, f)) => (i, f),
            None => (digits, ""),
        };
        let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if int.is_empty() || !all_digits(int) || !all_digits(frac) {
            return None;
        }
        if digits.contains('.') && frac.is_empty() {
            return None;
        }
        let int = int.trim_start_matches('0').to_string();
        let frac = frac.trim_end_matches('0').to_string();
        let negative = negative && !(int.is_empty() && frac.is_empty());
        Some(Self {
            negative,
            int,
            frac,
        })
    }

    fn cmp_magnitude(&self, other: &Self) -> Ordering {
        self.int
            .len()
            .cmp(&other.int.len())
            .then_with(|| self.int.cmp(&other.int))
            .then_with(|| self.frac.cmp(&other.frac))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => self.cmp_magnitude(other),
            (true, true) => other.cmp_magnitude(self),
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
