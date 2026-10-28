//! Cutscene index. Deltarune cutscenes are runs of `c_*` calls
//! (`c_sel(kr); c_walk("r", 4, 10); c_wait(30); ...`) inside code entries.
//! This module splits a source into blocks of such calls, with byte spans,
//! so the webpage can show them as editable steps and write a block back.

use serde::Serialize;

use crate::gml::{self, Tok};

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
    fn empty_args() {
        let s = steps("c_talk();");
        assert_eq!(s[0].args, Vec::<String>::new());
        assert_eq!(count("x = 1;"), 0);
    }
}
