//! HTTP API for the webpage. All routes are under `/api`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::chapters::{self, Chapter};
use crate::conflicts;
use crate::cutscene;
use crate::dialogue::{self, DialogueLine};
use crate::enemies;
use crate::error::{Error, Result};
use crate::flags;
use crate::gml;
use crate::jobs::Jobs;
use crate::live;
use crate::patterns::{self, Pattern, PatternStore};
use crate::saves::{self, Saves};
use crate::store::{self, ASSET_TYPES, ChapterStore};
use crate::utmt::Utmt;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Deltarune install dir (holds `data.win` and `chapterN_*/`).
    pub game_dir: Option<PathBuf>,
    /// Path to the UndertaleModCli executable.
    pub utmt_cli: Option<PathBuf>,
    /// Game save folder (holds `filech1_0`, ...). Found automatically when not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_dir: Option<PathBuf>,
}

pub struct AppState {
    pub workspace: PathBuf,
    pub settings: RwLock<Settings>,
    pub jobs: Jobs,
    dialogue_cache: Mutex<HashMap<String, Arc<Vec<DialogueLine>>>>,
    cutscene_cache: Mutex<HashMap<String, Arc<CutsceneList>>>,
    enemy_cache: Mutex<HashMap<String, Arc<Vec<enemies::Record>>>>,
    flag_cache: Mutex<HashMap<String, Arc<Vec<flags::Ref>>>>,
    /// Hot reload commands not acknowledged by the game yet.
    live: Mutex<live::Queue>,
    /// Goes up on each invalidate, so an index that was built from older files is not kept.
    epoch: AtomicU64,
    /// Chapter dir -> lang files (read again when a file changes).
    lang_cache: Mutex<HashMap<PathBuf, (dialogue::LangStamp, Arc<dialogue::Langs>)>>,
    /// Held while an asset is changed, so a patch cannot check its rev while another write runs.
    write_lock: Mutex<()>,
}

/// Code entries with `c_*` steps: (name, step count).
type CutsceneList = Vec<(String, usize)>;

/// Gets a per-chapter index from a cache, or builds it. The index is not kept when files changed during the build.
fn cached<T>(cache: &Mutex<HashMap<String, Arc<T>>>, epoch: &AtomicU64, chapter: &str, build: impl FnOnce() -> T) -> Arc<T> {
    if let Some(c) = cache.lock().unwrap().get(chapter) {
        return c.clone();
    }
    let start = epoch.load(Ordering::SeqCst);
    let v = Arc::new(build());
    let mut c = cache.lock().unwrap();
    if epoch.load(Ordering::SeqCst) == start {
        c.insert(chapter.to_owned(), v.clone());
    }
    v
}

type St = State<Arc<AppState>>;

impl AppState {
    pub fn new(workspace: PathBuf, settings: Settings) -> Arc<Self> {
        Arc::new(Self {
            workspace,
            settings: RwLock::new(settings),
            jobs: Jobs::default(),
            dialogue_cache: Mutex::default(),
            cutscene_cache: Mutex::default(),
            enemy_cache: Mutex::default(),
            flag_cache: Mutex::default(),
            live: Mutex::default(),
            epoch: AtomicU64::new(0),
            lang_cache: Mutex::default(),
            write_lock: Mutex::default(),
        })
    }

    fn write_lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.write_lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn settings_path(&self) -> PathBuf {
        self.workspace.join("settings.json")
    }

    pub fn load_saved_settings(workspace: &std::path::Path) -> Settings {
        std::fs::read(workspace.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn chapters(&self) -> Result<Vec<Chapter>> {
        let dir = self
            .settings
            .read()
            .unwrap()
            .game_dir
            .clone()
            .ok_or_else(|| Error::Conflict("game_dir is not set".into()))?;
        Ok(chapters::discover(&dir)?)
    }

    fn chapter(&self, id: &str) -> Result<(Chapter, ChapterStore)> {
        let ch = self
            .chapters()?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| Error::NotFound(format!("chapter '{id}'")))?;
        let store = ChapterStore::new(&self.workspace, &ch.id);
        Ok((ch, store))
    }

    /// Store for an extracted chapter.
    fn extracted(&self, id: &str) -> Result<ChapterStore> {
        let (_, store) = self.chapter(id)?;
        if !store.is_extracted() {
            return Err(Error::Conflict(format!(
                "chapter '{id}' is not extracted; POST /api/chapters/{id}/extract"
            )));
        }
        Ok(store)
    }

    fn utmt(&self) -> Result<Utmt> {
        let cli = self
            .settings
            .read()
            .unwrap()
            .utmt_cli
            .clone()
            .ok_or_else(|| Error::Conflict("utmt_cli is not set".into()))?;
        Ok(Utmt { cli })
    }

    fn invalidate(&self, chapter: &str) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.dialogue_cache.lock().unwrap().remove(chapter);
        self.cutscene_cache.lock().unwrap().remove(chapter);
        self.enemy_cache.lock().unwrap().remove(chapter);
        self.flag_cache.lock().unwrap().remove(chapter);
    }

