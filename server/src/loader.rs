//! Mod loader: installs `.dremod` packages, puts the enabled mods into the game, keeps backups.
//!
//! ```text
//! <workspace>/loader/
//!   state.json               load order, enabled flags, what is in the game now
//!   mods/<id>/               unpacked packages
//!   backup/<chapter>/        original data file and lang files of the game
//!   saves/<time>-<kind>/     copies of the save folder
//!   deploy/                  temporary merged projects and builds
//! ```
//!
//! Apply: for each chapter, the enabled mods (in load order, a later mod wins per asset) are merged
//! into one UTMT project and built on the original data file from the backup. The output replaces the
//! game data file. A chapter without enabled mods gets its original file back. A game data file that
//! is not the one the loader wrote (game update, Steam file check) becomes the new original.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use walkdir::WalkDir;

use crate::chapters::Chapter;
use crate::error::{Error, Result};
use crate::hash::file_sha256;
use crate::jobs::Jobs;
use crate::langmod;
use crate::modpkg::{self, Manifest};
use crate::store::{ASSET_TYPES, ChapterStore, validate_name};
use crate::utmt::Utmt;
use crate::versions;

/// Automatic save backups to keep.
const MAX_AUTO_SAVES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileStamp {
    pub size: u64,
    /// Modification time, ms since the Unix epoch.
    pub mtime: Option<u64>,
    pub sha256: String,
}

fn mtime_ms(m: &std::fs::Metadata) -> Option<u64> {
    let d = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    u64::try_from(d.as_millis()).ok()
}

impl FileStamp {
    pub fn of(path: &Path) -> std::io::Result<Self> {
        let m = std::fs::metadata(path)?;
        Ok(Self {
            size: m.len(),
            mtime: mtime_ms(&m),
            sha256: file_sha256(path)?,
        })
    }

