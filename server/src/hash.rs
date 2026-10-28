//! SHA-256 of files. Game data files are large, so their hashes are kept in memory by path, size, and time.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

type Key = (PathBuf, u64, Option<SystemTime>);
static CACHE: LazyLock<Mutex<HashMap<Key, String>>> = LazyLock::new(Mutex::default);

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 (hex) of a file. Read again only when its size or modification time changes.
pub fn file_sha256(path: &Path) -> std::io::Result<String> {
    let meta = std::fs::metadata(path)?;
    let key = (path.to_path_buf(), meta.len(), meta.modified().ok());
    if let Some(h) = CACHE.lock().unwrap().get(&key) {
        return Ok(h.clone());
    }
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let h = hex(&hasher.finalize());
    CACHE.lock().unwrap().insert(key, h.clone());
    Ok(h)
}

/// SHA-256 (hex) of a set of named files: name, length, and bytes of each, in name order.
pub fn files_sha256(files: &[PathBuf]) -> std::io::Result<String> {
    let mut named: Vec<(&str, &PathBuf)> = files.iter().filter_map(|p| Some((p.file_name()?.to_str()?, p))).collect();
    named.sort();
    let mut hasher = Sha256::new();
    for (name, p) in named {
        let bytes = std::fs::read(p)?;
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_files() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.bin");
        std::fs::write(&a, b"abc").unwrap();
        assert_eq!(
            file_sha256(&a).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let b = tmp.path().join("b.png");
        std::fs::write(&b, b"x").unwrap();
        let one = files_sha256(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(one, files_sha256(&[b.clone(), a.clone()]).unwrap());
        std::fs::write(&b, b"y").unwrap();
        assert_ne!(one, files_sha256(&[a, b]).unwrap());
    }
}
