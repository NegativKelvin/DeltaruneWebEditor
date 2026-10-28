//! Mod conflict checker: finds assets that two or more mods change, and says if the changes can go together.
//!
//! A mod is a UTMT project folder (holds `project.json`), like `<workspace>/<chapter>/mod/`.
//! Each mod is compared with the extracted original (`base/`):
//! - code: changed line ranges (line diff). Different ranges can merge; the same or touching lines conflict.
//! - asset JSON: changed top-level fields. The same field with different values conflicts.
//! - side files (sprite frames, sounds, shader text): a file changed in two mods with different bytes conflicts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::store::{ASSET_TYPES, AssetType, ChapterStore};

pub struct Mod {
    pub label: String,
    pub dir: PathBuf,
}

/// Mods in `<workspace>/<chapter>/mods/*/`.
pub fn mods_folder(store: &ChapterStore) -> Vec<Mod> {
    let Ok(rd) = std::fs::read_dir(store.root.join("mods")) else {
        return Vec::new();
    };
    let mut out: Vec<Mod> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("project.json").is_file())
        .filter_map(|e| {
            Some(Mod {
                label: e.file_name().to_str()?.to_owned(),
                dir: e.path(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

pub fn check_mod_dir(dir: &Path) -> Result<()> {
    if dir.join("project.json").is_file() {
        Ok(())
    } else {
        Err(Error::BadRequest(format!(
            "'{}' is not a mod folder (no project.json)",
            dir.display()
        )))
    }
}

#[derive(Debug, Serialize)]
pub struct Finding {
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub name: String,
    /// Labels of the mods that change the asset.
    pub mods: Vec<String>,
    /// `same` (all changes are equal), `merge` (different parts), `conflict`.
    pub kind: &'static str,
    pub details: Vec<String>,
}

/// What one mod changed in one asset, compared with base.
#[derive(Default)]
struct Change {
    added: bool,
    /// Changed base line ranges `[a, b)` of the GML (code only).
    hunks: Vec<(usize, usize)>,
    gml: Option<String>,
    fields: BTreeMap<String, Value>,
    files: BTreeMap<String, Vec<u8>>,
}

fn read_json(p: &Path) -> Option<serde_json::Map<String, Value>> {
    match serde_json::from_slice(&std::fs::read(p).ok()?).ok()? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

/// Side files of an asset in one project dir: name -> path.
fn side_files(project: &Path, ty: &AssetType, name: &str) -> BTreeMap<String, PathBuf> {
    let json = project.join(ChapterStore::rel_json(ty, name));
    let dir = json.parent().unwrap_or(project).to_path_buf();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return BTreeMap::new();
    };
    rd.filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && *p != json)
        .filter(|p| ty.individual_dir || p.file_stem().and_then(|s| s.to_str()) == Some(name))
        .filter(|p| !(ty.slug == "code" && p.extension().is_some_and(|x| x == "gml")))
        .filter_map(|p| Some((p.file_name()?.to_str()?.to_owned(), p)))
        .collect()
}

fn change(base: &Path, m: &Path, ty: &AssetType, name: &str) -> Change {
    let rel = ChapterStore::rel_json(ty, name);
    let (bj, mj) = (base.join(&rel), m.join(&rel));
    let mut c = Change {
        added: !bj.is_file() && !(ty.slug == "code" && bj.with_extension("gml").is_file()),
        ..Default::default()
    };
    if let Some(mo) = read_json(&mj) {
        let bo = read_json(&bj).unwrap_or_default();
        for (k, v) in mo {
            if bo.get(&k) != Some(&v) {
                c.fields.insert(k, v);
            }
        }
    }
    if ty.slug == "code" {
        let mg = std::fs::read_to_string(mj.with_extension("gml")).ok();
        let bg = std::fs::read_to_string(bj.with_extension("gml")).unwrap_or_default();
        if let Some(mg) = &mg
            && *mg != bg
        {
            c.hunks = hunks(&bg.lines().collect::<Vec<_>>(), &mg.lines().collect::<Vec<_>>());
        }
        c.gml = mg;
    }
    let bf = side_files(base, ty, name);
    for (f, p) in side_files(m, ty, name) {
        let bytes = std::fs::read(&p).unwrap_or_default();
        if bf.get(&f).and_then(|b| std::fs::read(b).ok()).as_ref() != Some(&bytes) {
            c.files.insert(f, bytes);
        }
    }
    c
}

/// Base line ranges `[a, b)` that `new` changes (an insert is `(a, a)`).
/// Common start and end lines are cut first; a middle part too large for the table counts as one change.
pub fn hunks(old: &[&str], new: &[&str]) -> Vec<(usize, usize)> {
    let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suf = old[pre..]
        .iter()
        .rev()
        .zip(new[pre..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (a, b) = (&old[pre..old.len() - suf], &new[pre..new.len() - suf]);
    if a.is_empty() && b.is_empty() {
        return Vec::new();
    }
    if a.len().saturating_mul(b.len()) > 4_000_000 {
        return vec![(pre, old.len() - suf)];
    }
    // LCS table, filled from the end.
    let (n, m) = (a.len(), b.len());
    let w = m + 1;
    let mut t = vec![0u16; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i * w + j] = if a[i] == b[j] {
                t[(i + 1) * w + j + 1] + 1
            } else {
                t[(i + 1) * w + j].max(t[i * w + j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut open: Option<usize> = None;
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            if let Some(s) = open.take() {
                out.push((pre + s, pre + i));
            }
            i += 1;
            j += 1;
        } else {
            open.get_or_insert(i);
            if j < m && (i == n || t[i * w + j + 1] >= t[(i + 1) * w + j]) {
                j += 1;
            } else {
                i += 1;
            }
        }
    }
    if let Some(s) = open {
        out.push((pre + s, pre + n));
    }
    out
}

/// Two changed ranges touch or overlap (git also stops at edits on next lines).
fn touches(x: (usize, usize), y: (usize, usize)) -> bool {
    x.0 <= y.1 && y.0 <= x.1
}

fn lines(r: (usize, usize)) -> String {
    if r.1 <= r.0 + 1 {
        format!("line {}", r.0 + 1)
    } else {
        format!("lines {}-{}", r.0 + 1, r.1)
    }
}

/// Compares the changes of two mods in one asset: details of each conflict.
fn compare(x: &Change, y: &Change) -> Vec<String> {
    let mut d = Vec::new();
    if x.added && y.added {
        d.push("both mods add this asset".into());
    }
    if x.gml != y.gml && x.gml.is_some() && y.gml.is_some() {
        if x.added || y.added {
            if !(x.added && y.added) {
                d.push("code: one mod adds it, the other changes it".into());
            }
        } else {
            for hx in &x.hunks {
                if let Some(hy) = y.hunks.iter().find(|hy| touches(*hx, **hy)) {
                    d.push(format!("code: {} and {}", lines(*hx), lines(*hy)));
                }
            }
        }
    }
    for (k, v) in &x.fields {
        if y.fields.get(k).is_some_and(|w| w != v) {
            d.push(format!("field {k}"));
        }
    }
    for (f, v) in &x.files {
        if y.files.get(f).is_some_and(|w| w != v) {
            d.push(format!("file {f}"));
        }
    }
    d
}

/// Finds assets that two or more mods change.
pub fn check(base: &Path, mods: &[Mod]) -> Vec<Finding> {
    let mut out = Vec::new();
    for ty in ASSET_TYPES {
        let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, m) in mods.iter().enumerate() {
            for name in ChapterStore::scan(&m.dir, ty).into_keys() {
                by_name.entry(name).or_default().push(i);
            }
        }
        for (name, ids) in by_name.into_iter().filter(|(_, v)| v.len() > 1) {
            let changes: Vec<Change> = ids.iter().map(|&i| change(base, &mods[i].dir, ty, &name)).collect();
            let same = changes
                .windows(2)
                .all(|w| w[0].gml == w[1].gml && w[0].fields == w[1].fields && w[0].files == w[1].files);
            let mut details = BTreeSet::new();
            for a in 0..changes.len() {
                for b in a + 1..changes.len() {
                    for d in compare(&changes[a], &changes[b]) {
                        details.insert(if ids.len() > 2 {
                            format!("{} / {}: {d}", mods[ids[a]].label, mods[ids[b]].label)
                        } else {
                            d
                        });
                    }
                }
            }
            let kind = if same {
                "same"
            } else if details.is_empty() {
                "merge"
            } else {
                "conflict"
            };
            out.push(Finding {
                ty: ty.slug,
                name,
                mods: ids.iter().map(|&i| mods[i].label.clone()).collect(),
                kind,
                details: if same { Vec::new() } else { details.into_iter().collect() },
            });
        }
    }
    let rank = |k: &str| match k {
        "conflict" => 0,
        "merge" => 1,
        _ => 2,
    };
    out.sort_by(|a, b| {
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| a.ty.cmp(b.ty))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Asset count per mod.
pub fn count(m: &Mod) -> usize {
    ASSET_TYPES.iter().map(|t| ChapterStore::scan(&m.dir, t).len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_hunks() {
        let a = ["a", "b", "c", "d", "e"];
        assert_eq!(hunks(&a, &a), []);
        assert_eq!(hunks(&a, &["a", "B", "c", "d", "e"]), [(1, 2)]);
        assert_eq!(hunks(&a, &["a", "b", "x", "c", "d", "e"]), [(2, 2)]);
        assert_eq!(hunks(&a, &["a", "c", "d", "E"]), [(1, 2), (4, 5)]);
        assert_eq!(hunks(&[], &["x"]), [(0, 0)]);
        assert!(touches((1, 2), (2, 3)) && !touches((1, 2), (3, 4)));
    }

    fn write(p: &Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    #[test]
    fn finds_conflicts() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base");
        let src: String = (1..=20).map(|i| format!("line{i}\n")).collect();
        write(&base.join("code/gml_A.gml"), &src);
        write(&base.join("code/gml_B.gml"), &src);
        write(&base.join("rooms/room_a.json"), r#"{"$type":"Room","Width":320,"Height":240}"#);
        write(&base.join("sprites/spr_a/spr_a.json"), r#"{"$type":"Sprite"}"#);
        write(&base.join("sprites/spr_a/spr_a_0.png"), "0");
        let mk = |label: &str| {
            let dir = tmp.path().join(label);
            write(&dir.join("project.json"), "{}");
            Mod { label: label.into(), dir }
        };
        let (m1, m2) = (mk("one"), mk("two"));
        // gml_A: different lines (merge). gml_B: same line (conflict).
        write(&m1.dir.join("code/gml_A.gml"), &src.replace("line2\n", "X\n"));
        write(&m2.dir.join("code/gml_A.gml"), &src.replace("line15\n", "Y\n"));
        write(&m1.dir.join("code/gml_B.gml"), &src.replace("line5\n", "X\n"));
        write(&m2.dir.join("code/gml_B.gml"), &src.replace("line5\n", "Y\n"));
        // Room: different fields (merge). Sprite: same frame, other bytes (conflict).
        write(&m1.dir.join("rooms/room_a.json"), r#"{"$type":"Room","Width":640,"Height":240}"#);
        write(&m2.dir.join("rooms/room_a.json"), r#"{"$type":"Room","Width":320,"Height":480}"#);
        write(&m1.dir.join("sprites/spr_a/spr_a.json"), r#"{"$type":"Sprite"}"#);
        write(&m1.dir.join("sprites/spr_a/spr_a_0.png"), "1");
        write(&m2.dir.join("sprites/spr_a/spr_a.json"), r#"{"$type":"Sprite"}"#);
        write(&m2.dir.join("sprites/spr_a/spr_a_0.png"), "2");
        // New object in both, equal: same.
        write(&m1.dir.join("objects/obj_new.json"), r#"{"$type":"GameObject"}"#);
        write(&m2.dir.join("objects/obj_new.json"), r#"{"$type":"GameObject"}"#);
        // Only in one mod: not listed.
        write(&m1.dir.join("rooms/room_b.json"), r#"{"$type":"Room"}"#);

        let f = check(&base, &[m1, m2]);
        let get = |n: &str| f.iter().find(|x| x.name == n).unwrap();
        assert_eq!(f.len(), 5);
        assert_eq!(get("gml_A").kind, "merge");
        assert_eq!(get("gml_B").kind, "conflict");
        assert_eq!(get("gml_B").details, ["code: line 5 and line 5"]);
        assert_eq!(get("room_a").kind, "merge");
        assert_eq!(get("spr_a").kind, "conflict");
        assert_eq!(get("spr_a").details, ["file spr_a_0.png"]);
        assert_eq!(get("obj_new").kind, "same");
        assert_eq!(f[0].kind, "conflict");
        assert!(f.iter().all(|x| x.mods == ["one", "two"]));
    }
}
