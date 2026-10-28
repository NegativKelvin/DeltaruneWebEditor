//! Enemy and encounter index. Deltarune sets enemy stats and encounters with
//! assignments such as `global.monstermaxhp[myself] = 90;` inside `switch` cases
//! (`scr_monstersetup`, `scr_encountersetup`, ...). This module collects them,
//! grouped by code entry, case label, and array index, with byte spans for edits.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::gml::{self, Tok};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Field {
    /// Raw GML of the value.
    pub expr: String,
    pub start: usize,
    pub end: usize,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Record {
    pub code: String,
    /// Raw `case` label above the assignments, if any.
    pub case: Option<String>,
    /// Raw array index, for example `myself` or `0`.
    pub index: String,
    pub fields: BTreeMap<String, Field>,
    /// Revision of the source the spans refer to.
    pub rev: String,
}

fn tracked(field: &str) -> bool {
    field.starts_with("monster") || field == "battlemsg"
}

/// Finds `global.<field>[<index>] = <value>` assignments in one source.
pub fn scan_source(code: &str, src: &str) -> Vec<Record> {
    if !src.contains("global.monster") {
        return Vec::new();
    }
    let toks = gml::tokenize(src);
    let rev = gml::rev(src);
    let mut out: Vec<Record> = Vec::new();
    let mut case: Option<String> = None;
    let mut k = 0;
    while k < toks.len() {
        match toks[k].ident() {
            Some("case") => {
                let a = k + 1;
                let mut b = a;
                while b < toks.len() && !toks[b].is_punct(':') {
                    b += 1;
                }
                if b < toks.len() && b > a {
                    case = Some(src[toks[a].start..toks[b - 1].end].to_owned());
                }
                k = b + 1;
                continue;
            }
            Some("default") if toks.get(k + 1).is_some_and(|t| t.is_punct(':')) => {
                case = Some("default".into());
                k += 2;
                continue;
            }
            Some("global") => {}
            _ => {
                k += 1;
                continue;
            }
        }
        // global . field [ index ] = value
        let field = match (toks.get(k + 1), toks.get(k + 2), toks.get(k + 3)) {
            (Some(dot), Some(f), Some(br)) if dot.is_punct('.') && br.is_punct('[') => match &f.tok {
                Tok::Ident(n) if tracked(n) => n.clone(),
                _ => {
                    k += 1;
                    continue;
                }
            },
            _ => {
                k += 1;
                continue;
            }
        };
        let Some(close) = gml::matching(&toks, k + 3) else { break };
        if close == k + 4 || !toks.get(close + 1).is_some_and(|t| t.is_punct('=')) || toks.get(close + 2).is_some_and(|t| t.is_punct('=')) {
            k = close + 1;
            continue;
        }
        let index = src[toks[k + 4].start..toks[close - 1].end].to_owned();
        // The value runs to `;` at depth 0, or to the end of its line.
        let a = close + 2;
        let mut b = a;
        let mut depth = 0usize;
        while b < toks.len() {
            let t = &toks[b];
            match t.tok {
                Tok::Punct('(' | '[' | '{') => depth += 1,
                Tok::Punct(')' | ']' | '}') if depth > 0 => depth -= 1,
                Tok::Punct(';' | '}') if depth == 0 => break,
                _ => {}
            }
            if depth == 0 && b > a && t.line > toks[b - 1].line {
                break;
            }
            b += 1;
        }
        if b == a {
            k = a;
            continue;
        }
        let f = Field {
            expr: src[toks[a].start..toks[b - 1].end].to_owned(),
            start: toks[a].start,
            end: toks[b - 1].end,
            line: toks[k].line,
        };
        match out.iter_mut().find(|r| r.case == case && r.index == index) {
            Some(r) => {
                r.fields.insert(field, f);
            }
            None => out.push(Record {
                code: code.to_owned(),
                case: case.clone(),
                index,
                fields: BTreeMap::from([(field, f)]),
                rev: rev.clone(),
            }),
        }
        k = b;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_cases() {
        let src = r#"switch (global.monstertype[myself])
{
    case 1:
        global.monstername[myself] = stringsetloc("Rudinn", "scr_monstersetup_slash_1");
        global.monstermaxhp[myself] = 90;
        global.monsterhp[myself] = global.monstermaxhp[myself]
        global.monsterat[myself] = 6;
        break;
    case 2:
        global.monstermaxhp[myself] = 120;
        if (global.monstermaxhp[myself] == 3) {}
}
"#;
        let r = scan_source("gml_GlobalScript_scr_monstersetup", src);
        assert_eq!(r.len(), 2, "{r:#?}");
        assert_eq!(r[0].case.as_deref(), Some("1"));
        assert_eq!(r[0].index, "myself");
        assert_eq!(r[0].fields["monstermaxhp"].expr, "90");
        assert_eq!(r[0].fields["monsterhp"].expr, "global.monstermaxhp[myself]");
        assert_eq!(r[0].fields["monsterat"].expr, "6");
        let f = &r[0].fields["monstername"];
        assert_eq!(&src[f.start..f.end], f.expr);
        assert_eq!(r[1].fields.len(), 1);
    }

    #[test]
    fn encounter_indexes() {
        let src = "case 5:\nglobal.monstertype[0] = 3;\nglobal.monstertype[1] = 3;\nglobal.monsterinstancetype[0] = obj_rudinn_enemy;\nglobal.battlemsg[0] = \"* Hi\";";
        let r = scan_source("gml_GlobalScript_scr_encountersetup", src);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].fields.len(), 3);
        assert_eq!(r[0].fields["monsterinstancetype"].expr, "obj_rudinn_enemy");
        assert_eq!(r[1].index, "1");
    }
}
