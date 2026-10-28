//! On-disk asset store for one chapter.
//!
//! ```text
//! <workspace>/<chapter>/
//!   base/      full UTMT project export (read-only reference)
//!   generic/   generic JSON dump of every list in the data file (read-only)
//!   mod/       UTMT project that holds only edited or new assets
//!   build/     output of `project build`
//! ```
//!
//! Reads look in `mod/` first, then `base/`. Writes go to `mod/` only, so
//! `UndertaleModCli project build mod/project.json` applies only the changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use walkdir::WalkDir;

use crate::error::{Error, Result};

/// One UTMT serializable asset type.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AssetType {
    /// URL slug and directory name in the project.
    pub slug: &'static str,
    /// `$type` discriminator in the asset JSON.
    pub json_type: &'static str,
    /// The asset has its own directory (side files: png, ogg, shader text, ...).
    pub individual_dir: bool,
}

pub const ASSET_TYPES: &[AssetType] = &[
    AssetType {
        slug: "rooms",
        json_type: "Room",
        individual_dir: false,
    },
    AssetType {
        slug: "objects",
        json_type: "GameObject",
        individual_dir: false,
    },
    AssetType {
        slug: "code",
        json_type: "Code",
        individual_dir: false,
    },
    AssetType {
        slug: "scripts",
        json_type: "Script",
        individual_dir: false,
    },
    AssetType {
        slug: "sprites",
        json_type: "Sprite",
        individual_dir: true,
    },
    AssetType {
        slug: "backgrounds",
        json_type: "Background",
        individual_dir: true,
    },
    AssetType {
        slug: "sounds",
        json_type: "Sound",
        individual_dir: true,
    },
    AssetType {
        slug: "fonts",
        json_type: "Font",
        individual_dir: true,
    },
    AssetType {
        slug: "shaders",
        json_type: "Shader",
        individual_dir: true,
    },
    AssetType {
        slug: "paths",
        json_type: "Path",
        individual_dir: false,
    },
    AssetType {
        slug: "sequences",
        json_type: "Sequence",
        individual_dir: false,
    },
    AssetType {
        slug: "animcurves",
        json_type: "AnimationCurve",
        individual_dir: false,
    },
];

pub fn asset_type(slug: &str) -> Result<&'static AssetType> {
    ASSET_TYPES
        .iter()
        .find(|t| t.slug == slug)
        .ok_or_else(|| Error::NotFound(format!("asset type '{slug}'")))
}

