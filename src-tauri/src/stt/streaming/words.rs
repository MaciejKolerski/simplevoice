//! Word-level helpers for the LocalAgreement-2 stabilizer. These operate on
//! plain whitespace-split words (no timestamps needed): the stabilizer only
//! needs the *sequence* of words from each decode, and the audio buffer is only
//! ever cut at whole-buffer (silence) boundaries, never mid-word.

/// Whitespace-split a transcript into words.
pub fn split_words(text: &str) -> Vec<String> {
    text.split_whitespace().map(|s| s.to_string()).collect()
}

/// Normalized form used to compare words for stability: lowercased with leading
/// and trailing non-alphanumeric characters stripped (so "Test", "test," and
/// "test" all match). Interior characters (apostrophes, hyphens) are kept.
fn normalize(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// Count of leading words that match (by normalized form) across two sequences.
/// This is the core of LocalAgreement-n: the longest common word prefix.
pub fn lcp_words(a: &[String], b: &[String]) -> usize {
    let mut i = 0;
    while i < a.len() && i < b.len() && normalize(&a[i]) == normalize(&b[i]) {
        i += 1;
    }
    i
}

/// Join words with single spaces.
pub fn join_words(words: &[String]) -> String {
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_collapses_whitespace() {
        assert_eq!(split_words("  hello   world \n"), v(&["hello", "world"]));
        assert!(split_words("   ").is_empty());
    }

    #[test]
    fn lcp_is_case_and_edge_punctuation_insensitive() {
        assert_eq!(lcp_words(&v(&["Test", "trans"]), &v(&["test", "trans"])), 2);
        assert_eq!(lcp_words(&v(&["world."]), &v(&["world"])), 1);
        assert_eq!(lcp_words(&v(&["a", "b"]), &v(&["a", "c"])), 1);
        assert_eq!(lcp_words(&v(&["a"]), &v(&[])), 0);
    }

    #[test]
    fn lcp_keeps_interior_apostrophes() {
        assert_eq!(lcp_words(&v(&["don't"]), &v(&["don't"])), 1);
        assert_eq!(lcp_words(&v(&["don't"]), &v(&["dont"])), 0);
    }

    #[test]
    fn join_uses_single_spaces() {
        assert_eq!(join_words(&v(&["a", "b", "c"])), "a b c");
        assert_eq!(join_words(&[]), "");
    }
}
