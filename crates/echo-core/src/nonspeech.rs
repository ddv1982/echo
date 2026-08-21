pub fn strip_nonspeech(raw: &str) -> &str {
    let trimmed = raw.trim();
    if (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('(') && trimmed.ends_with(')'))
    {
        return "";
    }
    if raw.starts_with('*') && raw.ends_with('*') {
        return "";
    }
    if is_only(raw, is_music_glyph) || is_only(raw, is_dot) {
        return "";
    }
    raw
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
            ("♪", ""),
            ("♪♪", ""),
            ("...", ""),
            ("…", ""),
            ("Open (paren) here", "Open (paren) here"),
            ("[MUSIC] hello", "[MUSIC] hello"),
            ("(noise) continue", "(noise) continue"),
            ("Rate it 5 stars *", "Rate it 5 stars *"),
            ("He said \"music\" loudly", "He said \"music\" loudly"),
            ("claude code", "claude code"),
        ];
        for (raw, expected) in cases {
            assert_eq!(strip_nonspeech(raw), expected, "{raw:?}");
        }
    }
}
