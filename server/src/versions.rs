//! Version-safe edits. Each change in `mod/` is kept by asset name, with the hash of the game asset it
//! started from (`<chapter>/mod-base.json`). After a game update and a new extract, the report shows
//! which changed assets the game also changed, removed, or now uses as a name.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::store::{self, AssetType, ChapterStore};

/// `type/name` -> hash of the base asset when the change started (`None`: new asset).
pub type BaseRecord = BTreeMap<String, Option<String>>;

fn record_path(store: &ChapterStore) -> PathBuf {
    store.root.join("mod-base.json")
}

pub fn key(ty: &AssetType, name: &str) -> String {
    format!("{}/{name}", ty.slug)
}

pub fn load(store: &ChapterStore) -> BaseRecord {
    std::fs::read(record_path(store))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save(store: &ChapterStore, rec: &BaseRecord) -> Result<()> {
    std::fs::create_dir_all(&store.root)?;
    std::fs::write(record_path(store), serde_json::to_vec_pretty(rec)?)?;
    Ok(())
}

/// Sets the base hash of one asset.
pub fn set_base(store: &ChapterStore, ty: &AssetType, name: &str, hash: Option<String>) -> Result<()> {
    let mut rec = load(store);
    rec.insert(key(ty, name), hash);
    save(store, &rec)
}

/// Gives every changed asset without a record the hash of the current base (changes made before this record existed).
pub fn record_missing(store: &ChapterStore) -> Result<BaseRecord> {
    let mut rec = load(store);
    let mut added = false;
    for (ty, e) in store.changed_assets() {
        let k = key(ty, &e.name);
        if let std::collections::btree_map::Entry::Vacant(v) = rec.entry(k) {
            v.insert(store.asset_hash(ty, &e.name, false)?);
            added = true;
        }
    }
    if added {
        save(store, &rec)?;
    }
    Ok(rec)
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The game asset is the same as when the change started.
    Same,
    /// The game changed this asset. The change replaces the new game version.
    Changed,
    /// The game no longer has this asset.
    Removed,
    /// A new asset of the mod has a name that the game now uses.
    NameTaken,
    /// A new asset. The game does not use the name.
    New,
    /// No record of the base version.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
    pub status: Status,
}

/// Compares recorded base hashes with the current base of a chapter.
/// `items`: (type slug, name, record) where record `None` is unknown and `Some(None)` is a new asset.
pub fn compare<'a>(
    store: &ChapterStore,
    items: impl IntoIterator<Item = (&'a str, &'a str, Option<Option<&'a str>>)>,
) -> Result<Vec<Check>> {
    items
        .into_iter()
        .map(|(slug, name, rec)| {
            let ty = store::asset_type(slug)?;
            let now = store.asset_hash(ty, name, false)?;
            let status = match (rec, now.as_deref()) {
                (None, _) => Status::Unknown,
                (Some(None), None) => Status::New,
                (Some(None), Some(_)) => Status::NameTaken,
                (Some(Some(_)), None) => Status::Removed,
                (Some(Some(a)), Some(b)) if a == b => Status::Same,
                (Some(Some(_)), Some(_)) => Status::Changed,
            };
            Ok(Check {
                ty: slug.to_owned(),
                name: name.to_owned(),
                status,
            })
        })
        .collect()
}

/// Version report for all changed assets of a chapter.
pub fn report(store: &ChapterStore) -> Result<Vec<Check>> {
    let rec = load(store);
    let changed = store.changed_assets();
    let keys: Vec<String> = changed.iter().map(|(t, e)| key(t, &e.name)).collect();
    compare(
        store,
        changed
            .iter()
            .zip(&keys)
            .map(|((t, e), k)| (t.slug, e.name.as_str(), rec.get(k).map(|h| h.as_deref()))),
    )
}

/// Marks a changed asset as checked against the current game version.
pub fn accept(store: &ChapterStore, slug: &str, name: &str) -> Result<()> {
    let ty = store::asset_type(slug)?;
    if !store.list(ty).iter().any(|e| e.name == name && e.modified) {
        return Err(Error::NotFound(format!("no changes for {slug}/{name}")));
    }
    set_base(store, ty, name, store.asset_hash(ty, name, false)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn report_after_game_update() {
        let tmp = tempfile::tempdir().unwrap();
        let s = ChapterStore::new(tmp.path(), "chapter1");
        let base = s.base();
        std::fs::create_dir_all(base.join("rooms")).unwrap();
        std::fs::write(base.join("project.json"), "{}").unwrap();
        for r in ["room_a", "room_b", "room_c"] {
            std::fs::write(base.join(format!("rooms/{r}.json")), r#"{"$type":"Room","Width":1}"#).unwrap();
        }
        let rooms = store::asset_type("rooms").unwrap();
        for r in ["room_a", "room_b", "room_c", "room_new", "room_new2"] {
            s.write(rooms, r, Some(json!({"Width": 2})), None).unwrap();
        }
        assert!(report(&s).unwrap().iter().all(|c| matches!(c.status, Status::Same | Status::New)));

        // Game update: room_b changes, room_c goes, room_new2 is now a game room.
        std::fs::write(base.join("rooms/room_b.json"), r#"{"$type":"Room","Width":5}"#).unwrap();
        std::fs::remove_file(base.join("rooms/room_c.json")).unwrap();
        std::fs::write(base.join("rooms/room_new2.json"), r#"{"$type":"Room"}"#).unwrap();
        let got: BTreeMap<String, Status> = report(&s).unwrap().into_iter().map(|c| (c.name, c.status)).collect();
        assert_eq!(got["room_a"], Status::Same);
        assert_eq!(got["room_b"], Status::Changed);
        assert_eq!(got["room_c"], Status::Removed);
        assert_eq!(got["room_new"], Status::New);
        assert_eq!(got["room_new2"], Status::NameTaken);

        accept(&s, "rooms", "room_b").unwrap();
        let b = report(&s).unwrap().into_iter().find(|c| c.name == "room_b").unwrap();
        assert_eq!(b.status, Status::Same);
        assert!(accept(&s, "rooms", "room_zzz").is_err());

        // Revert then change again: the record starts from the current base.
        std::fs::remove_file(record_path(&s)).unwrap();
        assert_eq!(report(&s).unwrap()[0].status, Status::Unknown);
        record_missing(&s).unwrap();
        assert!(report(&s).unwrap().iter().all(|c| c.status != Status::Unknown));
    }
}
