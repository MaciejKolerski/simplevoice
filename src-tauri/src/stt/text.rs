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

// Filler interjections per language. Conservative lists: only sounds that are not
// real words (e.g. Polish "no" / German "so" are real words and are excluded).
const FILLERS_EN: &[&str] = &["uh", "uhh", "um", "umm", "hmm", "er", "erm", "ah", "eh", "mm"];
const FILLERS_PL: &[&str] = &["yyy", "yy", "eee", "ee", "mmm", "eem", "yhy"];
const FILLERS_DE: &[&str] = &["äh", "ähm", "öh", "hm", "ähem"];

/// Removes filler interjections ("uh", "um", …) for the given language. Tokens are
/// matched by their alphanumeric, lowercased core so attached punctuation is dropped
/// with the filler ("Um, hello" -> "hello"). Unknown/`None` language uses the English
/// set (those sounds are not words in most languages). Off by default; gated by the
/// caller on the `filler_removal_enabled` setting.
pub(crate) fn remove_fillers(text: &str, lang: Option<&str>) -> String {
    let fillers: &[&str] = match lang.unwrap_or("").split('-').next().unwrap_or("") {
        "pl" => FILLERS_PL,
        "de" => FILLERS_DE,
        _ => FILLERS_EN,
    };
    let kept: Vec<&str> = text
        .split_whitespace()
        .filter(|w| {
            let core: String = w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
            !fillers.contains(&core.as_str())
        })
        .collect();
    kept.join(" ")
}

/// Capitalizes the first letter of the text and of each sentence (after `.`, `!`,
/// `?`). For ASR models that emit all-lowercase text. Conservative: only changes the
/// case of the first alphabetic character after sentence punctuation; everything else
/// is left as-is. Off by default; gated by the caller on `sentence_case_enabled`.
pub(crate) fn sentence_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cap_next = true;
    for c in text.chars() {
        if cap_next && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            cap_next = false;
        } else {
            out.push(c);
            if matches!(c, '.' | '!' | '?') {
                cap_next = true;
            }
        }
    }
    out
}

/// Voice formatting commands per language: (spoken phrase words lowercased,
/// replacement, is_break). Multi-word phrases are listed first so they match before
/// their single-word prefixes. `is_break` replacements (newline/paragraph) suppress
/// surrounding spaces; the rest are punctuation that attaches to the previous word.
fn formatting_commands(lang: Option<&str>) -> &'static [(&'static [&'static str], &'static str, bool)] {
    match lang.unwrap_or("").split('-').next().unwrap_or("") {
        "pl" => &[
            (&["nowy", "akapit"], "\n\n", true),
            (&["nowa", "linia"], "\n", true),
            (&["znak", "zapytania"], "?", false),
            (&["przecinek"], ",", false),
            (&["kropka"], ".", false),
            (&["wykrzyknik"], "!", false),
            (&["dwukropek"], ":", false),
            (&["średnik"], ";", false),
        ],
        "de" => &[
            (&["neuer", "absatz"], "\n\n", true),
            (&["neue", "zeile"], "\n", true),
            (&["komma"], ",", false),
            (&["punkt"], ".", false),
            (&["fragezeichen"], "?", false),
            (&["ausrufezeichen"], "!", false),
            (&["doppelpunkt"], ":", false),
        ],
        _ => &[
            (&["new", "paragraph"], "\n\n", true),
            (&["new", "line"], "\n", true),
            (&["question", "mark"], "?", false),
            (&["exclamation", "mark"], "!", false),
            (&["exclamation", "point"], "!", false),
            (&["full", "stop"], ".", false),
            (&["comma"], ",", false),
            (&["period"], ".", false),
            (&["colon"], ":", false),
            (&["semicolon"], ";", false),
        ],
    }
}

/// Replaces spoken formatting commands ("new line", "comma", …) with their symbols.
/// Off by default; gated by the caller on `formatting_commands_enabled`.
pub(crate) fn apply_formatting_commands(text: &str, lang: Option<&str>) -> String {
    let cmds = formatting_commands(lang);
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out = String::new();
    let mut last_break = true;
    let mut i = 0;
    while i < words.len() {
        let matched = cmds.iter().find_map(|(phrase, repl, is_break)| {
            let n = phrase.len();
            if i + n <= words.len()
                && (0..n).all(|k| {
                    words[i + k]
                        .trim_matches(|c: char| !c.is_alphanumeric())
                        .to_lowercase()
                        == phrase[k]
                })
            {
                Some((n, *repl, *is_break))
            } else {
                None
            }
        });
        match matched {
            Some((n, repl, is_break)) => {
                out.push_str(repl);
                last_break = is_break;
                i += n;
            }
            None => {
                if !last_break {
                    out.push(' ');
                }
                out.push_str(words[i]);
                last_break = false;
                i += 1;
            }
        }
    }
    out
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

    #[test]
    fn removes_english_fillers() {
        assert_eq!(remove_fillers("um hello uh world", Some("en")), "hello world");
        assert_eq!(remove_fillers("Um, hello.", Some("en")), "hello.");
        assert_eq!(remove_fillers("uh um hmm", None), "");
    }

    #[test]
    fn keeps_real_words_and_other_languages() {
        // "no" is a real Polish word and must not be treated as a filler.
        assert_eq!(remove_fillers("no i co", Some("pl")), "no i co");
        assert_eq!(remove_fillers("yyy no dobrze", Some("pl")), "no dobrze");
        assert_eq!(remove_fillers("hello world", Some("en")), "hello world");
    }

    #[test]
    fn sentence_case_capitalizes_starts() {
        assert_eq!(sentence_case("hello world. how are you?"), "Hello world. How are you?");
        assert_eq!(sentence_case("one. two! three?"), "One. Two! Three?");
    }

    #[test]
    fn sentence_case_leaves_interior_case_alone() {
        assert_eq!(sentence_case("i saw NASA today"), "I saw NASA today");
        assert_eq!(sentence_case(""), "");
    }

    #[test]
    fn formatting_punctuation_attaches_to_previous_word() {
        assert_eq!(apply_formatting_commands("hello comma world period", Some("en")), "hello, world.");
        assert_eq!(apply_formatting_commands("really question mark", Some("en")), "really?");
    }

    #[test]
    fn formatting_newline_and_multiword() {
        assert_eq!(apply_formatting_commands("line one new line line two", Some("en")), "line one\nline two");
        assert_eq!(apply_formatting_commands("pierwsza nowa linia druga", Some("pl")), "pierwsza\ndruga");
        assert_eq!(apply_formatting_commands("a przecinek b", Some("pl")), "a, b");
    }

    #[test]
    fn formatting_leaves_normal_text() {
        assert_eq!(apply_formatting_commands("just normal words", Some("en")), "just normal words");
    }
}
