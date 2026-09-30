//! Shared helpers for fitting text into width-constrained widgets.
//!
//! iced has no built-in ellipsis, so long user content (file names, layer
//! names, paths, property values) would otherwise wrap and break row
//! heights or spill past fixed-width panels. `elide` trims to a character
//! budget and marks the cut with a single ellipsis.

/// Truncate `s` to at most `max` characters, replacing the overflow with a
/// trailing ellipsis. `max` includes the ellipsis character; `max == 0`
/// yields an empty string. Counts by `char` so multi-byte text is safe.
pub fn elide(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::elide;

    #[test]
    fn short_unchanged() {
        assert_eq!(elide("abc", 8), "abc");
        assert_eq!(elide("abc", 3), "abc");
    }

    #[test]
    fn long_truncated() {
        assert_eq!(elide("abcdef", 4), "abc…");
        assert_eq!(elide("abcdef", 1), "…");
        assert_eq!(elide("abcdef", 0), "");
    }

    #[test]
    fn multibyte_safe() {
        // No panic on a char boundary and the budget counts chars.
        assert_eq!(elide("çağrışım", 4), "çağ…");
    }
}
