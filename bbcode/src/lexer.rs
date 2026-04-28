#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token<'a> {
    Text(&'a str),
    Open {
        tag: &'a str,
        attr: Option<&'a str>,
        raw: &'a str,
    },
    Close {
        tag: &'a str,
        raw: &'a str,
    },
    Star {
        raw: &'a str,
    },
}

pub fn tokenize(input: &str) -> Vec<Token<'_>> {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut text_start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }

        if let Some((tok, end)) = scan_tag(input, i) {
            if text_start < i {
                out.push(Token::Text(&input[text_start..i]));
            }
            out.push(tok);
            i = end;
            text_start = end;
            continue;
        }

        i += 1;
    }

    if text_start < bytes.len() {
        out.push(Token::Text(&input[text_start..]));
    }
    out
}

fn scan_tag(input: &str, start: usize) -> Option<(Token<'_>, usize)> {
    let bytes = input.as_bytes();
    debug_assert_eq!(bytes[start], b'[');

    let mut p = start + 1;
    let mut closing = false;
    if p < bytes.len() && bytes[p] == b'/' {
        closing = true;
        p += 1;
    }

    if p < bytes.len() && bytes[p] == b'*' && !closing {
        if p + 1 < bytes.len() && bytes[p + 1] == b']' {
            let end = p + 2;
            return Some((
                Token::Star {
                    raw: &input[start..end],
                },
                end,
            ));
        }
        return None;
    }

    let name_start = p;
    if p >= bytes.len() || !bytes[p].is_ascii_alphabetic() {
        return None;
    }
    p += 1;
    while p < bytes.len() {
        let b = bytes[p];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'*' {
            p += 1;
        } else {
            break;
        }
    }
    let name = &input[name_start..p];

    if closing {
        if p < bytes.len() && bytes[p] == b']' {
            let end = p + 1;
            return Some((
                Token::Close {
                    tag: name,
                    raw: &input[start..end],
                },
                end,
            ));
        }
        return None;
    }

    let mut attr: Option<&str> = None;
    if p < bytes.len() && bytes[p] == b'=' {
        p += 1;
        let attr_start = p;
        let mut quote: Option<u8> = None;
        if p < bytes.len() && (bytes[p] == b'"' || bytes[p] == b'\'') {
            quote = Some(bytes[p]);
            p += 1;
        }
        while p < bytes.len() {
            let b = bytes[p];
            if let Some(q) = quote {
                if b == q {
                    p += 1;
                    break;
                }
                if b == b'\n' || b == b'\r' {
                    return None;
                }
                p += 1;
            } else {
                if b == b']' {
                    break;
                }
                if b == b'\n' || b == b'\r' {
                    return None;
                }
                p += 1;
            }
        }
        attr = Some(&input[attr_start..p]);
    }

    if p >= bytes.len() || bytes[p] != b']' {
        return None;
    }
    let end = p + 1;
    Some((
        Token::Open {
            tag: name,
            attr,
            raw: &input[start..end],
        },
        end,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text() {
        let toks = tokenize("hello world");
        assert_eq!(toks, vec![Token::Text("hello world")]);
    }

    #[test]
    fn simple_pair() {
        let toks = tokenize("[b]hi[/b]");
        assert_eq!(
            toks,
            vec![
                Token::Open {
                    tag: "b",
                    attr: None,
                    raw: "[b]"
                },
                Token::Text("hi"),
                Token::Close {
                    tag: "b",
                    raw: "[/b]"
                },
            ]
        );
    }

    #[test]
    fn quoted_attr() {
        let toks = tokenize(r#"[color="Red"]x[/color]"#);
        assert_eq!(
            toks[0],
            Token::Open {
                tag: "color",
                attr: Some(r#""Red""#),
                raw: r#"[color="Red"]"#,
            }
        );
    }

    #[test]
    fn unquoted_attr() {
        let toks = tokenize("[size=4]x[/size]");
        assert_eq!(
            toks[0],
            Token::Open {
                tag: "size",
                attr: Some("4"),
                raw: "[size=4]",
            }
        );
    }

    #[test]
    fn star() {
        let toks = tokenize("[*]item");
        assert_eq!(toks[0], Token::Star { raw: "[*]" });
    }

    #[test]
    fn malformed_left_alone() {
        let toks = tokenize("price [25] tokens");
        assert_eq!(toks, vec![Token::Text("price [25] tokens")]);
    }

    #[test]
    fn unterminated_open() {
        let toks = tokenize("foo [b bar");
        assert_eq!(toks, vec![Token::Text("foo [b bar")]);
    }

    #[test]
    fn newline_breaks_attr() {
        let toks = tokenize("[url=http://x\n]y[/url]");
        assert!(matches!(toks[0], Token::Text(_)));
    }
}
