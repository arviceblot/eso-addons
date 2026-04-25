use std::fmt::{self, Write};

use crate::{Document, Element, Node};

impl fmt::Display for Document<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("(doc")?;
        for child in &self.children {
            f.write_str(" ")?;
            child.fmt(f)?;
        }
        f.write_str(")")
    }
}

impl fmt::Display for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Node::Text(s) => write_quoted(f, s),
            Node::Element(e) => e.fmt(f),
        }
    }
}

impl fmt::Display for Element<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("(")?;
        f.write_str(self.tag)?;
        if let Some(attr) = self.attr {
            f.write_str(" :attr ")?;
            write_quoted(f, crate::unquote(attr))?;
        }
        if self.auto_closed {
            f.write_str(" :auto-closed")?;
        }
        for child in &self.children {
            f.write_str(" ")?;
            child.fmt(f)?;
        }
        f.write_str(")")
    }
}

fn write_quoted(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    f.write_str("\"")?;
    for c in s.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            c if (c as u32) < 0x20 => write!(f, "\\x{:02x}", c as u32)?,
            c => f.write_char(c)?,
        }
    }
    f.write_str("\"")
}

#[cfg(test)]
mod tests {
    use crate::parse;

    #[test]
    fn round_trip_sexp() {
        let s = parse(r#"[b]hello[i]world[/i][/b]"#).to_string();
        assert_eq!(s, r#"(doc (b "hello" (i "world")))"#);
    }

    #[test]
    fn list_sexp() {
        let s = parse("[list][*]a[*]b[/list]").to_string();
        assert_eq!(
            s,
            r#"(doc (list (* :auto-closed "a") (* :auto-closed "b")))"#
        );
    }

    #[test]
    fn attr_sexp() {
        let s = parse(r#"[color="Red"]hi[/color]"#).to_string();
        assert_eq!(s, r#"(doc (color :attr "Red" "hi"))"#);
    }
}
