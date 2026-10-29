//! Global variables: where the game reads and writes each `global.<name>` and
//! `global.<name>[index]` (Deltarune keeps story state in `global.flag[n]` and `global.plot`),
//! and the line layout of the save file, read from the game's own save script.

use std::collections::HashMap;

use serde::Serialize;

use crate::gml::{Tok, Token, matching, split_args, tokenize};

/// One read or write of a global variable.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Ref {
    pub code: String,
    pub line: usize,
    pub name: String,
    /// Index text: a number (`"5"`), an expression (`"i + 1"`), or none for a plain variable.
    pub index: Option<String>,
    pub write: bool,
    /// What the code does with the value: `= 40`, `+= 1`, `>= 30`, ...
    pub detail: Option<String>,
}

/// One variable (or array element) with all its refs counted.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub name: String,
    pub index: Option<String>,
    pub reads: usize,
    pub writes: usize,
    /// Numbers that the code sets or compares (story points for `global.plot`).
    pub values: Vec<String>,
}

fn punct(t: &[Token], k: usize, c: char) -> bool {
    t.get(k).is_some_and(|x| x.is_punct(c))
}

/// Text of tokens `[a, b)` from the source, with runs of white space made one space.
fn text(src: &str, t: &[Token], a: usize, b: usize) -> String {
    if a >= b {
        return String::new();
    }
    src[t[a].start..t[b - 1].end].split_whitespace().collect::<Vec<_>>().join(" ")
}

/// End (exclusive token index) of the value that starts at `a`: up to `;`, `)`, `]`, `,`,
/// `&&`, `||`, or a new line at bracket depth 0.
fn value_end(t: &[Token], a: usize) -> usize {
    let line = t.get(a).map_or(0, |x| x.line);
    let mut depth = 0usize;
    let mut k = a;
    while k < t.len() {
        let x = &t[k];
        if depth == 0 && (x.line != line || matches!(x.tok, Tok::Punct(';' | ')' | ']' | ',' | '}'))) {
            break;
        }
        if depth == 0 && ((x.is_punct('&') && punct(t, k + 1, '&')) || (x.is_punct('|') && punct(t, k + 1, '|'))) {
            break;
        }
        match x.tok {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => depth -= 1,
            _ => {}
        }
        k += 1;
    }
    k
}

fn short(s: String) -> String {
    if s.chars().count() > 60 {
        s.chars().take(57).collect::<String>() + "..."
    } else {
        s
    }
}

