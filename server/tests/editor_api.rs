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
async fn flags_saves_live_conflicts() {
    let (t, app) = setup();
    let ch = "/api/chapters/chapter1";
    // Flag browser: the cutscene code reads and writes flags.
    let src = "if (global.flag[7] == 0)\n    global.flag[7] = 1;\nglobal.plot = 40;\n";
    call(
        &app,
        "PUT",
        &format!("{ch}/assets/code/gml_Object_obj_cs_Step_0"),
        Some(json!({ "source": src })),
    )
    .await;
    let (_, f) = call(&app, "GET", &format!("{ch}/flags?name=flag"), None).await;
    assert_eq!(
        f["items"][0],
        json!({"name": "flag", "index": "7", "reads": 1, "writes": 1, "values": ["0", "1"]})
    );
    let (_, r) = call(&app, "GET", &format!("{ch}/flags/refs?name=plot"), None).await;
    assert_eq!(r[0]["detail"], "= 40");
    assert_eq!(r[0]["write"], true);
    let (_, n) = call(
        &app,
        "PUT",
        &format!("{ch}/flag-notes"),
        Some(json!({ "flag[7]": "door open", "x": " " })),
    )
    .await;
    assert_eq!(n, json!({ "flag[7]": "door open" }));

    // Save layout from the save script, save file edit with backup.
    let save_src = "file_text_write_string(f, global.truename);\nfile_text_writeln(f);\nfor (i = 0; i < 2; i++)\n{\n    file_text_write_real(f, global.flag[i]);\n    file_text_writeln(f);\n}\n";
    call(
        &app,
        "PUT",
        &format!("{ch}/assets/code/gml_GlobalScript_scr_saveprocess"),
        Some(json!({ "source": save_src })),
    )
    .await;
    let (_, l) = call(&app, "GET", &format!("{ch}/save-layout"), None).await;
    assert_eq!(l["fields"][2]["label"], "global.flag[1]", "{l}");
    let (st, _) = call(&app, "GET", "/api/saves", None).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let save = t.path().join("save");
    std::fs::create_dir_all(&save).unwrap();
    std::fs::write(save.join("filech1_0"), "Kris\r\n0\r\n0\r\n").unwrap();
    let (st, _) = call(&app, "PUT", "/api/settings", Some(json!({ "save_dir": save }))).await;
    assert_eq!(st, StatusCode::OK);
    let (_, list) = call(&app, "GET", "/api/saves", None).await;
    assert_eq!(list["files"][0]["chapter"], 1);
    let (_, s) = call(&app, "GET", "/api/saves/filech1_0", None).await;
    assert_eq!(s["lines"], json!(["Kris", "0", "0"]));
    let (st, b) = call(&app, "PUT", "/api/saves/filech1_0", Some(json!({ "lines": ["Kris", "1", "0"] }))).await;
    assert_eq!(st, StatusCode::OK);
    assert!(b["backup"].as_str().unwrap().starts_with("backup-filech1_0-"));
    assert_eq!(std::fs::read_to_string(save.join("filech1_0")).unwrap(), "Kris\r\n1\r\n0\r\n");
    let (st, _) = call(&app, "GET", "/api/saves/settings.json", None).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    call(&app, "POST", "/api/saves/filech1_0/copy", Some(json!({ "name": "plot40" }))).await;
    let (st, _) = call(&app, "POST", "/api/saves/filech1_2/load", Some(json!({ "name": "plot40" }))).await;
    assert_eq!(st, StatusCode::OK);
    assert!(save.join("filech1_2").is_file());

    // Hot reload: install, commands, status.
    std::fs::write(
        t.path().join("ws/chapter1/base/code/gml_Object_obj_time_Step_0.gml"),
        "global.time += 1;\n",
    )
    .unwrap();
    let (st, i) = call(&app, "POST", &format!("{ch}/live"), None).await;
    assert_eq!(st, StatusCode::OK, "{i}");
    let (_, s) = call(&app, "GET", &format!("{ch}/live"), None).await;
    assert_eq!(s["installed"], true);
    let cmds = json!({ "commands": [{ "op": "flag", "args": ["7", "1"] }, { "op": "goto", "args": ["room_town"] }] });
    let (st, _) = call(&app, "POST", "/api/live/commands", Some(cmds)).await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        std::fs::read_to_string(save.join("dre_live/cmd.txt"))
            .unwrap()
            .contains("goto\nroom_town\n")
    );
    let (_, s) = call(&app, "GET", "/api/live", None).await;
    assert_eq!((s["connected"].clone(), s["pending"].clone()), (json!(false), json!(2)));
    let (st, _) = call(
        &app,
        "POST",
        "/api/live/commands",
        Some(json!({ "commands": [{ "op": "flag", "args": ["7", "x"] }] })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, u) = call(&app, "DELETE", &format!("{ch}/live"), None).await;
    assert_eq!(st, StatusCode::OK, "{u}");

    // Conflicts: this mod and a mod in mods/ change the same line of the save script.
    let other = t.path().join("ws/chapter1/mods/other");
    std::fs::create_dir_all(other.join("code")).unwrap();
    std::fs::write(other.join("project.json"), "{}").unwrap();
    std::fs::write(
        other.join("code/gml_GlobalScript_scr_saveprocess.gml"),
        save_src.replace("i < 2", "i < 3"),
    )
    .unwrap();
    let (_, m) = call(&app, "GET", &format!("{ch}/mods"), None).await;
    assert_eq!(m["mods"][0]["label"], "other");
    let (st, c) = call(&app, "POST", &format!("{ch}/conflicts"), None).await;
    assert_eq!(st, StatusCode::OK, "{c}");
    let f = c["findings"].as_array().unwrap();
    assert_eq!(f.len(), 1, "{c}");
    assert_eq!(f[0]["name"], "gml_GlobalScript_scr_saveprocess");
    assert_eq!(f[0]["kind"], "conflict");
    let (st, _) = call(
        &app,
        "POST",
        &format!("{ch}/conflicts"),
        Some(json!({ "paths": [t.path().join("game")] })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
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

#[tokio::test]
async fn cutscene_put_checks_and_adds_sections() {
    let (_t, app) = setup();
    let uri = "/api/chapters/chapter1/cutscenes/gml_Object_obj_cs_Step_0";
    let (_, cs) = call(&app, "GET", uri, None).await;
    assert_eq!(cs["sections"][0]["con"], "1");
    let b = &cs["blocks"][0];
    let bad = json!({ "rev": cs["rev"], "blocks": [{ "start": b["start"], "end": b["end"], "indent": "    ", "steps": [{ "func": "c_wait", "args": ["1); x = (2"] }] }] });
    let (st, _) = call(&app, "PUT", uri, Some(bad)).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let good = json!({
        "rev": cs["rev"],
        "blocks": [{ "start": b["start"], "end": b["end"], "indent": "    ", "steps": [{ "func": "c_sel", "args": ["su"] }, { "func": "c_wait", "args": ["2"] }] }],
        "sections": [{ "con": 2, "next": 3, "steps": [{ "func": "c_wait", "args": ["9"] }] }],
    });
    let (st, out) = call(&app, "PUT", uri, Some(good.clone())).await;
    assert_eq!(st, StatusCode::OK, "{out}");
    assert_eq!(
        out["source"],
        "if (con == 1)\n{\n    c_sel(su);\n    c_wait(2);\n}\nif (con == 2)\n{\n    con = 3;\n    c_wait(9);\n}\n"
    );
    assert_eq!(out["sections"][1]["next"], json!(["3"]));
    let (st, _) = call(&app, "PUT", uri, Some(good)).await;
    assert_eq!(st, StatusCode::CONFLICT);
}

#[tokio::test]
async fn wizards_plan_apply() {
    let (_t, app) = setup();
    let base = "/api/chapters/chapter1/wizards";
    let npc = json!({ "object": "obj_npc_w", "sprite": "spr_a", "lines": ["* Hi."] });
    let (st, plan) = call(&app, "POST", &format!("{base}/npc?dry_run=true"), Some(npc.clone())).await;
    assert_eq!(st, StatusCode::OK, "{plan}");
    assert_eq!(plan["applied"], false);
    assert_eq!(plan["writes"].as_array().unwrap().len(), 3);
    let (st, _) = call(&app, "GET", "/api/chapters/chapter1/assets/objects/obj_npc_w", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, plan) = call(&app, "POST", &format!("{base}/npc"), Some(npc.clone())).await;
    assert_eq!(st, StatusCode::OK, "{plan}");
    let (_, obj) = call(&app, "GET", "/api/chapters/chapter1/assets/objects/obj_npc_w", None).await;
    assert_eq!(obj["data"]["Sprite"], "spr_a");
    // The dialogue index sees the new text (cache cleared).
    let (_, dlg) = call(
        &app,
        "GET",
        "/api/chapters/chapter1/dialogue?code=gml_Object_obj_npc_w_Step_0",
        None,
    )
    .await;
    assert_eq!(dlg["items"][0]["text"], "* Hi./%");
    let (st, _) = call(&app, "POST", &format!("{base}/npc"), Some(npc)).await;
    assert_eq!(st, StatusCode::CONFLICT);
    // Enemy: a new case in the stats switch.
    let (st, out) = call(
        &app,
        "POST",
        &format!("{base}/enemy"),
        Some(json!({ "object": "obj_e_w", "parent": "", "name": "W", "hp": 7 })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{out}");
    let (_, en) = call(&app, "GET", "/api/chapters/chapter1/enemies", None).await;
    let rec = en.as_array().unwrap().iter().find(|r| r["case"] == "2").unwrap();
    assert_eq!(rec["fields"]["monstermaxhp"]["expr"], "7");
    let (st, _) = call(&app, "POST", &format!("{base}/nothing"), Some(json!({}))).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = call(&app, "POST", &format!("{base}/npc"), Some(json!({ "lines": [] }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn text_check_and_replace() {
    let (_t, app) = setup();
    let (st, r) = call(&app, "GET", "/api/chapters/chapter1/text/check?chars=4&face_chars=4", None).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["total"], 1);
    assert_eq!(r["items"][0]["problems"][0]["kind"], "width");
    let (_, r) = call(&app, "GET", "/api/chapters/chapter1/text/check", None).await;
    assert_eq!(r["total"], 0);
    let rep = json!({ "find": "hi", "replace": "Yo", "ignore_case": true, "dry_run": true });
    let (_, r) = call(&app, "POST", "/api/chapters/chapter1/text/replace", Some(rep)).await;
    assert_eq!(r["changes"][0]["after"], "* Yo./%");
    let (_, dlg) = call(&app, "GET", "/api/chapters/chapter1/dialogue", None).await;
    assert_eq!(dlg["items"][0]["text"], "* Hi./%");
    let rep = json!({ "find": "hi", "replace": "Yo", "ignore_case": true });
    let (st, r) = call(&app, "POST", "/api/chapters/chapter1/text/replace", Some(rep)).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["changes"].as_array().unwrap().len(), 1);
    let (_, dlg) = call(&app, "GET", "/api/chapters/chapter1/dialogue", None).await;
    assert_eq!(dlg["items"][0]["text"], "* Yo./%");
    let (st, _) = call(
        &app,
        "POST",
        "/api/chapters/chapter1/text/replace",
        Some(json!({ "find": "(", "regex": true })),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}
