//! Deltarune save files (`filech<chapter>_<slot>` in the save folder) and named copies of them
//! (presets and automatic backups) in `<workspace>/saves/`.
//!
//! The game writes one value per line with `file_text_writeln`. The line meanings come from the
//! game's own save script (see `flags::save_layout`).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::flags;
use crate::store::validate_name;

/// Deltarune Steam app id (for the Proton save folder on Linux).
const STEAM_APP_ID: &str = "1671210";
/// Backups kept per save file.
const MAX_BACKUPS: usize = 20;

/// Usual save folder: `%LOCALAPPDATA%\DELTARUNE` on Windows, the Proton prefix on Linux, Application Support on macOS.
/// Only a folder that exists is returned.
pub fn default_dir(game_dir: Option<&Path>) -> Option<PathBuf> {
    let mut c = Vec::new();
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        c.push(PathBuf::from(l).join("DELTARUNE"));
    }
    if let Some(steamapps) = game_dir.and_then(|g| g.ancestors().find(|a| a.file_name().is_some_and(|n| n == "steamapps"))) {
        c.push(steamapps.join(format!(
            "compatdata/{STEAM_APP_ID}/pfx/drive_c/users/steamuser/AppData/Local/DELTARUNE"
        )));
    }
    if let Some(home) = std::env::var_os("HOME") {
        c.push(PathBuf::from(home).join("Library/Application Support/com.tobyfox.deltarune"));
    }
    c.into_iter().find(|p| p.is_dir())
}

/// `filech<N>_<slot>`: returns (chapter, slot).
pub fn parse_name(name: &str) -> Option<(u32, u32)> {
    let (c, s) = name.strip_prefix("filech")?.split_once('_')?;
    let ok = |x: &str| !x.is_empty() && x.len() <= 2 && x.chars().all(|c| c.is_ascii_digit());
    (ok(c) && ok(s)).then(|| Some((c.parse().ok()?, s.parse().ok()?))).flatten()
}

fn check_save_name(name: &str) -> Result<()> {
    parse_name(name)
        .map(|_| ())
        .ok_or_else(|| Error::BadRequest(format!("'{name}' is not a save file name (filech<chapter>_<slot>)")))
}

#[derive(Debug, Serialize)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
}