/// Accepts asset and file names made by UTMT (`MakeValidFilenameIdentifier`) plus `.`/`-` for side files.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if ok {
        Ok(())
    } else {
        Err(Error::BadRequest(format!("invalid name '{name}'")))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetEntry {
    pub name: String,
    pub modified: bool,
    /// The asset is only in `mod/` (new asset).
    pub added: bool,
}

#[derive(Debug, Clone)]
pub struct ChapterStore {
    pub root: PathBuf,
}

impl ChapterStore {
    pub fn new(workspace: &Path, chapter_id: &str) -> Self {
        Self {
            root: workspace.join(chapter_id),
        }
    }
    pub fn base(&self) -> PathBuf {
        self.root.join("base")
    }
    pub fn generic(&self) -> PathBuf {
        self.root.join("generic")
    }
    pub fn mod_dir(&self) -> PathBuf {
        self.root.join("mod")
    }
    pub fn build_dir(&self) -> PathBuf {
        self.root.join("build")
    }
    pub fn is_extracted(&self) -> bool {
        self.base().join("project.json").is_file()
    }

    /// The asset exists at this JSON path. Code may have only `<name>.gml` (UTMT omits default JSON).
    fn exists(json: &Path, ty: &AssetType) -> bool {
        json.is_file() || (ty.slug == "code" && json.with_extension("gml").is_file())
    }

    /// Maps asset name -> JSON path for one type in one project dir.
    fn scan(project: &Path, ty: &AssetType) -> BTreeMap<String, PathBuf> {
        let dir = project.join(ty.slug);
        let depth = if ty.individual_dir { 2 } else { 1 };
        WalkDir::new(&dir)
            .min_depth(depth)
            .max_depth(depth)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path()
                        .extension()
                        .is_some_and(|x| x == "json" || (ty.slug == "code" && x == "gml"))
            })
            .filter_map(|e| Some((e.path().file_stem()?.to_str()?.to_owned(), e.path().with_extension("json"))))
            .collect()
    }

    pub fn list(&self, ty: &AssetType) -> Vec<AssetEntry> {
        let base = Self::scan(&self.base(), ty);
        let modded = Self::scan(&self.mod_dir(), ty);
        let mut names: Vec<&String> = base.keys().chain(modded.keys()).collect();
        names.sort();
        names.dedup();
        names
            .into_iter()
            .map(|n| AssetEntry {
                name: n.clone(),
                modified: modded.contains_key(n),
                added: !base.contains_key(n),
            })
            .collect()
    }

    pub fn counts(&self) -> BTreeMap<&'static str, usize> {
        ASSET_TYPES.iter().map(|t| (t.slug, self.list(t).len())).collect()
    }

    /// Path of the asset JSON relative to a project dir.
    fn rel_json(ty: &AssetType, name: &str) -> PathBuf {
        if ty.individual_dir {
            PathBuf::from(ty.slug).join(name).join(format!("{name}.json"))
        } else {
            PathBuf::from(ty.slug).join(format!("{name}.json"))
        }
    }

    /// Resolves the JSON file for an asset: mod first, then base.
    fn resolve(&self, ty: &AssetType, name: &str) -> Result<(PathBuf, bool)> {
        validate_name(name)?;
        let rel = Self::rel_json(ty, name);
        let m = self.mod_dir().join(&rel);
        if Self::exists(&m, ty) {
            return Ok((m, true));
        }
        let b = self.base().join(&rel);
        if Self::exists(&b, ty) {
            return Ok((b, false));
        }
        Err(Error::NotFound(format!("{}/{name}", ty.slug)))
    }

    /// Side files that belong to an asset (gml source, png frames, shader text, audio, ...).
    fn side_files(json_path: &Path, ty: &AssetType, name: &str) -> Vec<PathBuf> {
        let dir = json_path.parent().unwrap_or(Path::new("."));
        let Ok(rd) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p != json_path)
            .filter(|p| {
                // Flat types share a dir; only keep `<name>.<ext>` files. Individual dirs own all files.
                ty.individual_dir || p.file_stem().and_then(|s| s.to_str()) == Some(name)
            })
            .collect();
        files.sort();
        files
    }

    pub fn read(&self, ty: &AssetType, name: &str) -> Result<Value> {
        let (path, modified) = self.resolve(ty, name)?;
        let data: Value = if path.is_file() {
            serde_json::from_slice(&std::fs::read(&path)?)?
        } else {
            serde_json::json!({ "$type": ty.json_type })
        };
        let files: Vec<String> = Self::side_files(&path, ty, name)
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(str::to_owned))
            .collect();
        let mut out = serde_json::json!({
            "type": ty.slug,
            "name": name,
            "modified": modified,
            "data": data,
            "files": files,
        });
        if ty.slug == "code" {
            let gml = path.with_extension("gml");
            let src = std::fs::read_to_string(gml).unwrap_or_default();
            out["rev"] = Value::String(crate::gml::rev(&src));
            out["source"] = Value::String(src);
        }
        Ok(out)
    }

    /// Copies an existing base asset into mod/ so it can be changed there.
    fn ensure_in_mod(&self, ty: &AssetType, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        self.ensure_mod_project()?;
        let rel = Self::rel_json(ty, name);
        let dst = self.mod_dir().join(&rel);
        if Self::exists(&dst, ty) {
            return Ok(dst);
        }
        let src = self.base().join(&rel);
        std::fs::create_dir_all(dst.parent().unwrap())?;
        if Self::exists(&src, ty) {
            for f in Self::side_files(&src, ty, name)
                .into_iter()
                .chain(src.is_file().then(|| src.clone()))
            {
                std::fs::copy(&f, dst.parent().unwrap().join(f.file_name().unwrap()))?;
            }
        }
        Ok(dst)
    }

    /// Makes `mod/project.json` from the base one, if missing.
    pub fn ensure_mod_project(&self) -> Result<()> {
        let dst = self.mod_dir().join("project.json");
        if dst.is_file() {
            return Ok(());
        }
        let src = self.base().join("project.json");
        if !src.is_file() {
            return Err(Error::Conflict("chapter is not extracted".into()));
        }
        let mut project: Value = serde_json::from_slice(&std::fs::read(src)?)?;
        if let Some(name) = project.get("Name").and_then(Value::as_str) {
            project["Name"] = Value::String(format!("{name} (mod)"));
        }
        std::fs::create_dir_all(self.mod_dir())?;
        std::fs::write(dst, serde_json::to_vec_pretty(&project)?)?;
        Ok(())
    }

    /// Writes asset JSON (and gml source for code) into mod/.
    pub fn write(&self, ty: &AssetType, name: &str, data: Option<Value>, source: Option<String>) -> Result<Value> {
        let path = self.ensure_in_mod(ty, name)?;
        if let Some(mut data) = data {
            let obj = data
                .as_object_mut()
                .ok_or_else(|| Error::BadRequest("data must be a JSON object".into()))?;
            match obj.get("$type").and_then(Value::as_str) {
                None => {
                    obj.insert("$type".into(), Value::String(ty.json_type.into()));
                }
                Some(t) if t != ty.json_type => {
                    return Err(Error::BadRequest(format!("$type '{t}' does not match '{}'", ty.json_type)));
                }
                Some(_) => {}
            }
            // `$type` must stay the first key for System.Text.Json polymorphic reads.
            let t = obj.remove("$type").unwrap();
            let mut ordered = serde_json::Map::with_capacity(obj.len() + 1);
            ordered.insert("$type".into(), t);
            ordered.append(obj);
            data = Value::Object(ordered);
            std::fs::write(&path, serde_json::to_vec_pretty(&data)?)?;
        } else if !Self::exists(&path, ty) && !(ty.slug == "code" && source.is_some()) {
            return Err(Error::BadRequest(format!("new asset needs 'data' ($type {})", ty.json_type)));
        }
        if let Some(src) = source {
            if ty.slug != "code" {
                return Err(Error::BadRequest("'source' is only for code assets".into()));
            }
            std::fs::write(path.with_extension("gml"), src)?;
        }
        self.read(ty, name)
    }

    /// Path of a side file, mod first then base.
    pub fn file_path(&self, ty: &AssetType, name: &str, file: &str) -> Result<PathBuf> {
        validate_name(file)?;
        let (json, _) = self.resolve(ty, name)?;
        Self::side_files(&json, ty, name)
            .into_iter()
            .find(|p| p.file_name().and_then(|f| f.to_str()) == Some(file))
            .ok_or_else(|| Error::NotFound(format!("{}/{name}/{file}", ty.slug)))
    }

    /// Writes a side file into mod/. Flat types only accept `<name>.<ext>`.
    pub fn write_file(&self, ty: &AssetType, name: &str, file: &str, bytes: &[u8]) -> Result<()> {
        validate_name(file)?;
        if !ty.individual_dir && Path::new(file).file_stem().and_then(|s| s.to_str()) != Some(name) {
            return Err(Error::BadRequest(format!(
                "file for {}/{name} must be named '{name}.<ext>'",
                ty.slug
            )));
        }
        if file.ends_with(".json") {
            return Err(Error::BadRequest("write asset JSON through the asset endpoint".into()));
        }
        let json = self.ensure_in_mod(ty, name)?;
        if !Self::exists(&json, ty) {
            return Err(Error::NotFound(format!("{}/{name}", ty.slug)));
        }
        std::fs::write(json.parent().unwrap().join(file), bytes)?;
        Ok(())
    }

    /// Removes a side file from the mod copy of an asset. The base file stays, but reads no longer see it.
    pub fn delete_file(&self, ty: &AssetType, name: &str, file: &str) -> Result<()> {
        validate_name(file)?;
        if file.ends_with(".json") || (ty.slug == "code" && file.ends_with(".gml")) {
            return Err(Error::BadRequest("delete the asset through the asset endpoint".into()));
        }
        self.file_path(ty, name, file)?;
        let json = self.ensure_in_mod(ty, name)?;
        std::fs::remove_file(json.parent().unwrap().join(file))?;
        Ok(())
    }

    /// Changes byte ranges of a code source. `rev` must be the revision the offsets come from.
    pub fn patch_code(&self, name: &str, rev: &str, edits: &[crate::gml::Edit]) -> Result<Value> {
        let ty = asset_type("code")?;
        let cur = self.read(ty, name)?;
        let src = cur["source"].as_str().unwrap_or_default();
        if crate::gml::rev(src) != rev {
            return Err(Error::Conflict(format!("code '{name}' changed; reload it and try again")));
        }
        let new = crate::gml::apply_edits(src, edits)?;
        self.write(ty, name, None, Some(new))
    }

    /// Selected top-level fields of every asset of one type: name -> {field: value}.
    pub fn index(&self, ty: &AssetType, fields: &[&str]) -> BTreeMap<String, Value> {
        self.list(ty)
            .into_iter()
            .filter_map(|e| {
                let (path, _) = self.resolve(ty, &e.name).ok()?;
                let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
                let picked: serde_json::Map<String, Value> =
                    fields.iter().filter_map(|f| Some(((*f).to_owned(), v.get(*f)?.clone()))).collect();
                Some((e.name, Value::Object(picked)))
            })
            .collect()
    }

    /// Removes an asset from mod/ (revert to base, or delete a new asset).
    pub fn revert(&self, ty: &AssetType, name: &str) -> Result<()> {
        validate_name(name)?;
        let json = self.mod_dir().join(Self::rel_json(ty, name));
        if !Self::exists(&json, ty) {
            return Err(Error::NotFound(format!("no changes for {}/{name}", ty.slug)));
        }
        if ty.individual_dir {
            std::fs::remove_dir_all(json.parent().unwrap())?;
        } else {
            for f in Self::side_files(&json, ty, name).into_iter().chain(json.is_file().then_some(json)) {
                std::fs::remove_file(f)?;
            }
        }
        Ok(())
    }

    /// Generic dump: index of all lists.
    pub fn generic_index(&self) -> Result<Value> {
        Ok(serde_json::from_slice(&std::fs::read(self.generic().join("_index.json"))?)?)
    }

    /// Generic dump of one list, with paging. Singletons (GeneralInfo, ...) return the whole object.
    pub fn generic_list(&self, list: &str, offset: usize, limit: usize, q: Option<&str>) -> Result<Value> {
        validate_name(list)?;
        let v: Value = serde_json::from_slice(&std::fs::read(self.generic().join(format!("{list}.json")))?)?;
        let Value::Array(items) = v else { return Ok(v) };
        let q = q.map(str::to_lowercase);
        let filtered: Vec<&Value> = items
            .iter()
            .filter(|item| match &q {
                None => true,
                Some(q) => item_name(item).is_some_and(|n| n.to_lowercase().contains(q.as_str())),
            })
            .collect();
        let total = filtered.len();
        let page: Vec<&Value> = filtered.into_iter().skip(offset).take(limit).collect();
        Ok(serde_json::json!({ "total": total, "offset": offset, "items": page }))
    }

    /// Plain-text search in all GML source (mod overrides base).
    pub fn search_code(&self, needle: &str, max: usize) -> Vec<Value> {
        let ty = asset_type("code").unwrap();
        let mut hits = Vec::new();
        for entry in self.list(ty) {
            let root = if entry.modified { self.mod_dir() } else { self.base() };
            let path = root.join(Self::rel_json(ty, &entry.name)).with_extension("gml");
            let Ok(src) = std::fs::read_to_string(path) else {
                continue;
            };
            for (i, line) in src.lines().enumerate() {
                if line.contains(needle) {
                    hits.push(serde_json::json!({ "code": entry.name, "line": i + 1, "text": line.trim() }));
                    if hits.len() >= max {
                        return hits;
                    }
                }
            }
        }
        hits
    }

    /// All GML sources (mod overrides base), for indexers.
    pub fn code_sources(&self) -> impl Iterator<Item = (String, String)> + '_ {
        let ty = asset_type("code").unwrap();
        self.list(ty).into_iter().filter_map(move |entry| {
            let root = if entry.modified { self.mod_dir() } else { self.base() };
            let path = root.join(Self::rel_json(ty, &entry.name)).with_extension("gml");
            std::fs::read_to_string(path).ok().map(|s| (entry.name, s))
        })
    }
}

