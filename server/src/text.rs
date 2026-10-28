//! Text tools: line length check against the text box, translation coverage, and find and replace in all dialogue.
//!
//! The webpage measures text with the same rules (`measureText` in `web/js/editors/textpreview.js`).
//! `tests/fixtures/text_cases.json` holds cases that both must pass.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dialogue::{DialogueLine, Langs};
use crate::error::{Error, Result};
use crate::gml;

/// Text box limits. Defaults are for the Deltarune dialogue box with the default font.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Limits {
    /// Characters per line without a face (wide characters count 2).
    pub chars: usize,
    /// Characters per line when a face shows.
    pub face_chars: usize,
    /// Lines per box.
    pub lines: usize,
    /// Pixels per line without a face, when a font is given. 0: use `chars`.
    pub px: f64,
    /// Pixels per line when a face shows.
    pub face_px: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            chars: 33,
            face_chars: 26,
            lines: 3,
            px: 0.0,
            face_px: 0.0,
        }
    }
}

/// Character advances of a font (`Glyphs[].Shift` of the UTMT font JSON).
#[derive(Debug, Clone, Default)]
pub struct Font {
    pub advance: HashMap<u32, f64>,
    /// Advance for characters that the font does not have.
    pub fallback: f64,
}

impl Font {
    pub fn from_json(v: &Value) -> Result<Self> {
        let glyphs = v
            .get("Glyphs")
            .and_then(Value::as_array)
            .filter(|g| !g.is_empty())
            .ok_or_else(|| Error::BadRequest("the font has no glyphs".into()))?;
        let advance: HashMap<u32, f64> = glyphs
            .iter()
            .filter_map(|g| Some((g.get("Character")?.as_u64()? as u32, g.get("Shift")?.as_f64()?)))
            .collect();
        let fallback = advance
            .get(&('M' as u32))
            .copied()
            .unwrap_or_else(|| advance.values().copied().fold(0.0, f64::max));
        Ok(Self { advance, fallback })
    }

    fn width(&self, s: &str) -> f64 {
        s.chars()
            .map(|c| self.advance.get(&(c as u32)).copied().unwrap_or(self.fallback))
            .sum()
    }
}

/// East Asian wide characters take two cells in the text box.
pub fn char_cells(c: char) -> usize {
    let u = c as u32;
    let wide = matches!(u, 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6);
    if wide { 2 } else { 1 }
}

pub fn cells(s: &str) -> usize {
    s.chars().map(char_cells).sum()
}

/// Visible text of each line, per box. Control codes are removed: `\cY`, `\E1` (a backslash, a letter, one more character),
/// `^1` pauses, and `/` `%` at the end. `&` and newlines start a new line. `~1` shows as `[1]`, as in the preview.
pub fn visible(text: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = text.chars().collect();
    let mut boxes = vec![vec![String::new()]];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let line = boxes.last_mut().unwrap().last_mut().unwrap();
        if c == '\\' && next.is_some_and(|x| x.is_ascii_alphabetic()) && chars.get(i + 2).is_some_and(|x| x.is_ascii_alphanumeric()) {
            i += 3;
            continue;
        }
        if c == '^' && next.is_some_and(|x| x.is_ascii_digit()) {
            i += 2;
            continue;
        }
        if c == '&' || c == '\n' {
            boxes.last_mut().unwrap().push(String::new());
        } else if (c == '/' || c == '%') && chars[i..].iter().all(|x| *x == '/' || *x == '%') {
            break;
        } else if c == '~' && next.is_some_and(|x| x.is_ascii_digit()) {
            line.push('[');
            line.push(next.unwrap());
            line.push(']');
            i += 2;
            continue;
        } else {
            line.push(c);
        }
        i += 1;
    }
    boxes
}

