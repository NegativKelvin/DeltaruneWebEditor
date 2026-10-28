//! Dialogue index. Deltarune text lives in GML calls such as
//! `msgsetloc(0, "* Hello./%", "obj_town_slash_Step_0_gml_12_0")` and in
//! `lang/lang_*.json` files (key -> text). This module finds both.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::gml::{self, Tok};

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
    /// Byte range of the text literal in the source, for edits.
    pub span: Option<[usize; 2]>,
    pub key: Option<String>,
    pub speaker: Option<String>,
    /// Revision of the source the spans refer to.
    pub rev: String,
}

/// Finds dialogue calls in one GML source.
pub fn scan_source(code: &str, src: &str) -> Vec<DialogueLine> {
    let toks = gml::tokenize(src);
    let rev = gml::rev(src);
    let mut out = Vec::new();
    let mut speaker: Option<String> = None;
    for k in 0..toks.len() {
        let Some(ident) = toks[k].ident() else { continue };
        if !toks.get(k + 1).is_some_and(|t| t.is_punct('(')) || (k > 0 && toks[k - 1].is_punct('.')) {
            continue;
        }
        let is_speaker = SPEAKER_FUNCS.contains(&ident);
        let text_func = TEXT_FUNCS.iter().find(|(n, _)| *n == ident);
        if !is_speaker && text_func.is_none() {
            continue;
        }
        let Some(close) = gml::matching(&toks, k + 1) else { continue };
        let args = gml::split_args(&toks, k + 1, close);
        // A lone string literal argument: (value, byte span).
        let lit = |a: Option<&(usize, usize)>| match a {
            Some(&(x, y)) if y == x + 1 => match &toks[x].tok {
                Tok::Str(v) => Some((v.clone(), [toks[x].start, toks[x].end])),
                _ => None,
            },
            _ => None,
        };
        if is_speaker {
            speaker = lit(args.first()).map(|x| x.0);
            continue;
        }
        let (name, text_idx) = *text_func.unwrap();
        let localized = name.ends_with("loc") || name == "scr_84_get_lang_string";
        let key = if localized { lit(args.last()).map(|x| x.0) } else { None };
        let text = lit(args.get(text_idx));
        if text.is_none() && key.is_none() {
            continue;
        }
        out.push(DialogueLine {
            code: code.to_owned(),
            line: toks[k].line,
            func: name.into(),
            span: text.as_ref().map(|x| x.1),
            text: text.map(|x| x.0),
            key,
            speaker: speaker.clone(),
            rev: rev.clone(),
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
        let [a, b] = lines[0].span.unwrap();
        assert_eq!(&src[a..b], r#""\"Hey,\" Kris./""#);
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