/// Finds every `global.<name>` and `global.<name>[...]` in one code entry.
pub fn scan_source(code: &str, src: &str) -> Vec<Ref> {
    let t = tokenize(src);
    let mut out = Vec::new();
    for k in 0..t.len() {
        if t[k].ident() != Some("global") || !punct(&t, k + 1, '.') || (k > 0 && punct(&t, k - 1, '.')) {
            continue;
        }
        let Some(name) = t.get(k + 2).and_then(Token::ident) else {
            continue;
        };
        let mut end = k + 3;
        let mut parts = Vec::new();
        while punct(&t, end, '[') {
            let Some(close) = matching(&t, end) else { break };
            parts.push(text(src, &t, end + 1, close));
            end = close + 1;
        }
        let index = (!parts.is_empty()).then(|| parts.join("]["));
        // `++x` / `--x`: the token before the two signs must not end an operand (`a++ + x` is rare in decompiled code).
        let prefix = k >= 2
            && ((punct(&t, k - 1, '+') && punct(&t, k - 2, '+')) || (punct(&t, k - 1, '-') && punct(&t, k - 2, '-')))
            && !(k >= 3 && matches!(t[k - 3].tok, Tok::Ident(_) | Tok::Num(_) | Tok::Punct(')' | ']')));
        let op = |c: char| punct(&t, end, c);
        let (write, detail) = if prefix {
            (true, Some(if punct(&t, k - 1, '+') { "++".into() } else { "--".into() }))
        } else if op('=') && !punct(&t, end + 1, '=') {
            (true, Some(format!("= {}", text(src, &t, end + 1, value_end(&t, end + 1)))))
        } else if ['+', '-', '*', '/', '|', '&', '^', '%'].iter().any(|&c| op(c)) && punct(&t, end + 1, '=') {
            let o = match t[end].tok {
                Tok::Punct(c) => c,
                _ => unreachable!(),
            };
            (true, Some(format!("{o}= {}", text(src, &t, end + 2, value_end(&t, end + 2)))))
        } else if (op('+') && punct(&t, end + 1, '+')) || (op('-') && punct(&t, end + 1, '-')) {
            (true, Some(if op('+') { "++".into() } else { "--".into() }))
        } else {
            // Comparison: `==`, `!=`, `<`, `<=`, `>`, `>=`.
            let cmp = match (t.get(end).map(|x| &x.tok), t.get(end + 1).map(|x| &x.tok)) {
                (Some(Tok::Punct('=')), Some(Tok::Punct('='))) => Some(("==", 2)),
                (Some(Tok::Punct('!')), Some(Tok::Punct('='))) => Some(("!=", 2)),
                (Some(Tok::Punct('<')), Some(Tok::Punct('='))) => Some(("<=", 2)),
                (Some(Tok::Punct('>')), Some(Tok::Punct('='))) => Some((">=", 2)),
                (Some(Tok::Punct('<')), _) => Some(("<", 1)),
                (Some(Tok::Punct('>')), _) => Some((">", 1)),
                _ => None,
            };
            let d = cmp.map(|(o, n)| format!("{o} {}", text(src, &t, end + n, value_end(&t, end + n))));
            (false, d)
        };
        out.push(Ref {
            code: code.to_owned(),
            line: t[k].line,
            name: name.to_owned(),
            index,
            write,
            detail: detail.map(short),
        });
    }
    out
}

/// Number in a detail (`= 40`, `>= -1`), if the value is one number.
fn detail_number(d: &str) -> Option<&str> {
    let v = d.split_once(' ')?.1.trim();
    v.parse::<f64>().is_ok().then_some(v)
}

/// Key that sorts `flag[2]` before `flag[10]`.
fn sort_key(s: &Summary) -> (String, u8, i64, String) {
    let idx = s.index.clone().unwrap_or_default();
    match idx.parse::<i64>() {
        Ok(n) => (s.name.clone(), 0, n, idx),
        Err(_) => (s.name.clone(), u8::from(s.index.is_some()), 0, idx),
    }
}

/// Groups refs by variable and index.
pub fn summarize(refs: &[Ref]) -> Vec<Summary> {
    let mut map: HashMap<(&str, Option<&str>), Summary> = HashMap::new();
    for r in refs {
        let s = map.entry((&r.name, r.index.as_deref())).or_insert_with(|| Summary {
            name: r.name.clone(),
            index: r.index.clone(),
            reads: 0,
            writes: 0,
            values: Vec::new(),
        });
        if r.write {
            s.writes += 1;
        } else {
            s.reads += 1;
        }
        if let Some(v) = r.detail.as_deref().and_then(detail_number)
            && s.values.len() < 100
            && !s.values.iter().any(|x| x == v)
        {
            s.values.push(v.to_owned());
        }
    }
    let mut out: Vec<Summary> = map.into_values().collect();
    for s in &mut out {
        s.values
            .sort_by(|a, b| a.parse::<f64>().unwrap_or(0.0).total_cmp(&b.parse::<f64>().unwrap_or(0.0)));
    }
    out.sort_by_cached_key(sort_key);
    out
}

/// One line of the save file.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SaveField {
    /// 0-based line number.
    pub line: usize,
    /// The value the game writes, for example `global.flag[12]`.
    pub label: String,
    /// `string` or `real`.
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SaveLayout {
    /// Code entry that the layout comes from.
    pub code: Option<String>,
    pub fields: Vec<SaveField>,
    pub warnings: Vec<String>,
}

