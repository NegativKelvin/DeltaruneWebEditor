//! Cutscene index. Deltarune cutscenes are runs of `c_*` calls
//! (`c_sel(kr); c_walk("r", 4, 10); c_wait(30); ...`) inside code entries.
//! This module splits a source into blocks of such calls, with byte spans,
//! so the webpage can show them as editable steps and write a block back.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::gml::{self, Tok, Token};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Step {
    pub func: String,
    /// Raw GML of each argument.
    pub args: Vec<String>,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Block {
    /// Byte range from the first step to the end of the last step.
    pub start: usize,
    pub end: usize,
    pub line: usize,
    /// Leading whitespace of the first step's line.
    pub indent: String,
    /// The nearest line above the block that is not a brace, for example `if (con == 10)`.
    pub context: String,
    pub steps: Vec<Step>,
}

fn is_step_func(name: &str) -> bool {
    name.len() > 2 && name.starts_with("c_")
}

/// Finds the `c_*` statements in a source.
pub fn steps(src: &str) -> Vec<Step> {
    let toks = gml::tokenize(src);
    let mut out = Vec::new();
    let mut k = 0;
    while k < toks.len() {
        let t = &toks[k];
        let starts_statement = k == 0
            || matches!(toks[k - 1].tok, Tok::Punct(';' | '{' | '}' | ')'))
            || (toks[k - 1].line < t.line
                && !matches!(
                    toks[k - 1].tok,
                    Tok::Punct('.' | '=' | ',' | '(' | '+' | '-' | '*' | '/' | '&' | '|' | '!' | '<' | '>')
                ));
        let name = match t.ident() {
            Some(n) if starts_statement && is_step_func(n) && toks.get(k + 1).is_some_and(|x| x.is_punct('(')) => n.to_owned(),
            _ => {
                k += 1;
                continue;
            }
        };
        let Some(close) = gml::matching(&toks, k + 1) else { break };
        let args = gml::split_args(&toks, k + 1, close)
            .into_iter()
            .map(|(a, b)| {
                if a < b {
                    src[toks[a].start..toks[b - 1].end].to_owned()
                } else {
                    String::new()
                }
            })
            .collect();
        let mut last = close;
        if toks.get(close + 1).is_some_and(|x| x.is_punct(';')) {
            last += 1;
        }
        out.push(Step {
            func: name,
            args,
            line: t.line,
            start: t.start,
            end: toks[last].end,
        });
        k = last + 1;
    }
    out
}

/// Groups steps that only have whitespace between them.
pub fn blocks(src: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for s in steps(src) {
        if let Some(b) = out.last_mut()
            && src[b.end..s.start].trim().is_empty()
        {
            b.end = s.end;
            b.steps.push(s);
            continue;
        }
        let line_start = src[..s.start].rfind('\n').map_or(0, |p| p + 1);
        let indent: String = src[line_start..s.start].chars().take_while(|c| c.is_whitespace()).collect();
        let context = src[..line_start]
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty() && *l != "{" && *l != "}")
            .unwrap_or("")
            .to_owned();
        out.push(Block {
            start: s.start,
            end: s.end,
            line: s.line,
            indent,
            context,
            steps: vec![s],
        });
    }
    out
}

/// Number of `c_*` steps, for the cutscene list. Cheap pre-check first.
pub fn count(src: &str) -> usize {
    if !src.contains("c_") {
        return 0;
    }
    steps(src).len()
}

/// One `if (con == N)` part of a cutscene: a node of the state graph.
/// Deltarune cutscene objects step through `con` values (`con = 2;` starts the next part).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Section {
    /// Raw value that `con` is tested against.
    pub con: String,
    pub line: usize,
    /// Byte range from `if` to the end of the body.
    pub start: usize,
    pub end: usize,
    /// Values that the body gives to `con` (`con = 3`, `con++`, `con += 2`).
    pub next: Vec<String>,
    /// Indexes of the blocks in the body.
    pub blocks: Vec<usize>,
}

