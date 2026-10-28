//! `.dremod` mod package: a zip file with only the changes, never the game data file.
//!
//! ```text
//! dremod.json                  manifest (see `Manifest`)
//! <chapter>/project.json       UTMT project of the changes (from workspace/<chapter>/mod/)
//! <chapter>/<type>/...         changed and new assets, by name
//! <chapter>/lang/<file>.json   changed translation keys only
//! ```
//!
//! Install runs `UndertaleModCli project build` on the player's own data file, so the package works
//! on other game versions too while the changed assets still exist. Each asset has the hash of the
//! game asset it started from; the version check compares it with the player's game.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::error::{Error, Result};
use crate::langmod;
use crate::store::{ASSET_TYPES, ChapterStore, validate_name};
use crate::versions;

pub const FORMAT: u32 = 1;
pub const MANIFEST: &str = "dremod.json";
/// File name of an xdelta patch in a chapter dir of a package.
pub const XDELTA_FILE: &str = "patch.xdelta";
/// Limits for a package that is installed: total unpacked bytes and file count.
const MAX_UNPACKED: u64 = 8 << 30;
const MAX_FILES: usize = 500_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// File-name safe id. Installing a package with the same id replaces the old one.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// Editor version that made the package.
    #[serde(default)]
    pub editor: String,
    /// Unix time.
    #[serde(default)]
    pub created: u64,
    pub chapters: BTreeMap<String, ChapterManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterManifest {
    /// File name of the data file (`data.win`).
    pub data_file: String,
    /// The original data file that the mod was made on.
    pub game_sha256: Option<String>,
    pub game_size: Option<u64>,
    #[serde(default)]
    pub assets: Vec<AssetRef>,
    /// Lang files with changed keys.
    #[serde(default)]
    pub lang: Vec<String>,
    /// xdelta patch of the whole data file (`<chapter>/patch.xdelta`). It needs the exact game version
    /// `game_sha256`. Asset changes of other mods are built on its output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xdelta: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRef {
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
    /// Hash of the game asset that the change started from. `None`: a new asset.
    pub base: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportRequest {
    pub id: String,
    pub name: Option<String>,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    /// Chapters to put in the package. Default: every chapter with changes.
    pub chapters: Option<Vec<String>>,
}

/// One chapter to export: id, original data file, store.
pub struct ExportChapter {
    pub id: String,
    pub data_file: PathBuf,
    pub store: ChapterStore,
}

/// `launcher` or `chapterN`.
pub fn validate_chapter_id(id: &str) -> Result<()> {
    let ok = id == "launcher"
        || id
            .strip_prefix("chapter")
            .is_some_and(|n| !n.is_empty() && n.len() < 4 && n.bytes().all(|b| b.is_ascii_digit()));
    if ok {
        Ok(())
    } else {
        Err(Error::BadRequest(format!("invalid chapter id '{id}'")))
    }
}

/// A package file path is allowed: `<chapter>/project.json`, `<chapter>/lang/<f>.json`, `<chapter>/<type>/...`.
fn allowed_path(parts: &[&str], m: &Manifest) -> bool {
    let Some((ch, rest)) = parts.split_first() else { return false };
    if !m.chapters.contains_key(*ch) || parts.iter().any(|p| validate_name(p).is_err()) {
        return false;
    }
    match rest {
        ["project.json"] => true,
        ["patch.xdelta"] => m.chapters[*ch].xdelta.is_some(),
        ["lang", f] => langmod::validate_file(f).is_ok(),
        [ty, _] => ASSET_TYPES.iter().any(|t| t.slug == *ty && !t.individual_dir),
        [ty, _, _] => ASSET_TYPES.iter().any(|t| t.slug == *ty && t.individual_dir),
        _ => false,
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Writes `<out_dir>/<id>-<version>.dremod`. Returns its path and manifest.
pub fn export(out_dir: &Path, req: &ExportRequest, chapters: &[ExportChapter]) -> Result<(PathBuf, Manifest)> {
    validate_name(&req.id)?;
    if !req.version.is_empty() {
        validate_name(&req.version)?;
    }
    let mut manifest = Manifest {
        format: FORMAT,
        id: req.id.clone(),
        name: req.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| req.id.clone()),
        version: req.version.clone(),
        author: req.author.clone(),
        description: req.description.clone(),
        editor: env!("CARGO_PKG_VERSION").into(),
        created: now(),
        chapters: BTreeMap::new(),
    };
    // (path in zip, file on disk)
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for c in chapters {
        validate_chapter_id(&c.id)?;
        let rec = versions::record_missing(&c.store)?;
        let assets: Vec<AssetRef> = c
            .store
            .changed_assets()
            .into_iter()
            .map(|(t, e)| AssetRef {
                base: rec.get(&versions::key(t, &e.name)).cloned().flatten(),
                ty: t.slug.into(),
                name: e.name,
            })
            .collect();
        let lang: Vec<String> = langmod::list(&c.store)
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .map(|(f, _)| f)
            .collect();
        if assets.is_empty() && lang.is_empty() {
            continue;
        }
        if !assets.is_empty() {
            let m = c.store.mod_dir();
            for e in WalkDir::new(&m)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                let rel = e.path().strip_prefix(&m).unwrap();
                let rel: Vec<&str> = rel.iter().filter_map(|p| p.to_str()).collect();
                let zip_path = format!("{}/{}", c.id, rel.join("/"));
                files.push((zip_path, e.path().to_path_buf()));
            }
        }
        for f in &lang {
            files.push((format!("{}/lang/{f}", c.id), c.store.lang_dir().join(f)));
        }
        let meta = std::fs::metadata(&c.data_file).ok();
        manifest.chapters.insert(
            c.id.clone(),
            ChapterManifest {
                data_file: c.data_file.file_name().and_then(|f| f.to_str()).unwrap_or("data.win").into(),
                game_sha256: crate::hash::file_sha256(&c.data_file).ok(),
                game_size: meta.map(|m| m.len()),
                assets,
                lang,
                xdelta: None,
            },
        );
    }
    if manifest.chapters.is_empty() {
        return Err(Error::BadRequest("no changes to export".into()));
    }
    files.retain(|(p, _)| allowed_path(&p.split('/').collect::<Vec<_>>(), &manifest));

    std::fs::create_dir_all(out_dir)?;
    let file_name = if req.version.is_empty() {
        format!("{}.dremod", req.id)
    } else {
        format!("{}-{}.dremod", req.id, req.version)
    };
    let out = out_dir.join(&file_name);
    let tmp = out_dir.join(format!("{file_name}.tmp"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&tmp)?);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let zerr = |e: zip::result::ZipError| Error::Other(format!("zip: {e}"));
    zip.start_file(MANIFEST, opts).map_err(zerr)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    for (p, disk) in &files {
        zip.start_file(p.as_str(), opts).map_err(zerr)?;
        std::io::copy(&mut std::fs::File::open(disk)?, &mut zip)?;
    }
    zip.finish().map_err(zerr)?;
    std::fs::rename(&tmp, &out)?;
    Ok((out, manifest))
}

fn bad(e: impl std::fmt::Display) -> Error {
    Error::BadRequest(format!("not a valid .dremod package: {e}"))
}

fn check_manifest(m: &Manifest) -> Result<()> {
    if m.format > FORMAT {
        return Err(Error::BadRequest(format!(
            "package format {} is newer than this editor ({FORMAT}); update the editor",
            m.format
        )));
    }
    validate_name(&m.id)?;
    for (ch, c) in &m.chapters {
        validate_chapter_id(ch)?;
        validate_name(&c.data_file)?;
        for a in &c.assets {
            crate::store::asset_type(&a.ty)?;
            validate_name(&a.name)?;
        }
        for f in &c.lang {
            langmod::validate_file(f)?;
        }
        if c.xdelta.as_deref().is_some_and(|x| x != XDELTA_FILE) {
            return Err(Error::BadRequest(format!("xdelta file must be '{XDELTA_FILE}'")));
        }
    }
    Ok(())
}

/// Reads the manifest of a package file.
pub fn read_manifest(pkg: &Path) -> Result<Manifest> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pkg)?).map_err(bad)?;
    let mut f = zip.by_name(MANIFEST).map_err(bad)?;
    let mut buf = Vec::new();
    f.by_ref().take(1 << 20).read_to_end(&mut buf)?;
    let m: Manifest = serde_json::from_slice(&buf).map_err(bad)?;
    check_manifest(&m)?;
    Ok(m)
}

