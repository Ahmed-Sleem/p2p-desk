use std::fmt;
use std::str::FromStr;

use fastnum::D256;
use fastnum::decimal::{Context, RoundingMode};
use serde::de::{Error as DeError, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

pub const MAX_INPUT_DIGITS: usize = 38;
pub const MAX_INPUT_SCALE: u32 = 28;

const fn calculation_context() -> Context {
    Context::default()
        .without_traps()
        .with_rounding_mode(RoundingMode::HalfEven)
}

/// Exact fixed-precision decimal transported as a JSON string.
///
/// The inner value is deliberately private: domain calculations must use the
/// checked operations below and cannot enter through a binary float. D256 gives
/// 76 significant decimal digits; validated inputs are bounded to 38 digits and
/// scale 28 so multiplication retains exact headroom.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExactDecimal(D256);

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DecimalParseError {
    #[error("a decimal value is required")]
    Empty,
    #[error("use plain decimal notation with digits and at most one decimal point")]
    InvalidNotation,
    #[error("the decimal exceeds 38 significant digits or scale 28")]
    OutOfRange,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ArithmeticError {
    #[error("decimal arithmetic overflow or invalid operation")]
    Overflow,
    #[error("exact decimal operation unexpectedly lost precision")]
    Inexact,
    #[error("division by zero")]
    DivisionByZero,
    #[error("decimal scale must be between 0 and 28")]
    InvalidScale,
}

impl ExactDecimal {
    pub const ZERO: Self = Self(D256::parse_str("0", calculation_context()));
    pub const ONE: Self = Self(D256::parse_str("1", calculation_context()));
    pub const HUNDRED: Self = Self(D256::parse_str("100", calculation_context()));

    pub fn from_i64(value: i64) -> Self {
        Self(D256::from(value).with_ctx(calculation_context()))
    }

    pub fn from_u64(value: u64) -> Self {
        Self(D256::from(value).with_ctx(calculation_context()))
    }

    pub fn from_usize(value: usize) -> Result<Self, ArithmeticError> {
        let value = u64::try_from(value).map_err(|_| ArithmeticError::Overflow)?;
        Ok(Self::from_u64(value))
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    pub fn is_positive(self) -> bool {
        self.0.is_positive() && !self.0.is_zero()
    }

    pub fn is_negative(self) -> bool {
        self.0.is_negative() && !self.0.is_zero()
    }

    pub fn abs(self) -> Self {
        Self(self.0.abs())
    }

    pub fn min(self, other: Self) -> Self {
        if self <= other { self } else { other }
    }

    pub fn max(self, other: Self) -> Self {
        if self >= other { self } else { other }
    }

    pub fn scale(self) -> u32 {
        self.0.fractional_digits_count().max(0) as u32
    }

    pub fn canonical(self) -> String {
        if self.is_zero() {
            return "0".to_owned();
        }
        let reduced = self.0.reduce();
        let scale = reduced.fractional_digits_count().max(0) as usize;
        format!("{reduced:.scale$}")
    }

    pub fn checked_add(self, other: Self) -> Result<Self, ArithmeticError> {
        checked_exact_result(self.0 + other.0)
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, ArithmeticError> {
        checked_exact_result(self.0 - other.0)
    }

    pub fn checked_mul(self, other: Self) -> Result<Self, ArithmeticError> {
        checked_exact_result(self.0 * other.0)
    }

    pub fn checked_neg(self) -> Result<Self, ArithmeticError> {
        Self::ZERO.checked_sub(self)
    }

    pub fn checked_sum(values: impl IntoIterator<Item = Self>) -> Result<Self, ArithmeticError> {
        values.into_iter().try_fold(Self::ZERO, Self::checked_add)
    }

    pub fn floor_nonnegative_to_usize(self) -> Result<usize, ArithmeticError> {
        if self.is_negative() {
            return Err(ArithmeticError::Overflow);
        }
        let canonical = self.canonical();
        canonical
            .split_once('.')
            .map_or(canonical.as_str(), |(integer, _)| integer)
            .parse()
            .map_err(|_| ArithmeticError::Overflow)
    }

    /// Deterministic division at D256 precision. Repeating decimal results may
    /// raise the inexact signal and are rounded by the approved HalfEven context.
    pub fn checked_div(self, other: Self) -> Result<Self, ArithmeticError> {
        if other.is_zero() {
            return Err(ArithmeticError::DivisionByZero);
        }
        checked_finite_result(self.0 / other.0)
    }

    /// Quantize only at an explicit output boundary using midpoint-nearest-even.
    pub fn quantize(self, scale: u32) -> Result<Self, ArithmeticError> {
        if scale > MAX_INPUT_SCALE {
            return Err(ArithmeticError::InvalidScale);
        }
        checked_finite_result(
            self.0
                .with_rounding_mode(RoundingMode::HalfEven)
                .rescale(scale as i16),
        )
    }
}

impl Default for ExactDecimal {
    fn default() -> Self {
        Self::ZERO
    }
}

impl FromStr for ExactDecimal {
    type Err = DecimalParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let shape = inspect_plain_decimal(input)?;
        if shape.digits > MAX_INPUT_DIGITS || shape.scale > MAX_INPUT_SCALE {
            return Err(DecimalParseError::OutOfRange);
        }
        D256::from_str(input, calculation_context())
            .map(Self)
            .map_err(|_| DecimalParseError::OutOfRange)
    }
}

impl fmt::Display for ExactDecimal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.canonical())
    }
}