/// Picks the save script (`scr_saveprocess`) and reads its layout.
pub fn save_layout<'a>(sources: impl Iterator<Item = (String, String)> + 'a) -> SaveLayout {
    let mut best = SaveLayout {
        warnings: vec!["no save script (scr_saveprocess) found".into()],
        ..Default::default()
    };
    for (name, src) in sources {
        if !name.contains("scr_saveprocess") {
            continue;
        }
        let mut l = layout_of(&src);
        l.code = Some(name);
        if best.code.is_none() || l.fields.len() > best.fields.len() {
            best = l;
        }
    }
    best
}

const MAX_STEPS: usize = 200_000;

struct Walker<'a> {
    src: &'a str,
    t: Vec<Token>,
    env: HashMap<String, i64>,
    fields: Vec<SaveField>,
    /// Labels on the line that is not ended yet.
    cur: Vec<(String, &'static str)>,
    warnings: Vec<String>,
    steps: usize,
}

/// Reads the line layout from a save script: each `file_text_write_string/real(file, value)` call
/// (also `ossafe_...`) adds `value` to the current line, `file_text_writeln` ends it.
/// `for`, `repeat`, and simple `while` loops with number bounds run, with the loop variable put into the labels.
pub fn layout_of(src: &str) -> SaveLayout {
    let mut w = Walker {
        src,
        t: tokenize(src),
        env: HashMap::new(),
        fields: Vec::new(),
        cur: Vec::new(),
        warnings: Vec::new(),
        steps: 0,
    };
    let n = w.t.len();
    w.walk(0, n);
    if !w.cur.is_empty() {
        w.end_line();
    }
    if w.steps >= MAX_STEPS {
        w.warn("save script is too long to read; the layout stops early");
    }
    if w.fields.is_empty() {
        w.warn("the save script writes no file_text lines");
    }
    SaveLayout {
        code: None,
        fields: w.fields,
        warnings: w.warnings,
    }
}

