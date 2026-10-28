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
