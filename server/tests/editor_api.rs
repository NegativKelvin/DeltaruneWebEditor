//! API tests for the endpoints that the webpage editors use. No UndertaleModCli needed:
//! the test makes an "extracted" workspace by hand.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use dre_server::api::{AppState, Settings};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

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

const CUTSCENE: &str = "if (con == 1)\n{\n    c_sel(kr);\n    c_wait(30);\n    c_msgsetloc(0, \"* Hi./%\", \"k_1\");\n}\n";
const MONSTERS: &str = "switch (global.monstertype[myself])\n{\n    case 1:\n        global.monstermaxhp[myself] = 90;\n        global.monsterat[myself] = 6;\n        break;\n}\n";

fn setup() -> (tempfile::TempDir, axum::Router) {
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("game");
    std::fs::create_dir_all(game.join("chapter1_windows")).unwrap();
    std::fs::write(game.join("chapter1_windows/data.win"), b"FORM").unwrap();
    let base = tmp.path().join("ws/chapter1/base");
    for d in ["code", "objects", "sprites/spr_a"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    std::fs::write(base.join("project.json"), r#"{"Name":"DELTARUNE"}"#).unwrap();
    std::fs::write(base.join("code/gml_Object_obj_cs_Step_0.gml"), CUTSCENE).unwrap();
    std::fs::write(base.join("code/gml_GlobalScript_scr_monstersetup.gml"), MONSTERS).unwrap();
    std::fs::write(
        base.join("objects/obj_cs.json"),
        r#"{"$type":"GameObject","Sprite":"spr_a","Visible":false}"#,
    )
    .unwrap();
    std::fs::write(
        base.join("sprites/spr_a/spr_a.json"),
        r#"{"$type":"Sprite","Width":4,"TextureCount":2}"#,
    )
    .unwrap();
    std::fs::write(base.join("sprites/spr_a/spr_a_0.png"), b"0").unwrap();
    std::fs::write(base.join("sprites/spr_a/spr_a_1.png"), b"1").unwrap();
    let state = AppState::new(
        tmp.path().join("ws"),
        Settings {
            game_dir: Some(game),
            utmt_cli: None,
            ..Default::default()
        },
    );
    (tmp, dre_server::api::router(state))
}

#[tokio::test]
async fn cutscene_blocks_and_patch() {
    let (_t, app) = setup();
    let (_, list) = call(&app, "GET", "/api/chapters/chapter1/cutscenes", None).await;
    assert_eq!(list["items"][0], json!({"code": "gml_Object_obj_cs_Step_0", "steps": 3}));
    let (_, cs) = call(&app, "GET", "/api/chapters/chapter1/cutscenes/gml_Object_obj_cs_Step_0", None).await;
    let block = &cs["blocks"][0];
    assert_eq!(block["context"], "if (con == 1)");
    assert_eq!(block["steps"][1]["args"], json!(["30"]));

    let edit =
        json!({ "rev": cs["rev"], "edits": [{ "start": block["start"], "end": block["end"], "text": "c_wait(5);\n    c_sel(su);" }] });
    let (st, out) = call(
        &app,
        "POST",
        "/api/chapters/chapter1/code/gml_Object_obj_cs_Step_0/patch",
        Some(edit.clone()),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{out}");
    assert_eq!(out["source"], "if (con == 1)\n{\n    c_wait(5);\n    c_sel(su);\n}\n");
    // Same revision again: the source changed, so the server refuses.
    let (st, _) = call(
        &app,
        "POST",
        "/api/chapters/chapter1/code/gml_Object_obj_cs_Step_0/patch",
        Some(edit),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);
    // Indexes see the change (cache is cleared).
    let (_, list) = call(&app, "GET", "/api/chapters/chapter1/cutscenes", None).await;
    assert_eq!(list["items"][0]["steps"], 2);
    let (_, dlg) = call(&app, "GET", "/api/chapters/chapter1/dialogue", None).await;
    assert_eq!(dlg["total"], 0);
}

#[tokio::test]
async fn dialogue_span_patch() {
    let (_t, app) = setup();
    let (_, dlg) = call(&app, "GET", "/api/chapters/chapter1/dialogue", None).await;
    let line = &dlg["items"][0];
    assert_eq!(line["text"], "* Hi./%");
    let edit = json!({ "rev": line["rev"], "edits": [{ "start": line["span"][0], "end": line["span"][1], "string": "* \"New\"./%" }] });
    let (st, out) = call(
        &app,
        "POST",
        "/api/chapters/chapter1/code/gml_Object_obj_cs_Step_0/patch",
        Some(edit),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        out["source"]
            .as_str()
            .unwrap()
            .contains(r#"c_msgsetloc(0, "* \"New\"./%", "k_1");"#)
    );
    let (_, dlg) = call(&app, "GET", "/api/chapters/chapter1/dialogue", None).await;
    assert_eq!(dlg["items"][0]["text"], "* \"New\"./%");
}

#[tokio::test]
async fn enemies_index_and_files() {
    let (_t, app) = setup();
    let (_, en) = call(&app, "GET", "/api/chapters/chapter1/enemies", None).await;
    let rec = &en[0];
    assert_eq!(rec["case"], "1");
    assert_eq!(rec["fields"]["monstermaxhp"]["expr"], "90");
    let f = &rec["fields"]["monstermaxhp"];
    let edit = json!({ "rev": rec["rev"], "edits": [{ "start": f["start"], "end": f["end"], "text": "120" }] });
    let (st, _) = call(
        &app,
        "POST",
        "/api/chapters/chapter1/code/gml_GlobalScript_scr_monstersetup/patch",
        Some(edit),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, en) = call(&app, "GET", "/api/chapters/chapter1/enemies", None).await;
    assert_eq!(en[0]["fields"]["monstermaxhp"]["expr"], "120");

    let (_, idx) = call(&app, "GET", "/api/chapters/chapter1/index/objects?fields=Sprite,Visible", None).await;
    assert_eq!(idx, json!({"obj_cs": {"Sprite": "spr_a", "Visible": false}}));

    let (st, _) = call(
        &app,
        "DELETE",
        "/api/chapters/chapter1/assets/sprites/spr_a/files/spr_a_1.png",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (_, spr) = call(&app, "GET", "/api/chapters/chapter1/assets/sprites/spr_a", None).await;
    assert_eq!(spr["files"], json!(["spr_a_0.png"]));
    assert_eq!(spr["modified"], true);
}

#[tokio::test]
async fn bullet_patterns() {
    let (_t, app) = setup();
    let base = "/api/chapters/chapter1/patterns";
    let (_, list) = call(&app, "GET", base, None).await;
    assert_eq!(list, json!([]));
    let (st, _) = call(&app, "PUT", &format!("{base}/bad%2Fname"), Some(json!({}))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = call(&app, "PUT", &format!("{base}/w1"), Some(json!({ "emitters": [{ "interval": 0 }] }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, p) = call(
        &app,
        "PUT",
        &format!("{base}/w1"),
        Some(json!({ "duration": 50, "emitters": [{ "count": 2 }] })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(p["emitters"][0]["interval"], 20);
    let (_, list) = call(&app, "GET", base, None).await;
    assert_eq!(list[0]["name"], "w1");
    // GML of an unsaved body, and of the saved pattern.
    let (st, g) = call(&app, "POST", &format!("{base}/w1/gml"), Some(json!({ "duration": 7 }))).await;
    assert_eq!(st, StatusCode::OK);
    assert!(g["step"]["source"].as_str().unwrap().contains("if (dre_t > 7 && "));
    let (_, g) = call(&app, "POST", &format!("{base}/w1/gml"), None).await;
    assert!(g["step"]["source"].as_str().unwrap().contains("if (dre_t > 50 && "));
    let (st, r) = call(&app, "POST", &format!("{base}/w1/install"), None).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let (_, obj) = call(&app, "GET", "/api/chapters/chapter1/assets/objects/obj_dre_wave_w1", None).await;
    assert_eq!(obj["data"]["Events"][1]["CodeEntry"], "gml_Object_obj_dre_wave_w1_Step_0");
    let (_, code) = call(
        &app,
        "GET",
        "/api/chapters/chapter1/assets/code/gml_Object_obj_dre_wave_w1_Step_0",
        None,
    )
    .await;
    assert!(code["source"].as_str().unwrap().contains("instance_create"));
    let (st, _) = call(&app, "DELETE", &format!("{base}/w1"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _) = call(&app, "GET", &format!("{base}/w1"), None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // Plain delete keeps the wave object; delete with uninstall removes it.
    let obj_uri = "/api/chapters/chapter1/assets/objects/obj_dre_wave_w1";
    let (st, _) = call(&app, "GET", obj_uri, None).await;
    assert_eq!(st, StatusCode::OK);
    call(&app, "POST", &format!("{base}/w2/install"), Some(json!({}))).await;
    let (st, _) = call(&app, "DELETE", &format!("{base}/w2?uninstall=true"), None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, _) = call(&app, "GET", "/api/chapters/chapter1/assets/objects/obj_dre_wave_w2", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn utmt_cli_not_in_workspace() {
    let (t, app) = setup();
    // A file sent through the API lands in the workspace; it must not become the CLI.
    let (st, _) = call(&app, "PUT", "/api/chapters/chapter1/assets/sprites/spr_a/files/spr_a_0.png", None).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let inside = t.path().join("ws/chapter1/mod/sprites/spr_a/spr_a_0.png");
    let (st, _) = call(&app, "PUT", "/api/settings", Some(json!({ "utmt_cli": inside }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let outside = t.path().join("game/chapter1_windows/data.win");
    let (st, s) = call(&app, "PUT", "/api/settings", Some(json!({ "utmt_cli": outside }))).await;
    assert_eq!(st, StatusCode::OK, "{s}");
    let (st, _) = call(&app, "POST", "/api/jobs/99/cancel", None).await;
    assert_eq!(st, StatusCode::CONFLICT);
}

#[tokio::test]
async fn bad_put_changes_nothing() {
    let (_t, app) = setup();
    let uri = "/api/chapters/chapter1/assets/objects/obj_cs";
    let (st, _) = call(&app, "PUT", uri, Some(json!({ "data": { "Visible": true }, "source": "x" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (_, obj) = call(&app, "GET", uri, None).await;
    assert_eq!(obj["modified"], false);
    assert_eq!(obj["data"]["Visible"], false);
}

#[tokio::test]
async fn export_lang_and_version_check() {
    let (tmp, app) = setup();
    let (st, _) = call(
        &app,
        "PUT",
        "/api/chapters/chapter1/assets/objects/obj_cs",
        Some(json!({ "data": { "Sprite": "spr_a", "Visible": true } })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, vc) = call(&app, "GET", "/api/chapters/chapter1/version-check", None).await;
    assert_eq!(vc["items"], json!([{ "type": "objects", "name": "obj_cs", "status": "same" }]));

    let (st, l) = call(
        &app,
        "PUT",
        "/api/chapters/chapter1/lang/lang_ja.json",
        Some(json!({ "k_1": "* やあ。/%" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{l}");
    let (_, li) = call(&app, "GET", "/api/chapters/chapter1/lang", None).await;
    assert_eq!(li["changed"], json!({ "lang_ja.json": 1 }));
    let (st, _) = call(&app, "PUT", "/api/chapters/chapter1/lang/x.txt", Some(json!({ "a": "b" }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    let (st, ex) = call(&app, "POST", "/api/export", Some(json!({ "id": "t", "name": "Test" }))).await;
    assert_eq!(st, StatusCode::OK, "{ex}");
    assert_eq!(ex["manifest"]["chapters"]["chapter1"]["lang"], json!(["lang_ja.json"]));
    let (_, list) = call(&app, "GET", "/api/exports", None).await;
    assert_eq!(list[0]["file"], "t.dremod");
    let (st, _) = call(&app, "GET", "/api/exports/..%2Fsettings.json", None).await;
    assert_ne!(st, StatusCode::OK);
    let (st, _) = call(&app, "POST", "/api/export", Some(json!({ "id": "t", "chapters": ["chapter2"] }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Game update: the base object changes after the edit.
    std::fs::write(
        tmp.path().join("ws/chapter1/base/objects/obj_cs.json"),
        r#"{"$type":"GameObject","Sprite":"spr_b"}"#,
    )
    .unwrap();
    let (_, vc) = call(&app, "GET", "/api/chapters/chapter1/version-check", None).await;
    assert_eq!(vc["items"][0]["status"], "changed");
    let (st, _) = call(
        &app,
        "POST",
        "/api/chapters/chapter1/version-check/accept",
        Some(json!({ "type": "objects", "name": "obj_cs" })),
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (_, vc) = call(&app, "GET", "/api/chapters/chapter1/version-check", None).await;
    assert_eq!(vc["items"][0]["status"], "same");

    let (st, status) = call(&app, "GET", "/api/loader", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(status["chapters"][0]["state"], "original");
    let (st, _) = call(&app, "PUT", "/api/loader/mods", Some(json!([{ "id": "nope", "enabled": true }]))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}
