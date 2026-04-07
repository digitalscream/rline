//! Type-safe position wrappers for document coordinates.

/// A zero-based line index within a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineIndex(pub usize);

/// A zero-based character offset within a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CharOffset(pub usize);

/// A zero-based byte offset within a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteOffset(pub usize);

impl std::fmt::Display for LineIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for CharOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for ByteOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<usize> for LineIndex {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl From<usize> for CharOffset {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl From<usize> for ByteOffset {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- LineIndex ---

    #[test]
    fn test_line_index_display_formats_inner_value() {
        let idx = LineIndex(42);
        assert_eq!(
            format!("{idx}"),
            "42",
            "Display should format the inner usize"
        );
    }

    #[test]
    fn test_line_index_from_usize() {
        let idx = LineIndex::from(7);
        assert_eq!(idx.0, 7, "From<usize> should set the inner value");
    }

    #[test]
    fn test_line_index_eq_same_value() {
        assert_eq!(
            LineIndex(3),
            LineIndex(3),
            "equal inner values should be equal"
        );
    }

    #[test]
    fn test_line_index_eq_different_value() {
        assert_ne!(
            LineIndex(1),
            LineIndex(2),
            "different inner values should not be equal"
        );
    }

    #[test]
    fn test_line_index_ord_less() {
        assert!(
            LineIndex(0) < LineIndex(1),
            "smaller inner value should be less"
        );
    }

    #[test]
    fn test_line_index_ord_greater() {
        assert!(
            LineIndex(5) > LineIndex(3),
            "larger inner value should be greater"
        );
    }

    // --- CharOffset ---

    #[test]
    fn test_char_offset_display_formats_inner_value() {
        let off = CharOffset(10);
        assert_eq!(
            format!("{off}"),
            "10",
            "Display should format the inner usize"
        );
    }

    #[test]
    fn test_char_offset_from_usize() {
        let off = CharOffset::from(99);
        assert_eq!(off.0, 99, "From<usize> should set the inner value");
    }

    #[test]
    fn test_char_offset_eq() {
        assert_eq!(
            CharOffset(5),
            CharOffset(5),
            "equal inner values should be equal"
        );
        assert_ne!(
            CharOffset(5),
            CharOffset(6),
            "different inner values should not be equal"
        );
    }

    #[test]
    fn test_char_offset_ord() {
        assert!(
            CharOffset(1) < CharOffset(2),
            "smaller value should be less"
        );
        assert!(
            CharOffset(10) > CharOffset(0),
            "larger value should be greater"
        );
    }

    // --- ByteOffset ---

    #[test]
    fn test_byte_offset_display_formats_inner_value() {
        let off = ByteOffset(255);
        assert_eq!(
            format!("{off}"),
            "255",
            "Display should format the inner usize"
        );
    }

    #[test]
    fn test_byte_offset_from_usize() {
        let off = ByteOffset::from(128);
        assert_eq!(off.0, 128, "From<usize> should set the inner value");
    }

    #[test]
    fn test_byte_offset_eq() {
        assert_eq!(ByteOffset(0), ByteOffset(0), "zero offsets should be equal");
        assert_ne!(
            ByteOffset(0),
            ByteOffset(1),
            "different offsets should not be equal"
        );
    }

    #[test]
    fn test_byte_offset_ord() {
        assert!(
            ByteOffset(100) < ByteOffset(200),
            "smaller offset should be less"
        );
        assert!(
            ByteOffset(200) > ByteOffset(100),
            "larger offset should be greater"
        );
    }

    #[test]
    fn test_display_zero_value() {
        assert_eq!(format!("{}", LineIndex(0)), "0", "zero should display as 0");
        assert_eq!(
            format!("{}", CharOffset(0)),
            "0",
            "zero should display as 0"
        );
        assert_eq!(
            format!("{}", ByteOffset(0)),
            "0",
            "zero should display as 0"
        );
    }
}
