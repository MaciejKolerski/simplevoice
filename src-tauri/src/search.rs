//! Voice search commands: turn an utterance that opens with the wake-word prefix
//! and a site keyword ("hey google …") into a browser search on that site, with
//! the rest of the utterance as the query. The Dictionary view edits these;
//! `transcribe_audio` matches on the raw transcription and, on a hit, opens the
//! built URL instead of typing the text.
//!
//! This module is pure (no Tauri): `match_search_command` decides whether an
//! utterance is a command and builds the URL; the config read and the actual
//! browser open live in `lib.rs`.

/// A single voice-search command. `triggers` are the site keyword(s) spoken after
/// the global wake-word prefix (e.g. "google" for "hey google"); `url` is a
/// template whose `%s` (or `{query}` / `{q}`) placeholder is replaced with the
/// percent-encoded query. Built-ins ship enabled; `builtin` only marks origin for
/// the UI (all fields stay user-editable).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchCommand {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    pub url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub builtin: bool,
}

fn default_true() -> bool {
    true
}

/// Built-in commands, enabled by default so voice search works out of the box
/// (even before the Dictionary tab is opened and the config is seeded). The
/// frontend seeds the same list via the `get_default_search_commands` command, so
/// this is the single source of truth.
///
/// Triggers are the site *keyword* only — the spoken wake word is the global,
/// user-configurable prefix (`search_command_prefix`, default "hey"), so the
/// effective phrase is "{prefix} {keyword}" ("hey google"). Keywords are the
/// plain brand names, so they read the same in every language; a couple of
/// commands add a natural spacing/word variant ("duck duck go", "chat gpt").
/// Users add their own aliases per command in the Dictionary view.
///
/// Deep-link notes: Google/Bing/DuckDuckGo/YouTube use documented search params.
/// ChatGPT (`?q=`) and Grok (`?q=`) prefill and submit the prompt. Claude
/// (`/new?q=`) prefills a new chat. Gemini has no native URL prompt param, so the
/// query is best-effort — the link still opens Gemini.
pub fn default_search_commands() -> Vec<SearchCommand> {
    fn cmd(id: &str, name: &str, triggers: &[&str], url: &str) -> SearchCommand {
        SearchCommand {
            id: id.to_string(),
            name: name.to_string(),
            triggers: triggers.iter().map(|s| s.to_string()).collect(),
            url: url.to_string(),
            enabled: true,
            builtin: true,
        }
    }
    vec![
        cmd("google", "Google", &["google"], "https://www.google.com/search?q=%s"),
        cmd("youtube", "YouTube", &["youtube"], "https://www.youtube.com/results?search_query=%s"),
        cmd("bing", "Bing", &["bing"], "https://www.bing.com/search?q=%s"),
        cmd("duckduckgo", "DuckDuckGo", &["duckduckgo", "duck duck go"], "https://duckduckgo.com/?q=%s"),
        cmd("chatgpt", "ChatGPT", &["chatgpt", "chat gpt"], "https://chatgpt.com/?q=%s"),
        cmd("claude", "Claude", &["claude"], "https://claude.ai/new?q=%s"),
        cmd("gemini", "Gemini", &["gemini"], "https://gemini.google.com/app?q=%s"),
        cmd("grok", "Grok", &["grok"], "https://grok.com/?q=%s"),
    ]
}

/// Lowercased alphanumeric core of a whitespace token (attached punctuation
/// stripped). Mirrors `stt::text::token_core`; kept local so this module stays
/// self-contained and independently testable.
fn token_core(token: &str) -> String {
    token
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

/// Percent-encodes a query for a URL query component (RFC 3986): unreserved bytes
/// pass through, everything else (spaces, punctuation, and every byte of non-ASCII
/// UTF-8 such as Polish "ł"/"ę") becomes `%XX`. Encoding raw UTF-8 bytes keeps
/// multibyte characters correct.
pub fn percent_encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => {
                out.push('%');
                out.push(char::from_digit((b >> 4) as u32, 16).unwrap().to_ascii_uppercase());
                out.push(char::from_digit((b & 0xf) as u32, 16).unwrap().to_ascii_uppercase());
            }
        }
    }
    out
}

