use crate::lexer::{Token, tokenize};
use crate::{Document, Element, Node};

const MAX_DEPTH: usize = 64;

fn is_list_tag(tag: &str) -> bool {
    tag.eq_ignore_ascii_case("list")
        || tag.eq_ignore_ascii_case("ul")
        || tag.eq_ignore_ascii_case("ol")
}

fn is_list_item_tag(tag: &str) -> bool {
    tag == "*" || tag.eq_ignore_ascii_case("li")
}

struct Frame<'a> {
    tag: &'a str,
    attr: Option<&'a str>,
    raw_open: &'a str,
    children: Vec<Node<'a>>,
}

pub fn parse(input: &str) -> Document<'_> {
    let tokens = tokenize(input);
    let mut stack: Vec<Frame<'_>> = vec![Frame {
        tag: "",
        attr: None,
        raw_open: "",
        children: Vec::new(),
    }];

    for tok in tokens {
        match tok {
            Token::Text(s) => push_text(&mut stack, s),

            Token::Open { tag, attr, raw } => {
                if stack.len() > MAX_DEPTH {
                    push_text(&mut stack, raw);
                    continue;
                }
                if is_list_item_tag(tag) {
                    open_list_item(&mut stack, tag, attr, raw);
                    continue;
                }
                stack.push(Frame {
                    tag,
                    attr,
                    raw_open: raw,
                    children: Vec::new(),
                });
            }

            Token::Close { tag, raw } => {
                let idx = stack.iter().rposition(|f| f.tag.eq_ignore_ascii_case(tag));
                match idx {
                    Some(target) if target >= 1 => {
                        while stack.len() > target + 1 {
                            close_top(&mut stack, true);
                        }
                        close_top(&mut stack, false);
                    }
                    _ => push_text(&mut stack, raw),
                }
            }

            Token::Star { raw } => {
                open_list_item(&mut stack, "*", None, raw);
            }
        }
    }

    while stack.len() > 1 {
        close_top(&mut stack, true);
    }

    Document {
        children: stack.pop().unwrap().children,
    }
}

fn open_list_item<'a>(
    stack: &mut Vec<Frame<'a>>,
    tag: &'a str,
    attr: Option<&'a str>,
    raw: &'a str,
) {
    let list_idx = stack.iter().rposition(|f| is_list_tag(f.tag));
    let item_idx = stack.iter().rposition(|f| is_list_item_tag(f.tag));
    match (list_idx, item_idx) {
        (Some(li), Some(ii)) if ii > li => {
            while stack.len() > ii + 1 {
                close_top(stack, true);
            }
            close_top(stack, true);
            stack.push(Frame {
                tag,
                attr,
                raw_open: raw,
                children: Vec::new(),
            });
        }
        (Some(_), _) => {
            stack.push(Frame {
                tag,
                attr,
                raw_open: raw,
                children: Vec::new(),
            });
        }
        _ => push_text(stack, raw),
    }
}

fn push_text<'a>(stack: &mut Vec<Frame<'a>>, s: &'a str) {
    if s.is_empty() {
        return;
    }
    stack.last_mut().unwrap().children.push(Node::Text(s));
}

