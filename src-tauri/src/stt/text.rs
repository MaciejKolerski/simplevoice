//! Text post-processing applied above every engine. This module is the home of the
//! post-processing chain (D-series of TRANSCRIPTION_IMPROVEMENTS.md). It starts with
//! repetition collapse (D2-core); filler-word removal, custom-word correction, OpenCC
//! and formatting commands are added here as those items land.

/// Collapses a run of 3+ consecutive identical words (ASCII-case-insensitive) down to
/// a single occurrence. Targets Whisper looping artifacts ("the the the the" -> "the")
/// while leaving ordinary repetition alone (a pair like "very very" is kept).
pub(crate) fn collapse_repeats(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let mut j = i + 1;
        while j < words.len() && words[j].eq_ignore_ascii_case(words[i]) {
            j += 1;
        }
        if j - i >= 3 {
            out.push(words[i]);
        } else {
            out.extend_from_slice(&words[i..j]);
        }
        i = j;
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_three_or_more_repeats() {
        assert_eq!(collapse_repeats("the the the the end"), "the end");
        assert_eq!(collapse_repeats("go go go"), "go");
    }

    #[test]
    fn keeps_pairs_and_singles() {
        assert_eq!(collapse_repeats("very very good"), "very very good");
        assert_eq!(collapse_repeats("hello world"), "hello world");
    }

    #[test]
    fn is_ascii_case_insensitive() {
        assert_eq!(collapse_repeats("No no no thanks"), "No thanks");
    }

    #[test]
    fn handles_empty_and_extra_whitespace() {
        assert_eq!(collapse_repeats(""), "");
        assert_eq!(collapse_repeats("  a   a   a  "), "a");
    }

    #[test]
    fn collapses_only_the_repeated_run() {
        assert_eq!(collapse_repeats("a a a b c c c d"), "a b c d");
    }
}
