//! LocalAgreement-2 stabilizer (Macháček et al., arXiv:2307.14743).
//!
//! Fed a fresh full-buffer word hypothesis on every re-decode, it commits only
//! the words that agree across two consecutive decodes. Committed words are
//! immutable (append-only); the tentative tail may still change. This is what
//! guarantees we never emit half a word: a word is committed only after a
//! second decode (with more right-context) confirms it.

use super::words::lcp_words;

pub struct Stabilizer {
    committed: Vec<String>,
    /// The previous decode's tentative tail, re-based to the current committed
    /// offset, so consecutive decodes are compared on the same word positions.
    prev_tail: Vec<String>,
}

impl Stabilizer {
    pub fn new() -> Self {
        Self { committed: Vec::new(), prev_tail: Vec::new() }
    }

    /// Feed a fresh full-buffer word hypothesis. Newly agreed words are appended
    /// to `committed`; returns the current tentative tail (unconfirmed words).
    pub fn observe(&mut self, hyp: &[String]) -> Vec<String> {
        let start = self.committed.len().min(hyp.len());
        let tail = &hyp[start..];
        let agreed = lcp_words(tail, &self.prev_tail);
        self.committed.extend_from_slice(&tail[..agreed]);
        let tentative = tail[agreed..].to_vec();
        self.prev_tail = tentative.clone();
        tentative
    }

    /// Commit the remaining tentative tail (end of utterance / buffer reset).
    pub fn flush(&mut self) {
        let tail = std::mem::take(&mut self.prev_tail);
        self.committed.extend(tail);
    }

    pub fn committed(&self) -> &[String] {
        &self.committed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn commits_words_as_they_stabilize_across_decodes() {
        let mut s = Stabilizer::new();

        // First decode: nothing is confirmed yet (no prior decode to agree with).
        assert_eq!(s.observe(&v(&["test"])), v(&["test"]));
        assert_eq!(s.committed(), &v(&[])[..]);

        // Second decode agrees on "test" -> committed; "trans" still tentative.
        assert_eq!(s.observe(&v(&["test", "trans"])), v(&["trans"]));
        assert_eq!(s.committed(), &v(&["test"])[..]);

        // Third decode agrees on "trans" -> committed; "na" tentative.
        assert_eq!(s.observe(&v(&["test", "trans", "na"])), v(&["na"]));
        assert_eq!(s.committed(), &v(&["test", "trans"])[..]);
    }

    #[test]
    fn never_commits_a_word_that_changed_before_agreeing_twice() {
        let mut s = Stabilizer::new();
        s.observe(&v(&["a"]));
        s.observe(&v(&["a", "b"])); // commits "a", tentative "b"
        assert_eq!(s.committed(), &v(&["a"])[..]);

        // "b" turns into "x" -> not committed (disagreement).
        s.observe(&v(&["a", "x"]));
        assert_eq!(s.committed(), &v(&["a"])[..]);

        // "x" repeats -> now it agrees twice and commits.
        s.observe(&v(&["a", "x"]));
        assert_eq!(s.committed(), &v(&["a", "x"])[..]);
    }

    #[test]
    fn flush_commits_the_remaining_tentative_tail() {
        let mut s = Stabilizer::new();
        s.observe(&v(&["a"]));
        s.observe(&v(&["a", "b"])); // committed ["a"], tentative ["b"]
        s.flush();
        assert_eq!(s.committed(), &v(&["a", "b"])[..]);
    }

    #[test]
    fn shorter_hypothesis_does_not_panic_or_uncommit() {
        let mut s = Stabilizer::new();
        s.observe(&v(&["a", "b"]));
        s.observe(&v(&["a", "b"])); // committed ["a", "b"]
        assert_eq!(s.committed(), &v(&["a", "b"])[..]);
        // A later decode returns fewer words: committed stays intact.
        assert_eq!(s.observe(&v(&["a"])), v(&[]));
        assert_eq!(s.committed(), &v(&["a", "b"])[..]);
    }
}