impl Serialize for ExactDecimal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.canonical())
    }
}

impl<'de> Deserialize<'de> for ExactDecimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ExactDecimalVisitor;

        impl Visitor<'_> for ExactDecimalVisitor {
            type Value = ExactDecimal;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a plain decimal encoded as a JSON string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: DeError,
            {
                value.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(ExactDecimalVisitor)
    }
}

#[derive(Clone, Copy)]
struct DecimalShape {
    digits: usize,
    scale: u32,
}

fn inspect_plain_decimal(input: &str) -> Result<DecimalShape, DecimalParseError> {
    if input.is_empty() {
        return Err(DecimalParseError::Empty);
    }
    let unsigned = input.strip_prefix('-').unwrap_or(input);
    if unsigned.is_empty() {
        return Err(DecimalParseError::InvalidNotation);
    }

    let mut decimal_index = None;
    let mut digits = 0_usize;
    for (index, byte) in unsigned.bytes().enumerate() {
        match byte {
            b'0'..=b'9' => digits += 1,
            b'.' if index > 0 && decimal_index.is_none() && index + 1 < unsigned.len() => {
                decimal_index = Some(index);
            }
            _ => return Err(DecimalParseError::InvalidNotation),
        }
    }
    let scale = decimal_index
        .map(|index| (unsigned.len() - index - 1) as u32)
        .unwrap_or(0);
    Ok(DecimalShape { digits, scale })
}

fn checked_exact_result(value: D256) -> Result<ExactDecimal, ArithmeticError> {
    if !value.is_finite()
        || value.is_op_overflow()
        || value.is_op_underflow()
        || value.is_op_invalid()
        || value.is_op_div_by_zero()
    {
        return Err(ArithmeticError::Overflow);
    }
    if value.is_op_inexact() {
        return Err(ArithmeticError::Inexact);
    }
    checked_finite_result(value)
}

