//! Offline evaluation metrics for the transcription harness (Etap 0 / H1).
//! Pure and dependency-free: no audio, no model loading. Unit-tested directly.

/// Lowercases (Unicode-aware), drops punctuation (keeping intra-word apostrophes
/// and hyphens), collapses whitespace, and splits into word tokens. Pure-punctuation
/// tokens are dropped so a stray "-" never counts as a word.
pub fn normalize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|t| t.chars().any(|c| c.is_alphanumeric()))
        .map(|t| t.to_string())
        .collect()
}

/// Classic Levenshtein edit distance with two rolling rows (O(min) memory).
/// Generic so the same routine scores word slices and char slices.
pub fn edit_distance<T: Eq>(a: &[T], b: &[T]) -> usize {
    let n = b.len();
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];
    for (i, ai) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, bj) in b.iter().enumerate() {
            let cost = if ai == bj { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Word Error Rate = word edits / reference word count. Empty reference yields
/// 0.0 against empty hypothesis, 1.0 otherwise.
pub fn word_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let r = normalize(reference);
    let h = normalize(hypothesis);
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    edit_distance(&r, &h) as f64 / r.len() as f64
}

/// Character Error Rate over the normalized, space-joined text.
pub fn char_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let r: Vec<char> = normalize(reference).join(" ").chars().collect();
    let h: Vec<char> = normalize(hypothesis).join(" ").chars().collect();
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    edit_distance(&r, &h) as f64 / r.len() as f64
}

pub fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

pub fn median(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lowercases_strips_punctuation_and_collapses_space() {
        assert_eq!(normalize("Hello,  World!"), vec!["hello", "world"]);
        assert_eq!(normalize("  spaced   out  "), vec!["spaced", "out"]);
    }

    #[test]
    fn normalize_keeps_intra_word_apostrophe_and_hyphen() {
        assert_eq!(normalize("don't stop"), vec!["don't", "stop"]);
        assert_eq!(normalize("state-of-the-art"), vec!["state-of-the-art"]);
    }

    #[test]
    fn normalize_drops_standalone_punctuation_tokens() {
        assert_eq!(normalize("a - b"), vec!["a", "b"]);
    }

    #[test]
    fn normalize_preserves_unicode_diacritics_lowercased() {
        assert_eq!(normalize("Łódź ÖL"), vec!["łódź", "öl"]);
    }

    #[test]
    fn edit_distance_basic_cases() {
        assert_eq!(edit_distance::<u8>(&[], &[]), 0);
        assert_eq!(edit_distance(b"abc", b"abc"), 0);
        assert_eq!(edit_distance(b"abc", b"abd"), 1); // substitution
        assert_eq!(edit_distance(b"abc", b"abcd"), 1); // insertion
        assert_eq!(edit_distance(b"abc", b"ab"), 1); // deletion
        assert_eq!(edit_distance(b"", b"abc"), 3);
    }

    #[test]
    fn wer_is_edits_over_reference_words() {
        // 4 reference words, one substituted -> 0.25
        assert!((word_error_rate("the quick brown fox", "the quick green fox") - 0.25).abs() < 1e-9);
        assert_eq!(word_error_rate("same words here", "same words here"), 0.0);
    }

    #[test]
    fn wer_empty_reference_edge_cases() {
        assert_eq!(word_error_rate("", ""), 0.0);
        assert_eq!(word_error_rate("", "extra"), 1.0);
    }

    #[test]
    fn cer_counts_character_edits() {
        // "kitten" vs "sitting": 3 char edits over 6 reference chars.
        assert!((char_error_rate("kitten", "sitting") - 0.5).abs() < 1e-9);
    }

    #[test]
    fn mean_and_median() {
        assert_eq!(mean(&[1.0, 2.0, 3.0]), 2.0);
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(mean(&[]), 0.0);
        assert_eq!(median(&[]), 0.0);
    }
}