impl Walker<'_> {
    fn warn(&mut self, s: &str) {
        if !self.warnings.iter().any(|w| w == s) {
            self.warnings.push(s.to_owned());
        }
    }

    fn end_line(&mut self) {
        let cur = std::mem::take(&mut self.cur);
        let kind = cur.first().map_or("string", |c| c.1);
        let label = cur.into_iter().map(|c| c.0).collect::<Vec<_>>().join(" + ");
        self.fields.push(SaveField {
            line: self.fields.len(),
            label,
            kind,
        });
    }

    /// Label text of tokens `[a, b)` with known loop variables put in.
    fn label(&self, a: usize, b: usize) -> String {
        let mut s = String::new();
        for k in a..b {
            let x = &self.t[k];
            if k > a && self.t[k - 1].end < x.start && needs_space(&self.t[k - 1], x) {
                s.push(' ');
            }
            match (&x.tok, self.env.get(x.ident().unwrap_or_default())) {
                (Tok::Ident(_), Some(v)) if !(k > a && self.t[k - 1].is_punct('.')) => s.push_str(&v.to_string()),
                _ => s.push_str(&self.src[x.start..x.end]),
            }
        }
        s
    }

    /// Value of a bound: a number, a known variable, or `a + n` / `a - n`.
    fn eval(&self, a: usize, b: usize) -> Option<i64> {
        let one = |k: usize| -> Option<i64> {
            match &self.t.get(k)?.tok {
                Tok::Num(n) => n.parse().ok(),
                Tok::Ident(i) => self.env.get(i).copied(),
                _ => None,
            }
        };
        match b.checked_sub(a)? {
            1 => one(a),
            2 if self.t[a].is_punct('-') => one(a + 1).map(|v| -v),
            3 if self.t[a + 1].is_punct('+') => Some(one(a)? + one(a + 2)?),
            3 if self.t[a + 1].is_punct('-') => Some(one(a)? - one(a + 2)?),
            _ => None,
        }
    }

    /// Statement or block that starts at `k`: returns `[a, b)` to walk and the index after it.
    fn body(&self, k: usize, end: usize) -> (usize, usize, usize) {
        if k < end && self.t[k].is_punct('{') {
            let close = matching(&self.t, k).unwrap_or(end).min(end);
            return (k + 1, close, close + 1);
        }
        let mut depth = 0usize;
        let mut j = k;
        while j < end {
            match self.t[j].tok {
                Tok::Punct('(' | '[' | '{') => depth += 1,
                Tok::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
                Tok::Punct(';') if depth == 0 => return (k, j + 1, j + 1),
                _ => {}
            }
            j += 1;
        }
        (k, end, end)
    }

    /// `v < n`, `v <= n`: (variable, last value).
    fn cond(&self, a: usize, b: usize) -> Option<(String, i64)> {
        let v = self.t.get(a)?.ident()?.to_owned();
        if self.t.get(a + 1)?.is_punct('<') {
            if self.t.get(a + 2)?.is_punct('=') {
                return Some((v, self.eval(a + 3, b)?));
            }
            return Some((v, self.eval(a + 2, b)? - 1));
        }
        None
    }

    fn run_loop(&mut self, var: &str, from: i64, to: i64, a: usize, b: usize) {
        let mut i = from;
        while i <= to && self.steps < MAX_STEPS {
            self.env.insert(var.to_owned(), i);
            self.walk(a, b);
            i += 1;
        }
        self.env.remove(var);
    }

    fn walk(&mut self, a: usize, b: usize) {
        let mut k = a;
        while k < b && self.steps < MAX_STEPS {
            self.steps += 1;
            let id = self.t[k].ident().map(str::to_owned);
            match id.as_deref() {
                Some("for") if k + 1 < b && self.t[k + 1].is_punct('(') => {
                    let close = matching(&self.t, k + 1).unwrap_or(b).min(b);
                    let (ba, bb, next) = self.body(close + 1, b);
                    // Header: init; cond; step.
                    let semis: Vec<usize> = (k + 2..close).filter(|&j| self.t[j].is_punct(';')).collect();
                    let parsed = (semis.len() == 2)
                        .then(|| {
                            let mut ia = k + 2;
                            if self.t[ia].ident() == Some("var") {
                                ia += 1;
                            }
                            let v = self.t.get(ia)?.ident()?.to_owned();
                            if !self.t.get(ia + 1)?.is_punct('=') {
                                return None;
                            }
                            let from = self.eval(ia + 2, semis[0])?;
                            let (cv, to) = self.cond(semis[0] + 1, semis[1])?;
                            (cv == v && self.step_is_inc(&v, semis[1] + 1, close)).then_some((v, from, to))
                        })
                        .flatten();
                    match parsed {
                        Some((v, from, to)) => self.run_loop(&v, from, to, ba, bb),
                        None => {
                            self.warn("a loop in the save script has bounds that are not numbers; its lines are read once");
                            self.walk(ba, bb);
                        }
                    }
                    k = next;
                }
                Some("repeat") if k + 1 < b && self.t[k + 1].is_punct('(') => {
                    let close = matching(&self.t, k + 1).unwrap_or(b).min(b);
                    let (ba, bb, next) = self.body(close + 1, b);
                    match self.eval(k + 2, close) {
                        Some(n) => {
                            for _ in 0..n.max(0) {
                                if self.steps >= MAX_STEPS {
                                    break;
                                }
                                self.walk(ba, bb);
                            }
                        }
                        None => {
                            self.warn("a loop in the save script has bounds that are not numbers; its lines are read once");
                            self.walk(ba, bb);
                        }
                    }
                    k = next;
                }
                Some("while") if k + 1 < b && self.t[k + 1].is_punct('(') => {
                    let close = matching(&self.t, k + 1).unwrap_or(b).min(b);
                    let (ba, bb, next) = self.body(close + 1, b);
                    let parsed = self
                        .cond(k + 2, close)
                        .and_then(|(v, to)| Some((self.env.get(&v).copied()?, v, to)));
                    match parsed {
                        Some((from, v, to)) => self.run_loop(&v, from, to, ba, bb),
                        None => {
                            self.warn("a loop in the save script has bounds that are not numbers; its lines are read once");
                            self.walk(ba, bb);
                        }
                    }
                    k = next;
                }
                Some("if" | "else" | "switch") => {
                    self.warn("the save script has conditions; some lines can differ from the labels");
                    k += 1;
                }
                Some(f) if k + 1 < b && self.t[k + 1].is_punct('(') && is_write(f) => {
                    let close = matching(&self.t, k + 1).unwrap_or(b).min(b);
                    if f.ends_with("file_text_writeln") {
                        self.end_line();
                    } else if let Some(&(va, vb)) = split_args(&self.t, k + 1, close).get(1) {
                        let kind = if f.ends_with("real") { "real" } else { "string" };
                        let l = self.label(va, vb);
                        self.cur.push((l, kind));
                    }
                    k = close + 1;
                }
                Some(v) if !(k > a && self.t[k - 1].is_punct('.')) && k + 1 < b && self.t[k + 1].is_punct('=') => {
                    // `v = n;` keeps n for a later `while (v < ...)`.
                    let end = (k + 2..b)
                        .find(|&j| self.t[j].is_punct(';') || self.t[j].line != self.t[k].line)
                        .unwrap_or(b);
                    match self.eval(k + 2, end) {
                        Some(n) => self.env.insert(v.to_owned(), n),
                        None => self.env.remove(v),
                    };
                    k += 2;
                }
                _ => k += 1,
            }
        }
    }

    /// `v++`, `++v`, `v += 1`, `v = v + 1`.
    fn step_is_inc(&self, v: &str, a: usize, b: usize) -> bool {
        let s: Vec<String> = (a..b).map(|k| self.src[self.t[k].start..self.t[k].end].to_owned()).collect();
        let s: Vec<&str> = s.iter().map(String::as_str).collect();
        matches!(s.as_slice(), [x, "+", "+"] | ["+", "+", x] | [x, "+", "=", "1"] if *x == v)
            || matches!(s.as_slice(), [x, "=", y, "+", "1"] if *x == v && *y == v)
    }
}