    /// Game save folder: the setting, else the usual place for the platform.
    fn save_dir(&self) -> Result<PathBuf> {
        let s = self.settings.read().unwrap();
        s.save_dir
            .clone()
            .or_else(|| saves::default_dir(s.game_dir.as_deref()))
            .ok_or_else(|| Error::Conflict("save_dir is not set and the Deltarune save folder was not found".into()))
    }

    fn saves(&self) -> Result<Saves> {
        Ok(Saves::new(self.save_dir()?, &self.workspace))
    }

    fn live_dir(&self) -> Result<PathBuf> {
        Ok(self.save_dir()?.join(live::DIR))
    }

    fn flag_refs(&self, chapter: &str, store: &ChapterStore) -> Arc<Vec<flags::Ref>> {
        cached(&self.flag_cache, &self.epoch, chapter, || {
            store
                .code_sources()
                .flat_map(|(name, src)| flags::scan_source(&name, &src))
                .collect()
        })
    }

    /// `lang/*.json` of a chapter dir, from the cache while no file changed.
    fn langs(&self, dir: &std::path::Path) -> Arc<dialogue::Langs> {
        let stamp = dialogue::lang_stamp(dir);
        if let Some((st, l)) = self.lang_cache.lock().unwrap().get(dir)
            && *st == stamp
        {
            return l.clone();
        }
        let l = Arc::new(dialogue::load_lang(dir));
        self.lang_cache.lock().unwrap().insert(dir.to_path_buf(), (stamp, l.clone()));
        l
    }

    fn dialogue(&self, chapter: &str, store: &ChapterStore) -> Arc<Vec<DialogueLine>> {
        cached(&self.dialogue_cache, &self.epoch, chapter, || {
            store
                .code_sources()
                .flat_map(|(name, src)| dialogue::scan_source(&name, &src))
                .collect()
        })
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| Error::Other(e.to_string()))?
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") })) }),
        )
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/asset-types", get(|| async { Json(ASSET_TYPES) }))
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/jobs/{id}/cancel", post(cancel_job))
        .route("/api/chapters", get(list_chapters))
        .route("/api/chapters/{ch}/extract", post(extract))
        .route("/api/chapters/{ch}/build", post(build))
        .route("/api/chapters/{ch}/changes", get(changes))
        .route("/api/chapters/{ch}/assets", get(asset_counts))
        .route("/api/chapters/{ch}/assets/{ty}", get(list_assets))
        .route(
            "/api/chapters/{ch}/assets/{ty}/{name}",
            get(get_asset).put(put_asset).delete(revert_asset),
        )
        .route(
            "/api/chapters/{ch}/assets/{ty}/{name}/files/{file}",
            get(get_file).put(put_file).delete(delete_file),
        )
        .route("/api/chapters/{ch}/code/{name}/patch", post(patch_code))
        .route("/api/chapters/{ch}/index/{ty}", get(asset_index))
        .route("/api/chapters/{ch}/cutscenes", get(list_cutscenes))
        .route("/api/chapters/{ch}/cutscenes/{code}", get(get_cutscene))
        .route("/api/chapters/{ch}/enemies", get(get_enemies))
        .route("/api/chapters/{ch}/generic", get(generic_index))
        .route("/api/chapters/{ch}/generic/{list}", get(generic_list))
        .route("/api/chapters/{ch}/dialogue", get(get_dialogue))
        .route("/api/chapters/{ch}/search", get(search))
        .route("/api/chapters/{ch}/patterns", get(list_patterns))
        .route(
            "/api/chapters/{ch}/patterns/{name}",
            get(get_pattern).put(put_pattern).delete(delete_pattern),
        )
        .route("/api/chapters/{ch}/patterns/{name}/gml", post(pattern_gml))
        .route("/api/chapters/{ch}/patterns/{name}/install", post(install_pattern))
        .route("/api/chapters/{ch}/flags", get(list_flags))
        .route("/api/chapters/{ch}/flags/refs", get(flag_refs))
        .route("/api/chapters/{ch}/flag-notes", get(get_flag_notes).put(put_flag_notes))
        .route("/api/chapters/{ch}/save-layout", get(save_layout))
        .route("/api/saves", get(list_saves))
        .route("/api/saves/{file}", get(read_save).put(write_save))
        .route("/api/saves/{file}/copy", post(copy_save))
        .route("/api/saves/{file}/load", post(load_save))
        .route("/api/save-copies/{name}", axum::routing::delete(delete_save_copy))
        .route("/api/live", get(live_status))
        .route("/api/live/commands", post(live_commands))
        .route("/api/live/sprite/{name}", post(live_sprite))
        .route("/api/live/flags", get(live_flags))
        .route(
            "/api/chapters/{ch}/live",
            get(live_install_status).post(live_install).delete(live_uninstall),
        )
        .route("/api/chapters/{ch}/mods", get(list_mods))
        .route("/api/chapters/{ch}/conflicts", post(check_conflicts))
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
        .with_state(state)
}

