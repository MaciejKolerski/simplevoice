//! Word-level helpers for the LocalAgreement-2 stabilizer. These operate on
//! plain whitespace-split words (no timestamps needed): the stabilizer only
//! needs the *sequence* of words from each decode, and the audio buffer is only
//! ever cut at whole-buffer (silence) boundaries, never mid-word.

/// True for scripts written without spaces between words (CJK, Hangul, Thai),
/// where the agreement unit must be a single character rather than a whole word.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF |   // Hiragana + Katakana
        0x3400..=0x4DBF |   // CJK Extension A
        0x4E00..=0x9FFF |   // CJK Unified Ideographs
        0xAC00..=0xD7AF |   // Hangul syllables
        0x0E00..=0x0E7F)    // Thai
}

/// Split a transcript into agreement units. Whitespace-separated runs of
/// non-CJK text are words (as before); each CJK character is its own unit, so
/// LocalAgreement can commit space-less scripts character-by-character instead of
/// degrading to one giant token (G7).
pub fn split_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if is_cjk(c) {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(c.to_string());
        } else if c.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
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

/// Join units back into text. Latin words get single spaces; consecutive CJK
/// characters are joined with no space, matching how those scripts are written.
pub fn join_words(words: &[String]) -> String {
    let mut out = String::new();
    for w in words {
        if !out.is_empty() {
            let prev_cjk = out.chars().last().map_or(false, is_cjk);
            let next_cjk = w.chars().next().map_or(false, is_cjk);
            if !(prev_cjk && next_cjk) {
                out.push(' ');
            }
        }
        out.push_str(w);
    }
    out
}

/// Append `words` onto an already-joined `prefix`, applying the exact same
/// boundary rule as [`join_words`] between the last char of `prefix` and the
/// first unit. Lets a caller keep a running joined string in O(appended) work
/// instead of re-cloning and re-joining the whole history on every update (G1):
/// `join_onto(&join_words(a), b) == join_words(&[a, b].concat())`.
pub fn join_onto(prefix: &str, words: &[String]) -> String {
    let tail = join_words(words);
    if prefix.is_empty() {
        return tail;
    }
    if tail.is_empty() {
        return prefix.to_string();
    }
    let prev_cjk = prefix.chars().last().map_or(false, is_cjk);
    let next_cjk = tail.chars().next().map_or(false, is_cjk);
    let mut out = String::with_capacity(prefix.len() + 1 + tail.len());
    out.push_str(prefix);
    if !(prev_cjk && next_cjk) {
        out.push(' ');
    }
    out.push_str(&tail);
    out
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

    #[test]
    fn split_breaks_cjk_into_characters() {
        assert_eq!(split_words("你好世界"), v(&["你", "好", "世", "界"]));
        assert_eq!(split_words("你好 world"), v(&["你", "好", "world"]));
    }

    #[test]
    fn join_omits_spaces_between_cjk() {
        assert_eq!(join_words(&v(&["你", "好"])), "你好");
        assert_eq!(join_words(&v(&["你", "world"])), "你 world");
        assert_eq!(join_words(&v(&["hello", "world"])), "hello world");
    }

    #[test]
    fn join_onto_matches_full_join() {
        // join_onto(join_words(a), b) must equal join_words(a ++ b) for every
        // boundary case the streaming cache relies on.
        let cases: &[(&[&str], &[&str])] = &[
            (&["alpha", "beta"], &["gamma", "delta"]),
            (&[], &["a", "b"]),
            (&["a", "b"], &[]),
            (&["你", "好"], &["世", "界"]),   // CJK | CJK -> no space at seam
            (&["hello"], &["你"]),            // latin | CJK -> space
            (&["你"], &["world"]),            // CJK | latin -> space
        ];
        for (a, b) in cases {
            let a = v(a);
            let b = v(b);
            let mut all = a.clone();
            all.extend(b.clone());
            assert_eq!(join_onto(&join_words(&a), &b), join_words(&all), "a={a:?} b={b:?}");
        }
    }
}
