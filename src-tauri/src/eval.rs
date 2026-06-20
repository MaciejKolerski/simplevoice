//! Offline evaluation metrics for the transcription harness (Etap 0 / H1).
//! Pure and dependency-free: no audio, no model loading. Unit-tested directly.

use serde::{Deserialize, Serialize};

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

/// Classic Levenshtein edit distance with two rolling rows (O(b) memory).
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

#[derive(Debug, Clone, Deserialize)]
pub struct EvalClip {
    pub wav: String,
    pub reference: String,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvalManifest {
    pub clips: Vec<EvalClip>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipResult {
    pub name: String,
    /// The engine's actual transcription, recorded so a run is self-documenting
    /// (you can read what came out, not only how far it was from the reference).
    pub hypothesis: String,
    /// True when the normalized hypothesis equals the normalized reference, i.e.
    /// the engine returned exactly what was recorded (word-for-word after casing
    /// and punctuation normalization).
    pub exact_match: bool,
    pub wer: f64,
    pub cer: f64,
    pub audio_secs: f64,
    pub elapsed_ms: u128,
    pub rtf: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Aggregate {
    pub clips: usize,
    pub mean_wer: f64,
    pub median_wer: f64,
    pub mean_cer: f64,
    pub median_cer: f64,
    pub median_latency_ms: f64,
    pub median_rtf: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalReport {
    pub results: Vec<ClipResult>,
    pub aggregate: Aggregate,
}

/// Scores one clip: WER/CER against the reference plus latency and real-time
/// factor (processing time / audio duration).
pub fn score_clip(
    name: &str,
    reference: &str,
    hypothesis: &str,
    audio_secs: f64,
    elapsed: std::time::Duration,
) -> ClipResult {
    let elapsed_ms = elapsed.as_millis();
    let rtf = if audio_secs > 0.0 {
        (elapsed_ms as f64 / 1000.0) / audio_secs
    } else {
        0.0
    };
    ClipResult {
        name: name.to_string(),
        hypothesis: hypothesis.to_string(),
        exact_match: normalize(reference) == normalize(hypothesis),
        wer: word_error_rate(reference, hypothesis),
        cer: char_error_rate(reference, hypothesis),
        audio_secs,
        elapsed_ms,
        rtf,
    }
}

impl EvalReport {
    pub fn from_results(results: Vec<ClipResult>) -> Self {
        let wers: Vec<f64> = results.iter().map(|r| r.wer).collect();
        let cers: Vec<f64> = results.iter().map(|r| r.cer).collect();
        let lats: Vec<f64> = results.iter().map(|r| r.elapsed_ms as f64).collect();
        let rtfs: Vec<f64> = results.iter().map(|r| r.rtf).collect();
        let aggregate = Aggregate {
            clips: results.len(),
            mean_wer: mean(&wers),
            median_wer: median(&wers),
            mean_cer: mean(&cers),
            median_cer: median(&cers),
            median_latency_ms: median(&lats),
            median_rtf: median(&rtfs),
        };
        Self { results, aggregate }
    }

    pub fn render_table(&self) -> String {
        use std::fmt::Write;
        let mut s = String::new();
        let a = &self.aggregate;
        let _ = writeln!(s, "clips:            {}", a.clips);
        let _ = writeln!(s, "WER     mean {:.3}   median {:.3}", a.mean_wer, a.median_wer);
        let _ = writeln!(s, "CER     mean {:.3}   median {:.3}", a.mean_cer, a.median_cer);
        let _ = writeln!(s, "latency median {:.0} ms", a.median_latency_ms);
        let _ = writeln!(s, "RTF     median {:.2}", a.median_rtf);
        s
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
        assert_eq!(edit_distance(b"abc", b""), 3);
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
    fn cer_empty_reference_edge_cases() {
        assert_eq!(char_error_rate("", ""), 0.0);
        assert_eq!(char_error_rate("", "extra"), 1.0);
    }

    #[test]
    fn mean_and_median() {
        assert_eq!(mean(&[1.0, 2.0, 3.0]), 2.0);
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(mean(&[]), 0.0);
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn score_clip_computes_rtf_and_metrics() {
        let r = score_clip(
            "clip1",
            "the quick brown fox",
            "the quick green fox",
            10.0,
            std::time::Duration::from_millis(5000),
        );
        assert_eq!(r.name, "clip1");
        assert_eq!(r.hypothesis, "the quick green fox");
        assert!(!r.exact_match);
        assert!((r.wer - 0.25).abs() < 1e-9);
        assert_eq!(r.elapsed_ms, 5000);
        assert!((r.rtf - 0.5).abs() < 1e-9);
    }

    #[test]
    fn score_clip_exact_match_ignores_case_and_punctuation() {
        let r = score_clip(
            "c",
            "Hello, world.",
            "hello world",
            1.0,
            std::time::Duration::from_millis(100),
        );
        assert!(r.exact_match);
        assert_eq!(r.wer, 0.0);
    }

    #[test]
    fn manifest_deserializes_with_optional_language() {
        let json = r#"{"clips":[
            {"wav":"a.wav","reference":"hello","language":"en"},
            {"wav":"b.wav","reference":"czesc"}
        ]}"#;
        let m: EvalManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.clips.len(), 2);
        assert_eq!(m.clips[0].language.as_deref(), Some("en"));
        assert_eq!(m.clips[1].language, None);
    }

    #[test]
    fn report_aggregates_and_serializes() {
        let results = vec![
            score_clip("a", "a b", "a b", 2.0, std::time::Duration::from_millis(1000)),
            score_clip("b", "a b", "a c", 2.0, std::time::Duration::from_millis(3000)),
        ];
        let report = EvalReport::from_results(results);
        assert_eq!(report.aggregate.clips, 2);
        assert!((report.aggregate.median_latency_ms - 2000.0).abs() < 1e-9);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"median_wer\""));
    }
}
