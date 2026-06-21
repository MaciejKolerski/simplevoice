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

/// Corrects transcribed words toward a user dictionary (names, brands, jargon).
/// An exact case-insensitive match is renormalized to the configured casing
/// ("chatgpt" -> "ChatGPT"); otherwise a strict fuzzy match (normalized edit
/// distance <= 0.25, length within 2, core >= 4 chars) snaps near-misses
/// ("kubernetis" -> "Kubernetes"). Reuses `eval::edit_distance` (no new dependency).
/// Off by default; the caller passes an empty slice to disable.
pub(crate) fn apply_custom_words(text: &str, custom: &[String]) -> String {
    if custom.is_empty() {
        return text.to_string();
    }
    let lowers: Vec<Vec<char>> = custom.iter().map(|c| c.to_lowercase().chars().collect()).collect();
    text.split_whitespace()
        .map(|w| {
            let core = w.trim_matches(|c: char| !c.is_alphanumeric());
            if core.is_empty() {
                return w.to_string();
            }
            let core_chars: Vec<char> = core.to_lowercase().chars().collect();
            for (i, lw) in lowers.iter().enumerate() {
                if *lw == core_chars {
                    return w.replace(core, &custom[i]);
                }
            }
            if core_chars.len() >= 4 {
                let mut best: Option<(usize, f64)> = None;
                for (i, lw) in lowers.iter().enumerate() {
                    if (core_chars.len() as i64 - lw.len() as i64).abs() > 2 {
                        continue;
                    }
                    let dist = crate::eval::edit_distance(&core_chars, lw);
                    let maxlen = core_chars.len().max(lw.len());
                    let norm = dist as f64 / maxlen as f64;
                    if best.map_or(true, |(_, b)| norm < b) {
                        best = Some((i, norm));
                    }
                }
                if let Some((i, norm)) = best {
                    if norm > 0.0 && norm <= 0.25 {
                        return w.replace(core, &custom[i]);
                    }
                }
            }
            w.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A single user dictionary rule: a spoken trigger phrase mapped to an action.
/// `value` is the replacement for `Text` rules and ignored for `Time`/`Date`.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DictionaryRule {
    pub trigger: String,
    pub action: RuleAction,
    #[serde(default)]
    pub value: Option<String>,
}

/// Dictionary action kind. Unknown values from a newer config are tolerated and
/// skipped (forward-compatible) rather than failing the whole config read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuleAction {
    Text,
    Time,
    Date,
    #[serde(other)]
    Unknown,
}

/// Lowercased alphanumeric core of a whitespace token (attached punctuation stripped).
fn token_core(token: &str) -> String {
    token.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// Replaces spoken trigger phrases with their action result: literal text, the
/// current time (`%H:%M:%S`), or the current date (`%Y-%m-%d`). Matching is
/// case-insensitive, on word boundaries, and multi-word (the longest trigger wins).
/// Single-word `Text` rules also snap near-miss typos via the same fuzzy rule the
/// former `apply_custom_words` used. `now` is injected for deterministic tests.
/// Off when `rules` is empty (the caller skips the call).
pub(crate) fn apply_dictionary_rules(
    text: &str,
    rules: &[DictionaryRule],
    now: chrono::NaiveDateTime,
) -> String {
    struct Prepared {
        cores: Vec<String>,
        replacement: String,
        is_text: bool,
    }

    let mut prepared: Vec<Prepared> = Vec::new();
    for r in rules {
        let cores: Vec<String> = r
            .trigger
            .split_whitespace()
            .map(token_core)
            .filter(|c| !c.is_empty())
            .collect();
        if cores.is_empty() {
            continue;
        }
        let replacement = match r.action {
            RuleAction::Text => match &r.value {
                Some(v) if !v.is_empty() => v.clone(),
                _ => continue,
            },
            RuleAction::Time => now.format("%H:%M:%S").to_string(),
            RuleAction::Date => now.format("%Y-%m-%d").to_string(),
            RuleAction::Unknown => continue,
        };
        prepared.push(Prepared { cores, replacement, is_text: matches!(r.action, RuleAction::Text) });
    }
    if prepared.is_empty() {
        return text.to_string();
    }
    // Longest trigger first so multi-word phrases win over their single-word prefixes.
    prepared.sort_by(|a, b| b.cores.len().cmp(&a.cores.len()));

    // Single-word Text rules are eligible for fuzzy typo-snapping.
    let fuzzy: Vec<(Vec<char>, &str)> = prepared
        .iter()
        .filter(|p| p.cores.len() == 1 && p.is_text)
        .map(|p| (p.cores[0].chars().collect(), p.replacement.as_str()))
        .collect();

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        if let Some(p) = prepared.iter().find(|p| {
            let n = p.cores.len();
            i + n <= words.len() && (0..n).all(|k| token_core(words[i + k]) == p.cores[k])
        }) {
            let n = p.cores.len();
            let lead: String = words[i].chars().take_while(|c| !c.is_alphanumeric()).collect();
            let trail: String = {
                let rev: String = words[i + n - 1].chars().rev().take_while(|c| !c.is_alphanumeric()).collect();
                rev.chars().rev().collect()
            };
            out.push(format!("{}{}{}", lead, p.replacement, trail));
            i += n;
            continue;
        }

        let core_chars: Vec<char> = token_core(words[i]).chars().collect();
        if core_chars.len() >= 4 {
            let mut best: Option<(usize, f64)> = None;
            for (idx, (fc, _)) in fuzzy.iter().enumerate() {
                if (core_chars.len() as i64 - fc.len() as i64).abs() > 2 {
                    continue;
                }
                let dist = crate::eval::edit_distance(&core_chars, fc);
                let norm = dist as f64 / core_chars.len().max(fc.len()) as f64;
                if best.map_or(true, |(_, b)| norm < b) {
                    best = Some((idx, norm));
                }
            }
            if let Some((idx, norm)) = best {
                if norm > 0.0 && norm <= 0.25 {
                    // Preserve attached punctuation, mirroring the exact-match branch.
                    let lead: String = words[i].chars().take_while(|c| !c.is_alphanumeric()).collect();
                    let trail: String = {
                        let rev: String = words[i].chars().rev().take_while(|c| !c.is_alphanumeric()).collect();
                        rev.chars().rev().collect()
                    };
                    out.push(format!("{}{}{}", lead, fuzzy[idx].1, trail));
                    i += 1;
                    continue;
                }
            }
        }

        out.push(words[i].to_string());
        i += 1;
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

    #[test]
    fn custom_words_exact_casing() {
        let cw = vec!["ChatGPT".to_string()];
        assert_eq!(apply_custom_words("i use chatgpt daily", &cw), "i use ChatGPT daily");
    }

    #[test]
    fn custom_words_fuzzy_snaps_near_misses() {
        let cw = vec!["Kubernetes".to_string()];
        assert_eq!(apply_custom_words("deploy kubernetis today", &cw), "deploy Kubernetes today");
    }

    #[test]
    fn custom_words_no_false_positives() {
        let cw = vec!["ChatGPT".to_string()];
        assert_eq!(apply_custom_words("the cat sat down", &cw), "the cat sat down");
        assert_eq!(apply_custom_words("anything", &[]), "anything");
    }

    fn rule(trigger: &str, action: RuleAction, value: Option<&str>) -> DictionaryRule {
        DictionaryRule {
            trigger: trigger.to_string(),
            action,
            value: value.map(|s| s.to_string()),
        }
    }

    fn now_fixed() -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 6, 21)
            .unwrap()
            .and_hms_opt(15, 0, 39)
            .unwrap()
    }

    #[test]
    fn dict_text_single_word_case_insensitive() {
        let r = vec![rule("chatgpt", RuleAction::Text, Some("ChatGPT"))];
        assert_eq!(apply_dictionary_rules("i use chatgpt daily", &r, now_fixed()), "i use ChatGPT daily");
    }

    #[test]
    fn dict_text_multiword_phrase() {
        let r = vec![rule("czat dżi pi ti", RuleAction::Text, Some("ChatGPT"))];
        assert_eq!(apply_dictionary_rules("powiedz czat dżi pi ti teraz", &r, now_fixed()), "powiedz ChatGPT teraz");
    }

    #[test]
    fn dict_longest_trigger_wins() {
        let r = vec![
            rule("new", RuleAction::Text, Some("NEW")),
            rule("new york", RuleAction::Text, Some("NYC")),
        ];
        assert_eq!(apply_dictionary_rules("i love new york today", &r, now_fixed()), "i love NYC today");
        assert_eq!(apply_dictionary_rules("a new day", &r, now_fixed()), "a NEW day");
    }

    #[test]
    fn dict_word_boundary_no_false_positive() {
        let r = vec![rule("cat", RuleAction::Text, Some("CAT"))];
        assert_eq!(apply_dictionary_rules("category cat", &r, now_fixed()), "category CAT");
    }

    #[test]
    fn dict_preserves_attached_punctuation() {
        let r = vec![rule("kubernetes", RuleAction::Text, Some("Kubernetes"))];
        assert_eq!(apply_dictionary_rules("deploy kubernetes, now", &r, now_fixed()), "deploy Kubernetes, now");
    }

    #[test]
    fn dict_time_and_date() {
        let rt = vec![rule("obecna godzina", RuleAction::Time, None)];
        assert_eq!(apply_dictionary_rules("teraz obecna godzina koniec", &rt, now_fixed()), "teraz 15:00:39 koniec");
        let rd = vec![rule("dzisiejsza data", RuleAction::Date, None)];
        assert_eq!(apply_dictionary_rules("dzisiejsza data", &rd, now_fixed()), "2026-06-21");
    }

    #[test]
    fn dict_fuzzy_for_single_word_text_rules() {
        let r = vec![rule("kubernetes", RuleAction::Text, Some("Kubernetes"))];
        assert_eq!(apply_dictionary_rules("deploy kubernetis today", &r, now_fixed()), "deploy Kubernetes today");
    }

    #[test]
    fn dict_no_fuzzy_for_multiword_triggers() {
        let r = vec![rule("new york", RuleAction::Text, Some("NYC"))];
        // A near-miss of a multi-word trigger must NOT match — multi-word triggers
        // are exact-only; only single-word Text rules get fuzzy snapping. The exact
        // phrase still substitutes mid-sentence.
        assert_eq!(apply_dictionary_rules("visit new yor today", &r, now_fixed()), "visit new yor today");
        assert_eq!(apply_dictionary_rules("visit new york today", &r, now_fixed()), "visit NYC today");
    }

    #[test]
    fn dict_no_fuzzy_for_single_word_time_date_rules() {
        let r = vec![rule("godzina", RuleAction::Time, None)];
        // Fuzzy snapping is Text-only; a near-miss of a Time/Date trigger stays put,
        // while the exact trigger still fires.
        assert_eq!(apply_dictionary_rules("powiedz godzin teraz", &r, now_fixed()), "powiedz godzin teraz");
        assert_eq!(apply_dictionary_rules("powiedz godzina teraz", &r, now_fixed()), "powiedz 15:00:39 teraz");
    }

    #[test]
    fn dict_empty_and_invalid_rules_passthrough() {
        assert_eq!(apply_dictionary_rules("nothing here", &[], now_fixed()), "nothing here");
        let r = vec![
            rule("x", RuleAction::Unknown, Some("Y")),
            rule("z", RuleAction::Text, None),
        ];
        assert_eq!(apply_dictionary_rules("x z stays", &r, now_fixed()), "x z stays");
    }
}