/// Index of the token after a statement that starts at `k` (to `;` at depth 0, or a full `{}` body).
fn statement_end(toks: &[Token], k: usize) -> usize {
    if toks.get(k).is_some_and(|t| t.is_punct('{')) {
        return gml::matching(toks, k).map_or(toks.len(), |c| c + 1);
    }
    let mut depth = 0usize;
    let mut j = k;
    while j < toks.len() {
        match toks[j].tok {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') if depth > 0 => depth -= 1,
            Tok::Punct('}') => return j,
            Tok::Punct(';') if depth == 0 => return j + 1,
            _ => {}
        }
        // Statements without `;` end at the line end.
        if depth == 0 && j > k && toks[j].line > toks[j - 1].line && !toks[j - 1].is_punct('=') {
            return j;
        }
        j += 1;
    }
    j
}

/// Finds the `if (con == N ...)` sections of a source and the `con` values that each one sets.
pub fn sections(src: &str) -> Vec<Section> {
    let toks = gml::tokenize(src);
    let all_blocks = blocks(src);
    let mut out = Vec::new();
    let mut k = 0;
    while k + 5 < toks.len() {
        let is_test = toks[k].ident() == Some("if")
            && toks[k + 1].is_punct('(')
            && toks[k + 2].ident() == Some("con")
            && toks[k + 3].is_punct('=')
            && toks[k + 4].is_punct('=')
            && matches!(toks[k + 5].tok, Tok::Num(_) | Tok::Ident(_));
        if !is_test {
            k += 1;
            continue;
        }
        let Some(close) = gml::matching(&toks, k + 1) else { break };
        let con = src[toks[k + 5].start..toks[k + 5].end].to_owned();
        let body_end = statement_end(&toks, close + 1);
        let end = toks.get(body_end.saturating_sub(1)).map_or(src.len(), |t| t.end);
        let mut next = Vec::new();
        for j in close + 1..body_end {
            if toks[j].ident() != Some("con") || (j > 0 && toks[j - 1].is_punct('.')) {
                continue;
            }
            let (a, b, c) = (toks.get(j + 1), toks.get(j + 2), toks.get(j + 3));
            let v = match (a.map(|t| &t.tok), b.map(|t| &t.tok), c.map(|t| &t.tok)) {
                (Some(Tok::Punct('=')), Some(Tok::Num(n)), _) => Some(n.clone()),
                (Some(Tok::Punct('+')), Some(Tok::Punct('+')), _) => con.parse::<i64>().ok().map(|n| (n + 1).to_string()),
                (Some(Tok::Punct('+')), Some(Tok::Punct('=')), Some(Tok::Num(n))) => {
                    con.parse::<i64>().ok().zip(n.parse::<i64>().ok()).map(|(a, b)| (a + b).to_string())
                }
                _ => None,
            };
            if let Some(v) = v
                && !next.contains(&v)
            {
                next.push(v);
            }
        }
        let start = toks[k].start;
        out.push(Section {
            con,
            line: toks[k].line,
            start,
            end,
            next,
            blocks: (0..all_blocks.len())
                .filter(|&i| all_blocks[i].start >= start && all_blocks[i].end <= end)
                .collect(),
        });
        k = close + 1;
    }
    out
}

/// A step as the webpage sends it.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct StepIn {
    pub func: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// Checks that one argument is a single GML expression: brackets match, strings close, no top-level `,` or `;`, no comments.
pub fn check_arg(a: &str) -> Result<()> {
    let bad = |why: &str| Err(Error::BadRequest(format!("argument `{a}`: {why}")));
    if a.trim().is_empty() {
        return bad("is empty");
    }
    let toks = gml::tokenize(a);
    let mut stack = Vec::new();
    let mut prev_end = 0;
    for t in &toks {
        if !a[prev_end..t.start].trim().is_empty() {
            return bad("has a comment");
        }
        prev_end = t.end;
        match t.tok {
            Tok::Punct(c @ ('(' | '[' | '{')) => stack.push(c),
            Tok::Punct(c @ (')' | ']' | '}')) => {
                let want = match c {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if stack.pop() != Some(want) {
                    return bad("brackets do not match");
                }
            }
            Tok::Punct(',' | ';') if stack.is_empty() => return bad("has a `,` or `;` outside brackets"),
            _ => {}
        }
    }
    if !a[prev_end..].trim().is_empty() {
        return bad("has a comment");
    }
    if !stack.is_empty() {
        return bad("brackets do not match");
    }
    // An open string runs to the end of the source: a marker after it is then not a token.
    let probe = gml::tokenize(&format!("{a}\n0"));
    if probe.last().is_none_or(|t| t.tok != Tok::Num("0".into())) {
        return bad("has a string that is not closed");
    }
    Ok(())
}

/// GML of steps, one per line: `c_walk("r", 4, 10);`.
pub fn render_steps(steps: &[StepIn], indent: &str) -> Result<String> {
    let mut lines = Vec::with_capacity(steps.len());
    for s in steps {
        if !is_step_func(&s.func) || !s.func.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(Error::BadRequest(format!(
                "function `{}` must be a name that starts with c_",
                s.func
            )));
        }
        for a in &s.args {
            check_arg(a)?;
        }
        let args: Vec<&str> = s.args.iter().map(|a| a.trim()).collect();
        lines.push(format!("{}({});", s.func, args.join(", ")));
    }
    Ok(lines.join(&format!("\n{indent}")))
}