/// Fills a URL template's placeholder with the percent-encoded query. Accepts the
/// browser-familiar `%s` as well as `{query}` / `{q}`. A template without any
/// placeholder is returned unchanged (the site opens without a query).
pub fn build_search_url(template: &str, query: &str) -> String {
    let enc = percent_encode_query(query);
    template
        .replace("%s", &enc)
        .replace("{query}", &enc)
        .replace("{q}", &enc)
}

/// Legacy/redundant wake words dropped from the front of a keyword. Lets a stored
/// full-phrase trigger ("hej google") from before the prefix was global — or a
/// keyword into which the user redundantly re-typed the wake word — still resolve
/// to the bare site keyword. Covers the historical defaults ("hej"/"hey") and the
/// currently configured prefix's first word.
fn is_wake_word(core: &str, prefix_first: Option<&str>) -> bool {
    core == "hej" || core == "hey" || prefix_first == Some(core)
}

/// If `text` opens with the wake-word `prefix` followed by one of the commands'
/// keyword triggers, returns the URL to open (site + the rest of the utterance as
/// the query); otherwise `None`. Matching is anchored at the start of the utterance
/// (a command replaces the whole dictation, so a mid-sentence "hey google" is left
/// as ordinary text), case-insensitive on word cores, and multi-word — the longest
/// matching keyword wins so "duck duck go" beats a hypothetical "duck". An empty
/// `prefix` means no wake word is required (keyword-only, higher false-positive
/// risk — the user's choice). `commands` should already be filtered to enabled
/// entries by the caller.
pub fn match_search_command(text: &str, prefix: &str, commands: &[SearchCommand]) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let cores: Vec<String> = words.iter().map(|w| token_core(w)).collect();

    // The utterance must open with the prefix tokens (if any are configured).
    let prefix_cores: Vec<String> = prefix
        .split_whitespace()
        .map(token_core)
        .filter(|c| !c.is_empty())
        .collect();
    let p = prefix_cores.len();
    if p > cores.len() || !(0..p).all(|k| cores[k] == prefix_cores[k]) {
        return None;
    }
    let prefix_first = prefix_cores.first().map(|s| s.as_str());

    let mut best: Option<(usize, &SearchCommand)> = None;
    for cmd in commands {
        if cmd.url.trim().is_empty() {
            continue;
        }
        for trigger in &cmd.triggers {
            let mut kcores: Vec<String> = trigger
                .split_whitespace()
                .map(token_core)
                .filter(|c| !c.is_empty())
                .collect();
            // Drop a leading wake word so legacy full-phrase triggers and redundant
            // keywords still resolve (keeping at least one token as the keyword).
            if kcores.len() > 1 && is_wake_word(&kcores[0], prefix_first) {
                kcores.remove(0);
            }
            if kcores.is_empty() || p + kcores.len() > cores.len() {
                continue;
            }
            if (0..kcores.len()).all(|k| cores[p + k] == kcores[k]) {
                let n = kcores.len();
                // Longest keyword wins; ties keep the first command in list order.
                if best.map_or(true, |(bn, _)| n > bn) {
                    best = Some((n, cmd));
                }
            }
        }
    }

    let (n, cmd) = best?;
    // The query is the original words after the prefix + keyword, so spelling and
    // casing are preserved. Leading punctuation that survived the split
    // ("hey google - co…") is trimmed; encoding handles the rest.
    let query = words[p + n..].join(" ");
    let query = query
        .trim()
        .trim_start_matches(|c: char| matches!(c, ',' | '.' | ':' | ';' | '-' | '!' | '?') || c.is_whitespace())
        .trim();
    Some(build_search_url(&cmd.url, query))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds() -> Vec<SearchCommand> {
        default_search_commands()
    }

    #[test]
    fn matches_google_and_builds_query() {
        let url = match_search_command("hej google ile lat", "hej", &cmds()).unwrap();
        assert_eq!(url, "https://www.google.com/search?q=ile%20lat");
    }

    #[test]
    fn is_case_insensitive_and_ignores_trigger_punctuation() {
        // Whisper-style output: capitalized, comma after the trigger, trailing "?".
        let url = match_search_command("Hej Google, ile lat?", "hej", &cmds()).unwrap();
        assert_eq!(url, "https://www.google.com/search?q=ile%20lat%3F");
    }

    #[test]
    fn encodes_polish_utf8() {
        let url = match_search_command("hej google łódź", "hej", &cmds()).unwrap();
        assert_eq!(url, "https://www.google.com/search?q=%C5%82%C3%B3d%C5%BA");
    }

    #[test]
    fn longest_trigger_wins() {
        let url = match_search_command("hej duck duck go koty", "hej", &cmds()).unwrap();
        assert_eq!(url, "https://duckduckgo.com/?q=koty");
    }

    #[test]
    fn name_variants_route_to_same_site() {
        // A command may carry a couple of natural name variants; both resolve.
        let a = match_search_command("hej chat gpt co słychać", "hej", &cmds()).unwrap();
        let b = match_search_command("hej chatgpt co słychać", "hej", &cmds()).unwrap();
        assert!(a.starts_with("https://chatgpt.com/?q="));
        assert_eq!(a, b);
    }

    #[test]
    fn youtube_uses_its_own_param() {
        let url = match_search_command("hej youtube lofi", "hej", &cmds()).unwrap();
        assert_eq!(url, "https://www.youtube.com/results?search_query=lofi");
    }

    #[test]
    fn only_anchored_at_start() {
        // A mid-sentence trigger is ordinary dictation, not a command.
        assert!(match_search_command("powiedz hej google teraz", "hej", &cmds()).is_none());
    }

    #[test]
    fn non_command_utterance_returns_none() {
        assert!(match_search_command("dzień dobry wszystkim", "hej", &cmds()).is_none());
        assert!(match_search_command("", "hej", &cmds()).is_none());
    }

    #[test]
    fn trigger_only_opens_site_with_empty_query() {
        let url = match_search_command("hej google", "hej", &cmds()).unwrap();
        assert_eq!(url, "https://www.google.com/search?q=");
    }

    #[test]
    fn disabled_or_empty_list_never_matches() {
        assert!(match_search_command("hej google test", "hej", &[]).is_none());
    }

    #[test]
    fn prefix_is_configurable() {
        // A different wake word entirely.
        let url = match_search_command("komputer youtube muzyka", "komputer", &cmds()).unwrap();
        assert_eq!(url, "https://www.youtube.com/results?search_query=muzyka");
        // English wake word.
        let url = match_search_command("hey google cats", "hey", &cmds()).unwrap();
        assert_eq!(url, "https://www.google.com/search?q=cats");
    }

    #[test]
    fn wrong_prefix_does_not_match() {
        // Utterance opens with "hej" but the configured prefix is "hey".
        assert!(match_search_command("hej google test", "hey", &cmds()).is_none());
        // …and the reverse.
        assert!(match_search_command("hey google test", "hej", &cmds()).is_none());
    }

    #[test]
    fn empty_prefix_matches_bare_keyword() {
        let url = match_search_command("google koty", "", &cmds()).unwrap();
        assert_eq!(url, "https://www.google.com/search?q=koty");
        // With no prefix, a leading wake word is just ordinary words → no match.
        assert!(match_search_command("hej google koty", "", &cmds()).is_none());
    }

    #[test]
    fn legacy_full_phrase_trigger_still_resolves() {
        // A command stored the old way (keyword still carries the wake word).
        let legacy = vec![SearchCommand {
            id: "x".into(),
            name: "X".into(),
            triggers: vec!["hej google".into()],
            url: "https://x/?q=%s".into(),
            enabled: true,
            builtin: false,
        }];
        let url = match_search_command("hej google koty", "hej", &legacy).unwrap();
        assert_eq!(url, "https://x/?q=koty");
    }

    #[test]
    fn custom_placeholders_supported() {
        assert_eq!(build_search_url("https://x.com/s?query={query}", "a b"), "https://x.com/s?query=a%20b");
        assert_eq!(build_search_url("https://x.com/s?q={q}", "a b"), "https://x.com/s?q=a%20b");
        // No placeholder: template is returned unchanged.
        assert_eq!(build_search_url("https://x.com", "a b"), "https://x.com");
    }

    #[test]
    fn percent_encoding_reserved_chars() {
        assert_eq!(percent_encode_query("a&b=c d"), "a%26b%3Dc%20d");
        assert_eq!(percent_encode_query("aA0-_.~"), "aA0-_.~");
    }
}
