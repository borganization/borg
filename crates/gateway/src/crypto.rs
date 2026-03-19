/// Constant-time byte comparison to prevent timing attacks.
/// Uses the `subtle` crate which handles differing lengths safely.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_bytes() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn different_length() {
        assert!(!constant_time_eq(b"hello", b"hi"));
    }

    #[test]
    fn different_content() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn empty_slices() {
        assert!(constant_time_eq(b"", b""));
    }
}
