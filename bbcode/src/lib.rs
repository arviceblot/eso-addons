//! BBCode tokenizer and tree parser.
//!
//! `parse(&str)` returns a [`Document`] that borrows from the input.

mod lexer;
mod parser;
mod sexp;

pub use lexer::{Token, tokenize};
pub use parser::parse;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document<'a> {
    pub children: Vec<Node<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node<'a> {
    Text(&'a str),
    Element(Element<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element<'a> {
    pub tag: &'a str,
    pub attr: Option<&'a str>,
    pub raw_open: &'a str,
    pub children: Vec<Node<'a>>,
    pub auto_closed: bool,
}

impl Element<'_> {
    pub fn is(&self, name: &str) -> bool {
        self.tag.eq_ignore_ascii_case(name)
    }

    pub fn attr_value(&self) -> Option<&str> {
        self.attr.map(unquote)
    }
}

pub fn unquote(s: &str) -> &str {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}