    /// The file is still the one this stamp was made from.
    pub fn matches(&self, path: &Path) -> bool {
        let Ok(m) = std::fs::metadata(path) else { return false };
        if m.len() != self.size {
            return false;
        }
        mtime_ms(&m) == self.mtime || file_sha256(path).is_ok_and(|h| h == self.sha256)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModEntry {
    pub id: String,
    pub enabled: bool,
}

/// What the loader put into one chapter of the game.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Applied {
    pub mods: Vec<String>,
    /// The data file the loader wrote. `None`: the data file is the original.
    pub data: Option<FileStamp>,
    /// Lang files the loader wrote.
    #[serde(default)]
    pub lang: BTreeMap<String, FileStamp>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoaderState {
    /// Load order: a later mod wins when two mods change the same asset.
    pub mods: Vec<ModEntry>,
    pub applied: BTreeMap<String, Applied>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveBackup {
    pub name: String,
    pub created: u64,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct Loader {
    pub root: PathBuf,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Copies through a temporary file, so the destination is never half written.
fn copy_atomic(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst.parent().unwrap())?;
    let mut tmp = dst.as_os_str().to_owned();
    tmp.push(".dre-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::copy(src, &tmp)?;
    std::fs::rename(&tmp, dst).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn write_atomic(dst: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dst.parent().unwrap())?;
    let mut tmp = dst.as_os_str().to_owned();
    tmp.push(".dre-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dst)
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<(usize, u64)> {
    let (mut n, mut bytes) = (0, 0);
    for e in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let rel = e.path().strip_prefix(src).unwrap();
        let out = dst.join(rel);
        if e.file_type().is_dir() {
            std::fs::create_dir_all(&out)?;
        } else if e.file_type().is_file() {
            std::fs::create_dir_all(out.parent().unwrap())?;
            bytes += std::fs::copy(e.path(), &out)?;
            n += 1;
        }
    }
    Ok((n, bytes))
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((file_sha256(a), file_sha256(b)), (Ok(x), Ok(y)) if x == y)
}

/// Default Deltarune save folder of this system, if it exists.
pub fn default_save_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut c: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        c.extend(env("LOCALAPPDATA").map(|d| d.join("DELTARUNE")));
    } else if cfg!(target_os = "macos") {
        c.extend(env("HOME").map(|h| h.join("Library/Application Support/com.tobyfox.deltarune")));
    } else if let Some(h) = env("HOME") {
        c.push(h.join(".config/DELTARUNE"));
        // Steam Proton (app 1671210).
        c.push(h.join(".local/share/Steam/steamapps/compatdata/1671210/pfx/drive_c/users/steamuser/AppData/Local/DELTARUNE"));
        c.push(h.join(".steam/steam/steamapps/compatdata/1671210/pfx/drive_c/users/steamuser/AppData/Local/DELTARUNE"));
    }
    c.into_iter().find(|d| d.is_dir())
}

/// Per chapter, what the enabled mods want: mods with assets, and lang file -> mods.
struct Want {
    /// Mod with an xdelta patch for the data file (the last one in load order).
    xdelta: Option<String>,
    asset_mods: Vec<String>,
    lang: BTreeMap<String, Vec<String>>,
    all: Vec<String>,
}

impl Loader {
    pub fn new(workspace: &Path) -> Self {
        Self {
            root: workspace.join("loader"),
        }
    }
    fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
    }
    fn mods_dir(&self) -> PathBuf {
        self.root.join("mods")
    }
    pub fn mod_dir(&self, id: &str) -> PathBuf {
        self.mods_dir().join(id)
    }
    pub fn backup_dir(&self, chapter: &str) -> PathBuf {
        self.root.join("backup").join(chapter)
    }
    fn saves_dir(&self) -> PathBuf {
        self.root.join("saves")
    }
    fn deploy_dir(&self) -> PathBuf {
        self.root.join("deploy")
    }
    fn backup_data(&self, ch: &Chapter) -> PathBuf {
        self.backup_dir(&ch.id).join(ch.data_file.file_name().unwrap())
    }

    pub fn load_state(&self) -> LoaderState {
        std::fs::read(self.state_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_state(&self, s: &LoaderState) -> Result<()> {
        write_atomic(&self.state_path(), &serde_json::to_vec_pretty(s)?)?;
        Ok(())
    }

    pub fn manifest(&self, id: &str) -> Result<Manifest> {
        validate_name(id)?;
        let p = self.mod_dir(id).join(modpkg::MANIFEST);
        let b = std::fs::read(&p).map_err(|_| Error::NotFound(format!("mod '{id}'")))?;
        Ok(serde_json::from_slice(&b)?)
    }

    fn has_xdelta(&self, id: &str, chapter: &str) -> bool {
        self.manifest(id)
            .is_ok_and(|m| m.chapters.get(chapter).is_some_and(|c| c.xdelta.is_some()))
    }

    /// Installs an xdelta patch as a mod. `chapter`: the chapter it is for; `None` tries each chapter
    /// (the patch must have window checksums). The patch must fit the original data file of the chapter.
    pub fn install_xdelta(&self, patch: &Path, name: &str, chapter: Option<&str>, chapters: &[Chapter]) -> Result<Manifest> {
        let info = crate::xdelta::info(&std::fs::read(patch)?)?;
        let id: String = name
            .trim_end_matches(".xdelta")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .trim_start_matches('.')
            .chars()
            .take(100)
            .collect();
        let id = if id.is_empty() { "xdelta_mod".to_owned() } else { id };
        let candidates: Vec<&Chapter> = match chapter {
            Some(c) => vec![
                chapters
                    .iter()
                    .find(|x| x.id == c)
                    .ok_or_else(|| Error::NotFound(format!("chapter '{c}'")))?,
            ],
            None if !info.checksums => {
                return Err(Error::BadRequest(
                    "this patch has no checksums, so the chapter cannot be found; choose the chapter".into(),
                ));
            }
            None => chapters.iter().collect(),
        };
        std::fs::create_dir_all(self.deploy_dir())?;
        let probe = self.deploy_dir().join(format!("probe-{}", std::process::id()));
        let mut found = None;
        let mut last_err = None;
        for ch in candidates {
            let original = self.original_data_file(ch);
            match crate::xdelta::decode(&original, patch, &probe) {
                Ok(_) => {
                    found = Some((ch, file_sha256(&original)?, std::fs::metadata(&original)?.len()));
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }
        let _ = std::fs::remove_file(&probe);
        let Some((ch, sha, size)) = found else {
            return Err(match (chapter, last_err) {
                (Some(_), Some(e)) => e,
                _ => Error::BadRequest("the patch fits no chapter of this game (other game version?)".into()),
            });
        };
        let m = Manifest {
            format: modpkg::FORMAT,
            id: id.clone(),
            name: name.to_owned(),
            version: String::new(),
            author: String::new(),
            description: format!("xdelta patch for {}", ch.id),
            editor: env!("CARGO_PKG_VERSION").into(),
            created: now_ms() / 1000,
            chapters: BTreeMap::from([(
                ch.id.clone(),
                modpkg::ChapterManifest {
                    data_file: ch.data_file.file_name().unwrap().to_string_lossy().into_owned(),
                    game_sha256: Some(sha),
                    game_size: Some(size),
                    assets: Vec::new(),
                    lang: Vec::new(),
                    xdelta: Some(modpkg::XDELTA_FILE.into()),
                },
            )]),
        };
        let dst = self.mod_dir(&id);
        if dst.exists() {
            std::fs::remove_dir_all(&dst)?;
        }
        std::fs::create_dir_all(dst.join(&ch.id))?;
        std::fs::copy(patch, dst.join(&ch.id).join(modpkg::XDELTA_FILE))?;
        std::fs::write(dst.join(modpkg::MANIFEST), serde_json::to_vec_pretty(&m)?)?;
        let mut s = self.load_state();
        if !s.mods.iter().any(|e| e.id == id) {
            s.mods.push(ModEntry { id, enabled: true });
        }
        self.save_state(&s)?;
        Ok(m)
    }

    /// Installs (or replaces) a package. A new mod goes last in the load order, enabled.
    pub fn install(&self, pkg: &Path) -> Result<Manifest> {
        let m = modpkg::read_manifest(pkg)?;
        std::fs::create_dir_all(self.mods_dir())?;
        let tmp = self.mods_dir().join(format!(".{}.tmp", m.id));
        if tmp.exists() {
            std::fs::remove_dir_all(&tmp)?;
        }
        let m = modpkg::unpack(pkg, &tmp).inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&tmp);
        })?;
        let dst = self.mod_dir(&m.id);
        if dst.exists() {
            std::fs::remove_dir_all(&dst)?;
        }
        std::fs::rename(&tmp, &dst)?;
        let mut s = self.load_state();
        if !s.mods.iter().any(|e| e.id == m.id) {
            s.mods.push(ModEntry {
                id: m.id.clone(),
                enabled: true,
            });
        }
        self.save_state(&s)?;
        Ok(m)
    }

    /// Sets load order and enabled flags. The list must hold each installed mod once.
    pub fn set_mods(&self, mods: Vec<ModEntry>) -> Result<()> {
        let mut s = self.load_state();
        let have: BTreeSet<&str> = s.mods.iter().map(|e| e.id.as_str()).collect();
        let got: BTreeSet<&str> = mods.iter().map(|e| e.id.as_str()).collect();
        if have != got || got.len() != mods.len() {
            return Err(Error::BadRequest("give each installed mod once".into()));
        }
        s.mods = mods;
        self.save_state(&s)
    }

    /// Removes a mod from the loader. The game keeps it until the next apply.
    pub fn uninstall(&self, id: &str) -> Result<()> {
        validate_name(id)?;
        let mut s = self.load_state();
        let before = s.mods.len();
        s.mods.retain(|e| e.id != id);
        if s.mods.len() == before {
            return Err(Error::NotFound(format!("mod '{id}'")));
        }
        self.save_state(&s)?;
        let d = self.mod_dir(id);
        if d.exists() {
            std::fs::remove_dir_all(d)?;
        }
        Ok(())
    }

    /// The original data file of a chapter: the backup while the game file is the one the loader wrote.
    pub fn original_data_file(&self, ch: &Chapter) -> PathBuf {
        let s = self.load_state();
        let ours = s
            .applied
            .get(&ch.id)
            .and_then(|a| a.data.as_ref())
            .is_some_and(|st| st.matches(&ch.data_file));
        let b = self.backup_data(ch);
        if ours && b.is_file() { b } else { ch.data_file.clone() }
    }

    fn want(&self, s: &LoaderState, chapter: &str) -> Result<Want> {
        let mut w = Want {
            xdelta: None,
            asset_mods: Vec::new(),
            lang: BTreeMap::new(),
            all: Vec::new(),
        };
        for e in s.mods.iter().filter(|e| e.enabled) {
            let m = self.manifest(&e.id)?;
            let Some(c) = m.chapters.get(chapter) else { continue };
            if !c.assets.is_empty() {
                w.asset_mods.push(e.id.clone());
            }
            if c.xdelta.is_some() {
                w.xdelta = Some(e.id.clone());
            }
            for f in &c.lang {
                w.lang.entry(f.clone()).or_default().push(e.id.clone());
            }
            w.all.push(e.id.clone());
        }
        Ok(w)
    }

    /// Loader view: mods with manifests, and the state of each game chapter.
    pub fn status(&self, chapters: &[Chapter]) -> Result<Value> {
        let s = self.load_state();
        let mods: Vec<Value> = s
            .mods
            .iter()
            .map(|e| json!({ "id": e.id, "enabled": e.enabled, "manifest": self.manifest(&e.id).ok() }))
            .collect();
        let mut out = Vec::new();
        for ch in chapters {
            let a = s.applied.get(&ch.id);
            let want = self.want(&s, &ch.id).map(|w| w.all).unwrap_or_default();
            let game_updated = a.is_some_and(|a| {
                a.data.as_ref().is_some_and(|st| !st.matches(&ch.data_file))
                    || a.lang.iter().any(|(f, st)| !st.matches(&ch.dir.join("lang").join(f)))
            });
            let applied: Vec<String> = a.map(|a| a.mods.clone()).unwrap_or_default();
            let state = match (a, game_updated) {
                (_, true) => "game_updated",
                (Some(_), false) => "modded",
                (None, _) => "original",
            };
            out.push(json!({
                "id": ch.id,
                "state": state,
                "backup": self.backup_data(ch).is_file(),
                "applied_mods": applied,
                "wanted_mods": want,
                "pending": game_updated || want != applied,
            }));
        }
        Ok(json!({ "mods": mods, "chapters": out }))
    }

    /// Version check of a mod against this game: per chapter, same game version, or a check of each asset
    /// against the extracted chapter (when it was extracted from the same game file).
    pub fn check(&self, workspace: &Path, id: &str, chapters: &[Chapter]) -> Result<Vec<Value>> {
        let m = self.manifest(id)?;
        let mut out = Vec::new();
        for (cid, cm) in &m.chapters {
            let Some(ch) = chapters.iter().find(|c| &c.id == cid) else {
                out.push(json!({ "chapter": cid, "in_game": false }));
                continue;
            };
            let sha = file_sha256(&self.original_data_file(ch)).ok();
            let same = sha.is_some() && cm.game_sha256 == sha;
            let store = ChapterStore::new(workspace, cid);
            let meta_sha: Option<String> = std::fs::read(store.root.join("meta.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|v| v["data_file_sha256"].as_str().map(str::to_owned));
            let items = cm.assets.iter().map(|a| (a.ty.as_str(), a.name.as_str(), Some(a.base.as_deref())));
            let (checked, assets) = if same {
                let all: Vec<Value> = cm
                    .assets
                    .iter()
                    .map(|a| json!({ "type": a.ty, "name": a.name, "status": if a.base.is_some() { "same" } else { "new" } }))
                    .collect();
                (true, all)
            } else if store.is_extracted() && meta_sha.is_some() && meta_sha == sha {
                let v = versions::compare(&store, items)?;
                (true, v.into_iter().map(|c| json!(c)).collect())
            } else {
                (false, Vec::new())
            };
            out.push(json!({
                "chapter": cid,
                "in_game": true,
                "xdelta": cm.xdelta.is_some(),
                "same_version": same,
                "checked": checked,
                "assets": assets,
                "lang": cm.lang,
            }));
        }
        Ok(out)
    }

    /// Makes `deploy/<chapter>/`: the assets of the mods, in order, a later mod replaces an earlier one per asset.
    fn merge(&self, chapter: &str, mods: &[String], log: &dyn Fn(String)) -> Result<PathBuf> {
        let d = self.deploy_dir().join(chapter);
        if d.exists() {
            std::fs::remove_dir_all(&d)?;
        }
        std::fs::create_dir_all(&d)?;
        let mut owner: HashMap<String, String> = HashMap::new();
        for id in mods {
            let src = self.mod_dir(id).join(chapter);
            if !d.join("project.json").is_file() && src.join("project.json").is_file() {
                std::fs::copy(src.join("project.json"), d.join("project.json"))?;
            }
            for ty in ASSET_TYPES {
                let td = src.join(ty.slug);
                let Ok(rd) = std::fs::read_dir(&td) else { continue };
                let out_dir = d.join(ty.slug);
                std::fs::create_dir_all(&out_dir)?;
                let mut names: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
                for e in rd.filter_map(|e| e.ok()) {
                    let p = e.path();
                    let name = if ty.individual_dir {
                        p.is_dir().then(|| e.file_name().to_string_lossy().into_owned())
                    } else {
                        p.is_file()
                            .then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                            .flatten()
                    };
                    if let Some(n) = name {
                        names.entry(n).or_default().push(p);
                    }
                }
                for (name, paths) in names {
                    let key = format!("{}/{name}", ty.slug);
                    if let Some(prev) = owner.insert(key.clone(), id.clone()) {
                        log(format!("{chapter}: {key}: mod '{id}' replaces mod '{prev}'"));
                        if ty.individual_dir {
                            std::fs::remove_dir_all(out_dir.join(&name))?;
                        } else {
                            for f in std::fs::read_dir(&out_dir)?.filter_map(|e| e.ok()) {
                                if f.path().file_stem().is_some_and(|s| s.to_string_lossy() == name) {
                                    std::fs::remove_file(f.path())?;
                                }
                            }
                        }
                    }
                    for p in paths {
                        if ty.individual_dir {
                            copy_tree(&p, &out_dir.join(&name))?;
                        } else {
                            std::fs::copy(&p, out_dir.join(p.file_name().unwrap()))?;
                        }
                    }
                }
            }
        }
        if !d.join("project.json").is_file() {
            return Err(Error::Other(format!("{chapter}: no project.json in the mods")));
        }
        Ok(d)
    }

    /// Backs up the game data file when it is not the one the loader wrote. Returns the original file.
    fn backup_original(&self, ch: &Chapter, applied: Option<&Applied>, log: &dyn Fn(String)) -> Result<PathBuf> {
        let g = &ch.data_file;
        let b = self.backup_data(ch);
        let ours = applied.and_then(|a| a.data.as_ref()).is_some_and(|st| st.matches(g));
        if ours {
            if !b.is_file() {
                return Err(Error::Other(format!(
                    "{}: the backup of the original data file is missing; restore the game files (Steam: Verify integrity)",
                    ch.id
                )));
            }
        } else if !b.is_file() || !same_file(g, &b) {
            if b.is_file() {
                log(format!(
                    "{}: the game data file changed (game update?); it is the new backup",
                    ch.id
                ));
            } else {
                log(format!("{}: backup of {}", ch.id, g.display()));
            }
            copy_atomic(g, &b)?;
        }
        Ok(b)
    }

    /// Lang files: backup of originals, merged overrides into the game, or the original back.
    fn apply_lang(
        &self,
        ch: &Chapter,
        want: &Want,
        applied: Option<&Applied>,
        log: &dyn Fn(String),
    ) -> Result<BTreeMap<String, FileStamp>> {
        let files: BTreeSet<String> = want
            .lang
            .keys()
            .cloned()
            .chain(applied.into_iter().flat_map(|a| a.lang.keys().cloned()))
            .collect();
        let mut out = BTreeMap::new();
        for f in files {
            let gf = ch.dir.join("lang").join(&f);
            let bf = self.backup_dir(&ch.id).join("lang").join(&f);
            let ours = applied.and_then(|a| a.lang.get(&f)).is_some_and(|st| st.matches(&gf));
            if !ours && gf.is_file() && (!bf.is_file() || !same_file(&gf, &bf)) {
                copy_atomic(&gf, &bf)?;
            }
            let mods = want.lang.get(&f).cloned().unwrap_or_default();
            if mods.is_empty() {
                if ours && bf.is_file() {
                    copy_atomic(&bf, &gf)?;
                    log(format!("{}: lang/{f}: original back", ch.id));
                }
                continue;
            }
            if !bf.is_file() {
                log(format!("{}: lang/{f}: the game has no such file; skipped", ch.id));
                continue;
            }
            let overrides: Vec<langmod::Overrides> = mods
                .iter()
                .map(|id| {
                    let p = self.mod_dir(id).join(&ch.id).join("lang").join(&f);
                    Ok(serde_json::from_slice(&std::fs::read(p)?)?)
                })
                .collect::<Result<_>>()?;
            let bytes = langmod::apply(&std::fs::read(&bf)?, &overrides)?;
            write_atomic(&gf, &bytes)?;
            log(format!(
                "{}: lang/{f}: {} key(s) from {}",
                ch.id,
                overrides.iter().map(|o| o.len()).sum::<usize>(),
                mods.join(", ")
            ));
            out.insert(f, FileStamp::of(&gf)?);
        }
        Ok(out)
    }

    /// Puts the enabled mods into the game (see the module docs). Backs up the save folder first.
    pub async fn apply(&self, jobs: &Jobs, job: u64, utmt: Option<Utmt>, chapters: Vec<Chapter>, save_dir: Option<PathBuf>) -> Result<()> {
        let this = self.clone();
        let j = jobs.clone();
        let log = move |l: String| j.log(job, l);
        if let Some(sd) = save_dir.filter(|d| d.is_dir()) {
            let (l, me) = (log.clone(), this.clone());
            let b = blocking(move || me.backup_saves(&sd, "auto")).await?;
            l(format!("saves: backup '{}' ({} files)", b.name, b.files));
        } else {
            log("saves: no save folder found; set it in Setup to back up saves".into());
        }
        let mut s = self.load_state();
        for ch in chapters {
            let want = self.want(&s, &ch.id)?;
            let prev = s.applied.get(&ch.id).cloned();
            if want.all.is_empty() && prev.is_none() {
                continue;
            }
            let (l, me, c, p) = (log.clone(), this.clone(), ch.clone(), prev.clone());
            let original = blocking(move || me.backup_original(&c, p.as_ref(), &l)).await?;
            let mut applied = Applied {
                mods: want.all.clone(),
                ..Applied::default()
            };
            let mut source = original.clone();
            if let Some(xid) = &want.xdelta {
                let others: Vec<&String> = want.all.iter().filter(|m| *m != xid && self.has_xdelta(m, &ch.id)).collect();
                if !others.is_empty() {
                    log(format!(
                        "{}: more than one xdelta patch; only '{xid}' (last in load order) is used, not {:?}",
                        ch.id, others
                    ));
                }
                let patch = self.mod_dir(xid).join(&ch.id).join(modpkg::XDELTA_FILE);
                let out = self
                    .deploy_dir()
                    .join(format!("{}.xdelta", ch.id))
                    .join(ch.data_file.file_name().unwrap());
                let (o, d) = (original.clone(), out.clone());
                blocking(move || {
                    std::fs::create_dir_all(d.parent().unwrap())?;
                    crate::xdelta::decode(&o, &patch, &d)
                })
                .await
                .map_err(|e| Error::BadRequest(format!("{}: mod '{xid}': {e}", ch.id)))?;
                log(format!("{}: xdelta patch of '{xid}' applied", ch.id));
                source = out;
            }
            if want.asset_mods.is_empty() && want.xdelta.is_some() {
                let (o, g) = (source.clone(), ch.data_file.clone());
                applied.data = Some(
                    blocking(move || {
                        copy_atomic(&o, &g)?;
                        Ok(FileStamp::of(&g)?)
                    })
                    .await?,
                );
                log(format!("{}: wrote {}", ch.id, ch.data_file.display()));
            } else if want.asset_mods.is_empty() {
                if prev
                    .as_ref()
                    .and_then(|a| a.data.as_ref())
                    .is_some_and(|st| st.matches(&ch.data_file))
                {
                    let (o, g) = (original.clone(), ch.data_file.clone());
                    blocking(move || Ok(copy_atomic(&o, &g)?)).await?;
                    log(format!("{}: original data file back", ch.id));
                }
            } else {
                let utmt = utmt
                    .clone()
                    .ok_or_else(|| Error::Conflict("utmt_cli is not set; it is needed to install mods".into()))?;
                let (l, me, id, mods) = (log.clone(), this.clone(), ch.id.clone(), want.asset_mods.clone());
                let project_dir = blocking(move || me.merge(&id, &mods, &l)).await?;
                log(format!("{}: build with {}", ch.id, want.asset_mods.join(", ")));
                let out = self
                    .deploy_dir()
                    .join(format!("{}.out", ch.id))
                    .join(ch.data_file.file_name().unwrap());
                tokio::fs::create_dir_all(out.parent().unwrap()).await?;
                utmt.build_project(jobs, job, &project_dir, &project_dir.join("project.json"), &source, &out)
                    .await
                    .map_err(Error::Other)?;
                let g = ch.data_file.clone();
                let stamp = blocking(move || {
                    copy_atomic(&out, &g)?;
                    Ok(FileStamp::of(&g)?)
                })
                .await?;
                log(format!("{}: wrote {}", ch.id, ch.data_file.display()));
                applied.data = Some(stamp);
            }
            let (l, me, c, w, p) = (log.clone(), this.clone(), ch.clone(), want, prev);
            applied.lang = blocking(move || me.apply_lang(&c, &w, p.as_ref(), &l)).await?;
            // Read the state again: only `applied` changes here.
            let mut cur = self.load_state();
            if applied.data.is_none() && applied.lang.is_empty() {
                cur.applied.remove(&ch.id);
            } else {
                cur.applied.insert(ch.id.clone(), applied);
            }
            self.save_state(&cur)?;
            s.applied = cur.applied;
        }
        let d = self.deploy_dir();
        if d.exists() {
            let _ = tokio::fs::remove_dir_all(d).await;
        }
        log("apply: done".into());
        Ok(())
    }

    /// Puts the original game files back. Mods stay installed; apply puts them in again.
    pub fn restore(&self, chapters: &[Chapter]) -> Result<Vec<String>> {
        let mut s = self.load_state();
        let mut done = Vec::new();
        for ch in chapters {
            let Some(a) = s.applied.remove(&ch.id) else { continue };
            if a.data.as_ref().is_some_and(|st| st.matches(&ch.data_file)) {
                copy_atomic(&self.backup_data(ch), &ch.data_file)?;
                done.push(format!("{}/{}", ch.id, ch.data_file.file_name().unwrap().to_string_lossy()));
            }
            for (f, st) in &a.lang {
                let gf = ch.dir.join("lang").join(f);
                let bf = self.backup_dir(&ch.id).join("lang").join(f);
                if st.matches(&gf) && bf.is_file() {
                    copy_atomic(&bf, &gf)?;
                    done.push(format!("{}/lang/{f}", ch.id));
                }
            }
            self.save_state(&s)?;
        }
        Ok(done)
    }

    pub fn save_backups(&self) -> Vec<SaveBackup> {
        let mut v: Vec<SaveBackup> = std::fs::read_dir(self.saves_dir())
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let created = name.split('-').next()?.parse().ok()?;
                let (files, bytes) = WalkDir::new(e.path())
                    .into_iter()
                    .filter_map(|x| x.ok())
                    .filter(|x| x.file_type().is_file())
                    .fold((0, 0), |(n, b), x| (n + 1, b + x.metadata().map(|m| m.len()).unwrap_or(0)));
                Some(SaveBackup {
                    name,
                    created,
                    files,
                    bytes,
                })
            })
            .collect();
        v.sort_by(|a, b| b.name.cmp(&a.name));
        v
    }

    /// Copies the save folder to `saves/<ms>-<kind>`. Keeps the newest automatic backups only.
    pub fn backup_saves(&self, save_dir: &Path, kind: &str) -> Result<SaveBackup> {
        if !save_dir.is_dir() {
            return Err(Error::Conflict(format!("save folder '{}' is not a directory", save_dir.display())));
        }
        let created = now_ms();
        let name = format!("{created}-{kind}");
        let (files, bytes) = copy_tree(save_dir, &self.saves_dir().join(&name))?;
        let auto: Vec<SaveBackup> = self.save_backups().into_iter().filter(|b| b.name.ends_with("-auto")).collect();
        for old in auto.iter().skip(MAX_AUTO_SAVES) {
            let _ = std::fs::remove_dir_all(self.saves_dir().join(&old.name));
        }
        Ok(SaveBackup {
            name,
            created,
            files,
            bytes,
        })
    }

    /// Makes the save folder the same as a backup. The current saves get a backup first.
    pub fn restore_saves(&self, save_dir: &Path, name: &str) -> Result<SaveBackup> {
        validate_name(name)?;
        let src = self.saves_dir().join(name);
        if !src.is_dir() {
            return Err(Error::NotFound(format!("save backup '{name}'")));
        }
        let before = self.backup_saves(save_dir, "before-restore")?;
        for e in std::fs::read_dir(save_dir)?.filter_map(|e| e.ok()) {
            if e.path().is_dir() {
                std::fs::remove_dir_all(e.path())?;
            } else {
                std::fs::remove_file(e.path())?;
            }
        }
        copy_tree(&src, save_dir)?;
        Ok(before)
    }

    pub fn delete_save_backup(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        let d = self.saves_dir().join(name);
        if !d.is_dir() {
            return Err(Error::NotFound(format!("save backup '{name}'")));
        }
        std::fs::remove_dir_all(d)?;
        Ok(())
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| Error::Other(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modpkg::{ExportChapter, ExportRequest};
    use serde_json::Value;

    struct Env {
        _tmp: tempfile::TempDir,
        ws: PathBuf,
        ch: Chapter,
        loader: Loader,
    }

    /// Workspace with chapter1 extracted, and a game dir with data.win and lang_ja.json.
    fn env() -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let ws = tmp.path().join("ws");
        let dir = tmp.path().join("game/chapter1_windows");
        std::fs::create_dir_all(dir.join("lang")).unwrap();
        std::fs::write(dir.join("data.win"), b"ORIGINAL").unwrap();
        std::fs::write(dir.join("lang/lang_ja.json"), r#"{"a":"A","b":"B"}"#).unwrap();
        let s = ChapterStore::new(&ws, "chapter1");
        std::fs::create_dir_all(s.base().join("rooms")).unwrap();
        std::fs::write(s.base().join("project.json"), "{}").unwrap();
        std::fs::write(s.base().join("rooms/room_a.json"), r#"{"$type":"Room"}"#).unwrap();
        let ch = Chapter {
            id: "chapter1".into(),
            number: 1,
            data_file: dir.join("data.win"),
            dir,
        };
        let loader = Loader::new(&ws);
        Env { _tmp: tmp, ws, ch, loader }
    }

    fn package(e: &Env, id: &str, lang: &[(&str, &str)], room: bool) -> PathBuf {
        let s = ChapterStore::new(&e.ws, "chapter1");
        let rooms = crate::store::asset_type("rooms").unwrap();
        let _ = s.revert(rooms, "room_a");
        let _ = langmod::clear(&s, "lang_ja.json");
        if room {
            s.write(rooms, "room_a", Some(json!({"Width": id.len()})), None).unwrap();
        }
        if !lang.is_empty() {
            let o: serde_json::Map<String, Value> = lang.iter().map(|(k, v)| ((*k).into(), json!(v))).collect();
            langmod::merge(&s, "lang_ja.json", o).unwrap();
        }
        let req = ExportRequest {
            id: id.into(),
            name: None,
            version: String::new(),
            author: String::new(),
            description: String::new(),
            chapters: None,
        };
        let c = [ExportChapter {
            id: "chapter1".into(),
            data_file: e.ch.data_file.clone(),
            store: s,
        }];
        modpkg::export(&e.ws.join("exports"), &req, &c).unwrap().0
    }

    fn lang(e: &Env) -> Value {
        serde_json::from_slice(&std::fs::read(e.ch.dir.join("lang/lang_ja.json")).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn lang_mods_apply_restore_and_game_update() {
        let e = env();
        let p1 = package(&e, "m1", &[("a", "A1")], false);
        let p2 = package(&e, "m2", &[("a", "A2"), ("b", "B2")], false);
        e.loader.install(&p1).unwrap();
        e.loader.install(&p2).unwrap();
        let st = e.loader.status(std::slice::from_ref(&e.ch)).unwrap();
        assert_eq!(st["chapters"][0]["pending"], true);
        let jobs = Jobs::default();
        let job = jobs.start("apply", "game").unwrap();
        e.loader.apply(&jobs, job, None, vec![e.ch.clone()], None).await.unwrap();
        assert_eq!(lang(&e), json!({"a": "A2", "b": "B2"}));
        let st = e.loader.status(std::slice::from_ref(&e.ch)).unwrap();
        assert_eq!(st["chapters"][0]["state"], "modded");
        assert_eq!(st["chapters"][0]["pending"], false);

        // Load order: m2 first, then m1 wins for "a".
        e.loader
            .set_mods(vec![
                ModEntry {
                    id: "m2".into(),
                    enabled: true,
                },
                ModEntry {
                    id: "m1".into(),
                    enabled: true,
                },
            ])
            .unwrap();
        e.loader.apply(&jobs, job, None, vec![e.ch.clone()], None).await.unwrap();
        assert_eq!(lang(&e), json!({"a": "A1", "b": "B2"}));

        // Disable all: the original comes back.
        e.loader
            .set_mods(vec![
                ModEntry {
                    id: "m2".into(),
                    enabled: false,
                },
                ModEntry {
                    id: "m1".into(),
                    enabled: false,
                },
            ])
            .unwrap();
        e.loader.apply(&jobs, job, None, vec![e.ch.clone()], None).await.unwrap();
        assert_eq!(lang(&e), json!({"a": "A", "b": "B"}));
        assert!(e.loader.load_state().applied.is_empty());

        // Game update while modded: the new file becomes the original.
        e.loader
            .set_mods(vec![
                ModEntry {
                    id: "m2".into(),
                    enabled: false,
                },
                ModEntry {
                    id: "m1".into(),
                    enabled: true,
                },
            ])
            .unwrap();
        e.loader.apply(&jobs, job, None, vec![e.ch.clone()], None).await.unwrap();
        std::fs::write(e.ch.dir.join("lang/lang_ja.json"), r#"{"a":"NEW","b":"B","c":"C"}"#).unwrap();
        let st = e.loader.status(std::slice::from_ref(&e.ch)).unwrap();
        assert_eq!(st["chapters"][0]["state"], "game_updated");
        e.loader.apply(&jobs, job, None, vec![e.ch.clone()], None).await.unwrap();
        assert_eq!(lang(&e), json!({"a": "A1", "b": "B", "c": "C"}));
        assert_eq!(
            e.loader.restore(std::slice::from_ref(&e.ch)).unwrap(),
            ["chapter1/lang/lang_ja.json"]
        );
        assert_eq!(lang(&e), json!({"a": "NEW", "b": "B", "c": "C"}));

        e.loader.uninstall("m1").unwrap();
        assert!(e.loader.uninstall("m1").is_err());
        assert!(e.loader.set_mods(vec![]).is_err());
    }

    #[tokio::test]
    async fn asset_mods_need_utmt_and_merge_in_order() {
        let e = env();
        e.loader.install(&package(&e, "one", &[], true)).unwrap();
        e.loader.install(&package(&e, "three", &[], true)).unwrap();
        let jobs = Jobs::default();
        let job = jobs.start("apply", "game").unwrap();
        let r = e.loader.apply(&jobs, job, None, vec![e.ch.clone()], None).await;
        assert!(matches!(r, Err(Error::Conflict(_))));
        // The backup is made before the build.
        assert_eq!(std::fs::read(e.loader.backup_data(&e.ch)).unwrap(), b"ORIGINAL");
        assert_eq!(std::fs::read(&e.ch.data_file).unwrap(), b"ORIGINAL");

        let d = e
            .loader
            .merge("chapter1", &["one".into(), "three".into()], &|l| jobs.log(job, l))
            .unwrap();
        let room: Value = serde_json::from_slice(&std::fs::read(d.join("rooms/room_a.json")).unwrap()).unwrap();
        assert_eq!(room["Width"], 5);
        assert!(
            jobs.get(job)
                .unwrap()
                .log
                .iter()
                .any(|l| l.contains("mod 'three' replaces mod 'one'"))
        );

        // Same game version: no extract needed for the check.
        let c = e.loader.check(&e.ws, "one", std::slice::from_ref(&e.ch)).unwrap();
        assert_eq!(c[0]["same_version"], true);
        assert_eq!(c[0]["assets"][0]["status"], "same");
        std::fs::write(&e.ch.data_file, b"UPDATED").unwrap();
        let c = e.loader.check(&e.ws, "one", std::slice::from_ref(&e.ch)).unwrap();
        assert_eq!(c[0]["same_version"], false);
        assert_eq!(c[0]["checked"], false);
    }

    #[tokio::test]
    async fn xdelta_mod_install_apply_and_wrong_version() {
        let e = env();
        let target = e.ws.join("modded.win");
        std::fs::write(&target, b"ORIGINAL with an xdelta change").unwrap();
        let patch = e.ws.join("my mod.xdelta");
        crate::xdelta::encode(&e.ch.data_file, &target, &patch).unwrap();
        let chapters = std::slice::from_ref(&e.ch);
        let m = e.loader.install_xdelta(&patch, "my mod.xdelta", None, chapters).unwrap();
        assert_eq!(m.id, "my_mod");
        assert!(m.chapters["chapter1"].xdelta.is_some());
        let c = e.loader.check(&e.ws, "my_mod", chapters).unwrap();
        assert_eq!(c[0]["xdelta"], true);
        assert_eq!(c[0]["same_version"], true);

        let jobs = Jobs::default();
        let job = jobs.start("apply", "game").unwrap();
        e.loader.apply(&jobs, job, None, vec![e.ch.clone()], None).await.unwrap();
        assert_eq!(std::fs::read(&e.ch.data_file).unwrap(), std::fs::read(&target).unwrap());
        assert_eq!(e.loader.original_data_file(&e.ch), e.loader.backup_data(&e.ch));
        e.loader.restore(chapters).unwrap();
        assert_eq!(std::fs::read(&e.ch.data_file).unwrap(), b"ORIGINAL");

        // Another game version: install and apply refuse the patch.
        std::fs::write(&e.ch.data_file, b"UPDATED GAME FILE, OTHER VERSION").unwrap();
        assert!(e.loader.install_xdelta(&patch, "x", None, chapters).is_err());
        assert!(e.loader.apply(&jobs, job, None, vec![e.ch.clone()], None).await.is_err());
        assert_eq!(std::fs::read(&e.ch.data_file).unwrap(), b"UPDATED GAME FILE, OTHER VERSION");
    }

    #[test]
    fn save_backups() {
        let e = env();
        let saves = e.ws.join("saves_dir");
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(saves.join("filech1_0"), b"one").unwrap();
        let b = e.loader.backup_saves(&saves, "manual").unwrap();
        assert_eq!(b.files, 1);
        std::fs::write(saves.join("filech1_0"), b"two").unwrap();
        std::fs::write(saves.join("filech1_1"), b"new").unwrap();
        e.loader.restore_saves(&saves, &b.name).unwrap();
        assert_eq!(std::fs::read(saves.join("filech1_0")).unwrap(), b"one");
        assert!(!saves.join("filech1_1").exists());
        let list = e.loader.save_backups();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|x| x.name.ends_with("-before-restore") && x.files == 2));
        e.loader.delete_save_backup(&b.name).unwrap();
        assert!(e.loader.restore_saves(&saves, "../x").is_err());
    }
}