fn info(path: &Path, name: String) -> Option<FileInfo> {
    let m = std::fs::metadata(path).ok()?;
    let modified = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    let cs = parse_name(&name);
    Some(FileInfo {
        name,
        size: m.len(),
        modified,
        chapter: cs.map(|x| x.0),
        slot: cs.map(|x| x.1),
    })
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// Text of a save file as lines.
#[derive(Debug, Serialize)]
pub struct SaveText {
    pub lines: Vec<String>,
    /// `\r\n` or `\n`.
    pub eol: &'static str,
}

pub struct Saves {
    /// Game save folder.
    pub dir: PathBuf,
    /// `<workspace>/saves`: presets and backups.
    pub copies: PathBuf,
}

impl Saves {
    pub fn new(dir: PathBuf, workspace: &Path) -> Self {
        Self {
            dir,
            copies: workspace.join("saves"),
        }
    }

    pub fn list(&self) -> Result<Vec<FileInfo>> {
        let mut out: Vec<FileInfo> = std::fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_owned();
                parse_name(&name)?;
                info(&e.path(), name)
            })
            .collect();
        out.sort_by_key(|f| (f.chapter, f.slot));
        Ok(out)
    }

    pub fn copies(&self) -> Vec<FileInfo> {
        let Ok(rd) = std::fs::read_dir(&self.copies) else {
            return Vec::new();
        };
        let mut out: Vec<FileInfo> = rd
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_str()?.strip_suffix(".txt")?.to_owned();
                info(&e.path(), name)
            })
            .collect();
        out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
        out
    }

    fn file(&self, name: &str) -> Result<PathBuf> {
        check_save_name(name)?;
        Ok(self.dir.join(name))
    }

    fn copy_path(&self, name: &str) -> Result<PathBuf> {
        validate_name(name)?;
        Ok(self.copies.join(format!("{name}.txt")))
    }

    pub fn read(&self, name: &str) -> Result<SaveText> {
        let bytes = std::fs::read(self.file(name)?)?;
        let text = String::from_utf8(bytes).map_err(|_| Error::BadRequest(format!("'{name}' is not UTF-8 text")))?;
        let (lines, eol, _) = flags::split_lines(&text);
        Ok(SaveText { lines, eol })
    }

    /// Copies the current file to a backup. Returns the backup name (none when the file is missing).
    fn backup(&self, name: &str) -> Result<Option<String>> {
        let src = self.file(name)?;
        if !src.is_file() {
            return Ok(None);
        }
        std::fs::create_dir_all(&self.copies)?;
        let prefix = format!("backup-{name}-");
        let b = format!("{prefix}{}", now_ms());
        std::fs::copy(&src, self.copy_path(&b)?)?;
        // Keep the newest backups of this file.
        let mut old: Vec<FileInfo> = self.copies().into_iter().filter(|c| c.name.starts_with(&prefix)).collect();
        old.sort_by(|a, b| b.name.cmp(&a.name));
        for f in old.into_iter().skip(MAX_BACKUPS) {
            let _ = std::fs::remove_file(self.copy_path(&f.name)?);
        }
        Ok(Some(b))
    }

    /// Writes new lines. The line ending and the last new line of the old file stay. Returns the backup name.
    pub fn write(&self, name: &str, lines: &[String]) -> Result<Option<String>> {
        let path = self.file(name)?;
        if lines.iter().any(|l| l.contains(['\n', '\r'])) {
            return Err(Error::BadRequest("a value must not contain a line break".into()));
        }
        let (eol, trailing) = match std::fs::read_to_string(&path) {
            Ok(t) => {
                let (_, e, tr) = flags::split_lines(&t);
                (e, tr)
            }
            Err(_) => ("\r\n", true),
        };
        let b = self.backup(name)?;
        std::fs::write(&path, flags::join_lines(lines, eol, trailing))?;
        Ok(b)
    }

    /// Saves a named copy of a save file.
    pub fn save_copy(&self, file: &str, copy: &str) -> Result<()> {
        let src = self.file(file)?;
        let dst = self.copy_path(copy)?;
        std::fs::create_dir_all(&self.copies)?;
        std::fs::copy(src, dst)?;
        Ok(())
    }

    /// Puts a named copy into a save slot (the old file gets a backup first). Returns the backup name.
    pub fn load_copy(&self, file: &str, copy: &str) -> Result<Option<String>> {
        let dst = self.file(file)?;
        let src = self.copy_path(copy)?;
        if !src.is_file() {
            return Err(Error::NotFound(format!("save copy '{copy}'")));
        }
        let b = self.backup(file)?;
        std::fs::copy(src, dst)?;
        Ok(b)
    }

    pub fn delete_copy(&self, copy: &str) -> Result<()> {
        std::fs::remove_file(self.copy_path(copy)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(parse_name("filech1_0"), Some((1, 0)));
        assert_eq!(parse_name("filech12_5"), Some((12, 5)));
        assert_eq!(parse_name("filech1_"), None);
        assert_eq!(parse_name("filech1_0.bak"), None);
        assert_eq!(parse_name("../filech1_0"), None);
        assert_eq!(parse_name("dr.ini"), None);
    }

    #[test]
    fn write_backup_copies() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("save");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("filech1_0"), "Kris\r\n10\r\n").unwrap();
        std::fs::write(dir.join("other.txt"), "x").unwrap();
        let s = Saves::new(dir.clone(), &tmp.path().join("ws"));
        assert_eq!(s.list().unwrap().len(), 1);
        let t = s.read("filech1_0").unwrap();
        assert_eq!(t.lines, ["Kris", "10"]);
        assert_eq!(t.eol, "\r\n");

        let b = s.write("filech1_0", &["Kris".into(), "99".into()]).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("filech1_0")).unwrap(), "Kris\r\n99\r\n");
        assert_eq!(std::fs::read_to_string(s.copy_path(&b).unwrap()).unwrap(), "Kris\r\n10\r\n");
        assert!(s.write("filech1_0", &["a\nb".into()]).is_err());
        assert!(s.write("../x", &[]).is_err());

        s.save_copy("filech1_0", "after_castle").unwrap();
        s.write("filech1_0", &["Noelle".into()]).unwrap();
        s.load_copy("filech1_1", "after_castle").unwrap();
        assert_eq!(s.read("filech1_1").unwrap().lines, ["Kris", "99"]);
        assert!(s.copies().iter().any(|c| c.name == "after_castle"));
        s.delete_copy("after_castle").unwrap();
        assert!(s.load_copy("filech1_1", "after_castle").is_err());
    }
}