async fn get_settings(State(s): St) -> Json<Value> {
    let mut v = json!(s.settings.read().unwrap().clone());
    // The folder that is used when save_dir is not set.
    v["save_dir_found"] = json!(s.save_dir().ok());
    Json(v)
}

async fn put_settings(State(s): St, Json(new): Json<Settings>) -> Result<Json<Settings>> {
    if let Some(d) = &new.game_dir
        && !d.is_dir()
    {
        return Err(Error::BadRequest(format!("game_dir '{}' is not a directory", d.display())));
    }
    if let Some(d) = &new.save_dir
        && !d.is_dir()
    {
        return Err(Error::BadRequest(format!("save_dir '{}' is not a directory", d.display())));
    }
    if let Some(c) = &new.utmt_cli
        && !c.is_file()
    {
        return Err(Error::BadRequest(format!("utmt_cli '{}' is not a file", c.display())));
    }
    // The API writes files only in the workspace. A CLI there could be a file sent through the API.
    if let Some(c) = &new.utmt_cli
        && std::fs::canonicalize(c)?.starts_with(std::fs::canonicalize(&s.workspace).unwrap_or_else(|_| s.workspace.clone()))
    {
        return Err(Error::BadRequest(format!(
            "utmt_cli '{}' is in the workspace; use a file outside it",
            c.display()
        )));
    }
    let merged = {
        let mut cur = s.settings.write().unwrap();
        if new.game_dir.is_some() {
            cur.game_dir = new.game_dir;
        }
        if new.utmt_cli.is_some() {
            cur.utmt_cli = new.utmt_cli;
        }
        if new.save_dir.is_some() {
            cur.save_dir = new.save_dir;
        }
        cur.clone()
    };
    std::fs::create_dir_all(&s.workspace)?;
    std::fs::write(s.settings_path(), serde_json::to_vec_pretty(&merged)?)?;
    Ok(Json(merged))
}

async fn list_jobs(State(s): St) -> impl IntoResponse {
    Json(s.jobs.list())
}

async fn get_job(State(s): St, Path(id): Path<u64>) -> Result<impl IntoResponse> {
    s.jobs.get(id).map(Json).ok_or_else(|| Error::NotFound(format!("job {id}")))
}

async fn cancel_job(State(s): St, Path(id): Path<u64>) -> Result<StatusCode> {
    if s.jobs.cancel(id) {
        Ok(StatusCode::ACCEPTED)
    } else {
        Err(Error::Conflict(format!("job {id} is not running")))
    }
}

async fn list_chapters(State(s): St) -> Result<Json<Vec<Value>>> {
    let ws = s.workspace.clone();
    let chapters = s.chapters()?;
    blocking(move || {
        Ok(chapters
            .into_iter()
            .map(|c| {
                let store = ChapterStore::new(&ws, &c.id);
                let meta: Option<Value> = std::fs::read(store.root.join("meta.json"))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok());
                json!({ "id": c.id, "number": c.number, "data_file": c.data_file, "extracted": store.is_extracted(), "meta": meta })
            })
            .collect())
    })
    .await
    .map(Json)
}

async fn extract(State(s): St, Path(ch): Path<String>) -> Result<impl IntoResponse> {
    let (chapter, store) = s.chapter(&ch)?;
    let utmt = s.utmt()?;
    let job = s
        .jobs
        .start("extract", &ch)
        .ok_or_else(|| Error::Conflict(format!("a job already runs for '{ch}'")))?;
    let st = s.clone();
    tokio::spawn(async move {
        let r = utmt.extract(&st.jobs, job, &chapter, &store).await;
        st.invalidate(&chapter.id);
        st.jobs.finish(job, r);
    });
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))))
}