/// A face shows: the speaker is a character, or the text changes the face (`\E`, `\F`).
pub fn has_face(text: &str, speaker: Option<&str>) -> bool {
    speaker.is_some_and(|s| !s.is_empty() && s != "no_name") || text.contains("\\E") || text.contains("\\F")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Problem {
    /// 0-based box and line.
    #[serde(rename = "box")]
    pub bx: usize,
    pub line: usize,
    /// `width` (line too long) or `lines` (too many lines in the box).
    pub kind: &'static str,
    pub value: f64,
    pub limit: f64,
    pub text: String,
}

/// Checks one text against the box limits.
pub fn check(text: &str, face: bool, lim: &Limits, font: Option<&Font>) -> Vec<Problem> {
    let (chars, px) = if face { (lim.face_chars, lim.face_px) } else { (lim.chars, lim.px) };
    let mut out = Vec::new();
    for (bi, lines) in visible(text).iter().enumerate() {
        for (li, l) in lines.iter().enumerate() {
            let (value, limit) = match font {
                Some(f) if px > 0.0 => (f.width(l), px),
                _ => (cells(l) as f64, chars as f64),
            };
            if value > limit {
                out.push(Problem {
                    bx: bi,
                    line: li,
                    kind: "width",
                    value,
                    limit,
                    text: l.clone(),
                });
            }
        }
        if lines.len() > lim.lines {
            out.push(Problem {
                bx: bi,
                line: lim.lines,
                kind: "lines",
                value: lines.len() as f64,
                limit: lim.lines as f64,
                text: lines[lim.lines..].join(" / "),
            });
        }
    }
    out
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckItem {
    pub code: String,
    pub line: usize,
    pub func: String,
    pub key: Option<String>,
    pub speaker: Option<String>,
    /// `en` for the text in the GML, else the language of `lang/lang_<x>.json`.
    pub lang: String,
    pub text: String,
    pub problems: Vec<Problem>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Coverage {
    /// Lines with a key.
    pub keys: usize,
    /// Keys that the language file does not have.
    pub missing: usize,
    /// Keys whose text is the same as the English text.
    pub same: usize,
    /// First missing keys.
    pub missing_keys: Vec<String>,
}

/// Checks all dialogue lines (and their translations) against the limits. `langs`: which languages to check, empty for all.
pub fn check_all(
    lines: &[DialogueLine],
    tr: &Langs,
    lim: &Limits,
    font: Option<&Font>,
    only: &[String],
) -> (Vec<CheckItem>, BTreeMap<String, Coverage>) {
    let want = |l: &str| only.is_empty() || only.iter().any(|x| x == l);
    let mut items = Vec::new();
    let mut cov: BTreeMap<String, Coverage> = tr.keys().filter(|l| want(l)).map(|l| (l.clone(), Coverage::default())).collect();
    let mut push = |dl: &DialogueLine, lang: &str, text: &str| {
        let face = has_face(text, dl.speaker.as_deref()) || dl.text.as_deref().is_some_and(|t| has_face(t, None));
        let problems = check(text, face, lim, font);
        if !problems.is_empty() {
            items.push(CheckItem {
                code: dl.code.clone(),
                line: dl.line,
                func: dl.func.clone(),
                key: dl.key.clone(),
                speaker: dl.speaker.clone(),
                lang: lang.to_owned(),
                text: text.to_owned(),
                problems,
            });
        }
    };
    for dl in lines {
        if want("en")
            && let Some(t) = &dl.text
        {
            push(dl, "en", t);
        }
        let Some(k) = &dl.key else { continue };
        for (lang, table) in tr {
            if !want(lang) {
                continue;
            }
            let c = cov.get_mut(lang).unwrap();
            c.keys += 1;
            match table.get(k) {
                Some(t) => {
                    if dl.text.as_ref() == Some(t) {
                        c.same += 1;
                    }
                    push(dl, lang, t);
                }
                None => {
                    c.missing += 1;
                    if c.missing_keys.len() < 200 {
                        c.missing_keys.push(k.clone());
                    }
                }
            }
        }
    }
    (items, cov)
}

/// Find and replace in the text of all dialogue lines.
#[derive(Debug, Clone, Deserialize)]
pub struct Replace {
    pub find: String,
    #[serde(default)]
    pub replace: String,
    /// `find` is a regular expression; `replace` can use `$1`.
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub ignore_case: bool,
    /// Only code entries whose name has this text.
    #[serde(default)]
    pub code: String,
    /// Only lines with this speaker.
    #[serde(default)]
    pub speaker: String,
    /// Return the changes, write nothing.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Change {
    pub code: String,
    pub line: usize,
    pub func: String,
    pub before: String,
    pub after: String,
    #[serde(skip)]
    pub span: [usize; 2],
}

impl Replace {
    fn matcher(&self) -> Result<regex::Regex> {
        if self.find.is_empty() {
            return Err(Error::BadRequest("'find' is empty".into()));
        }
        let pat = if self.regex { self.find.clone() } else { regex::escape(&self.find) };
        regex::RegexBuilder::new(&pat)
            .case_insensitive(self.ignore_case)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| Error::BadRequest(format!("find: {e}")))
    }

    /// The changes to `lines` (lines with an editable string literal only).
    pub fn changes(&self, lines: &[DialogueLine]) -> Result<Vec<Change>> {
        let re = self.matcher()?;
        let mut out = Vec::new();
        for l in lines {
            let (Some(text), Some(span)) = (&l.text, l.span) else { continue };
            if (!self.code.is_empty() && !l.code.contains(&self.code))
                || (!self.speaker.is_empty() && l.speaker.as_deref() != Some(&self.speaker))
            {
                continue;
            }
            let after = if self.regex {
                re.replace_all(text, self.replace.as_str())
            } else {
                re.replace_all(text, regex::NoExpand(&self.replace))
            };
            if after != *text {
                out.push(Change {
                    code: l.code.clone(),
                    line: l.line,
                    func: l.func.clone(),
                    before: text.clone(),
                    after: after.into_owned(),
                    span,
                });
            }
        }
        Ok(out)
    }
}

/// Span edits for the changes of one code entry.
pub fn edits(changes: &[&Change]) -> Vec<gml::Edit> {
    changes
        .iter()
        .map(|c| gml::Edit {
            start: c.span[0],
            end: c.span[1],
            text: None,
            string: Some(c.after.clone()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue::scan_source;

    #[derive(Deserialize)]
    struct Case {
        text: String,
        #[serde(default)]
        face: bool,
        visible: Vec<Vec<String>>,
        problems: usize,
    }

    #[test]
    fn shared_cases() {
        let cases: Vec<Case> = serde_json::from_str(include_str!("../tests/fixtures/text_cases.json")).unwrap();
        for c in cases {
            assert_eq!(visible(&c.text), c.visible, "{}", c.text);
            assert_eq!(check(&c.text, c.face, &Limits::default(), None).len(), c.problems, "{}", c.text);
        }
    }

    #[test]
    fn font_width_and_faces() {
        let f = Font::from_json(&serde_json::json!({ "Glyphs": [{ "Character": 65, "Shift": 10 }, { "Character": 77, "Shift": 12 }] }))
            .unwrap();
        assert_eq!(f.width("AAx"), 32.0);
        let lim = Limits {
            px: 25.0,
            ..Limits::default()
        };
        assert_eq!(check("AA&AAA/%", false, &lim, Some(&f)).len(), 1);
        assert!(Font::from_json(&serde_json::json!({})).is_err());
        assert!(has_face("x", Some("susie")) && !has_face("x", Some("no_name")) && has_face("\\E2 x", None));
        assert_eq!(cells("あa"), 3);
    }

    #[test]
    fn check_all_and_coverage() {
        let src = "scr_speaker(\"susie\");\nmsgsetloc(0, \"* 12345678901234567890123456789/%\", \"k1\");\nmsgnextloc(\"* ok&* two&* three&* four/%\", \"k2\");\n";
        let lines = scan_source("c", src);
        let tr: Langs = BTreeMap::from([("ja".to_owned(), BTreeMap::from([("k1".to_owned(), "* ok/%".to_owned())]))]);
        let (items, cov) = check_all(&lines, &tr, &Limits::default(), None, &[]);
        // Line 1: 31 cells > 26 (face). Line 2: four lines > 3.
        assert_eq!(items.len(), 2, "{items:#?}");
        assert_eq!(items[1].problems[0].kind, "lines");
        assert_eq!(
            (cov["ja"].keys, cov["ja"].missing, cov["ja"].missing_keys.clone()),
            (2, 1, vec!["k2".to_owned()])
        );
        let (items, _) = check_all(&lines, &tr, &Limits::default(), None, &["ja".to_owned()]);
        assert!(items.is_empty());
    }

    #[test]
    fn replace_plain_and_regex() {
        let src = "msgsetloc(0, \"* Kris, hi./%\", \"k1\");\nmsgnextloc(\"* KRIS?/%\", \"k2\");\nx = stringsetloc(\"$1\", \"k3\");\n";
        let lines = scan_source("c", src);
        let r = Replace {
            find: "kris".into(),
            replace: "$0 Dreemurr".into(),
            regex: false,
            ignore_case: true,
            code: String::new(),
            speaker: String::new(),
            dry_run: true,
        };
        let ch = r.changes(&lines).unwrap();
        assert_eq!(
            ch.iter().map(|c| c.after.as_str()).collect::<Vec<_>>(),
            ["* $0 Dreemurr, hi./%", "* $0 Dreemurr?/%"]
        );
        let r = Replace {
            find: r"\* (\w+)".into(),
            replace: "* [$1]".into(),
            regex: true,
            ignore_case: false,
            ..r
        };
        let ch = r.changes(&lines).unwrap();
        assert_eq!(ch[0].after, "* [Kris], hi./%");
        let out = gml::apply_edits(src, &edits(&ch.iter().collect::<Vec<_>>())).unwrap();
        assert!(out.contains("msgnextloc(\"* [KRIS]?/%\""), "{out}");
        assert!(
            Replace {
                find: "(".into(),
                ..r.clone()
            }
            .changes(&lines)
            .is_err()
        );
        assert!(Replace { find: String::new(), ..r }.changes(&lines).is_err());
    }
}
