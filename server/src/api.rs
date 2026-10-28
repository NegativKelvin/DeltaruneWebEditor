//! HTTP API for the webpage. All routes are under `/api`.

use std::collections::HashMap;
use std::path::PathBuf;
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
use crate::cutscene;
use crate::dialogue::{self, DialogueLine};
use crate::enemies;
use crate::error::{Error, Result};
use crate::gml;
use crate::jobs::Jobs;
use crate::store::{self, ASSET_TYPES, ChapterStore};
use crate::utmt::Utmt;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Deltarune install dir (holds `data.win` and `chapterN_*/`).
    pub game_dir: Option<PathBuf>,
    /// Path to the UndertaleModCli executable.
    pub utmt_cli: Option<PathBuf>,
}

pub struct AppState {
    pub workspace: PathBuf,
    pub settings: RwLock<Settings>,
    pub jobs: Jobs,
    dialogue_cache: Mutex<HashMap<String, Arc<Vec<DialogueLine>>>>,
    cutscene_cache: Mutex<HashMap<String, Arc<CutsceneList>>>,
    enemy_cache: Mutex<HashMap<String, Arc<Vec<enemies::Record>>>>,
}

/// Code entries with `c_*` steps: (name, step count).
type CutsceneList = Vec<(String, usize)>;

/// Gets a per-chapter index from a cache, or builds it.
fn cached<T>(cache: &Mutex<HashMap<String, Arc<T>>>, chapter: &str, build: impl FnOnce() -> T) -> Arc<T> {
    if let Some(c) = cache.lock().unwrap().get(chapter) {
        return c.clone();
    }
    let v = Arc::new(build());
    cache.lock().unwrap().insert(chapter.to_owned(), v.clone());
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
        })
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
        self.dialogue_cache.lock().unwrap().remove(chapter);
        self.cutscene_cache.lock().unwrap().remove(chapter);
        self.enemy_cache.lock().unwrap().remove(chapter);
    }

    fn dialogue(&self, chapter: &str, store: &ChapterStore) -> Arc<Vec<DialogueLine>> {
        cached(&self.dialogue_cache, chapter, || {
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
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
        .with_state(state)
}

async fn get_settings(State(s): St) -> Json<Settings> {
    Json(s.settings.read().unwrap().clone())
}

async fn put_settings(State(s): St, Json(new): Json<Settings>) -> Result<Json<Settings>> {
    if let Some(d) = &new.game_dir
        && !d.is_dir()
    {
        return Err(Error::BadRequest(format!("game_dir '{}' is not a directory", d.display())));
    }
    if let Some(c) = &new.utmt_cli
        && !c.is_file()
    {
        return Err(Error::BadRequest(format!("utmt_cli '{}' is not a file", c.display())));
    }
    let merged = {
        let mut cur = s.settings.write().unwrap();
        if new.game_dir.is_some() {
            cur.game_dir = new.game_dir;
        }
        if new.utmt_cli.is_some() {
            cur.utmt_cli = new.utmt_cli;
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
    let out = store.write(ty, &name, body.data, body.source)?;
    if ty.slug == "code" {
        s.invalidate(&ch);
    }
    Ok(Json(out))
}

async fn revert_asset(State(s): St, Path((ch, ty, name)): Path<(String, String, String)>) -> Result<StatusCode> {
    let store = s.extracted(&ch)?;
    let ty = store::asset_type(&ty)?;
    store.revert(ty, &name)?;
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
    store.write_file(ty, &name, &file, &body)?;
    if ty.slug == "code" {
        s.invalidate(&ch);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_file(State(s): St, Path((ch, ty, name, file)): Path<(String, String, String, String)>) -> Result<StatusCode> {
    let store = s.extracted(&ch)?;
    let ty = store::asset_type(&ty)?;
    store.delete_file(ty, &name, &file)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PatchCode {
    rev: String,
    edits: Vec<gml::Edit>,
}

async fn patch_code(State(s): St, Path((ch, name)): Path<(String, String)>, Json(body): Json<PatchCode>) -> Result<Json<Value>> {
    let store = s.extracted(&ch)?;
    let out = store.patch_code(&name, &body.rev, &body.edits)?;
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
        let all = cached(&st.cutscene_cache, &ch, || {
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
        let recs = cached(&st.enemy_cache, &ch, || {
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
        let langs = dialogue::load_lang(&chapter.dir);
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
