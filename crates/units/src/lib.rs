//! Integer nanometre coordinates for every Redrob Board tool.
//!
//! Floating point is banned from geometry: two tools rounding differently would disagree on
//! whether a track touches a pad. One nanometre resolves anything a PCB factory can make and
//! leaves `i64` with room for a board 9 000 km wide.

#![forbid(unsafe_code)]

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

use serde::{Deserialize, Serialize};

/// A length in nanometres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Nm(pub i64);

impl Nm {
    pub const ZERO: Nm = Nm(0);
    pub const NM_PER_MM: i64 = 1_000_000;
    pub const NM_PER_MIL: i64 = 25_400;

    /// Exact millimetres.
    pub const fn mm(v: i64) -> Nm {
        Nm(v * Self::NM_PER_MM)
    }

    /// Exact micrometres.
    pub const fn um(v: i64) -> Nm {
        Nm(v * 1_000)
    }

    /// Exact mils (thousandths of an inch).
    pub const fn mil(v: i64) -> Nm {
        Nm(v * Self::NM_PER_MIL)
    }

    /// Millimetres given as a decimal, rounded to the nearest nanometre. The only place a float is
    /// allowed in: reading human input such as `0.254`.
    pub fn from_mm_f64(v: f64) -> Nm {
        Nm((v * Self::NM_PER_MM as f64).round() as i64)
    }

    /// Millimetres as a decimal, for display and for formats that want them.
    pub fn to_mm_f64(self) -> f64 {
        self.0 as f64 / Self::NM_PER_MM as f64
    }

    pub const fn abs(self) -> Nm {
        Nm(self.0.abs())
    }

    pub fn min(self, other: Nm) -> Nm {
        Nm(self.0.min(other.0))
    }

    pub fn max(self, other: Nm) -> Nm {
        Nm(self.0.max(other.0))
    }

    /// Squared length as `i128`, so distances can be compared without a square root.
    pub const fn squared(self) -> i128 {
        (self.0 as i128) * (self.0 as i128)
    }
}

impl Add for Nm {
    type Output = Nm;
    fn add(self, rhs: Nm) -> Nm {
        Nm(self.0 + rhs.0)
    }
}

impl Sub for Nm {
    type Output = Nm;
    fn sub(self, rhs: Nm) -> Nm {
        Nm(self.0 - rhs.0)
    }
}

impl Neg for Nm {
    type Output = Nm;
    fn neg(self) -> Nm {
        Nm(-self.0)
    }
}

impl Mul<i64> for Nm {
    type Output = Nm;
    fn mul(self, rhs: i64) -> Nm {
        Nm(self.0 * rhs)
    }
}

impl Div<i64> for Nm {
    type Output = Nm;
    fn div(self, rhs: i64) -> Nm {
        Nm(self.0 / rhs)
    }
}

impl fmt::Display for Nm {
    /// Millimetres with up to six decimals, trailing zeros trimmed: `1.5mm`, `0.254mm`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let whole = abs / Self::NM_PER_MM as u64;
        let frac = abs % Self::NM_PER_MM as u64;
        if frac == 0 {
            write!(f, "{sign}{whole}mm")
        } else {
            let frac = format!("{frac:06}");
            write!(f, "{sign}{whole}.{}mm", frac.trim_end_matches('0'))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_are_exact() {
        assert_eq!(Nm::mm(1), Nm(1_000_000));
        assert_eq!(Nm::mil(10), Nm(254_000));
        assert_eq!(Nm::um(5), Nm(5_000));
        assert_eq!(Nm::from_mm_f64(0.254), Nm::mil(10));
        assert_eq!(Nm::from_mm_f64(-1.5), Nm(-1_500_000));
    }

    #[test]
    fn display_trims() {
        assert_eq!(Nm::mm(2).to_string(), "2mm");
        assert_eq!(Nm::mil(10).to_string(), "0.254mm");
        assert_eq!(Nm(-1_500_000).to_string(), "-1.5mm");
        assert_eq!(Nm(1).to_string(), "0.000001mm");
    }

    #[test]
    fn arithmetic() {
        assert_eq!(Nm::mm(3) - Nm::mm(1), Nm::mm(2));
        assert_eq!(Nm::mm(3) * 2, Nm::mm(6));
        assert_eq!(Nm::mm(3) / 3, Nm::mm(1));
        assert_eq!((-Nm::mm(1)).abs(), Nm::mm(1));
        assert_eq!(Nm::mm(2).squared(), 4_000_000_000_000);
    }
}
