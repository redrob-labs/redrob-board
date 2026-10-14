//! Minimal s-expression reader: atoms, quoted strings and lists. Enough for footprint files.

use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sexp {
    Atom(String),
    List(Vec<Sexp>),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SexpError {
    #[error("unbalanced parentheses at byte {0}")]
    Unbalanced(usize),
    #[error("unterminated string starting at byte {0}")]
    UnterminatedString(usize),
    #[error("trailing data after the top-level expression at byte {0}")]
    Trailing(usize),
    #[error("empty input")]
    Empty,
}

impl Sexp {
    pub fn parse(text: &str) -> Result<Sexp, SexpError> {
        let bytes = text.as_bytes();
        let mut i = 0;
        skip_ws(bytes, &mut i);
        if i >= bytes.len() {
            return Err(SexpError::Empty);
        }
        let value = parse_value(bytes, &mut i)?;
        skip_ws(bytes, &mut i);
        if i < bytes.len() {
            return Err(SexpError::Trailing(i));
        }
        Ok(value)
    }

    pub fn as_atom(&self) -> Option<&str> {
        match self {
            Sexp::Atom(a) => Some(a),
            Sexp::List(_) => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Sexp]> {
        match self {
            Sexp::List(l) => Some(l),
            Sexp::Atom(_) => None,
        }
    }

    /// The head atom of a list, e.g. `"pad"` for `(pad "1" smd ...)`.
    pub fn head(&self) -> Option<&str> {
        self.as_list()?.first()?.as_atom()
    }

    /// Items after the head.
    pub fn args(&self) -> &[Sexp] {
        self.as_list().map_or(&[], |l| &l[1.min(l.len())..])
    }

    /// Every child list whose head is `name`.
    pub fn children(&self, name: &str) -> impl Iterator<Item = &Sexp> + '_ {
        let name = name.to_string();
        self.args().iter().filter(move |c| c.head() == Some(name.as_str()))
    }

    /// First child list whose head is `name`.
    pub fn child(&self, name: &str) -> Option<&Sexp> {
        self.children(name).next()
    }

    /// Positional atom argument `n` (0-based, after the head).
    pub fn atom_arg(&self, n: usize) -> Option<&str> {
        self.args().get(n)?.as_atom()
    }
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

fn parse_value(b: &[u8], i: &mut usize) -> Result<Sexp, SexpError> {
    match b[*i] {
        b'(' => {
            let open = *i;
            *i += 1;
            let mut items = Vec::new();
            loop {
                skip_ws(b, i);
                if *i >= b.len() {
                    return Err(SexpError::Unbalanced(open));
                }
                if b[*i] == b')' {
                    *i += 1;
                    return Ok(Sexp::List(items));
                }
                items.push(parse_value(b, i)?);
            }
        }
        b')' => Err(SexpError::Unbalanced(*i)),
        b'"' => {
            let start = *i;
            *i += 1;
            let mut out = Vec::new();
            while *i < b.len() {
                match b[*i] {
                    b'"' => {
                        *i += 1;
                        return Ok(Sexp::Atom(String::from_utf8_lossy(&out).into_owned()));
                    }
                    b'\\' if *i + 1 < b.len() => {
                        *i += 1;
                        out.push(match b[*i] {
                            b'n' => b'\n',
                            b't' => b'\t',
                            other => other,
                        });
                        *i += 1;
                    }
                    c => {
                        out.push(c);
                        *i += 1;
                    }
                }
            }
            Err(SexpError::UnterminatedString(start))
        }
        _ => {
            let start = *i;
            while *i < b.len() && !b[*i].is_ascii_whitespace() && b[*i] != b'(' && b[*i] != b')' {
                *i += 1;
            }
            Ok(Sexp::Atom(String::from_utf8_lossy(&b[start..*i]).into_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_lists_and_strings() {
        let s = Sexp::parse(r#"(pad "1" smd rect (at -0.5 0) (size 0.6 0.5) (layers "F.Cu" F.Mask))"#).unwrap();
        assert_eq!(s.head(), Some("pad"));
        assert_eq!(s.atom_arg(0), Some("1"));
        assert_eq!(s.atom_arg(2), Some("rect"));
        assert_eq!(s.child("at").unwrap().atom_arg(0), Some("-0.5"));
        assert_eq!(s.child("layers").unwrap().args().len(), 2);
        assert_eq!(s.child("missing"), None);
    }

    #[test]
    fn escapes_and_errors() {
        assert_eq!(Sexp::parse(r#""a\"b""#).unwrap(), Sexp::Atom("a\"b".into()));
        assert_eq!(Sexp::parse("(a (b)"), Err(SexpError::Unbalanced(0)));
        assert_eq!(Sexp::parse("(a)) "), Err(SexpError::Trailing(3)));
        assert_eq!(Sexp::parse("  "), Err(SexpError::Empty));
        assert_eq!(Sexp::parse("\"abc"), Err(SexpError::UnterminatedString(0)));
    }
}
