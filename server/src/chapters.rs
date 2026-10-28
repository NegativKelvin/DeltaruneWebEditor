//! Finds the data files of each Deltarune chapter in the game directory.
//!
//! Layout (Steam, full game): `<game>/data.win` is the chapter select launcher,
//! and `<game>/chapterN_<platform>/data.win` holds chapter N.

use std::path::{Path, PathBuf};

use serde::Serialize;

const DATA_FILE_NAMES: &[&str] = &["data.win", "game.unx", "game.ios", "game.droid"];

#[derive(Debug, Clone, Serialize)]
pub struct Chapter {
    /// Stable id: `launcher`, `chapter1`, `chapter2`, ...
    pub id: String,
    /// Chapter number, 0 for the launcher.
    pub number: u32,
    pub data_file: PathBuf,
    /// Directory that holds the data file (and `lang/`, audio groups, ...).
    pub dir: PathBuf,
}

fn find_data_file(dir: &Path) -> Option<PathBuf> {
    DATA_FILE_NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// Parses `chapter3_windows` -> 3.
fn chapter_number(dir_name: &str) -> Option<u32> {
    let rest = dir_name.strip_prefix("chapter")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || !matches!(rest[digits.len()..].chars().next(), None | Some('_')) {
        return None;
    }
    digits.parse().ok()
}

pub fn discover(game_dir: &Path) -> std::io::Result<Vec<Chapter>> {
    let mut chapters = Vec::new();
    if let Some(data_file) = find_data_file(game_dir) {
        chapters.push(Chapter {
            id: "launcher".into(),
            number: 0,
            data_file,
            dir: game_dir.to_path_buf(),
        });
    }
    for entry in std::fs::read_dir(game_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(number) = chapter_number(&name) else {
            continue;
        };
        if let Some(data_file) = find_data_file(&entry.path()) {
            chapters.push(Chapter {
                id: format!("chapter{number}"),
                number,
                data_file,
                dir: entry.path(),
            });
        }
    }
    chapters.sort_by_key(|c| c.number);
    chapters.dedup_by_key(|c| c.number);
    Ok(chapters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chapter_dirs() {
        assert_eq!(chapter_number("chapter1_windows"), Some(1));
        assert_eq!(chapter_number("chapter12_mac"), Some(12));
        assert_eq!(chapter_number("chapter4"), Some(4));
        assert_eq!(chapter_number("chapterx"), None);
        assert_eq!(chapter_number("chapter1x"), None);
        assert_eq!(chapter_number("lang"), None);
    }

    #[test]
    fn discovers_layout() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("data.win"), b"x").unwrap();
        for d in ["chapter2_windows", "chapter1_windows", "chapter3_windows"] {
            std::fs::create_dir(tmp.path().join(d)).unwrap();
            std::fs::write(tmp.path().join(d).join("data.win"), b"x").unwrap();
        }
        std::fs::create_dir(tmp.path().join("chapter4_windows")).unwrap(); // no data file
        let ids: Vec<_> = discover(tmp.path()).unwrap().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["launcher", "chapter1", "chapter2", "chapter3"]);
    }
}