fn is_write(f: &str) -> bool {
    f.ends_with("file_text_write_string") || f.ends_with("file_text_write_real") || f.ends_with("file_text_writeln")
}

fn needs_space(a: &Token, b: &Token) -> bool {
    let word = |t: &Token| matches!(t.tok, Tok::Ident(_) | Tok::Num(_) | Tok::Str(_));
    word(a) && word(b) || matches!(a.tok, Tok::Punct('+' | '-' | '*' | '/')) || matches!(b.tok, Tok::Punct('+' | '-' | '*' | '/'))
}

/// Line endings and lines of a save file.
pub fn split_lines(text: &str) -> (Vec<String>, &'static str, bool) {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing = text.ends_with('\n');
    let body = text.strip_suffix('\n').unwrap_or(text);
    let body = body.strip_suffix('\r').unwrap_or(body);
    let lines = if text.is_empty() {
        Vec::new()
    } else {
        body.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned()).collect()
    };
    (lines, eol, trailing)
}

pub fn join_lines(lines: &[String], eol: &str, trailing: bool) -> String {
    let mut s = lines.join(eol);
    if trailing && !lines.is_empty() {
        s.push_str(eol);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes() {
        let src = "if (global.flag[5] == 1 && global.plot >= 30)\n{\n    global.flag[5] = 2;\n    global.flag[i + 1] += 3;\n    global.gold++;\n    ++global.lv;\n    x = global.char[0][1];\n}\nglobal.plot = 40;\n";
        let r = scan_source("c", src);
        let pick = |n: &str, i: Option<&str>| {
            r.iter()
                .filter(|x| x.name == n && x.index.as_deref() == i)
                .cloned()
                .collect::<Vec<_>>()
        };
        let f5 = pick("flag", Some("5"));
        assert_eq!(f5.len(), 2);
        assert!(!f5[0].write && f5[0].detail.as_deref() == Some("== 1"));
        assert!(f5[1].write && f5[1].detail.as_deref() == Some("= 2") && f5[1].line == 3);
        assert_eq!(pick("flag", Some("i + 1"))[0].detail.as_deref(), Some("+= 3"));
        assert!(pick("gold", None)[0].write);
        assert!(pick("lv", None)[0].write);
        let ch = pick("char", Some("0][1"));
        assert!(!ch[0].write && ch[0].detail.is_none());
        let plot = pick("plot", None);
        assert_eq!(plot[0].detail.as_deref(), Some(">= 30"));
        assert_eq!(plot[1].detail.as_deref(), Some("= 40"));

        let s = summarize(&r);
        let p = s.iter().find(|x| x.name == "plot").unwrap();
        assert_eq!((p.reads, p.writes), (1, 1));
        assert_eq!(p.values, ["30", "40"]);
        // flag[5] sorts before flag[i + 1].
        let flags: Vec<_> = s.iter().filter(|x| x.name == "flag").map(|x| x.index.clone().unwrap()).collect();
        assert_eq!(flags, ["5", "i + 1"]);
    }

    #[test]
    fn numeric_sort() {
        let r = scan_source("c", "global.flag[10] = 1; global.flag[2] = 1;");
        let s = summarize(&r);
        assert_eq!(s[0].index.as_deref(), Some("2"));
    }

    #[test]
    fn save_layout_loops() {
        let src = r#"function scr_saveprocess(argument0)
{
    file = file_text_open_write("filech1_" + string(argument0));
    file_text_write_string(file, global.truename);
    file_text_writeln(file);
    for (i = 0; i < 3; i += 1)
    {
        ossafe_file_text_write_real(file, global.char[i]);
        file_text_writeln(file);
    }
    for (var j = 0; j <= 1; j++)
    {
        for (k = 0; k < 2; k++)
        {
            file_text_write_real(file, global.spell[j][k]);
            file_text_writeln(file);
        }
    }
    i = 0;
    while (i < 2)
    {
        file_text_write_real(file, global.flag[i]);
        file_text_writeln(file);
        i++;
    }
    repeat (2)
    {
        file_text_write_real(file, 0);
        file_text_writeln(file);
    }
    file_text_write_real(file, global.plot);
    file_text_writeln(file);
    file_text_close(file);
}
"#;
        let l = layout_of(src);
        let labels: Vec<&str> = l.fields.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "global.truename",
                "global.char[0]",
                "global.char[1]",
                "global.char[2]",
                "global.spell[0][0]",
                "global.spell[0][1]",
                "global.spell[1][0]",
                "global.spell[1][1]",
                "global.flag[0]",
                "global.flag[1]",
                "0",
                "0",
                "global.plot"
            ],
            "{l:#?}"
        );
        assert_eq!(l.fields[0].kind, "string");
        assert_eq!(l.fields[1].kind, "real");
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    }

    #[test]
    fn save_layout_unknown_bounds() {
        let l = layout_of("for (i = 0; i < array_length(a); i++) { file_text_write_real(f, a[i]); file_text_writeln(f); }");
        assert_eq!(l.fields.len(), 1);
        assert_eq!(l.fields[0].label, "a[i]");
        assert_eq!(l.warnings.len(), 1);
        let picked = save_layout(
            vec![
                ("gml_Script_other".to_owned(), "file_text_writeln(f);".to_owned()),
                (
                    "gml_GlobalScript_scr_saveprocess".to_owned(),
                    "file_text_write_real(f, x); file_text_writeln(f);".to_owned(),
                ),
            ]
            .into_iter(),
        );
        assert_eq!(picked.code.as_deref(), Some("gml_GlobalScript_scr_saveprocess"));
        assert_eq!(picked.fields.len(), 1);
    }

    #[test]
    fn lines_round_trip() {
        for text in ["a\r\nb\r\n", "a\nb", "", "x\n"] {
            let (l, eol, tr) = split_lines(text);
            assert_eq!(join_lines(&l, eol, tr), text);
        }
        assert_eq!(split_lines("a\r\n\r\n").0, ["a", ""]);
    }
}
