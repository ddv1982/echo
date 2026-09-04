pub fn strip_nonspeech(raw: &str) -> &str {
    let trimmed = raw.trim();
    if wrapped_marker(trimmed).is_some_and(is_nonspeech_marker) {
        return "";
    }
    if is_only(trimmed, is_music_glyph) || is_only(trimmed, is_dot) {
        return "";
    }
    raw
}

fn wrapped_marker(raw: &str) -> Option<&str> {
    let (open, close) = (raw.chars().next()?, raw.chars().next_back()?);
    match (open, close) {
        ('[', ']') | ('(', ')') | ('*', '*') if raw.len() >= 2 => {
            Some(raw[open.len_utf8()..raw.len() - close.len_utf8()].trim())
        }
        _ => None,
    }
}

fn is_nonspeech_marker(marker: &str) -> bool {
    let marker = marker.to_lowercase();
    matches!(
        marker.as_str(),
        "blank_audio"
            | "blank audio"
            | "silence"
            | "silent"
            | "noise"
            | "background noise"
            | "applause"
            | "clapping"
            | "laughter"
            | "laughing"
            | "music"
            | "background music"
            | "musik"
            | "spannungsvolle musik"
            | "música"
            | "música de fundo"
            | "musique"
            | "muzyka"
            | "muzię"
            | "müzi̇k çaliyor"
            | "音楽"
            | "blender whirring"
            | "plastic crinkling"
            | "crunching"
            | "grunts"
    )
}

fn is_only(raw: &str, pred: impl Fn(char) -> bool) -> bool {
    let mut saw = false;
    for c in raw.chars().filter(|c| !c.is_whitespace()) {
        if !pred(c) {
            return false;
        }
        saw = true;
    }
    saw
}

fn is_music_glyph(c: char) -> bool {
    matches!(c, '♪' | '♫' | '♬' | '♩' | '♭' | '♮' | '♯')
}

fn is_dot(c: char) -> bool {
    c == '.' || c == '…'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_nonspeech_cases() {
        let cases = [
            ("[BLANK_AUDIO]", ""),
            ("[blank_audio]", ""),
            ("[MUSIK]", ""),
            ("[MÚSICA]", ""),
            ("[Música]", ""),
            ("[MÚSICA DE FUNDO]", ""),
            ("[Musique]", ""),
            ("[muzyka]", ""),
            ("[MUZIĘ]", ""),
            ("[MÜZİK ÇALIYOR]", ""),
            ("[音楽]", ""),
            ("[MUSIC]", ""),
            ("(music)", ""),
            ("(música)", ""),
            ("(音楽)", ""),
            ("(blender whirring)", ""),
            ("(plastic crinkling)", ""),
            ("(crunching)", ""),
            ("(grunts)", ""),
            ("* Musik *", ""),
            ("* Spannungsvolle Musik *", ""),
            ("  [MUSIC]  ", ""),
            ("  (noise)  ", ""),
            ("  * Musik *  ", ""),
            ("♪", ""),
            ("♪♪", ""),
            ("...", ""),
            ("…", ""),
            ("Open (paren) here", "Open (paren) here"),
            ("[MUSIC] hello", "[MUSIC] hello"),
            ("(noise) continue", "(noise) continue"),
            ("Rate it 5 stars *", "Rate it 5 stars *"),
            ("He said \"music\" loudly", "He said \"music\" loudly"),
            ("(yes)", "(yes)"),
            ("[OK]", "[OK]"),
            ("*hello*", "*hello*"),
            ("  (yes)  ", "  (yes)  "),
            ("claude code", "claude code"),
        ];
        for (raw, expected) in cases {
            assert_eq!(strip_nonspeech(raw), expected, "{raw:?}");
        }
    }
}