fn checked_finite_result(value: D256) -> Result<ExactDecimal, ArithmeticError> {
    if !value.is_finite()
        || value.is_op_overflow()
        || value.is_op_underflow()
        || value.is_op_invalid()
        || value.is_op_div_by_zero()
    {
        return Err(ArithmeticError::Overflow);
    }

    // fastnum operation signals are sticky and propagate to later operands.
    // Validate them above, finish any extra-precision rounding, then rebuild the
    // same finite coefficient/exponent with a clean approved context.
    let rounded = value.with_ctx(calculation_context());
    let clean = D256::from_parts(
        rounded.digits(),
        -(rounded.fractional_digits_count() as i32),
        rounded.sign(),
        calculation_context(),
    );
    Ok(ExactDecimal(clean))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_decimal_and_canonicalizes_without_float() {
        let value: ExactDecimal = "0010.5000".parse().expect("valid decimal");
        assert_eq!(value.canonical(), "10.5");
        assert_eq!(value.scale(), 4);
    }

    #[test]
    fn rejects_exponents_whitespace_plus_and_incomplete_values() {
        for input in [
            "", " 1", "1 ", "+1", "1e2", ".5", "1.", "--1", "NaN", "Infinity",
        ] {
            assert!(input.parse::<ExactDecimal>().is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn rejects_input_beyond_digit_and_scale_limits() {
        assert!(
            "123456789012345678901234567890123456789"
                .parse::<ExactDecimal>()
                .is_err()
        );
        assert!(
            "0.12345678901234567890123456789"
                .parse::<ExactDecimal>()
                .is_err()
        );
    }

    #[test]
    fn serializes_canonically_as_a_json_string_only() {
        let value: ExactDecimal = "10.50".parse().expect("valid decimal");
        assert_eq!(
            serde_json::to_string(&value).expect("serialize"),
            "\"10.5\""
        );
        let round_trip: ExactDecimal = serde_json::from_str("\"10.50\"").expect("deserialize");
        assert_eq!(round_trip, value);
        assert!(serde_json::from_str::<ExactDecimal>("10.50").is_err());
    }

    #[test]
    fn uses_explicit_midpoint_nearest_even_quantization() {
        let down: ExactDecimal = "6.5".parse().expect("valid");
        let up: ExactDecimal = "7.5".parse().expect("valid");
        assert_eq!(down.quantize(0).expect("quantize").canonical(), "6");
        assert_eq!(up.quantize(0).expect("quantize").canonical(), "8");
    }

    #[test]
    fn checked_division_fails_closed_on_zero() {
        assert_eq!(
            ExactDecimal::ONE.checked_div(ExactDecimal::ZERO),
            Err(ArithmeticError::DivisionByZero)
        );
    }

    #[test]
    fn multiplication_retains_exact_38_by_38_digit_headroom() {
        let left: ExactDecimal = "9999999999999999999.9999999999999999999"
            .parse()
            .expect("valid");
        let result = left.checked_mul(left).expect("76-digit exact product");
        assert!(!result.canonical().is_empty());
    }

    #[test]
    fn precision_loss_invalid_scale_and_negative_zero_fail_or_canonicalize_deterministically() {
        let maximum_input: ExactDecimal = "99999999999999999999999999999999999999"
            .parse()
            .expect("38 digits");
        let product = maximum_input
            .checked_mul(maximum_input)
            .expect("76-digit product");
        assert_eq!(
            product.checked_mul(maximum_input),
            Err(ArithmeticError::Inexact)
        );
        assert_eq!(
            ExactDecimal::ONE.quantize(29),
            Err(ArithmeticError::InvalidScale)
        );
        assert_eq!(
            "-0.000".parse::<ExactDecimal>().expect("zero").canonical(),
            "0"
        );
    }

    #[test]
    fn repeating_division_is_half_even_and_leaves_a_reusable_clean_value() {
        let third = ExactDecimal::ONE
            .checked_div(ExactDecimal::from_i64(3))
            .expect("finite repeating quotient");
        let rounded = third.quantize(2).expect("boundary quantization");
        assert_eq!(rounded.canonical(), "0.33");
        assert_eq!(
            rounded
                .checked_add(ExactDecimal::ONE)
                .expect("signals must not leak")
                .canonical(),
            "1.33"
        );
    }

    proptest::proptest! {
        #[test]
        fn checked_integer_addition_is_commutative(left in -1_000_000_i64..1_000_000, right in -1_000_000_i64..1_000_000) {
            let left = ExactDecimal::from_i64(left);
            let right = ExactDecimal::from_i64(right);
            proptest::prop_assert_eq!(left.checked_add(right), right.checked_add(left));
        }

        #[test]
        fn canonical_string_json_round_trip_is_lossless(value in -1_000_000_i64..1_000_000) {
            let original = ExactDecimal::from_i64(value);
            let json = serde_json::to_string(&original).expect("serialize exact decimal");
            let restored: ExactDecimal = serde_json::from_str(&json).expect("deserialize exact decimal");
            proptest::prop_assert_eq!(restored, original);
        }
    }
}
