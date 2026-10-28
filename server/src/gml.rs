//! Small GML tools: a tokenizer with byte spans, string quoting, source revisions,
//! and span edits. The structured editors (dialogue, cutscenes, enemies) use these
//! to find values in decompiled GML and to change only those values.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Num(String),
    /// String literal with its decoded value.
    Str(String),
    Punct(char),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    /// Byte offsets in the source.
    pub start: usize,
    pub end: usize,
    /// 1-based line of `start`.
    pub line: usize,
}

impl Token {
    pub fn is_punct(&self, c: char) -> bool {
        self.tok == Tok::Punct(c)
    }
    pub fn ident(&self) -> Option<&str> {
        match &self.tok {
            Tok::Ident(s) => Some(s),
            _ => None,
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Splits GML into tokens. Comments and whitespace are dropped. Never fails:
/// an unclosed string runs to the end of the source.
pub fn tokenize(src: &str) -> Vec<Token> {
    let b: Vec<(usize, char)> = src.char_indices().collect();
    let at = |i: usize| b.get(i).map(|x| x.1);
    let pos = |i: usize| b.get(i).map_or(src.len(), |x| x.0);
    let mut out = Vec::new();
    let mut line = 1;
    let mut i = 0;
    while i < b.len() {
        let c = b[i].1;
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && at(i + 1) == Some('/') {
            while i < b.len() && b[i].1 != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && at(i + 1) == Some('*') {
            i += 2;
            while i < b.len() && !(b[i].1 == '*' && at(i + 1) == Some('/')) {
                if b[i].1 == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i = (i + 2).min(b.len());
            continue;
        }
        let start = i;
        let start_line = line;
        let verbatim = c == '@' && matches!(at(i + 1), Some('"' | '\''));
        if c == '"' || verbatim {
            if verbatim {
                i += 1;
            }
            let quote = b[i].1;
            i += 1;
            let mut val = String::new();
            while i < b.len() && b[i].1 != quote {
                let ch = b[i].1;
                if ch == '\n' {
                    line += 1;
                }
                if ch == '\\' && !verbatim && i + 1 < b.len() {
                    i += 1;
                    val.push(match b[i].1 {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        o => o,
                    });
                } else {
                    val.push(ch);
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            out.push(Token {
                tok: Tok::Str(val),
                start: pos(start),
                end: pos(i),
                line: start_line,
            });
            continue;
        }
        let tok = if is_ident_start(c) {
            while at(i).is_some_and(is_ident) {
                i += 1;
            }
            Tok::Ident(src[pos(start)..pos(i)].to_owned())
        } else if c.is_ascii_digit()
            || (c == '.' && at(i + 1).is_some_and(|d| d.is_ascii_digit()))
            || (c == '$' && at(i + 1).is_some_and(|d| d.is_ascii_hexdigit()))
        {
            i += 1;
            while at(i).is_some_and(|d| d.is_ascii_alphanumeric() || d == '.') {
                i += 1;
            }
            Tok::Num(src[pos(start)..pos(i)].to_owned())
        } else {
            i += 1;
            Tok::Punct(c)
        };
        out.push(Token {
            tok,
            start: pos(start),
            end: pos(i),
            line: start_line,
        });
    }
    out
}

/// Makes a GML string literal for `s`.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Source revision: FNV-1a 64 of the text, hex. Edits must name the revision they were made on.
pub fn rev(src: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in src.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Index of the token that closes the bracket at `open` (`(`, `[`, `{`).
pub fn matching(toks: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (k, t) in toks.iter().enumerate().skip(open) {
        match t.tok {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits the tokens between `open` and its closing bracket at top-level commas.
/// Returns token index ranges `[a, b)` of each argument.
pub fn split_args(toks: &[Token], open: usize, close: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut a = open + 1;
    for (k, t) in toks.iter().enumerate().take(close).skip(open + 1) {
        match t.tok {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            Tok::Punct(',') if depth == 0 => {
                out.push((a, k));
                a = k + 1;
            }
            _ => {}
        }
    }
    if a < close || !out.is_empty() {
        out.push((a, close));
    }
    out
}

/// One replacement of the byte range `[start, end)`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    /// Raw GML to insert.
    #[serde(default)]
    pub text: Option<String>,
    /// Plain text to insert as a quoted GML string literal.
    #[serde(default)]
    pub string: Option<String>,
}

/// Applies non-overlapping edits. Offsets refer to the original source.
pub fn apply_edits(src: &str, edits: &[Edit]) -> Result<String> {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|e| (e.start, e.end));
    let mut out = String::with_capacity(src.len());
    let mut cur = 0;
    for e in sorted {
        if e.start < cur || e.end < e.start || e.end > src.len() || !src.is_char_boundary(e.start) || !src.is_char_boundary(e.end) {
            return Err(Error::BadRequest(format!("bad edit range {}..{}", e.start, e.end)));
        }
        out.push_str(&src[cur..e.start]);
        match (&e.text, &e.string) {
            (Some(t), None) => out.push_str(t),
            (None, Some(s)) => out.push_str(&quote(s)),
            (None, None) => {}
            (Some(_), Some(_)) => return Err(Error::BadRequest("give 'text' or 'string', not both".into())),
        }
        cur = e.end;
    }
    out.push_str(&src[cur..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_spans() {
        let src = "a = \"é\\\"x\"; // c\n/* b\n */ f(1.5, $FF)";
        let t = tokenize(src);
        assert_eq!(t[0].tok, Tok::Ident("a".into()));
        assert_eq!(t[2].tok, Tok::Str("é\"x".into()));
        assert_eq!(&src[t[2].start..t[2].end], "\"é\\\"x\"");
        let f = t.iter().position(|x| x.ident() == Some("f")).unwrap();
        assert_eq!(t[f].line, 3);
        assert_eq!(t[f + 2].tok, Tok::Num("1.5".into()));
        assert_eq!(t[f + 4].tok, Tok::Num("$FF".into()));
        let close = matching(&t, f + 1).unwrap();
        assert_eq!(split_args(&t, f + 1, close).len(), 2);
    }

    #[test]
    fn quote_round_trip() {
        let s = "* \"Hi\"\\n\nok";
        let q = quote(s);
        assert_eq!(tokenize(&q)[0].tok, Tok::Str(s.into()));
    }

    #[test]
    fn edits() {
        let src = "x = 1; y = \"a\";";
        let out = apply_edits(
            src,
            &[
                Edit {
                    start: 11,
                    end: 14,
                    text: None,
                    string: Some("b\"".into()),
                },
                Edit {
                    start: 4,
                    end: 5,
                    text: Some("20".into()),
                    string: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(out, "x = 20; y = \"b\\\"\";");
        assert!(
            apply_edits(
                src,
                &[Edit {
                    start: 3,
                    end: 99,
                    text: None,
                    string: None
                }]
            )
            .is_err()
        );
        assert_ne!(rev("a"), rev("b"));
    }
}