fn close_top<'a>(stack: &mut Vec<Frame<'a>>, auto_closed: bool) {
    let frame = stack.pop().unwrap();
    let element = Element {
        tag: frame.tag,
        attr: frame.attr,
        raw_open: frame.raw_open,
        children: frame.children,
        auto_closed,
    };
    stack
        .last_mut()
        .unwrap()
        .children
        .push(Node::Element(element));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_one_text<'a>(node: &Node<'a>, expected: &str) {
        match node {
            Node::Text(s) => assert_eq!(*s, expected),
            other => panic!("expected text, got {:?}", other),
        }
    }

    #[test]
    fn simple_pair() {
        let d = parse("[b]hi[/b]");
        assert_eq!(d.children.len(), 1);
        if let Node::Element(e) = &d.children[0] {
            assert!(e.is("b"));
            assert!(!e.auto_closed);
            assert_eq!(e.children.len(), 1);
            assert_one_text(&e.children[0], "hi");
        } else {
            panic!();
        }
    }

    #[test]
    fn nested() {
        let d = parse("[b][i]a[/i][/b]");
        if let Node::Element(b) = &d.children[0] {
            assert!(b.is("b"));
            if let Node::Element(i) = &b.children[0] {
                assert!(i.is("i"));
                assert_one_text(&i.children[0], "a");
                return;
            }
        }
        panic!();
    }

    #[test]
    fn unmatched_close_is_text() {
        let d = parse("foo[/b]");
        assert_eq!(d.children.len(), 2);
        assert_one_text(&d.children[0], "foo");
        assert_one_text(&d.children[1], "[/b]");
    }

    #[test]
    fn unclosed_open_auto_closes() {
        let d = parse("[b]hi");
        if let Node::Element(e) = &d.children[0] {
            assert!(e.is("b"));
            assert!(e.auto_closed);
        } else {
            panic!();
        }
    }

    #[test]
    fn list_items() {
        let d = parse("[list][*]a[*]b[/list]");
        let list = if let Node::Element(e) = &d.children[0] { e } else { panic!() };
        assert!(list.is("list"));
        assert_eq!(list.children.len(), 2);
        for (i, expected) in ["a", "b"].iter().enumerate() {
            if let Node::Element(item) = &list.children[i] {
                assert_eq!(item.tag, "*");
                assert!(item.auto_closed);
                assert_one_text(&item.children[0], expected);
            } else {
                panic!();
            }
        }
    }

    #[test]
    fn star_outside_list_is_text() {
        let d = parse("[*]hi");
        assert_eq!(d.children.len(), 2);
        assert_one_text(&d.children[0], "[*]");
        assert_one_text(&d.children[1], "hi");
    }

    #[test]
    fn case_insensitive_close() {
        let d = parse("[B]hi[/b]");
        if let Node::Element(e) = &d.children[0] {
            assert!(e.is("b"));
            assert!(!e.auto_closed);
        } else {
            panic!();
        }
    }

    #[test]
    fn cross_close_auto_closes_inner() {
        let d = parse("[b][i]hi[/b]");
        if let Node::Element(b) = &d.children[0] {
            assert!(b.is("b"));
            assert!(!b.auto_closed);
            if let Node::Element(i) = &b.children[0] {
                assert!(i.is("i"));
                assert!(i.auto_closed);
                return;
            }
        }
        panic!();
    }

    #[test]
    fn attrs() {
        let d = parse(r#"[url="http://x"]y[/url]"#);
        if let Node::Element(e) = &d.children[0] {
            assert!(e.is("url"));
            assert_eq!(e.attr, Some(r#""http://x""#));
            assert_eq!(e.attr_value(), Some("http://x"));
        } else {
            panic!();
        }
    }

    #[test]
    fn ul_li_aliases() {
        let d = parse("[ul][li]a[/li][li]b[/li][/ul]");
        let list = if let Node::Element(e) = &d.children[0] { e } else { panic!() };
        assert!(list.is("ul"));
        assert_eq!(list.children.len(), 2);
        for (i, expected) in ["a", "b"].iter().enumerate() {
            if let Node::Element(item) = &list.children[i] {
                assert!(item.is("li"));
                assert!(!item.auto_closed);
                assert_one_text(&item.children[0], expected);
            } else {
                panic!();
            }
        }
    }

    #[test]
    fn ol_li_implicit_close() {
        let d = parse("[ol][li]a[li]b[/ol]");
        let list = if let Node::Element(e) = &d.children[0] { e } else { panic!() };
        assert!(list.is("ol"));
        assert_eq!(list.children.len(), 2);
        for (i, expected) in ["a", "b"].iter().enumerate() {
            if let Node::Element(item) = &list.children[i] {
                assert!(item.is("li"));
                assert!(item.auto_closed);
                assert_one_text(&item.children[0], expected);
            } else {
                panic!();
            }
        }
    }

    #[test]
    fn star_inside_ul() {
        let d = parse("[ul][*]a[*]b[/ul]");
        let list = if let Node::Element(e) = &d.children[0] { e } else { panic!() };
        assert!(list.is("ul"));
        assert_eq!(list.children.len(), 2);
    }

    #[test]
    fn unmatched_close_split_into_text() {
        let d = parse("foo[/b]bar");
        assert_eq!(d.children.len(), 3);
        assert_one_text(&d.children[0], "foo");
        assert_one_text(&d.children[1], "[/b]");
        assert_one_text(&d.children[2], "bar");
    }
}
