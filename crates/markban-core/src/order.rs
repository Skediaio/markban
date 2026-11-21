//! Exact decimal numbers for the card `order` key.

use std::cmp::Ordering;
use std::fmt;
use thiserror::Error;

/// Decimals kept when allocating an order.
const SCALE: u32 = 6;
/// One whole unit in scaled integers (`10^SCALE`).
const UNIT: i128 = 1_000_000;
/// Most decimals an allocated order may have.
const MAX_SCALE: u32 = 18;
/// Longest integer part (in digits) that still fits scaled arithmetic.
const MAX_INT_DIGITS: usize = 30;

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

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negative {
            f.write_str("-")?;
        }
        f.write_str(if self.int.is_empty() { "0" } else { &self.int })?;
        if !self.frac.is_empty() {
            write!(f, ".{}", self.frac)?;
        }
        Ok(())
    }
}

impl Decimal {
    /// The value in millionths, or `None` when it has more than 6 decimals or is too large.
    #[must_use]
    pub fn to_scaled(&self) -> Option<i128> {
        if self.int.len() > MAX_INT_DIGITS {
            return None;
        }
        self.to_scale(SCALE)
    }

    /// The value in units of `10^-scale`, or `None` when it has more than `scale` decimals or
    /// does not fit.
    fn to_scale(&self, scale: u32) -> Option<i128> {
        if self.frac.len() > scale as usize {
            return None;
        }
        let int: i128 = if self.int.is_empty() {
            0
        } else {
            self.int.parse().ok()?
        };
        let frac: i128 = if self.frac.is_empty() {
            0
        } else {
            format!("{:0<width$}", self.frac, width = scale as usize)
                .parse()
                .ok()?
        };
        let value = int
            .checked_mul(10_i128.checked_pow(scale)?)?
            .checked_add(frac)?;
        Some(if self.negative { -value } else { value })
    }

    /// The decimal for a value in millionths, written with the fewest decimals.
    #[must_use]
    pub fn from_scaled(value: i128) -> Self {
        Self::from_scale(value, SCALE)
    }

    /// The decimal for a value in units of `10^-scale` (`scale` at most 18).
    fn from_scale(value: i128, scale: u32) -> Self {
        let scale = scale.min(MAX_SCALE);
        let magnitude = value.unsigned_abs();
        let unit = 10_u128.pow(scale);
        let int = magnitude / unit;
        let int = if int == 0 {
            String::new()
        } else {
            int.to_string()
        };
        let frac = format!("{:0width$}", magnitude % unit, width = scale as usize)
            .trim_end_matches('0')
            .to_string();
        Self {
            negative: value < 0,
            int,
            frac,
        }
    }

    /// The integer part as a signed number, `None` when it is too large to compute with.
    fn int_value(&self) -> Option<i128> {
        if self.int.len() > MAX_INT_DIGITS {
            return None;
        }
        let int: i128 = if self.int.is_empty() {
            0
        } else {
            self.int.parse().ok()?
        };
        Some(if self.negative { -int } else { int })
    }

    /// Smallest integer not below the value.
    fn ceil_int(&self) -> Option<i128> {
        let int = self.int_value()?;
        if !self.negative && !self.frac.is_empty() {
            int.checked_add(1)
        } else {
            Some(int)
        }
    }

    /// Largest integer not above the value.
    fn floor_int(&self) -> Option<i128> {
        let int = self.int_value()?;
        if self.negative && !self.frac.is_empty() {
            int.checked_sub(1)
        } else {
            Some(int)
        }
    }
}

/// No `order` can be allocated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OrderError {
    #[error(
        "no room for an order between {lo} and {hi} within {scale} decimals; position the card with `before`/`after` elsewhere in the column"
    )]
    NoRoom { lo: String, hi: String, scale: u32 },
    #[error(
        "order '{0}' has too many decimals or is too large to compute with; position the card with `before`/`after` another card, or fix that order by hand"
    )]
    Unrepresentable(String),
}

/// FNV-1a 64-bit hash, the deterministic source of the per-card offset.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Offset in whole units, in `[0, 100)`.
fn edge_offset(id: &str) -> i128 {
    i128::from(fnv1a(id) % 100)
}

/// Whole-number order `from + (1000 + offset) * direction`, where `from` is the integer
/// ceiling (towards the end) or floor (towards the top) of the extreme neighbour.
fn edge(id: &str, from: Option<&Decimal>, direction: i128) -> Result<Decimal, OrderError> {
    let Some(from) = from else {
        return Ok(Decimal::from_scaled(1000 * UNIT));
    };
    let unrepresentable = || OrderError::Unrepresentable(from.to_string());
    let base = if direction > 0 {
        from.ceil_int()
    } else {
        from.floor_int()
    }
    .ok_or_else(unrepresentable)?;
    let step = 1000 + edge_offset(id);
    base.checked_add(step * direction)
        .and_then(|whole| whole.checked_mul(UNIT))
        .map(Decimal::from_scaled)
        .ok_or_else(unrepresentable)
}

/// Order for the end of a column whose highest order is `max` (`1000` when `None`):
/// the next whole number at or above `max` plus 1000 plus an offset in `[0, 100)` derived
/// from `id`.
///
/// # Errors
/// The integer part of `max` is too large to compute with.
pub fn end(id: &str, max: Option<&Decimal>) -> Result<Decimal, OrderError> {
    edge(id, max, 1)
}

/// Order for the top of a column whose lowest order is `min` (`1000` when `None`):
/// the whole number at or below `min` minus 1000 minus the offset.
///
/// # Errors
/// The integer part of `min` is too large to compute with.
pub fn top(id: &str, min: Option<&Decimal>) -> Result<Decimal, OrderError> {
    edge(id, min, -1)
}

/// Order strictly between `lo` and `hi`: the midpoint moved by an offset derived from `id`
/// (at most a sixth of the gap either way), then written with the fewest decimals that stay
/// strictly between the neighbours. It works at one more decimal than the longest neighbour
/// (at least 6, at most 18).
///
/// # Errors
/// No value fits (equal or adjacent neighbours), or a neighbour is not representable.
pub fn between(id: &str, lo: &Decimal, hi: &Decimal) -> Result<Decimal, OrderError> {
    let longest = lo.frac.len().max(hi.frac.len());
    let scale = u32::try_from(longest.saturating_add(1))
        .unwrap_or(MAX_SCALE)
        .clamp(SCALE, MAX_SCALE);
    let no_room = || OrderError::NoRoom {
        lo: lo.to_string(),
        hi: hi.to_string(),
        scale,
    };
    let unrepresentable = |d: &Decimal| OrderError::Unrepresentable(d.to_string());
    let a = lo.to_scale(scale).ok_or_else(|| unrepresentable(lo))?;
    let b = hi.to_scale(scale).ok_or_else(|| unrepresentable(hi))?;
    let gap = b.checked_sub(a).ok_or_else(no_room)?;
    if gap < 2 {
        return Err(no_room());
    }
    let sixth = gap / 6;
    let offset = if sixth == 0 {
        0
    } else {
        i128::from(fnv1a(id)) % (2 * sixth + 1) - sixth
    };
    let target = a + gap / 2 + offset;
    for decimals in 0..=scale {
        let step = 10_i128.pow(scale - decimals);
        let floor = target.div_euclid(step) * step;
        let best = [floor, floor.saturating_add(step)]
            .into_iter()
            .filter(|c| a < *c && *c < b)
            .min_by_key(|c| (c - target).abs());
        if let Some(best) = best {
            return Ok(Decimal::from_scale(best, scale));
        }
    }
    Err(no_room())
}
