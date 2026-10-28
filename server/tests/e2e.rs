//! End-to-end test against a real UndertaleModCli build.
//! Runs only when `DRE_TEST_UTMT` is the path of UndertaleModCli (0.9.2.0+); skipped otherwise.
//!
//! Flow: make fixture data file -> extract -> read assets and dialogue -> edit room and code
//! -> build -> extract the built file again and check that the edits are in it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dre_server::api::{AppState, Settings};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn utmt() -> Option<PathBuf> {
    std::env::var_os("DRE_TEST_UTMT").map(PathBuf::from)
}

fn run_cli(cli: &Path, cwd: &Path, args: &[&str], env: &[(&str, &Path)]) {
    let mut cmd = std::process::Command::new(cli);
    cmd.args(args).current_dir(cwd).stdin(std::process::Stdio::null());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

async fn call(app: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let req = Request::builder().method(method).uri(uri).header("host", "localhost");
    let req = match body {
        Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn wait_job(app: &axum::Router, job: u64) -> Value {
    for _ in 0..300 {
        let (_, j) = call(app, "GET", &format!("/api/jobs/{job}"), None).await;
        if j["status"] != "running" {
            return j;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("job {job} timed out");
}

#[tokio::test]
async fn extract_edit_build_roundtrip() {
    let Some(cli) = utmt() else {
        eprintln!("skipped: set DRE_TEST_UTMT");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("game");
    let ch1 = game.join("chapter1_windows");
    std::fs::create_dir_all(ch1.join("lang")).unwrap();
    std::fs::write(ch1.join("lang/lang_ja.json"), r#"{"obj_talk_slash_Create_0_gml_2_0":"* ねえ。/%"}"#).unwrap();

    // Fixture data file.
    let fixture_script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/make_fixture.csx");
    run_cli(&cli, tmp.path(), &["new", "-o", "blank.win"], &[]);
    run_cli(
        &cli,
        tmp.path(),
        &["load", "blank.win", "-s", fixture_script.to_str().unwrap(), "-o", "chapter1.win"],
        &[],
    );
    std::fs::copy(tmp.path().join("chapter1.win"), ch1.join("data.win")).unwrap();

    let state = AppState::new(
        tmp.path().join("ws"),
        Settings {
            game_dir: Some(game),
            utmt_cli: Some(cli.clone()),
            save_dir: None,
        },
    );
    let app = dre_server::api::router(state);

    let (_, chapters) = call(&app, "GET", "/api/chapters", None).await;
    assert_eq!(chapters[0]["id"], "chapter1");

    let (st, j) = call(&app, "POST", "/api/chapters/chapter1/extract", None).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    let job = wait_job(&app, j["job"].as_u64().unwrap()).await;
    assert_eq!(job["status"], "succeeded", "{job:#}");

    let (_, objects) = call(&app, "GET", "/api/chapters/chapter1/assets/objects", None).await;
    assert!(
        objects["items"].as_array().unwrap().iter().any(|o| o["name"] == "obj_talk"),
        "{objects:#}"
    );
    let (_, dlg) = call(&app, "GET", "/api/chapters/chapter1/dialogue", None).await;
    assert_eq!(dlg["items"][0]["text"], "* Hey^1, Kris./%");
    assert_eq!(dlg["items"][0]["speaker"], "susie");
    assert_eq!(dlg["items"][0]["translations"]["ja"], "* ねえ。/%");
    let (_, gen_rooms) = call(&app, "GET", "/api/chapters/chapter1/generic/Rooms", None).await;
    assert_eq!(gen_rooms["total"], 1);

    // Edit: move the instance, change the code.
    let (_, room) = call(&app, "GET", "/api/chapters/chapter1/assets/rooms/room0", None).await;
    let mut data = room["data"].clone();
    data["GameObjects"][0]["X"] = json!(200);
    let (st, _) = call(
        &app,
        "PUT",
        "/api/chapters/chapter1/assets/rooms/room0",
        Some(json!({ "data": data })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let src = "scr_speaker(\"susie\");\nmsgsetloc(0, \"* Modded./%\", \"obj_talk_slash_Create_0_gml_2_0\");\nx = 99;\n";
    let (st, _) = call(
        &app,
        "PUT",
        "/api/chapters/chapter1/assets/code/gml_Object_obj_talk_Create_0",
        Some(json!({ "source": src })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Bullet pattern: save, install the wave object (the build compiles its GML).
    let pattern = json!({
        "duration": 90,
        "box_code": true,
        "box_keys": [{ "frame": 30, "x": 320, "y": 170, "w": 300, "h": 100 }],
        "emitters": [
            { "count": 6, "spread": 360, "aim": true, "gravity": 0.2, "jitter_x": 4, "jitter_angle": 3, "speed_step": 0.5,
              "rotate": true, "scale": 2, "damage": 3, "life": 60, "on_spawn": "image_alpha = 0.5;" },
            { "origin": "soul", "count": 1 },
        ],
    });
    let (st, _) = call(&app, "PUT", "/api/chapters/chapter1/patterns/t1", Some(pattern)).await;
    assert_eq!(st, StatusCode::OK);
    let (st, inst) = call(&app, "POST", "/api/chapters/chapter1/patterns/t1/install", None).await;
    assert_eq!(st, StatusCode::OK, "{inst:#}");
    assert_eq!(inst["object"], "obj_dre_wave_t1");

    // Hot reload object (the build compiles its GML) with the talk object as host.
    let (st, live) = call(
        &app,
        "POST",
        "/api/chapters/chapter1/live",
        Some(json!({ "host": "gml_Object_obj_talk_Create_0" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{live:#}");

    let (_, j) = call(&app, "POST", "/api/chapters/chapter1/build", None).await;
    let job = wait_job(&app, j["job"].as_u64().unwrap()).await;
    assert_eq!(job["status"], "succeeded", "{job:#}");

    // Check the built file.
    let built = tmp.path().join("ws/chapter1/build/data.win");
    let check = tmp.path().join("check");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("utmt-scripts/export_all.csx");
    run_cli(
        &cli,
        tmp.path(),
        &["load", built.to_str().unwrap(), "-s", script.to_str().unwrap()],
        &[("DRE_PROJECT_DIR", &check.join("p")), ("DRE_GENERIC_DIR", &check.join("g"))],
    );
    let gml = std::fs::read_to_string(check.join("p/code/gml_Object_obj_talk_Create_0.gml")).unwrap();
    assert!(gml.contains("* Modded./%") && gml.contains("x = 99"), "{gml}");
    let room: Value = serde_json::from_slice(&std::fs::read(check.join("p/rooms/room0.json")).unwrap()).unwrap();
    assert_eq!(room["GameObjects"][0]["X"], 200);
    let wave: Value = serde_json::from_slice(&std::fs::read(check.join("p/objects/obj_dre_wave_t1.json")).unwrap()).unwrap();
    assert_eq!(wave["Events"].as_array().unwrap().len(), 2, "{wave:#}");
    let step = std::fs::read_to_string(check.join("p/code/gml_Object_obj_dre_wave_t1_Step_0.gml")).unwrap();
    assert!(step.contains("instance_create") && step.contains("dre_t"), "{step}");
    let live: Value = serde_json::from_slice(&std::fs::read(check.join("p/objects/obj_dre_live.json")).unwrap()).unwrap();
    assert_eq!(live["Persistent"], true, "{live:#}");
    let live_step = std::fs::read_to_string(check.join("p/code/gml_Object_obj_dre_live_Step_0.gml")).unwrap();
    assert!(
        live_step.contains("sprite_replace") && live_step.contains("dre_live_seq"),
        "{live_step}"
    );
    assert!(gml.contains("obj_dre_live"), "{gml}");
}
