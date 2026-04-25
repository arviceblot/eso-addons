//! Decode HTML numeric and named entities and strip control / bidi
//! formatting characters. Active only when the `entities` feature is on;
//! otherwise [`decode`] returns its input unchanged.

use std::borrow::Cow;

#[cfg(feature = "entities")]
pub fn decode(s: &str) -> Cow<'_, str> {
    if !needs_decode(s) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        if bytes[i] == b'&'
            && let Some((ch, consumed)) = parse_entity(&s[i..])
        {
            if !is_dangerous(ch) {
                out.push(ch);
            }
            i += consumed;
            continue;
        }
        let ch = s[i..]
            .chars()
            .next()
            .expect("non-empty slice has a char");
        if !is_dangerous(ch) {
            out.push(ch);
        }
        i += ch.len_utf8();
    }
    Cow::Owned(out)
}

#[cfg(not(feature = "entities"))]
pub fn decode(s: &str) -> Cow<'_, str> {
    Cow::Borrowed(s)
}

#[cfg(feature = "entities")]
fn needs_decode(s: &str) -> bool {
    let mut has_high = false;
    for &b in s.as_bytes() {
        if b == b'&' {
            return true;
        }
        if b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r' {
            return true;
        }
        if b == 0x7F {
            return true;
        }
        if b >= 0x80 {
            has_high = true;
        }
    }
    has_high && s.chars().any(is_dangerous)
}

#[cfg(feature = "entities")]
fn is_dangerous(c: char) -> bool {
    let n = c as u32;
    if n < 0x20 && c != '\t' && c != '\n' && c != '\r' {
        return true;
    }
    if n == 0x7F {
        return true;
    }
    if (0x80..=0x9F).contains(&n) {
        return true;
    }
    if (0x202A..=0x202E).contains(&n) {
        return true;
    }
    if (0x2066..=0x2069).contains(&n) {
        return true;
    }
    if n == 0xFEFF {
        return true;
    }
    if (0xE0000..=0xE007F).contains(&n) {
        return true;
    }
    false
}

#[cfg(feature = "entities")]
fn parse_entity(s: &str) -> Option<(char, usize)> {
    let bytes = s.as_bytes();
    if bytes.len() < 4 {
        return None;
    }
    if bytes[1] == b'#' {
        let (radix, start) = if bytes.len() > 2 && (bytes[2] == b'x' || bytes[2] == b'X') {
            (16, 3)
        } else {
            (10, 2)
        };
        let max_end = (start + 8).min(bytes.len());
        for end in start..max_end {
            if bytes[end] == b';' {
                if end == start {
                    return None;
                }
                let body = &s[start..end];
                let n = u32::from_str_radix(body, radix).ok()?;
                let ch = char::from_u32(n)?;
                return Some((ch, end + 1));
            }
        }
        return None;
    }
    static NAMED: &[(&[u8], char)] = &[
        (b"amp;", '&'),
        (b"lt;", '<'),
        (b"gt;", '>'),
        (b"quot;", '"'),
        (b"apos;", '\''),
        (b"nbsp;", '\u{00A0}'),
    ];
    let rest = &bytes[1..];
    for (name, ch) in NAMED {
        if rest.starts_with(name) {
            return Some((*ch, 1 + name.len()));
        }
    }
    None
}

#[cfg(all(test, feature = "entities"))]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_safe() {
        assert!(matches!(decode("hello world"), Cow::Borrowed(_)));
        assert!(matches!(decode("привет мир"), Cow::Borrowed(_)));
        assert!(matches!(decode(""), Cow::Borrowed(_)));
    }

    #[test]
    fn decimal_entity() {
        assert_eq!(decode("&#1056;"), "Р");
        assert_eq!(decode("&#65;"), "A");
    }

    #[test]
    fn hex_entity() {
        assert_eq!(decode("&#x420;"), "Р");
        assert_eq!(decode("&#X420;"), "Р");
    }

    #[test]
    fn named_entities() {
        assert_eq!(decode("&amp;&lt;&gt;&quot;&apos;"), "&<>\"'");
        assert_eq!(decode("a&nbsp;b"), "a\u{00A0}b");
    }

    #[test]
    fn unknown_entity_kept_as_literal() {
        assert_eq!(decode("&copy;"), "&copy;");
        assert_eq!(decode("&"), "&");
        assert_eq!(decode("a & b"), "a & b");
    }

    #[test]
    fn malformed_numeric_kept_as_literal() {
        assert_eq!(decode("&#xyz;"), "&#xyz;");
        assert_eq!(decode("&#;"), "&#;");
        assert_eq!(decode("&#x;"), "&#x;");
    }

    #[test]
    fn surrogate_kept_as_literal() {
        assert_eq!(decode("&#xD800;"), "&#xD800;");
    }

    #[test]
    fn control_chars_stripped() {
        assert_eq!(decode("a\u{0001}b"), "ab");
        assert_eq!(decode("&#1;"), "");
        assert_eq!(decode("a\u{007F}b"), "ab");
    }

    #[test]
    fn bidi_stripped() {
        assert_eq!(decode("a\u{202E}b"), "ab");
        assert_eq!(decode("a&#x202E;b"), "ab");
        assert_eq!(decode("a\u{2068}b"), "ab");
    }

    #[test]
    fn tag_chars_stripped() {
        assert_eq!(decode("a\u{E0041}b"), "ab");
    }

    #[test]
    fn whitespace_preserved() {
        assert_eq!(decode("a\tb\nc\rd"), "a\tb\nc\rd");
    }

    #[test]
    fn zwj_preserved() {
        assert_eq!(decode("a\u{200D}b"), "a\u{200D}b");
    }

    #[test]
    fn bandits_sample() {
        let s = "&#1054;&#1087;&#1080;&#1089;&#1072;&#1085;&#1080;&#1077;";
        assert_eq!(decode(s), "Описание");
    }
}