/// Unpacks a package into the empty dir `dst`. Files outside the layout are not written.
pub fn unpack(pkg: &Path, dst: &Path) -> Result<Manifest> {
    let m = read_manifest(pkg)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pkg)?).map_err(bad)?;
    if zip.len() > MAX_FILES {
        return Err(bad("too many files"));
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(bad)?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_owned();
        if name == MANIFEST {
            continue;
        }
        let parts: Vec<&str> = name.split('/').collect();
        if !allowed_path(&parts, &m) {
            return Err(bad(format!("unexpected file '{name}'")));
        }
        let out = parts.iter().fold(dst.to_path_buf(), |p, c| p.join(c));
        std::fs::create_dir_all(out.parent().unwrap())?;
        let left = MAX_UNPACKED.saturating_sub(total);
        let n = std::io::copy(&mut f.by_ref().take(left + 1), &mut std::fs::File::create(&out)?)?;
        total += n;
        if total > MAX_UNPACKED {
            return Err(bad("unpacked size is too large"));
        }
    }
    std::fs::write(dst.join(MANIFEST), serde_json::to_vec_pretty(&m)?)?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn export_and_unpack() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ChapterStore::new(tmp.path(), "chapter1");
        std::fs::create_dir_all(s.base().join("sprites/spr_a")).unwrap();
        std::fs::write(s.base().join("project.json"), r#"{"Name":"D"}"#).unwrap();
        std::fs::write(s.base().join("sprites/spr_a/spr_a.json"), r#"{"$type":"Sprite"}"#).unwrap();
        std::fs::write(s.base().join("sprites/spr_a/spr_a_0.png"), b"0").unwrap();
        let spr = crate::store::asset_type("sprites").unwrap();
        s.write_file(spr, "spr_a", "spr_a_0.png", b"1").unwrap();
        let code = crate::store::asset_type("code").unwrap();
        s.write(code, "gml_Script_new", None, Some("return 1;".into())).unwrap();
        let Value::Object(o) = json!({"k": "v"}) else { panic!() };
        langmod::merge(&s, "lang_ja.json", o).unwrap();
        let game = tmp.path().join("data.win");
        std::fs::write(&game, b"FORM").unwrap();
        let req = ExportRequest {
            id: "my_mod".into(),
            name: Some("My mod".into()),
            version: "1.0".into(),
            author: String::new(),
            description: String::new(),
            chapters: None,
        };
        let chapters = [ExportChapter {
            id: "chapter1".into(),
            data_file: game,
            store: s.clone(),
        }];
        let (pkg, m) = export(&tmp.path().join("out"), &req, &chapters).unwrap();
        assert!(pkg.ends_with("my_mod-1.0.dremod"));
        let c = &m.chapters["chapter1"];
        assert_eq!(c.assets.len(), 2);
        let sprite = c.assets.iter().find(|a| a.ty == "sprites").unwrap();
        assert!(sprite.base.is_some());
        assert!(c.assets.iter().any(|a| a.ty == "code" && a.base.is_none()));
        assert_eq!(c.lang, ["lang_ja.json"]);
        assert_eq!(c.game_size, Some(4));

        let dst = tmp.path().join("unpacked");
        let m2 = unpack(&pkg, &dst).unwrap();
        assert_eq!(m2.id, "my_mod");
        assert_eq!(std::fs::read(dst.join("chapter1/sprites/spr_a/spr_a_0.png")).unwrap(), b"1");
        assert!(dst.join("chapter1/project.json").is_file());
        assert!(dst.join("chapter1/code/gml_Script_new.gml").is_file());
        assert!(dst.join("chapter1/lang/lang_ja.json").is_file());
        assert!(!dst.join("chapter1/data.win").exists());

        let empty = ChapterStore::new(tmp.path(), "chapter2");
        let none = [ExportChapter {
            id: "chapter2".into(),
            data_file: tmp.path().join("x"),
            store: empty,
        }];
        assert!(export(&tmp.path().join("out"), &req, &none).is_err());
    }

    #[test]
    fn rejects_bad_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("bad.dremod");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&pkg).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file(MANIFEST, opts).unwrap();
        zip.write_all(
            br#"{"format":1,"id":"x","name":"x","chapters":{"chapter1":{"data_file":"data.win","game_sha256":null,"game_size":null}}}"#,
        )
        .unwrap();
        zip.start_file("chapter1/../../evil.txt", opts).unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
        assert!(read_manifest(&pkg).is_ok());
        assert!(unpack(&pkg, &tmp.path().join("d")).is_err());
        assert!(!tmp.path().join("evil.txt").exists());
        assert!(validate_chapter_id("chapter3").is_ok() && validate_chapter_id("..").is_err());
    }
}