async fn build(State(s): St, Path(ch): Path<String>) -> Result<impl IntoResponse> {
    let store = s.extracted(&ch)?;
    let (chapter, _) = s.chapter(&ch)?;
    let utmt = s.utmt()?;
    let job = s
        .jobs
        .start("build", &ch)
        .ok_or_else(|| Error::Conflict(format!("a job already runs for '{ch}'")))?;
    let st = s.clone();
    tokio::spawn(async move {
        let r = utmt.build(&st.jobs, job, &chapter, &store).await.map(|_| ());
        st.jobs.finish(job, r);
    });
    Ok((StatusCode::ACCEPTED, Json(json!({ "job": job }))))
}

async fn changes(State(s): St, Path(ch): Path<String>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    blocking(move || {
        let mut out = serde_json::Map::new();
        for ty in ASSET_TYPES {
            let changed: Vec<_> = store.list(ty).into_iter().filter(|e| e.modified).collect();
            if !changed.is_empty() {
                out.insert(ty.slug.into(), serde_json::to_value(changed)?);
            }
        }
        Ok(Json(Value::Object(out)))
    })
    .await
}

async fn asset_counts(State(s): St, Path(ch): Path<String>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    blocking(move || {
        let counts = store.counts();
        Ok(Json(json!(
            ASSET_TYPES
                .iter()
                .map(|t| json!({ "slug": t.slug, "json_type": t.json_type, "count": counts[t.slug] }))
                .collect::<Vec<_>>()
        )))
    })
    .await
}

#[derive(Deserialize)]
struct ListQuery {
    q: Option<String>,
    #[serde(default)]
    offset: usize,
    limit: Option<usize>,
    code: Option<String>,
    max: Option<usize>,
    fields: Option<String>,
}

async fn list_assets(State(s): St, Path((ch, ty)): Path<(String, String)>, Query(q): Query<ListQuery>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let ty = store::asset_type(&ty)?;
    blocking(move || {
        let needle = q.q.map(|x| x.to_lowercase());
        let all: Vec<_> = store
            .list(ty)
            .into_iter()
            .filter(|e| needle.as_ref().is_none_or(|n| e.name.to_lowercase().contains(n)))
            .collect();
        let total = all.len();
        let items: Vec<_> = all.into_iter().skip(q.offset).take(q.limit.unwrap_or(usize::MAX)).collect();
        Ok(Json(json!({ "total": total, "offset": q.offset, "items": items })))
    })
    .await
}

async fn get_asset(State(s): St, Path((ch, ty, name)): Path<(String, String, String)>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let ty = store::asset_type(&ty)?;
    Ok(Json(store.read(ty, &name)?))
}

#[derive(Deserialize)]
struct PutAsset {
    data: Option<Value>,
    source: Option<String>,
}

async fn put_asset(State(s): St, Path((ch, ty, name)): Path<(String, String, String)>, Json(body): Json<PutAsset>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let ty = store::asset_type(&ty)?;
    if body.data.is_none() && body.source.is_none() {
        return Err(Error::BadRequest("give 'data' and/or 'source'".into()));
    }
    let out = {
        let _w = s.write_lock();
        store.write(ty, &name, body.data, body.source)?
    };
    if ty.slug == "code" {
        s.invalidate(&ch);
    }
    Ok(Json(out))
}

async fn revert_asset(State(s): St, Path((ch, ty, name)): Path<(String, String, String)>) -> Result<StatusCode> {
    let store = s.extracted(&ch)?;
    let ty = store::asset_type(&ty)?;
    {
        let _w = s.write_lock();
        store.revert(ty, &name)?;
    }
    if ty.slug == "code" {
        s.invalidate(&ch);
    }
    Ok(StatusCode::NO_CONTENT)
}

fn content_type(file: &str) -> &'static str {
    match file.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "json" => "application/json",
        "gml" | "txt" | "glsl" | "hlsl" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

async fn get_file(State(s): St, Path((ch, ty, name, file)): Path<(String, String, String, String)>) -> Result<impl IntoResponse> {
    let store = s.extracted(&ch)?;
    let ty = store::asset_type(&ty)?;
    let path = store.file_path(ty, &name, &file)?;
    let bytes = tokio::fs::read(path).await?;
    Ok((
        [(header::CONTENT_TYPE, content_type(&file)), (header::CACHE_CONTROL, "no-cache")],
        bytes,
    ))
}