fn item_name(item: &Value) -> Option<&str> {
    let v = item.get("value")?;
    v.as_str().or_else(|| v.get("Name")?.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, ChapterStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ChapterStore::new(tmp.path(), "chapter1");
        let base = store.base();
        std::fs::create_dir_all(base.join("rooms")).unwrap();
        std::fs::create_dir_all(base.join("code")).unwrap();
        std::fs::create_dir_all(base.join("sprites/spr_kris")).unwrap();
        std::fs::write(base.join("project.json"), r#"{"Name":"DELTARUNE"}"#).unwrap();
        std::fs::write(base.join("rooms/room_town.json"), r#"{"$type":"Room","Width":320}"#).unwrap();
        std::fs::write(base.join("code/gml_Object_obj_a_Create_0.json"), r#"{"$type":"Code"}"#).unwrap();
        std::fs::write(
            base.join("code/gml_Object_obj_a_Create_0.gml"),
            "x = 1;\nmsgsetloc(0, \"Hi\", \"k\");\n",
        )
        .unwrap();
        std::fs::write(base.join("sprites/spr_kris/spr_kris.json"), r#"{"$type":"Sprite"}"#).unwrap();
        std::fs::write(base.join("sprites/spr_kris/spr_kris_0.png"), b"png").unwrap();
        (tmp, store)
    }

    #[test]
    fn list_and_read_base() {
        let (_t, s) = setup();
        let rooms = s.list(asset_type("rooms").unwrap());
        assert_eq!(rooms.len(), 1);
        assert!(!rooms[0].modified);
        let spr = s.read(asset_type("sprites").unwrap(), "spr_kris").unwrap();
        assert_eq!(spr["files"], json!(["spr_kris_0.png"]));
        let code = s.read(asset_type("code").unwrap(), "gml_Object_obj_a_Create_0").unwrap();
        assert!(code["source"].as_str().unwrap().starts_with("x = 1;"));
        assert_eq!(s.counts()["rooms"], 1);
    }

    #[test]
    fn write_goes_to_mod_and_revert_restores() {
        let (_t, s) = setup();
        let rooms = asset_type("rooms").unwrap();
        let out = s
            .write(rooms, "room_town", Some(json!({"Width": 640, "$type": "Room"})), None)
            .unwrap();
        assert_eq!(out["modified"], true);
        assert_eq!(out["data"]["Width"], 640);
        let raw = std::fs::read_to_string(s.mod_dir().join("rooms/room_town.json")).unwrap();
        assert!(raw.trim_start().starts_with("{\n  \"$type\""));
        assert!(s.mod_dir().join("project.json").is_file());
        assert_eq!(s.read(rooms, "room_town").unwrap()["data"]["Width"], 640);
        s.revert(rooms, "room_town").unwrap();
        assert_eq!(s.read(rooms, "room_town").unwrap()["data"]["Width"], 320);
    }

    #[test]
    fn edit_code_source_copies_json() {
        let (_t, s) = setup();
        let code = asset_type("code").unwrap();
        s.write(code, "gml_Object_obj_a_Create_0", None, Some("x = 2;".into())).unwrap();
        assert!(s.mod_dir().join("code/gml_Object_obj_a_Create_0.json").is_file());
        assert_eq!(s.read(code, "gml_Object_obj_a_Create_0").unwrap()["source"], "x = 2;");
        assert_eq!(s.search_code("x = 2", 10).len(), 1);
        assert_eq!(s.search_code("x = 1", 10).len(), 0);
    }

    #[test]
    fn code_without_json() {
        let (_t, s) = setup();
        let code = asset_type("code").unwrap();
        std::fs::write(s.base().join("code/gml_Script_foo.gml"), "return 1;").unwrap();
        assert!(s.list(code).iter().any(|e| e.name == "gml_Script_foo"));
        assert_eq!(s.read(code, "gml_Script_foo").unwrap()["data"]["$type"], "Code");
        s.write(code, "gml_Script_foo", None, Some("return 2;".into())).unwrap();
        assert!(!s.mod_dir().join("code/gml_Script_foo.json").exists());
        assert_eq!(s.read(code, "gml_Script_foo").unwrap()["source"], "return 2;");
        s.write(code, "gml_Script_new", None, Some("return 3;".into())).unwrap();
        assert!(s.list(code).iter().any(|e| e.name == "gml_Script_new" && e.added));
        s.revert(code, "gml_Script_foo").unwrap();
        assert_eq!(s.read(code, "gml_Script_foo").unwrap()["source"], "return 1;");
    }

    #[test]
    fn sprite_side_files() {
        let (_t, s) = setup();
        let spr = asset_type("sprites").unwrap();
        s.write_file(spr, "spr_kris", "spr_kris_0.png", b"new").unwrap();
        let p = s.file_path(spr, "spr_kris", "spr_kris_0.png").unwrap();
        assert!(p.starts_with(s.mod_dir()));
        assert_eq!(std::fs::read(p).unwrap(), b"new");
        assert!(s.mod_dir().join("sprites/spr_kris/spr_kris.json").is_file());
    }

    #[test]
    fn new_asset_and_type_check() {
        let (_t, s) = setup();
        let rooms = asset_type("rooms").unwrap();
        assert!(s.write(rooms, "room_new", None, None).is_err());
        assert!(s.write(rooms, "room_new", Some(json!({"$type": "Sprite"})), None).is_err());
        s.write(rooms, "room_new", Some(json!({"Width": 1})), None).unwrap();
        let e = s.list(rooms).into_iter().find(|e| e.name == "room_new").unwrap();
        assert!(e.added && e.modified);
    }

    #[test]
    fn patch_delete_index() {
        let (_t, s) = setup();
        let code = asset_type("code").unwrap();
        let cur = s.read(code, "gml_Object_obj_a_Create_0").unwrap();
        let rev = cur["rev"].as_str().unwrap().to_owned();
        let edit = crate::gml::Edit {
            start: 4,
            end: 5,
            text: Some("7".into()),
            string: None,
        };
        let out = s
            .patch_code("gml_Object_obj_a_Create_0", &rev, std::slice::from_ref(&edit))
            .unwrap();
        assert!(out["source"].as_str().unwrap().starts_with("x = 7;"));
        assert!(matches!(
            s.patch_code("gml_Object_obj_a_Create_0", &rev, &[edit]),
            Err(Error::Conflict(_))
        ));

        let spr = asset_type("sprites").unwrap();
        s.delete_file(spr, "spr_kris", "spr_kris_0.png").unwrap();
        assert_eq!(s.read(spr, "spr_kris").unwrap()["files"], json!([]));
        assert!(s.base().join("sprites/spr_kris/spr_kris_0.png").is_file());
        assert!(s.delete_file(spr, "spr_kris", "spr_kris.json").is_err());

        let idx = s.index(asset_type("rooms").unwrap(), &["Width", "Nope"]);
        assert_eq!(idx["room_town"], json!({"Width": 320}));
    }

    #[test]
    fn rejects_traversal() {
        let (_t, s) = setup();
        let rooms = asset_type("rooms").unwrap();
        assert!(s.read(rooms, "../project").is_err());
        assert!(s.read(rooms, "..").is_err());
        assert!(s.generic_list("../x", 0, 1, None).is_err());
    }
}
