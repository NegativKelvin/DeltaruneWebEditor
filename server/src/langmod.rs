//! Translation changes. The game keeps translations in `<chapter dir>/lang/lang_*.json` (not in `data.win`).
//! A mod keeps only the changed keys in `workspace/<chapter>/lang/<file>`; install merges them into a copy
//! of the original file.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::store::{ChapterStore, validate_name};

pub type Overrides = Map<String, Value>;

pub fn validate_file(file: &str) -> Result<()> {
    validate_name(file)?;
    if !file.ends_with(".json") {
        return Err(Error::BadRequest(format!("lang file '{file}' must end with .json")));
    }
    Ok(())
}

/// Lang files of the game chapter dir.
pub fn game_files(chapter_dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(chapter_dir.join("lang"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|f| validate_file(f).is_ok())
        .collect();
    v.sort();
    v
}

/// Changed files: file -> number of changed keys.
pub fn list(store: &ChapterStore) -> BTreeMap<String, usize> {
    std::fs::read_dir(store.lang_dir())
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|f| validate_file(f).is_ok())
        .map(|f| {
            let n = read(store, &f).map(|o| o.len()).unwrap_or(0);
            (f, n)
        })
        .collect()
}

pub fn read(store: &ChapterStore, file: &str) -> Result<Overrides> {
    validate_file(file)?;
    match std::fs::read(store.lang_dir().join(file)) {
        Ok(b) => Ok(serde_json::from_slice(&b)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.into()),
    }
}

/// Merges changes into the overrides of a file: a string sets a key, `null` removes the change.
pub fn merge(store: &ChapterStore, file: &str, changes: Overrides) -> Result<Overrides> {
    if let Some((k, _)) = changes.iter().find(|(_, v)| !(v.is_string() || v.is_null())) {
        return Err(Error::BadRequest(format!("value of '{k}' must be a string or null")));
    }
    let mut cur = read(store, file)?;
    for (k, v) in changes {
        if v.is_null() {
            cur.remove(&k);
        } else {
            cur.insert(k, v);
        }
    }
    let path = store.lang_dir().join(file);
    if cur.is_empty() {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    } else {
        std::fs::create_dir_all(store.lang_dir())?;
        std::fs::write(path, serde_json::to_vec_pretty(&cur)?)?;
    }
    Ok(cur)
}

pub fn clear(store: &ChapterStore, file: &str) -> Result<()> {
    validate_file(file)?;
    let path = store.lang_dir().join(file);
    if !path.exists() {
        return Err(Error::NotFound(format!("no changes for lang/{file}")));
    }
    std::fs::remove_file(path)?;
    Ok(())
}

/// The original lang file with the overrides set, in order (a later override wins).
pub fn apply(original: &[u8], overrides: &[Overrides]) -> Result<Vec<u8>> {
    let mut v: Value = serde_json::from_slice(original)?;
    let obj = v
        .as_object_mut()
        .ok_or_else(|| Error::BadRequest("lang file is not a JSON object".into()))?;
    for o in overrides {
        for (k, val) in o {
            obj.insert(k.clone(), val.clone());
        }
    }
    Ok(serde_json::to_vec(&v)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_and_apply() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ChapterStore::new(tmp.path(), "chapter1");
        let Value::Object(ch) = json!({"a": "A2", "b": "B2"}) else {
            panic!()
        };
        merge(&s, "lang_ja.json", ch).unwrap();
        let Value::Object(ch) = json!({"b": null}) else { panic!() };
        assert_eq!(merge(&s, "lang_ja.json", ch).unwrap().len(), 1);
        assert_eq!(list(&s)["lang_ja.json"], 1);
        let Value::Object(bad) = json!({"x": 1}) else { panic!() };
        assert!(merge(&s, "lang_ja.json", bad).is_err());
        assert!(read(&s, "../x.json").is_err());

        let out = apply(br#"{"a":"A","c":"C"}"#, &[read(&s, "lang_ja.json").unwrap()]).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&out).unwrap(), json!({"a": "A2", "c": "C"}));
        clear(&s, "lang_ja.json").unwrap();
        assert!(list(&s).is_empty());
    }
}
