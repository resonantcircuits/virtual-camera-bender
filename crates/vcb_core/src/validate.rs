//! Range checks for configuration values, producing configuration errors.

use crate::Error;

/// Requires a finite value strictly greater than zero.
pub(crate) fn positive(field: &str, value: f64) -> Result<(), Error> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(Error::config(format!(
            "{field} must be positive, got {value}"
        )))
    }
}

/// Requires a finite value greater than or equal to zero.
pub(crate) fn non_negative(field: &str, value: f64) -> Result<(), Error> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(Error::config(format!(
            "{field} must be zero or positive, got {value}"
        )))
    }
}

/// Requires a finite value.
pub(crate) fn finite(field: &str, value: f64) -> Result<(), Error> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(Error::config(format!(
            "{field} must be finite, got {value}"
        )))
    }
}

/// Requires `low <= value <= high`.
pub(crate) fn within(field: &str, value: f64, low: f64, high: f64) -> Result<(), Error> {
    if value.is_finite() && (low..=high).contains(&value) {
        Ok(())
    } else {
        Err(Error::config(format!(
            "{field} must lie in [{low}, {high}], got {value}"
        )))
    }
}

/// Requires `low < value <= high`, the usual range of an efficiency or fraction.
pub(crate) fn fraction(field: &str, value: f64) -> Result<(), Error> {
    if value.is_finite() && value > 0.0 && value <= 1.0 {
        Ok(())
    } else {
        Err(Error::config(format!(
            "{field} must lie in (0, 1], got {value}"
        )))
    }
}

/// Requires an integer count of at least `min`.
pub(crate) fn at_least(field: &str, value: u32, min: u32) -> Result<(), Error> {
    if value >= min {
        Ok(())
    } else {
        Err(Error::config(format!(
            "{field} must be at least {min}, got {value}"
        )))
    }
}
