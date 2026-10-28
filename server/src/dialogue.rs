//! Dialogue index. Deltarune text lives in GML calls such as
//! `msgsetloc(0, "* Hello./%", "obj_town_slash_Step_0_gml_12_0")` and in
//! `lang/lang_*.json` files (key -> text). This module finds both.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

/// Text-bearing functions. `loc` variants take a localization key as the last argument.
/// The value is the argument index of the text.
const TEXT_FUNCS: &[(&str, usize)] = &[
    ("msgsetloc", 1),
    ("msgnextloc", 0),
    ("msgsetsubloc", 1),
    ("msgnextsubloc", 0),
    ("c_msgsetloc", 1),
    ("c_msgnextloc", 0),
    ("c_msgsetsubloc", 1),
    ("c_msgnextsubloc", 0),
    ("stringsetloc", 0),
    ("stringsetsubloc", 0),
    ("msgset", 1),
    ("msgnext", 0),
    ("c_msgset", 1),
    ("c_msgnext", 0),
    ("stringset", 0),
    ("scr_84_get_lang_string", usize::MAX),
];
const SPEAKER_FUNCS: &[&str] = &["scr_speaker", "c_speaker"];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DialogueLine {
    pub code: String,
    pub line: usize,
    pub func: String,
    pub text: Option<String>,
    pub key: Option<String>,
    pub speaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Arg {
    Str(String),
    Expr(String),
}

/// Reads a GML string literal at `s[i] == '"'` (or `@"`/`@'` verbatim). Returns (value, end index).
fn read_string(s: &[char], mut i: usize) -> Option<(String, usize)> {
    let verbatim = s[i] == '@';
    if verbatim {
        i += 1;
    }
    let quote = *s.get(i)?;
    if quote != '"' && !(verbatim && quote == '\'') {
        return None;
    }
    i += 1;
    let mut out = String::new();
    while i < s.len() {
        let c = s[i];
        if c == quote {
            return Some((out, i + 1));
        }
        if c == '\\' && !verbatim {
            i += 1;
            match s.get(i)? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(*other),
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    None
}

/// Parses a call argument list that starts after `(`. Returns (args, index after `)`).
fn read_args(s: &[char], mut i: usize) -> Option<(Vec<Arg>, usize)> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut only_str: Option<String> = None;
    let mut depth = 0usize;
    let flush = |cur: &mut String, only_str: &mut Option<String>, args: &mut Vec<Arg>| {
        let t = cur.trim();
        if !t.is_empty() {
            args.push(match only_str.take() {
                Some(v) if t.starts_with('"') || t.starts_with('@') => Arg::Str(v),
                _ => Arg::Expr(t.to_owned()),
            });
        }
        cur.clear();
        *only_str = None;
    };
    while i < s.len() {
        let c = s[i];
        if c == '"' || (c == '@' && matches!(s.get(i + 1), Some('"' | '\''))) {
            let (v, end) = read_string(s, i)?;
            let lone = depth == 0 && cur.trim().is_empty();
            cur.extend(&s[i..end]);
            only_str = if lone { Some(v) } else { None };
            i = end;
            continue;
        }
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth > 0 => depth -= 1,
            ')' => {
                flush(&mut cur, &mut only_str, &mut args);
                return Some((args, i + 1));
            }
            ',' if depth == 0 => {
                flush(&mut cur, &mut only_str, &mut args);
                i += 1;
                continue;
            }
            _ => {}
        }
        if !c.is_whitespace() {
            // Anything after the string literal makes this an expression.
            if only_str.is_some() {
                only_str = None;
            }
        }
        cur.push(c);
        i += 1;
    }
    None
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Finds dialogue calls in one GML source.
pub fn scan_source(code: &str, src: &str) -> Vec<DialogueLine> {
    let s: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut speaker: Option<String> = None;
    let mut line = 1usize;
    let mut i = 0usize;
    while i < s.len() {
        let c = s[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        // Skip comments and strings outside calls.
        if c == '/' && s.get(i + 1) == Some(&'/') {
            while i < s.len() && s[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && s.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < s.len() && !(s[i] == '*' && s[i + 1] == '/') {
                if s[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == '"' {
            match read_string(&s, i) {
                Some((v, end)) => {
                    line += v.matches('\n').count();
                    i = end;
                }
                None => i += 1,
            }
            continue;
        }
        if !(is_ident(c) && (i == 0 || !is_ident(s[i - 1]))) {
            i += 1;
            continue;
        }
        let start = i;
        while i < s.len() && is_ident(s[i]) {
            i += 1;
        }
        let ident: String = s[start..i].iter().collect();
        let mut j = i;
        while j < s.len() && s[j] == ' ' {
            j += 1;
        }
        if s.get(j) != Some(&'(') {
            continue;
        }
        let call_line = line;
        let is_speaker = SPEAKER_FUNCS.contains(&ident.as_str());
        let text_func = TEXT_FUNCS.iter().find(|(n, _)| *n == ident);
        if !is_speaker && text_func.is_none() {
            continue;
        }
        let Some((args, end)) = read_args(&s, j + 1) else {
            continue;
        };
        line += s[i..end].iter().filter(|&&c| c == '\n').count();
        i = end;
        let as_str = |a: Option<&Arg>| match a {
            Some(Arg::Str(v)) => Some(v.clone()),
            _ => None,
        };
        if is_speaker {
            speaker = as_str(args.first());
            continue;
        }
        let (name, text_idx) = *text_func.unwrap();
        let localized = name.ends_with("loc") || name == "scr_84_get_lang_string";
        let key = if localized { as_str(args.last()) } else { None };
        let text = as_str(args.get(text_idx));
        if text.is_none() && key.is_none() {
            continue;
        }
        out.push(DialogueLine {
            code: code.to_owned(),
            line: call_line,
            func: name.into(),
            text,
            key,
            speaker: speaker.clone(),
        });
    }
    out
}

/// Loads `lang/*.json` (key -> text) from a chapter dir. Returns language -> table.
pub fn load_lang(chapter_dir: &Path) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut langs = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(chapter_dir.join("lang")) else {
        return langs;
    };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        let Ok(serde_json::Value::Object(map)) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        let table = map.into_iter().filter_map(|(k, v)| Some((k, v.as_str()?.to_owned()))).collect();
        langs.insert(stem.trim_start_matches("lang_").to_owned(), table);
    }
    langs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_loc_calls_with_speaker() {
        let src = r#"
// msgsetloc(0, "comment", "no")
scr_speaker("susie");
msgsetloc(0, "\"Hey,\" Kris./", "obj_a_slash_Step_0_gml_3_0");
msgnextsubloc("* ~1 dollars./%", string(global.gold), "obj_a_slash_Step_0_gml_4_0");
var s = stringsetloc("Item", "scr_x_slash_scr_x_gml_1_0") + "!";
global.msg[0] = scr_84_get_lang_string("obj_old_1");
msgset(0, "* Plain./%");
"#;
        let lines = scan_source("gml_Object_obj_a_Step_0", src);
        assert_eq!(lines.len(), 5, "{lines:#?}");
        assert_eq!(lines[0].text.as_deref(), Some("\"Hey,\" Kris./"));
        assert_eq!(lines[0].key.as_deref(), Some("obj_a_slash_Step_0_gml_3_0"));
        assert_eq!(lines[0].speaker.as_deref(), Some("susie"));
        assert_eq!(lines[0].line, 4);
        assert_eq!(lines[1].text.as_deref(), Some("* ~1 dollars./%"));
        assert_eq!(lines[1].key.as_deref(), Some("obj_a_slash_Step_0_gml_4_0"));
        assert_eq!(lines[2].func, "stringsetloc");
        assert_eq!(lines[3].key.as_deref(), Some("obj_old_1"));
        assert_eq!(lines[3].text, None);
        assert_eq!(lines[4].key, None);
        assert_eq!(lines[4].line, 8);
    }

    #[test]
    fn expression_text_is_not_a_literal() {
        let lines = scan_source("c", r#"msgsetloc(0, "a" + b, "k");"#);
        assert_eq!(lines[0].text, None);
        assert_eq!(lines[0].key.as_deref(), Some("k"));
    }

    #[test]
    fn ignores_similar_identifiers() {
        assert!(scan_source("c", r#"my_msgsetloc(0, "a", "k"); xmsgset(0,"a");"#).is_empty());
    }

    #[test]
    fn loads_lang_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("lang")).unwrap();
        std::fs::write(tmp.path().join("lang/lang_ja.json"), r#"{"k":"こんにちは","n":1}"#).unwrap();
        let langs = load_lang(tmp.path());
        assert_eq!(langs["ja"]["k"], "こんにちは");
        assert!(!langs["ja"].contains_key("n"));
    }
}
