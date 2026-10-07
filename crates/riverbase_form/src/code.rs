//! Immutable key validation (`CCC-NNNN` = 3 letters + hyphen + 4 digits).

use crate::result::FormResult;

/// Expected length of a valid key (`ABC-1234`).
pub const KEY_LEN: usize = 8;

/// JSON Schema pattern for keys.
pub const KEY_PATTERN: &str = r"^[A-Za-z]{3}-\d{4}$";

/// Validate that `key` matches `CCC-NNNN` (3 letters, hyphen, 4 digits).
pub fn validate_key(key: &str) -> FormResult<()> {
    if key.len() != KEY_LEN {
        return Err(crate::errors::FRM_001.with_data(format!(
            "expected {KEY_LEN} chars (CCC-NNNN), got {}",
            key.len()
        )));
    }

    let bytes = key.as_bytes();
    for byte in &bytes[..3] {
        if !byte.is_ascii_alphabetic() {
            return Err(crate::errors::FRM_002
                .with_data(format!("positions 1-3 must be letters, got '{key}'")));
        }
    }
    if bytes[3] != b'-' {
        return Err(
            crate::errors::FRM_003.with_data(format!("position 4 must be '-', got '{key}'"))
        );
    }
    for byte in &bytes[4..8] {
        if !byte.is_ascii_digit() {
            return Err(crate::errors::FRM_004
                .with_data(format!("positions 5-8 must be digits, got '{key}'")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_key() {
        validate_key("ABC-1234").unwrap();
        validate_key("txt-0001").unwrap();
    }

    #[test]
    fn rejects_bad_key() {
        assert!(validate_key("AB-1234").is_err());
        assert!(validate_key("ABC1234").is_err());
        assert!(validate_key("123-4567").is_err());
    }
}
