use bbcode::{Element, Node};

use crate::decode::decode;
use crate::sanitize::{parse_color, sanitize_image_url, sanitize_url, sanitize_youtube_id};
use crate::style::{Style, size_from_attr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenKind {
    Spoiler,
    Blur,
}

#[derive(Debug)]
pub enum Block {
    Para(Vec<Inline>),
    List {
        ordered: bool,
        items: Vec<Vec<Block>>,
    },
    Indent(Vec<Block>),
    Code(String),
    Hidden(HiddenKind, Vec<Block>),
    Quote(Vec<Block>),
    Align(HAlign, Vec<Block>),
}

#[derive(Debug)]
pub enum Inline {
    Text { text: String, style: Style },
    Link { label: Vec<Inline>, url: String },
    Image(String),
    Youtube(String),
}

pub fn build_blocks(nodes: &[Node<'_>], style: &Style) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut inline: Vec<Inline> = Vec::new();
    for node in nodes {
        emit_node(node, style, &mut blocks, &mut inline);
    }
    flush_para(&mut inline, &mut blocks);
    blocks
}

fn emit_node(
    node: &Node<'_>,
    style: &Style,
    blocks: &mut Vec<Block>,
    inline: &mut Vec<Inline>,
) {
    match node {
        Node::Text(s) => inline.push(Inline::Text {
            text: decode(s).into_owned(),
            style: style.clone(),
        }),
        Node::Element(e) => emit_element(e, style, blocks, inline),
    }
}

fn emit_element(
    e: &Element<'_>,
    style: &Style,
    blocks: &mut Vec<Block>,
    inline: &mut Vec<Inline>,
) {
    let tag = e.tag.to_ascii_lowercase();
    match tag.as_str() {
        "b" => emit_inline_styled(e, style, |s| s.bold = true, blocks, inline),
        "i" => emit_inline_styled(e, style, |s| s.italic = true, blocks, inline),
        "u" => emit_inline_styled(e, style, |s| s.underline = true, blocks, inline),
        "s" | "strike" => emit_inline_styled(e, style, |s| s.strike = true, blocks, inline),
        "color" => {
            let mut s = style.clone();
            if let Some(c) = e.attr.and_then(parse_color) {
                s.color = Some(c);
            }
            emit_children_inline(e, &s, blocks, inline);
        }
        "size" => {
            let mut s = style.clone();
            if let Some(size) = size_from_attr(e.attr, style.size_pt) {
                s.size_pt = size;
            }
            emit_children_inline(e, &s, blocks, inline);
        }
        "font" => emit_children_inline(e, style, blocks, inline),

        "url" => emit_url(e, style, inline, false),
        "email" => emit_url(e, style, inline, true),
        "img" => emit_image(e, inline),
        "youtube" => emit_youtube(e, inline),

        "list" | "ul" | "ol" => {
            flush_para(inline, blocks);
            let ordered = tag == "ol"
                || matches!(e.attr_value(), Some(v) if !v.is_empty() && v != "*");
            let mut items: Vec<Vec<Block>> = Vec::new();
            for child in &e.children {
                if let Node::Element(item) = child
                    && (item.tag == "*" || item.is("li"))
                {
                    items.push(build_blocks(&item.children, style));
                }
            }
            blocks.push(Block::List { ordered, items });
        }
        "*" | "li" => emit_passthrough(e, style, blocks, inline),

        "indent" => {
            flush_para(inline, blocks);
            blocks.push(Block::Indent(build_blocks(&e.children, style)));
        }
        "code" | "highlight" | "pre" => {
            flush_para(inline, blocks);
            blocks.push(Block::Code(collect_text(&e.children)));
        }
        "spoiler" => {
            flush_para(inline, blocks);
            blocks.push(Block::Hidden(
                HiddenKind::Spoiler,
                build_blocks(&e.children, style),
            ));
        }
        "blur" => {
            flush_para(inline, blocks);
            blocks.push(Block::Hidden(
                HiddenKind::Blur,
                build_blocks(&e.children, style),
            ));
        }
        "quote" => {
            flush_para(inline, blocks);
            blocks.push(Block::Quote(build_blocks(&e.children, style)));
        }
        "center" => {
            flush_para(inline, blocks);
            blocks.push(Block::Align(HAlign::Center, build_blocks(&e.children, style)));
        }
        "left" => {
            flush_para(inline, blocks);
            blocks.push(Block::Align(HAlign::Left, build_blocks(&e.children, style)));
        }
        "right" => {
            flush_para(inline, blocks);
            blocks.push(Block::Align(HAlign::Right, build_blocks(&e.children, style)));
        }
        _ => emit_passthrough(e, style, blocks, inline),
    }
}

fn emit_inline_styled(
    e: &Element<'_>,
    style: &Style,
    apply: impl FnOnce(&mut Style),
    blocks: &mut Vec<Block>,
    inline: &mut Vec<Inline>,
) {
    let mut s = style.clone();
    apply(&mut s);
    emit_children_inline(e, &s, blocks, inline);
}

fn emit_children_inline(
    e: &Element<'_>,
    style: &Style,
    blocks: &mut Vec<Block>,
    inline: &mut Vec<Inline>,
) {
    for child in &e.children {
        emit_node(child, style, blocks, inline);
    }
}

fn emit_passthrough(
    e: &Element<'_>,
    style: &Style,
    blocks: &mut Vec<Block>,
    inline: &mut Vec<Inline>,
) {
    inline.push(Inline::Text {
        text: e.raw_open.to_string(),
        style: style.clone(),
    });
    for child in &e.children {
        emit_node(child, style, blocks, inline);
    }
}

fn resolve_link(s: &str, email: bool) -> Option<String> {
    if email {
        sanitize_email(s)
    } else {
        sanitize_url(s)
    }
}

fn emit_url(e: &Element<'_>, style: &Style, inline: &mut Vec<Inline>, email: bool) {
    let url = e.attr.and_then(|a| resolve_link(&decode(a), email)).or_else(|| {
        if let [Node::Text(t)] = e.children.as_slice() {
            resolve_link(&decode(t), email)
        } else {
            None
        }
    });
    let Some(url) = url else {
        inline.push(Inline::Text {
            text: e.raw_open.to_string(),
            style: style.clone(),
        });
        for child in &e.children {
            collect_inline(child, style, inline);
        }
        return;
    };
    let mut label_inlines: Vec<Inline> = Vec::new();
    let label_is_url_only = e.children.is_empty()
        || (e.children.len() == 1
            && matches!(&e.children[0], Node::Text(t) if resolve_link(&decode(t), email).is_some()));
    if label_is_url_only {
        let display: String = if email {
            url.strip_prefix("mailto:").unwrap_or(&url).to_string()
        } else {
            url.clone()
        };
        label_inlines.push(Inline::Text {
            text: display,
            style: style.clone(),
        });
    } else {
        for child in &e.children {
            collect_inline(child, style, &mut label_inlines);
        }
    }
    inline.push(Inline::Link {
        label: label_inlines,
        url,
    });
}

fn sanitize_email(raw: &str) -> Option<String> {
    let s = bbcode::unquote(raw).trim();
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix("mailto:") {
        return is_email_addr(rest).then(|| format!("mailto:{rest}"));
    }
    is_email_addr(s).then(|| format!("mailto:{s}"))
}

fn is_email_addr(s: &str) -> bool {
    let mut parts = s.splitn(2, '@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !s.chars().any(|c| c.is_whitespace())
}

fn emit_image(e: &Element<'_>, inline: &mut Vec<Inline>) {
    let url = collect_text(&e.children);
    if let Some(u) = sanitize_image_url(&url) {
        inline.push(Inline::Image(u));
    } else {
        inline.push(Inline::Text {
            text: e.raw_open.to_string(),
            style: Style::default(),
        });
    }
}

fn emit_youtube(e: &Element<'_>, inline: &mut Vec<Inline>) {
    let id = collect_text(&e.children);
    if let Some(id) = sanitize_youtube_id(id.trim()) {
        inline.push(Inline::Youtube(id));
    } else {
        inline.push(Inline::Text {
            text: e.raw_open.to_string(),
            style: Style::default(),
        });
    }
}

fn collect_inline(node: &Node<'_>, style: &Style, out: &mut Vec<Inline>) {
    match node {
        Node::Text(s) => out.push(Inline::Text {
            text: decode(s).into_owned(),
            style: style.clone(),
        }),
        Node::Element(e) => {
            let mut blocks: Vec<Block> = Vec::new();
            emit_element(e, style, &mut blocks, out);
        }
    }
}

fn collect_text(nodes: &[Node<'_>]) -> String {
    if let [Node::Text(s)] = nodes {
        return decode(s).into_owned();
    }
    let mut s = String::new();
    for n in nodes {
        flatten_text(n, &mut s);
    }
    decode(&s).into_owned()
}

fn flatten_text(node: &Node<'_>, out: &mut String) {
    match node {
        Node::Text(s) => out.push_str(s),
        Node::Element(e) => {
            for c in &e.children {
                flatten_text(c, out);
            }
        }
    }
}

fn flush_para(inline: &mut Vec<Inline>, blocks: &mut Vec<Block>) {
    if inline.is_empty() {
        return;
    }
    let para = std::mem::take(inline);
    blocks.push(Block::Para(para));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_addr_validation() {
        assert!(is_email_addr("a@b.c"));
        assert!(!is_email_addr("a@b"));
        assert!(!is_email_addr("@b.c"));
        assert!(!is_email_addr("a@"));
        assert!(!is_email_addr("a b@c.d"));
    }

    #[test]
    fn email_sanitize_prefixes_mailto() {
        assert_eq!(
            sanitize_email("user@example.com"),
            Some("mailto:user@example.com".to_string())
        );
        assert_eq!(
            sanitize_email("mailto:user@example.com"),
            Some("mailto:user@example.com".to_string())
        );
        assert_eq!(sanitize_email("not-an-email"), None);
        assert_eq!(sanitize_email("javascript:alert(1)"), None);
    }
}