/// A changed block: its byte range in the source revision, and the new steps (empty removes the block).
#[derive(Debug, Clone, Deserialize)]
pub struct BlockEdit {
    pub start: usize,
    pub end: usize,
    #[serde(default)]
    pub indent: String,
    pub steps: Vec<StepIn>,
}

/// A new `if (con == N)` section, put after the last section.
#[derive(Debug, Clone, Deserialize)]
pub struct NewSection {
    pub con: i64,
    /// `con` value that the section sets first, so the next section runs on the next step.
    #[serde(default)]
    pub next: Option<i64>,
    pub steps: Vec<StepIn>,
}

/// Turns block changes and new sections into span edits of `src`.
pub fn edits(src: &str, changed: &[BlockEdit], added: &[NewSection]) -> Result<Vec<gml::Edit>> {
    let mut out = Vec::new();
    for b in changed {
        if !b.indent.chars().all(|c| c == ' ' || c == '\t') {
            return Err(Error::BadRequest("indent must be spaces or tabs".into()));
        }
        out.push(gml::Edit {
            start: b.start,
            end: b.end,
            text: Some(render_steps(&b.steps, &b.indent)?),
            string: None,
        });
    }
    if !added.is_empty() {
        let secs = sections(src);
        let (at, indent) = match secs.iter().max_by_key(|s| s.end) {
            Some(s) => {
                let line_start = src[..s.start].rfind('\n').map_or(0, |p| p + 1);
                (
                    s.end,
                    src[line_start..s.start]
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .collect::<String>(),
                )
            }
            None if src.trim_start().starts_with("function") => {
                return Err(Error::BadRequest(
                    "this code is a function script; add the first `if (con == N)` section in the code editor".into(),
                ));
            }
            None => (src.trim_end().len(), String::new()),
        };
        let mut text = String::new();
        for s in added {
            if secs.iter().any(|x| x.con == s.con.to_string()) || added.iter().filter(|y| y.con == s.con).count() > 1 {
                return Err(Error::BadRequest(format!("section con == {} exists already", s.con)));
            }
            let inner = format!("{indent}    ");
            let mut body = Vec::new();
            if let Some(n) = s.next {
                body.push(format!("con = {n};"));
            }
            if !s.steps.is_empty() {
                body.push(render_steps(&s.steps, &inner)?);
            }
            text.push_str(&format!("\n{indent}if (con == {})\n{indent}{{\n", s.con));
            if !body.is_empty() {
                text.push_str(&format!("{inner}{}\n", body.join(&format!("\n{inner}"))));
            }
            text.push_str(&format!("{indent}}}"));
        }
        if at >= src.len() {
            text.push('\n');
        }
        out.push(gml::Edit {
            start: at,
            end: at,
            text: Some(text),
            string: None,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_blocks() {
        let src = "if (con == 1)\n{\n    c_sel(kr);\n    c_walk(\"r\", 4, f(2, 3));\n    c_wait(30)\n    con = 2;\n}\nif (con == 3)\n    c_msgsetloc(0, \"* Hi/%\", \"k\"); x = c_val(1);\n";
        let b = blocks(src);
        assert_eq!(b.len(), 2, "{b:#?}");
        assert_eq!(b[0].steps.len(), 3);
        assert_eq!(b[0].context, "if (con == 1)");
        assert_eq!(b[0].indent, "    ");
        assert_eq!(b[0].steps[1].args, vec!["\"r\"", "4", "f(2, 3)"]);
        assert_eq!(
            &src[b[0].start..b[0].end],
            "c_sel(kr);\n    c_walk(\"r\", 4, f(2, 3));\n    c_wait(30)"
        );
        assert_eq!(b[1].steps[0].func, "c_msgsetloc");
        assert_eq!(b[1].steps.len(), 1);
        assert_eq!(b[1].context, "if (con == 3)");
    }

    #[test]
    fn sections_and_next() {
        let src = "if (con == 1)\n{\n    con = 2;\n    c_sel(kr);\n    c_wait(5);\n}\nif (con == 2 && !i_ex(cutscene_master))\n    con++;\nif (con == 3)\n{\n    con += 2;\n    if (x)\n        con = 9;\n}\n";
        let s = sections(src);
        assert_eq!(s.len(), 3, "{s:#?}");
        assert_eq!(
            (s[0].con.as_str(), s[0].next.clone(), s[0].blocks.clone()),
            ("1", vec!["2".to_owned()], vec![0])
        );
        assert_eq!(s[1].next, vec!["3"]);
        assert_eq!(&src[s[1].start..s[1].end], "if (con == 2 && !i_ex(cutscene_master))\n    con++;");
        assert_eq!(s[2].next, vec!["5", "9"]);
    }

    #[test]
    fn args_are_checked() {
        for ok in ["\"r\"", "f(1, 2)", "a[0] + 1", "\"a, b; // c\"", "-1", "\"q\\\"\""] {
            check_arg(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
        }
        for bad in ["", " ", "1, 2", "f(1", "a)", "1; x = 2", "\"open", "1 // c", "(]"] {
            assert!(check_arg(bad).is_err(), "{bad}");
        }
        let st = [StepIn {
            func: "c_walk".into(),
            args: vec![" \"r\" ".into(), "4".into()],
        }];
        assert_eq!(render_steps(&st, "  ").unwrap(), "c_walk(\"r\", 4);");
        assert!(
            render_steps(
                &[StepIn {
                    func: "walk".into(),
                    args: vec![]
                }],
                ""
            )
            .is_err()
        );
        assert!(
            render_steps(
                &[StepIn {
                    func: "c_x(1);c_y".into(),
                    args: vec![]
                }],
                ""
            )
            .is_err()
        );
    }

    #[test]
    fn block_edits_and_new_sections() {
        let src = "if (con == 1)\n{\n    c_sel(kr);\n    c_wait(5);\n}\n";
        let b = &blocks(src)[0];
        let st = |f: &str, a: &[&str]| StepIn {
            func: f.into(),
            args: a.iter().map(|x| (*x).to_owned()).collect(),
        };
        let e = edits(
            src,
            &[BlockEdit {
                start: b.start,
                end: b.end,
                indent: b.indent.clone(),
                steps: vec![st("c_wait", &["1"]), st("c_sel", &["su"])],
            }],
            &[
                NewSection {
                    con: 2,
                    next: Some(3),
                    steps: vec![st("c_wait", &["30"])],
                },
                NewSection {
                    con: 3,
                    next: None,
                    steps: vec![],
                },
            ],
        )
        .unwrap();
        let out = gml::apply_edits(src, &e).unwrap();
        assert_eq!(
            out,
            "if (con == 1)\n{\n    c_wait(1);\n    c_sel(su);\n}\nif (con == 2)\n{\n    con = 3;\n    c_wait(30);\n}\nif (con == 3)\n{\n}\n"
        );
        let secs = sections(&out);
        assert_eq!(secs.iter().map(|s| s.con.as_str()).collect::<Vec<_>>(), ["1", "2", "3"]);
        assert!(
            edits(
                src,
                &[],
                &[NewSection {
                    con: 1,
                    next: None,
                    steps: vec![]
                }]
            )
            .is_err()
        );
        assert!(
            edits(
                "function f()\n{\n}\n",
                &[],
                &[NewSection {
                    con: 1,
                    next: None,
                    steps: vec![]
                }]
            )
            .is_err()
        );
    }

    #[test]
    fn empty_args() {
        let s = steps("c_talk();");
        assert_eq!(s[0].args, Vec::<String>::new());
        assert_eq!(count("x = 1;"), 0);
    }
}