async fn put_file(State(s): St, Path((ch, ty, name, file)): Path<(String, String, String, String)>, body: Bytes) -> Result<StatusCode> {
    let store = s.extracted(&ch)?;
    let ty = store::asset_type(&ty)?;
    {
        let _w = s.write_lock();
        store.write_file(ty, &name, &file, &body)?;
    }
    if ty.slug == "code" {
        s.invalidate(&ch);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_file(State(s): St, Path((ch, ty, name, file)): Path<(String, String, String, String)>) -> Result<StatusCode> {
    let store = s.extracted(&ch)?;
    let ty = store::asset_type(&ty)?;
    {
        let _w = s.write_lock();
        store.delete_file(ty, &name, &file)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PatchCode {
    rev: String,
    edits: Vec<gml::Edit>,
}

async fn patch_code(State(s): St, Path((ch, name)): Path<(String, String)>, Json(body): Json<PatchCode>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let out = {
        let _w = s.write_lock();
        store.patch_code(&name, &body.rev, &body.edits)?
    };
    s.invalidate(&ch);
    Ok(Json(out))
}

async fn asset_index(State(s): St, Path((ch, ty)): Path<(String, String)>, Query(q): Query<ListQuery>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let ty = store::asset_type(&ty)?;
    let fields = q.fields.unwrap_or_default();
    blocking(move || {
        let f: Vec<&str> = fields.split(',').map(str::trim).filter(|x| !x.is_empty()).collect();
        Ok(Json(json!(store.index(ty, &f))))
    })
    .await
}

async fn list_cutscenes(State(s): St, Path(ch): Path<String>, Query(q): Query<ListQuery>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let st = s.clone();
    blocking(move || {
        let all = cached(&st.cutscene_cache, &st.epoch, &ch, || {
            let mut v: Vec<(String, usize)> = store
                .code_sources()
                .map(|(name, src)| (name, cutscene::count(&src)))
                .filter(|x| x.1 > 0)
                .collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            v
        });
        let needle = q.q.map(|x| x.to_lowercase());
        let items: Vec<Value> = all
            .iter()
            .filter(|(n, _)| needle.as_ref().is_none_or(|q| n.to_lowercase().contains(q)))
            .map(|(n, c)| json!({ "code": n, "steps": c }))
            .collect();
        Ok(Json(json!({ "total": items.len(), "items": items })))
    })
    .await
}

async fn get_cutscene(State(s): St, Path((ch, code)): Path<(String, String)>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let asset = store.read(store::asset_type("code")?, &code)?;
    let src = asset["source"].as_str().unwrap_or_default();
    Ok(Json(json!({
        "code": code,
        "rev": asset["rev"],
        "modified": asset["modified"],
        "source": src,
        "blocks": cutscene::blocks(src),
    })))
}

async fn get_enemies(State(s): St, Path(ch): Path<String>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let st = s.clone();
    blocking(move || {
        let recs = cached(&st.enemy_cache, &st.epoch, &ch, || {
            store
                .code_sources()
                .flat_map(|(name, src)| enemies::scan_source(&name, &src))
                .collect()
        });
        Ok(Json(json!(*recs)))
    })
    .await
}

async fn generic_index(State(s): St, Path(ch): Path<String>) -> Result<Json<Value>> {
    Ok(Json(s.extracted(&ch)?.generic_index()?))
}

async fn generic_list(State(s): St, Path((ch, list)): Path<(String, String)>, Query(q): Query<ListQuery>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    blocking(move || Ok(Json(store.generic_list(&list, q.offset, q.limit.unwrap_or(100), q.q.as_deref())?))).await
}

async fn get_dialogue(State(s): St, Path(ch): Path<String>, Query(q): Query<ListQuery>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let (chapter, _) = s.chapter(&ch)?;
    let st = s.clone();
    blocking(move || {
        let lines = st.dialogue(&chapter.id, &store);
        let langs = st.langs(&chapter.dir);
        let needle = q.q.map(|x| x.to_lowercase());
        let matches = |l: &DialogueLine| {
            q.code.as_ref().is_none_or(|c| &l.code == c)
                && needle.as_ref().is_none_or(|n| {
                    l.text.as_ref().is_some_and(|t| t.to_lowercase().contains(n))
                        || l.key.as_ref().is_some_and(|k| {
                            k.to_lowercase().contains(n) || langs.values().any(|t| t.get(k).is_some_and(|v| v.to_lowercase().contains(n)))
                        })
                })
        };
        let filtered: Vec<&DialogueLine> = lines.iter().filter(|l| matches(l)).collect();
        let total = filtered.len();
        let items: Vec<Value> = filtered
            .into_iter()
            .skip(q.offset)
            .take(q.limit.unwrap_or(200))
            .map(|l| {
                let tr: serde_json::Map<String, Value> = l
                    .key
                    .as_ref()
                    .map(|k| {
                        langs
                            .iter()
                            .filter_map(|(lang, t)| Some((lang.clone(), Value::String(t.get(k)?.clone()))))
                            .collect()
                    })
                    .unwrap_or_default();
                let mut v = serde_json::to_value(l).unwrap();
                v["translations"] = Value::Object(tr);
                v
            })
            .collect();
        Ok(Json(
            json!({ "total": total, "offset": q.offset, "languages": langs.keys().collect::<Vec<_>>(), "items": items }),
        ))
    })
    .await
}

async fn search(State(s): St, Path(ch): Path<String>, Query(q): Query<ListQuery>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let needle =
        q.q.filter(|x| !x.is_empty())
            .ok_or_else(|| Error::BadRequest("q is required".into()))?;
    let max = q.max.unwrap_or(500).min(5000);
    blocking(move || Ok(Json(json!(store.search_code(&needle, max))))).await
}

async fn list_patterns(State(s): St, Path(ch): Path<String>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    Ok(Json(json!(PatternStore::new(&store).list())))
}

async fn get_pattern(State(s): St, Path((ch, name)): Path<(String, String)>) -> Result<Json<Pattern>> {
    let store = s.extracted(&ch)?;
    Ok(Json(PatternStore::new(&store).read(&name)?))
}

async fn put_pattern(State(s): St, Path((ch, name)): Path<(String, String)>, Json(pat): Json<Pattern>) -> Result<Json<Pattern>> {
    let store = s.extracted(&ch)?;
    PatternStore::new(&store).write(&name, &pat)?;
    Ok(Json(pat))
}

#[derive(Deserialize)]
struct DeletePattern {
    /// Also remove the installed wave object and its code from the mod.
    #[serde(default)]
    uninstall: bool,
}

async fn delete_pattern(State(s): St, Path((ch, name)): Path<(String, String)>, Query(q): Query<DeletePattern>) -> Result<StatusCode> {
    let store = s.extracted(&ch)?;
    PatternStore::new(&store).delete(&name)?;
    if q.uninstall {
        let st = s.clone();
        blocking(move || {
            let _w = st.write_lock();
            patterns::uninstall(&store, &name)
        })
        .await?;
        s.invalidate(&ch);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Body: a pattern (not saved yet), or empty for the saved one.
fn body_or_saved(store: &ChapterStore, name: &str, body: &Bytes) -> Result<Pattern> {
    if body.iter().all(u8::is_ascii_whitespace) {
        PatternStore::new(store).read(name)
    } else {
        Ok(serde_json::from_slice(body)?)
    }
}

async fn pattern_gml(State(s): St, Path((ch, name)): Path<(String, String)>, body: Bytes) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let pat = body_or_saved(&store, &name, &body)?;
    let g = patterns::to_gml(&name, &pat)?;
    Ok(Json(json!({
        "object": g.object,
        "create": { "code": g.create_code, "source": g.create },
        "step": { "code": g.step_code, "source": g.step },
    })))
}

/// Saves the pattern (when a body is sent) and writes its wave object into the mod.
async fn install_pattern(State(s): St, Path((ch, name)): Path<(String, String)>, body: Bytes) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let pat = body_or_saved(&store, &name, &body)?;
    PatternStore::new(&store).write(&name, &pat)?;
    let st = s.clone();
    let out = blocking(move || {
        let _w = st.write_lock();
        patterns::install(&store, &name, &pat)
    })
    .await?;
    s.invalidate(&ch);
    Ok(Json(out))
}

#[derive(Deserialize)]
struct FlagQuery {
    q: Option<String>,
    name: Option<String>,
    index: Option<String>,
    #[serde(default)]
    offset: usize,
    limit: Option<usize>,
}

/// Global variables with read and write counts. `name` gives one variable, `q` a part of the name.
async fn list_flags(State(s): St, Path(ch): Path<String>, Query(q): Query<FlagQuery>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let st = s.clone();
    blocking(move || {
        let refs = st.flag_refs(&ch, &store);
        let needle = q.q.map(|x| x.to_lowercase());
        let all: Vec<flags::Summary> = flags::summarize(&refs)
            .into_iter()
            .filter(|x| q.name.as_ref().is_none_or(|n| &x.name == n))
            .filter(|x| needle.as_ref().is_none_or(|n| x.name.to_lowercase().contains(n)))
            .collect();
        let total = all.len();
        let items: Vec<_> = all.into_iter().skip(q.offset).take(q.limit.unwrap_or(1000)).collect();
        Ok(Json(json!({ "total": total, "offset": q.offset, "items": items })))
    })
    .await
}

/// Every read and write of one variable (`index` omitted: the plain variable).
async fn flag_refs(State(s): St, Path(ch): Path<String>, Query(q): Query<FlagQuery>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let name = q.name.ok_or_else(|| Error::BadRequest("name is required".into()))?;
    let st = s.clone();
    blocking(move || {
        let refs = st.flag_refs(&ch, &store);
        let items: Vec<&flags::Ref> = refs.iter().filter(|r| r.name == name && r.index == q.index).collect();
        Ok(Json(json!(items)))
    })
    .await
}

fn flag_notes_path(store: &ChapterStore) -> PathBuf {
    store.root.join("flag-notes.json")
}

async fn get_flag_notes(State(s): St, Path(ch): Path<String>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    Ok(Json(
        std::fs::read(flag_notes_path(&store))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| json!({})),
    ))
}

/// Body: `{"flag[12]": "met Noelle", ...}`. Kept in `<workspace>/<chapter>/flag-notes.json`.
async fn put_flag_notes(
    State(s): St,
    Path(ch): Path<String>,
    Json(notes): Json<std::collections::BTreeMap<String, String>>,
) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let notes: std::collections::BTreeMap<String, String> = notes.into_iter().filter(|(_, v)| !v.trim().is_empty()).collect();
    std::fs::write(flag_notes_path(&store), serde_json::to_vec_pretty(&notes)?)?;
    Ok(Json(json!(notes)))
}

/// Line layout of the save file, from the chapter's save script.
async fn save_layout(State(s): St, Path(ch): Path<String>) -> Result<Json<flags::SaveLayout>> {
    let store = s.extracted(&ch)?;
    blocking(move || {
        let code = store::asset_type("code")?;
        let sources = store
            .list(code)
            .into_iter()
            .filter(|e| e.name.contains("scr_saveprocess"))
            .filter_map(|e| Some((e.name.clone(), store.read(code, &e.name).ok()?["source"].as_str()?.to_owned())))
            .collect::<Vec<_>>();
        Ok(Json(flags::save_layout(sources.into_iter())))
    })
    .await
}

async fn list_saves(State(s): St) -> Result<Json<Value>> {
    let saves = s.saves()?;
    Ok(Json(json!({ "dir": saves.dir, "files": saves.list()?, "copies": saves.copies() })))
}

async fn read_save(State(s): St, Path(file): Path<String>) -> Result<Json<saves::SaveText>> {
    Ok(Json(s.saves()?.read(&file)?))
}

#[derive(Deserialize)]
struct SaveBody {
    lines: Vec<String>,
}

/// Writes the lines. The old file goes to a backup first.
async fn write_save(State(s): St, Path(file): Path<String>, Json(b): Json<SaveBody>) -> Result<Json<Value>> {
    let backup = s.saves()?.write(&file, &b.lines)?;
    Ok(Json(json!({ "backup": backup })))
}

#[derive(Deserialize)]
struct CopyBody {
    name: String,
}

async fn copy_save(State(s): St, Path(file): Path<String>, Json(b): Json<CopyBody>) -> Result<StatusCode> {
    s.saves()?.save_copy(&file, &b.name)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn load_save(State(s): St, Path(file): Path<String>, Json(b): Json<CopyBody>) -> Result<Json<Value>> {
    let backup = s.saves()?.load_copy(&file, &b.name)?;
    Ok(Json(json!({ "backup": backup })))
}

async fn delete_save_copy(State(s): St, Path(name): Path<String>) -> Result<StatusCode> {
    s.saves()?.delete_copy(&name)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn live_status(State(s): St) -> Result<Json<Value>> {
    let dir = s.live_dir()?;
    let ack = live::read_ack(&dir);
    let q = s.live.lock().unwrap();
    let acked = ack.as_ref().is_some_and(|a| a.seq == q.seq.to_string());
    Ok(Json(json!({
        "dir": dir,
        "connected": live::connected(ack.as_ref()),
        "ack": ack,
        "seq": q.seq,
        "pending": if acked { 0 } else { q.pending.len() },
    })))
}

#[derive(Deserialize)]
struct LiveBody {
    commands: Vec<live::Command>,
}

async fn live_commands(State(s): St, Json(b): Json<LiveBody>) -> Result<Json<Value>> {
    let dir = s.live_dir()?;
    let st = s.clone();
    let seq = blocking(move || st.live.lock().unwrap().send(&dir, b.commands)).await?;
    Ok(Json(json!({ "seq": seq })))
}

#[derive(Deserialize)]
struct SpriteQuery {
    frames: u32,
    #[serde(default)]
    xo: i32,
    #[serde(default)]
    yo: i32,
}

/// Body: PNG strip (frames side by side). Writes it to `dre_live/<name>.png` and sends `sprite_replace`.
async fn live_sprite(State(s): St, Path(name): Path<String>, Query(q): Query<SpriteQuery>, body: Bytes) -> Result<Json<Value>> {
    if !body.starts_with(b"\x89PNG") {
        return Err(Error::BadRequest("body must be a PNG".into()));
    }
    let file = format!("{name}.png");
    let cmd = live::Command {
        op: "sprite".into(),
        args: vec![name, file.clone(), q.frames.to_string(), q.xo.to_string(), q.yo.to_string()],
    };
    cmd.validate()?;
    let dir = s.live_dir()?;
    let st = s.clone();
    let seq = blocking(move || {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(&file), &body)?;
        st.live.lock().unwrap().send(&dir, vec![cmd])
    })
    .await?;
    Ok(Json(json!({ "seq": seq })))
}

async fn live_flags(State(s): St) -> Result<Json<Value>> {
    let (values, age) = live::read_flags(&s.live_dir()?).ok_or_else(|| Error::NotFound("no flag dump yet; send dumpflags".into()))?;
    Ok(Json(json!({ "values": values, "age_ms": age })))
}

async fn live_install_status(State(s): St, Path(ch): Path<String>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    blocking(move || Ok(Json(live::status(&store)))).await
}

#[derive(Deserialize, Default)]
struct InstallBody {
    host: Option<String>,
}

async fn live_install(State(s): St, Path(ch): Path<String>, body: Bytes) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let b: InstallBody = if body.iter().all(u8::is_ascii_whitespace) {
        InstallBody::default()
    } else {
        serde_json::from_slice(&body)?
    };
    let st = s.clone();
    let out = blocking(move || {
        let _w = st.write_lock();
        live::install(&store, b.host.as_deref())
    })
    .await?;
    s.invalidate(&ch);
    Ok(Json(out))
}

async fn live_uninstall(State(s): St, Path(ch): Path<String>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let st = s.clone();
    let out = blocking(move || {
        let _w = st.write_lock();
        live::uninstall(&store)
    })
    .await?;
    s.invalidate(&ch);
    Ok(Json(json!({ "removed": out })))
}

fn mod_json(m: &conflicts::Mod) -> Value {
    json!({ "label": m.label, "dir": m.dir, "assets": conflicts::count(m) })
}

/// Mods in `<workspace>/<chapter>/mods/` (each a UTMT project folder).
async fn list_mods(State(s): St, Path(ch): Path<String>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    blocking(move || {
        Ok(Json(json!({
            "folder": store.root.join("mods"),
            "mods": conflicts::mods_folder(&store).iter().map(mod_json).collect::<Vec<_>>(),
        })))
    })
    .await
}

#[derive(Deserialize)]
struct ConflictBody {
    /// More mod folders (UTMT projects) to check.
    #[serde(default)]
    paths: Vec<PathBuf>,
    /// Check this chapter's own mod too (default true).
    own: Option<bool>,
}

async fn check_conflicts(State(s): St, Path(ch): Path<String>, body: Bytes) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let b: ConflictBody = if body.iter().all(u8::is_ascii_whitespace) {
        ConflictBody {
            paths: Vec::new(),
            own: None,
        }
    } else {
        serde_json::from_slice(&body)?
    };
    blocking(move || {
        let mut mods = Vec::new();
        if b.own.unwrap_or(true) && store.mod_dir().join("project.json").is_file() {
            mods.push(conflicts::Mod {
                label: "this mod".into(),
                dir: store.mod_dir(),
            });
        }
        mods.extend(conflicts::mods_folder(&store));
        for p in b.paths {
            conflicts::check_mod_dir(&p)?;
            let label = p.file_name().and_then(|n| n.to_str()).unwrap_or("mod").to_owned();
            mods.push(conflicts::Mod { label, dir: p });
        }
        // Labels must differ to read the result.
        let mut seen = std::collections::HashMap::<String, usize>::new();
        for m in &mut mods {
            let n = seen.entry(m.label.clone()).or_default();
            *n += 1;
            if *n > 1 {
                m.label = format!("{} ({n})", m.label);
            }
        }
        let findings = conflicts::check(&store.base(), &mods);
        Ok(Json(json!({
            "mods": mods.iter().map(mod_json).collect::<Vec<_>>(),
            "findings": findings,
        })))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_drops_index_built_during_invalidate() {
        let cache: Mutex<HashMap<String, Arc<u32>>> = Mutex::default();
        let epoch = AtomicU64::new(0);
        // Files change while the index builds: the old index is returned once, not kept.
        let v = cached(&cache, &epoch, "c", || {
            epoch.fetch_add(1, Ordering::SeqCst);
            1
        });
        assert_eq!(*v, 1);
        assert!(cache.lock().unwrap().is_empty());
        assert_eq!(*cached(&cache, &epoch, "c", || 2), 2);
        assert_eq!(*cached(&cache, &epoch, "c", || 3), 2);
    }
}
