//! Numeric conversions without the `as` operator, whose silent truncation, sign changes and wrapping hide mistakes.
//!
//! * [`SaturatingAs`]: integer to integer, clamps to the range of the target type (a length that does not fit becomes the maximum).
//! * [`WrappingAs`]: integer to integer where wrapping is the intent, for example ids that travel through the 32-bit `int` of Slint.
//! * [`Az`]: integer to float and float to float; never panics.
//! * [`FloatToInt`]: float to integer; rounds toward zero, saturates and maps NaN to 0 (exactly what `as` does), because the
//!   conversions of `az` panic on NaN.

pub use az::{Az, SaturatingAs, WrappingAs};

use az::SaturatingCast;

/// Float to integer conversion that never panics.
pub trait FloatToInt: Copy {
    /// Rounds toward zero; values outside the range of `T` become its minimum or maximum and NaN becomes 0.
    fn to_int<T: Default>(self) -> T
    where
        Self: SaturatingCast<T>;
}

macro_rules! impl_float_to_int {
    ($($float:ty),*) => {$(
        impl FloatToInt for $float {
            fn to_int<T: Default>(self) -> T
            where
                Self: SaturatingCast<T>,
            {
                if self.is_nan() { T::default() } else { self.saturating_cast() }
            }
        }
    )*};
}

impl_float_to_int!(f32, f64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_to_int_matches_the_as_operator() {
        assert_eq!(f32::NAN.to_int::<u32>(), 0);
        assert_eq!(f64::NAN.to_int::<i32>(), 0);
        assert_eq!(3.7f32.to_int::<u32>(), 3);
        assert_eq!((-3.7f64).to_int::<i32>(), -3);
        assert_eq!((-3.5f32).to_int::<u32>(), 0);
        assert_eq!(1e20f32.to_int::<u32>(), u32::MAX);
        assert_eq!(f64::NEG_INFINITY.to_int::<i32>(), i32::MIN);
    }

    #[test]
    fn integer_conversions_say_what_happens_on_overflow() {
        assert_eq!(70_000usize.saturating_as::<u16>(), u16::MAX);
        assert_eq!((-1i32).saturating_as::<usize>(), 0);
        assert_eq!((-1i32).wrapping_as::<u32>(), u32::MAX);
        assert_eq!(300u32.saturating_as::<u8>(), u8::MAX);
        assert!((16_777_217u32.az::<f32>() - 16_777_216.0).abs() < f32::EPSILON);
    }
}
