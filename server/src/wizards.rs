//! Templates and wizards: make all assets and GML for a new NPC, shop, enemy, room, or cutscene trigger.
//!
//! A wizard first makes a plan (every asset it writes, with its JSON or GML), so the webpage can show it
//! (`dry_run`). Apply writes the plan into `mod/`. A new asset must not exist yet. When a write fails,
//! the assets that were already written go back to their old state.
//!
//! The GML uses Deltarune names: `global.interact`, `button1_p()`, `msgsetloc`/`msgnextloc`, `scr_speaker`,
//! `obj_dialoguer`, `obj_mainchara`, `global.choicemsg`/`global.choice`, `scr_itemget`, `global.gold`,
//! `scr_cutscene_make`, `scr_maincharacters_actors`, `obj_cutscene_master`, `global.flag`, `global.entrance`,
//! `global.mnfight`, `global.turntimer`, `obj_monsterparent`, and the `global.monster*` setup arrays.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cutscene::{self, StepIn};
use crate::enemies;
use crate::error::{Error, Result};
use crate::gml::{self, Tok};
use crate::patterns;
use crate::store::{self, ChapterStore};

pub const KINDS: &[&str] = &["npc", "shop", "enemy", "room", "cutscene"];

/// One asset that a wizard writes.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlannedWrite {
    #[serde(rename = "type")]
    pub ty: &'static str,
    pub name: String,
    /// The asset is new (it must not exist). Else an existing asset changes.
    pub new: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Plan {
    pub writes: Vec<PlannedWrite>,
    /// What the user must know or do (for example: build the chapter, start the wave).
    pub notes: Vec<String>,
}

impl Plan {
    fn code(&mut self, name: impl Into<String>, source: String) {
        self.writes.push(PlannedWrite {
            ty: "code",
            name: name.into(),
            new: true,
            data: None,
            source: Some(source),
        });
    }
    fn asset(&mut self, ty: &'static str, name: impl Into<String>, new: bool, data: Value) {
        self.writes.push(PlannedWrite {
            ty,
            name: name.into(),
            new,
            data: Some(data),
            source: None,
        });
    }
    fn planned(&self, ty: &str, name: &str) -> bool {
        self.writes.iter().any(|w| w.ty == ty && w.name == name)
    }
}

/// Where to put an instance of the new object.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Place {
    /// Room name. Empty: do not place.
    pub room: String,
    pub x: i64,
    pub y: i64,
}

fn d_range() -> f64 {
    24.0
}
fn d_parent_enemy() -> String {
    "obj_monsterparent".into()
}
fn d_box() -> f64 {
    40.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct NpcParams {
    /// Object name, for example `obj_npc_guard`.
    pub object: String,
    #[serde(default)]
    pub sprite: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub solid: bool,
    /// `scr_speaker` name (face), for example `susie`. Empty: no speaker call.
    #[serde(default)]
    pub speaker: String,
    /// One text box per item. `/` and `%` are added at the end when missing.
    pub lines: Vec<String>,
    /// Talk distance in pixels.
    #[serde(default = "d_range")]
    pub range: f64,
    #[serde(default)]
    pub place: Place,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShopItem {
    /// Text of the choice, for example `Dark Candy`.
    pub name: String,
    /// Item number for `scr_itemget`.
    pub item: u32,
    pub price: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShopParams {
    pub object: String,
    #[serde(default)]
    pub sprite: Option<String>,
    #[serde(default)]
    pub speaker: String,
    /// 1 to 3 items. The last choice is "Leave".
    pub items: Vec<ShopItem>,
    #[serde(default)]
    pub greeting: Option<String>,
    #[serde(default)]
    pub bought: Option<String>,
    #[serde(default)]
    pub poor: Option<String>,
    #[serde(default)]
    pub full: Option<String>,
    #[serde(default)]
    pub bye: Option<String>,
    #[serde(default = "d_range")]
    pub range: f64,
    #[serde(default)]
    pub place: Place,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnemyParams {
    pub object: String,
    #[serde(default)]
    pub sprite: Option<String>,
    /// Empty: no parent.
    #[serde(default = "d_parent_enemy")]
    pub parent: String,
    /// Display name (`global.monstername`).
    pub name: String,
    #[serde(default)]
    pub hp: i64,
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub df: i64,
    #[serde(default)]
    pub exp: i64,
    #[serde(default)]
    pub gold: i64,
    /// Code entry with the `global.monster*` switch. Empty: found automatically.
    #[serde(default)]
    pub setup_code: String,
    /// Enemy type number (`case` label). None: highest number + 1.
    #[serde(default)]
    pub enemy_type: Option<i64>,
    /// Bullet pattern name: the enemy makes `obj_dre_wave_<name>` on its turn. Empty: no attack.
    #[serde(default)]
    pub wave: String,
    /// Length of the enemy turn in steps (`global.turntimer`). 0: the pattern duration, or 150.
    #[serde(default)]
    pub turn_frames: u32,
    /// Also add an encounter.
    #[serde(default)]
    pub encounter: bool,
    #[serde(default)]
    pub encounter_code: String,
    #[serde(default)]
    pub encounter_no: Option<i64>,
    /// Position in battle.
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub battle_text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Marker {
    /// Entrance letter: `obj_markerA`.
    pub letter: char,
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Door {
    /// Kris comes out at `obj_marker<letter>` of the target room.
    pub letter: char,
    pub x: i64,
    pub y: i64,
    #[serde(default = "d_box")]
    pub w: f64,
    #[serde(default = "d_box")]
    pub h: f64,
    pub target: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RoomParams {
    pub name: String,
    /// Room to copy the layout from (views, layers, settings). Empty: the first room.
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub doors: Vec<Door>,
    #[serde(default)]
    pub creation_code: String,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    /// Kris walks into the box.
    #[default]
    Touch,
    /// Kris presses the confirm key near the object.
    Talk,
    /// At once when the room starts.
    Auto,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CutsceneParams {
    pub object: String,
    #[serde(default)]
    pub trigger: Trigger,
    /// Touch box size.
    #[serde(default = "d_box")]
    pub w: f64,
    #[serde(default = "d_box")]
    pub h: f64,
    /// `global.flag` index: the cutscene runs only while the flag is 0, and sets it to 1 at the end.
    #[serde(default)]
    pub flag: Option<u32>,
    #[serde(default)]
    pub steps: Vec<StepIn>,
    #[serde(default)]
    pub place: Place,
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_') && s.len() <= 120
}

fn need_ident(what: &str, s: &str) -> Result<()> {
    if is_ident(s) {
        Ok(())
    } else {
        Err(Error::BadRequest(format!(
            "{what} '{s}' must be letters, digits, and _ (not first)"
        )))
    }
}

fn ty(slug: &str) -> &'static store::AssetType {
    store::asset_type(slug).expect("known asset type")
}

fn need_asset(store: &ChapterStore, plan: &Plan, slug: &'static str, name: &str, what: &str) -> Result<()> {
    need_ident(what, name)?;
    if plan.planned(slug, name) || store.has(ty(slug), name) {
        Ok(())
    } else {
        Err(Error::BadRequest(format!("{what} '{name}' does not exist")))
    }
}

fn n(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Text of one box: ends with `/` (wait for key), and the last one also with `%` (close).
fn box_text(t: &str, last: bool) -> String {
    let body = t.trim_end_matches(['/', '%']);
    format!("{body}{}", if last { "/%" } else { "/" })
}

/// GML that sets up the text box: text speed, face, speaker.
fn talk_head(o: &mut String, ind: &str, speaker: &str) {
    let _ = writeln!(o, "{ind}global.msc = 0;");
    let _ = writeln!(o, "{ind}global.typer = 5;");
    let _ = writeln!(o, "{ind}if (global.darkzone == 1)\n{ind}    global.typer = 6;");
    let _ = writeln!(o, "{ind}global.fc = 0;\n{ind}global.fe = 0;");
    if !speaker.is_empty() {
        let _ = writeln!(o, "{ind}scr_speaker({});", gml::quote(speaker));
    }
}

/// GML that shows text boxes with `obj_dialoguer`. `lines` are final box texts.
fn talk(o: &mut String, ind: &str, key: &str, speaker: &str, lines: &[String]) {
    talk_head(o, ind, speaker);
    for (i, l) in lines.iter().enumerate() {
        let k = gml::quote(&format!("{key}_{i}"));
        if i == 0 {
            let _ = writeln!(o, "{ind}msgsetloc(0, {}, {k});", gml::quote(l));
        } else {
            let _ = writeln!(o, "{ind}msgnextloc({}, {k});", gml::quote(l));
        }
    }
    let _ = writeln!(o, "{ind}instance_create(0, 0, obj_dialoguer);");
}

/// Condition: Kris presses the confirm key within `range` pixels.
fn talk_test(range: f64) -> String {
    format!(
        "global.interact == 0 && button1_p() && instance_exists(obj_mainchara) && distance_to_object(obj_mainchara) <= {}",
        n(range)
    )
}

fn object(sprite: Option<&str>, parent: Option<&str>, visible: bool, solid: bool, events: &[(&str, &str, String)]) -> Value {
    let mut o = patterns::new_object();
    o["Sprite"] = sprite.map_or(Value::Null, |s| json!(s));
    o["ParentObject"] = parent.map_or(Value::Null, |s| json!(s));
    o["Visible"] = json!(visible);
    o["Solid"] = json!(solid);
    o["Events"] = Value::Array(
        events
            .iter()
            .map(|(cat, sub, code)| json!({ "Category": cat, "Subtype": sub, "CodeEntry": code }))
            .collect(),
    );
    o
}

fn create_code(obj: &str) -> String {
    format!("gml_Object_{obj}_Create_0")
}
fn step_code(obj: &str) -> String {
    format!("gml_Object_{obj}_Step_0")
}

/// Adds an object with Create and Step code to the plan.
fn add_object(plan: &mut Plan, obj: &str, data_fn: impl FnOnce(&[(&str, &str, String)]) -> Value, create: String, step: String) {
    let events = [("Create", "", create_code(obj)), ("Step", "Step", step_code(obj))];
    plan.asset("objects", obj, true, data_fn(&events));
    plan.code(create_code(obj), create);
    plan.code(step_code(obj), step);
}

/// Room JSON from the plan (a room this wizard makes) or from the store.
fn room_data(store: &ChapterStore, plan: &mut Plan, room: &str) -> Result<usize> {
    if let Some(i) = plan.writes.iter().position(|w| w.ty == "rooms" && w.name == room) {
        return Ok(i);
    }
    need_asset(store, plan, "rooms", room, "room")?;
    let data = store.read(ty("rooms"), room)?["data"].clone();
    plan.asset("rooms", room, false, data);
    Ok(plan.writes.len() - 1)
}

/// Code entry name for instance creation code that is not used yet.
fn creation_code_name(store: &ChapterStore, plan: &Plan, room: &str, obj: &str) -> String {
    (0..)
        .map(|i| format!("gml_RoomCC_{room}_{obj}_{i}_Create"))
        .find(|c| !plan.planned("code", c) && !store.has(ty("code"), c))
        .unwrap()
}

/// Puts an instance into a room (UTMT reads X and Y as whole numbers) (old rooms: the list only; GMS2 rooms: also the first instance layer).
pub fn add_instance(room: &mut Value, obj: &str, x: i64, y: i64, creation_code: Option<&str>) -> Result<()> {
    let r = room
        .as_object_mut()
        .ok_or_else(|| Error::Other("room is not a JSON object".into()))?;
    let list = r.entry("GameObjects").or_insert_with(|| json!([]));
    let insts = list
        .as_array_mut()
        .ok_or_else(|| Error::Other("room GameObjects is not a list".into()))?;
    let i = insts.len();
    insts.push(json!({
        "ObjectName": obj, "X": x, "Y": y, "CreationCodeEntry": creation_code, "ScaleX": 1, "ScaleY": 1,
        "Color": 0xffffffffu32, "Rotation": 0, "PreCreateCodeEntry": null, "ImageSpeed": 1, "ImageIndex": 0,
    }));
    if let Some(layers) = r.get_mut("Layers").and_then(Value::as_array_mut)
        && !layers.is_empty()
    {
        let is_inst = |l: &Value| {
            l.get("$type")
                .and_then(Value::as_str)
                .map_or(l.get("InstanceIndices").is_some(), |t| t == "InstancesLayer" || t == "Instances")
        };
        let l = layers
            .iter_mut()
            .filter(|l| is_inst(l))
            .min_by(|a, b| a["Depth"].as_f64().unwrap_or(0.0).total_cmp(&b["Depth"].as_f64().unwrap_or(0.0)))
            .ok_or_else(|| Error::BadRequest("the room has no instance layer".into()))?;
        match l.get_mut("InstanceIndices").and_then(Value::as_array_mut) {
            Some(a) => a.push(json!(i)),
            None => {
                l["InstanceIndices"] = json!([i]);
            }
        }
    }
    if let Some(order) = r.get_mut("InstanceCreationOrderIDs").and_then(Value::as_array_mut) {
        order.push(json!(i));
    }
    Ok(())
}

fn place(store: &ChapterStore, plan: &mut Plan, p: &Place, obj: &str, creation: Option<String>) -> Result<()> {
    if p.room.is_empty() {
        return Ok(());
    }
    let ri = room_data(store, plan, &p.room)?;
    let cc = creation.map(|src| {
        let name = creation_code_name(store, plan, &p.room, obj);
        plan.code(name.clone(), src);
        name
    });
    let data = plan.writes[ri].data.as_mut().expect("room data");
    add_instance(data, obj, p.x, p.y, cc.as_deref())
}

fn check_new_object(store: &ChapterStore, obj: &str, sprite: Option<&str>) -> Result<Plan> {
    need_ident("object", obj)?;
    let plan = Plan::default();
    if store.has(ty("objects"), obj) {
        return Err(Error::Conflict(format!("object '{obj}' exists already")));
    }
    if let Some(s) = sprite {
        need_asset(store, &plan, "sprites", s, "sprite")?;
    }
    Ok(plan)
}

fn none_if_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|x| !x.is_empty())
}

pub fn npc(store: &ChapterStore, p: &NpcParams) -> Result<Plan> {
    let sprite = none_if_empty(&p.sprite);
    let parent = none_if_empty(&p.parent);
    let mut plan = check_new_object(store, &p.object, sprite)?;
    if let Some(par) = parent {
        need_asset(store, &plan, "objects", par, "parent")?;
    }
    if p.lines.is_empty() {
        return Err(Error::BadRequest("give at least one line".into()));
    }
    let lines: Vec<String> = p
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| box_text(l, i + 1 == p.lines.len()))
        .collect();
    let o = &p.object;
    let mut create = String::new();
    if parent.is_some() {
        create.push_str("event_inherited();\n");
    }
    create.push_str("dre_con = 0;\nimage_speed = 0;\n");
    let mut step = String::new();
    if parent.is_some() {
        step.push_str("event_inherited();\n");
    }
    let _ = writeln!(
        step,
        "if (dre_con == 0 && {})\n{{\n    dre_con = 1;\n    global.interact = 1;",
        talk_test(p.range)
    );
    talk(&mut step, "    ", &format!("{o}_dre"), &p.speaker, &lines);
    step.push_str("}\nelse if (dre_con == 1 && !instance_exists(obj_dialoguer))\n{\n    dre_con = 0;\n    global.interact = 0;\n}\n");
    add_object(&mut plan, o, |ev| object(sprite, parent, true, p.solid, ev), create, step);
    place(store, &mut plan, &p.place, o, None)?;
    plan.notes
        .push(format!("Kris talks to {o} with the confirm key within {} pixels.", n(p.range)));
    Ok(plan)
}

pub fn shop(store: &ChapterStore, p: &ShopParams) -> Result<Plan> {
    let sprite = none_if_empty(&p.sprite);
    let mut plan = check_new_object(store, &p.object, sprite)?;
    if p.items.is_empty() || p.items.len() > 3 {
        return Err(Error::BadRequest("give 1 to 3 items".into()));
    }
    let o = &p.object;
    let key = format!("{o}_dre");
    let text = |v: &Option<String>, d: &str| v.clone().filter(|x| !x.is_empty()).unwrap_or_else(|| d.to_owned());
    let choices = p.items.len() + 1;
    let mut step = format!(
        "if (dre_con == 0 && {})\n{{\n    dre_con = 1;\n    global.interact = 1;\n",
        talk_test(p.range)
    );
    for i in 0..4 {
        let label = match p.items.get(i) {
            Some(it) => format!("#{} ({}G)", it.name, it.price),
            None if i == p.items.len() => "#Leave".to_owned(),
            None => String::new(),
        };
        let _ = writeln!(
            step,
            "    global.choicemsg[{i}] = stringsetloc({}, {});",
            gml::quote(&label),
            gml::quote(&format!("{key}_c{i}"))
        );
    }
    let greet = box_text(&text(&p.greeting, "* Welcome!^1 What do you want?"), false);
    talk(
        &mut step,
        "    ",
        &format!("{key}_g"),
        &p.speaker,
        &[greet, format!("\\C{choices} ")],
    );
    step.push_str("}\nelse if (dre_con == 1 && !instance_exists(obj_dialoguer))\n{\n    dre_con = 2;\n    dre_msg = 4;\n    noroom = 0;\n");
    for (i, it) in p.items.iter().enumerate() {
        let _ = writeln!(
            step,
            "    {}if (global.choice == {i})\n    {{\n        if (global.gold < {price})\n            dre_msg = 1;\n        else\n        {{\n            scr_itemget({item});\n            if (noroom)\n                dre_msg = 2;\n            else\n            {{\n                global.gold -= {price};\n                dre_msg = 3;\n            }}\n        }}\n    }}",
            if i == 0 { "" } else { "else " },
            price = it.price,
            item = it.item
        );
    }
    let replies = [
        (1, text(&p.poor, "* You do not have enough money.")),
        (2, text(&p.full, "* You are carrying too much.")),
        (3, text(&p.bought, "* Thank you!")),
        (4, text(&p.bye, "* Come again!")),
    ];
    talk_head(&mut step, "    ", &p.speaker);
    for (m, t) in &replies {
        let call = format!(
            "msgsetloc(0, {}, {});",
            gml::quote(&box_text(t, true)),
            gml::quote(&format!("{key}_r{m}"))
        );
        let test = match m {
            1 => "if (dre_msg == 1)".to_owned(),
            4 => "else".to_owned(),
            _ => format!("else if (dre_msg == {m})"),
        };
        let _ = writeln!(step, "    {test}\n        {call}");
    }
    step.push_str("    instance_create(0, 0, obj_dialoguer);\n");
    step.push_str("}\nelse if (dre_con == 2 && !instance_exists(obj_dialoguer))\n{\n    dre_con = 0;\n    global.interact = 0;\n}\n");
    add_object(
        &mut plan,
        o,
        |ev| object(sprite, None, true, false, ev),
        "dre_con = 0;\ndre_msg = 0;\nimage_speed = 0;\n".into(),
        step,
    );
    place(store, &mut plan, &p.place, o, None)?;
    plan.notes
        .push("Simple shop: one choice box (items and Leave), gold check, scr_itemget. It is not a full Deltarune shop room.".into());
    Ok(plan)
}

/// Facts about a `switch` in a source: where a new case goes and how it is indented.
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchInfo {
    /// Byte position for the new case text (start of a line).
    pub at: usize,
    pub case_indent: String,
    pub body_indent: String,
    /// Highest number label.
    pub max_case: Option<i64>,
    pub labels: Vec<String>,
    /// The text before `at` does not end with a newline.
    pub needs_newline: bool,
}

fn line_indent(src: &str, pos: usize) -> (usize, String) {
    let ls = src[..pos].rfind('\n').map_or(0, |p| p + 1);
    (ls, src[ls..pos].chars().take_while(|c| c.is_whitespace()).collect())
}

/// Finds the `switch` whose body has the most `marker` names (for example `monstermaxhp`).
pub fn find_switch(src: &str, marker: &str) -> Option<SwitchInfo> {
    let toks = gml::tokenize(src);
    let mut best: Option<(usize, usize, usize)> = None; // (count, open brace, close brace)
    for k in 0..toks.len() {
        if toks[k].ident() != Some("switch") || !toks.get(k + 1).is_some_and(|t| t.is_punct('(')) {
            continue;
        }
        let Some(cp) = gml::matching(&toks, k + 1) else { continue };
        if !toks.get(cp + 1).is_some_and(|t| t.is_punct('{')) {
            continue;
        }
        let Some(cb) = gml::matching(&toks, cp + 1) else { continue };
        let count = toks[cp + 1..cb].iter().filter(|t| t.ident() == Some(marker)).count();
        if count > 0 && best.is_none_or(|b| count > b.0) {
            best = Some((count, cp + 1, cb));
        }
    }
    let (_, ob, cb) = best?;
    // Labels at the top level of this switch body.
    let mut labels = Vec::new();
    let mut first_case: Option<usize> = None;
    let mut default_at: Option<usize> = None;
    let mut depth = 0usize;
    let mut j = ob + 1;
    while j < cb {
        match toks[j].tok {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 {
            if toks[j].ident() == Some("case") {
                first_case.get_or_insert(j);
                let a = j + 1;
                let mut b = a;
                while b < cb && !toks[b].is_punct(':') {
                    b += 1;
                }
                if b > a {
                    labels.push(src[toks[a].start..toks[b - 1].end].to_owned());
                }
                j = b;
            } else if toks[j].ident() == Some("default") && toks.get(j + 1).is_some_and(|t| t.is_punct(':')) {
                first_case.get_or_insert(j);
                default_at.get_or_insert(j);
            }
        }
        j += 1;
    }
    let max_case = labels.iter().filter_map(|l| l.trim().parse::<i64>().ok()).max();
    let (case_indent, body_indent) = match first_case {
        Some(fc) => {
            let ci = line_indent(src, toks[fc].start).1;
            // First token on a later line after the label's `:`.
            let colon = (fc..cb).find(|&x| toks[x].is_punct(':')).unwrap_or(fc);
            let bi = toks[colon + 1..cb]
                .iter()
                .find(|t| t.line > toks[colon].line)
                .map(|t| line_indent(src, t.start).1)
                .filter(|b| b.len() > ci.len())
                .unwrap_or_else(|| format!("{ci}    "));
            (ci, bi)
        }
        None => {
            let ci = format!("{}    ", line_indent(src, toks[ob].start).1);
            let bi = format!("{ci}    ");
            (ci, bi)
        }
    };
    let target = default_at.unwrap_or(cb);
    let (ls, _) = line_indent(src, toks[target].start);
    let at_line_start = src[ls..toks[target].start].trim().is_empty();
    let at = if at_line_start { ls } else { toks[target].start };
    Some(SwitchInfo {
        at,
        case_indent,
        body_indent,
        max_case,
        labels,
        needs_newline: !at_line_start,
    })
}

/// Picks the code entry with the most `global.<marker>` names, names with `prefer` first.
fn find_setup_code(store: &ChapterStore, marker: &str, prefer: &str) -> Option<(String, String)> {
    let needle = format!("global.{marker}");
    store
        .code_sources()
        .filter(|(_, src)| src.contains(&needle))
        .max_by_key(|(name, src)| (name.contains(prefer), src.matches(&needle).count()))
}

/// Inserts `case <label>:` with assignment lines and `break;`.
fn insert_case(src: &str, sw: &SwitchInfo, label: i64, lines: &[String]) -> String {
    let mut t = String::new();
    if sw.needs_newline {
        t.push('\n');
    }
    let _ = writeln!(t, "{}case {label}:", sw.case_indent);
    for l in lines {
        let _ = writeln!(t, "{}{l}", sw.body_indent);
    }
    let _ = writeln!(t, "{}break;", sw.body_indent);
    if sw.needs_newline {
        t.push_str(&sw.case_indent);
    }
    format!("{}{t}{}", &src[..sw.at], &src[sw.at..])
}

/// Index expression of the `global.monster*` assignments in a source (`myself`, `0`, ...).
fn common_index(code: &str, src: &str, field: &str) -> Option<String> {
    let recs = enemies::scan_source(code, src);
    let mut counts: Vec<(String, usize)> = Vec::new();
    for r in recs.iter().filter(|r| r.fields.contains_key(field)) {
        match counts.iter_mut().find(|c| c.0 == r.index) {
            Some(c) => c.1 += 1,
            None => counts.push((r.index.clone(), 1)),
        }
    }
    counts.into_iter().max_by_key(|c| c.1).map(|c| c.0)
}

/// `xx + ` when the existing values are written as offsets (`global.monstermakex[0] = xx + 420`).
fn offset_prefix(code: &str, src: &str, field: &str) -> String {
    enemies::scan_source(code, src)
        .iter()
        .filter_map(|r| r.fields.get(field))
        .find_map(|f| {
            let (a, b) = f.expr.split_once('+')?;
            (is_ident(a.trim()) && b.trim().parse::<f64>().is_ok()).then(|| format!("{} + ", a.trim()))
        })
        .unwrap_or_default()
}

fn text_expr(src: &str, s: &str, key: &str) -> String {
    if src.contains("stringsetloc(") {
        format!("stringsetloc({}, {})", gml::quote(s), gml::quote(key))
    } else {
        gml::quote(s)
    }
}

fn read_code(store: &ChapterStore, plan: &Plan, name: &str) -> Result<String> {
    need_asset(store, plan, "code", name, "code entry")?;
    Ok(store.read(ty("code"), name)?["source"].as_str().unwrap_or_default().to_owned())
}

pub fn enemy(store: &ChapterStore, p: &EnemyParams) -> Result<Plan> {
    let sprite = none_if_empty(&p.sprite);
    let parent = Some(p.parent.as_str()).filter(|x| !x.is_empty());
    let mut plan = check_new_object(store, &p.object, sprite)?;
    if let Some(par) = parent {
        need_asset(store, &plan, "objects", par, "parent")?;
    }
    let o = &p.object;

    // Stats: a new case in the monster setup switch.
    let (setup_name, setup_src) = if p.setup_code.is_empty() {
        find_setup_code(store, "monstermaxhp", "monstersetup")
            .ok_or_else(|| Error::BadRequest("no code sets global.monstermaxhp; give setup_code".into()))?
    } else {
        (p.setup_code.clone(), read_code(store, &plan, &p.setup_code)?)
    };
    let sw = find_switch(&setup_src, "monstermaxhp")
        .ok_or_else(|| Error::BadRequest(format!("no switch with global.monstermaxhp in {setup_name}")))?;
    let etype = p.enemy_type.unwrap_or_else(|| sw.max_case.map_or(1, |m| m + 1));
    if sw.labels.iter().any(|l| l.trim() == etype.to_string()) {
        return Err(Error::Conflict(format!("enemy type {etype} exists already in {setup_name}")));
    }
    let idx = common_index(&setup_name, &setup_src, "monstermaxhp").unwrap_or_else(|| "myself".into());
    let name_expr = text_expr(&setup_src, &p.name, &format!("{o}_dre_name"));
    let stat = |f: &str, v: String| format!("global.{f}[{idx}] = {v};");
    let lines = vec![
        stat("monstername", name_expr),
        stat("monstermaxhp", p.hp.to_string()),
        stat("monsterhp", p.hp.to_string()),
        stat("monsterat", p.at.to_string()),
        stat("monsterdf", p.df.to_string()),
        stat("monsterexp", p.exp.to_string()),
        stat("monstergold", p.gold.to_string()),
    ];
    plan.writes.push(PlannedWrite {
        ty: "code",
        name: setup_name.clone(),
        new: false,
        data: None,
        source: Some(insert_case(&setup_src, &sw, etype, &lines)),
    });

    // Object: inherits the battle rules, starts the wave on its turn.
    let inherit = if parent.is_some() { "event_inherited();\n" } else { "" };
    let create = format!("{inherit}attacked = 0;\n");
    let mut step = inherit.to_owned();
    if !p.wave.is_empty() {
        patterns::validate_pattern_name(&p.wave)?;
        let wave_obj = patterns::wave_object(&p.wave);
        let frames = if p.turn_frames > 0 {
            p.turn_frames
        } else {
            patterns::PatternStore::new(store).read(&p.wave).map_or(150, |w| w.duration)
        };
        let _ = write!(
            step,
            "if (global.mnfight == 2 && attacked == 0)\n{{\n    attacked = 1;\n    global.turntimer = {frames};\n    instance_create(0, 0, {wave_obj});\n}}\nif (global.mnfight != 2)\n    attacked = 0;\n"
        );
        if !store.has(ty("objects"), &wave_obj) {
            plan.notes.push(format!(
                "Install the bullet pattern '{}' before you build: {wave_obj} does not exist yet.",
                p.wave
            ));
        }
    } else {
        step.push_str("// Attacks: make a wave object here when global.mnfight == 2.\n");
    }
    add_object(&mut plan, o, |ev| object(sprite, parent, true, false, ev), create, step);

    if p.encounter {
        let (enc_name, enc_src) = if p.encounter_code.is_empty() {
            find_setup_code(store, "monsterinstancetype", "encountersetup")
                .ok_or_else(|| Error::BadRequest("no code sets global.monsterinstancetype; give encounter_code".into()))?
        } else {
            (p.encounter_code.clone(), read_code(store, &plan, &p.encounter_code)?)
        };
        // The same code can hold the stats too: change the planned source.
        let enc_src = plan
            .writes
            .iter()
            .find(|w| w.ty == "code" && w.name == enc_name)
            .and_then(|w| w.source.clone())
            .unwrap_or(enc_src);
        let esw = find_switch(&enc_src, "monsterinstancetype")
            .ok_or_else(|| Error::BadRequest(format!("no switch with global.monsterinstancetype in {enc_name}")))?;
        let no = p.encounter_no.unwrap_or_else(|| esw.max_case.map_or(1, |m| m + 1));
        if esw.labels.iter().any(|l| l.trim() == no.to_string()) {
            return Err(Error::Conflict(format!("encounter {no} exists already in {enc_name}")));
        }
        let px = offset_prefix(&enc_name, &enc_src, "monstermakex");
        let py = offset_prefix(&enc_name, &enc_src, "monstermakey");
        let text = if p.battle_text.is_empty() {
            format!("* {} appears.", p.name)
        } else {
            p.battle_text.clone()
        };
        let lines = vec![
            format!("global.monsterinstancetype[0] = {o};"),
            format!("global.monstertype[0] = {etype};"),
            format!("global.monstermakex[0] = {px}{};", n(p.x)),
            format!("global.monstermakey[0] = {py}{};", n(p.y)),
            format!(
                "global.battlemsg[0] = {};",
                text_expr(&enc_src, &text, &format!("{o}_dre_battlemsg"))
            ),
        ];
        let new_src = insert_case(&enc_src, &esw, no, &lines);
        match plan.writes.iter_mut().find(|w| w.ty == "code" && w.name == enc_name) {
            Some(w) => w.source = Some(new_src),
            None => plan.writes.push(PlannedWrite {
                ty: "code",
                name: enc_name.clone(),
                new: false,
                data: None,
                source: Some(new_src),
            }),
        }
        plan.notes.push(format!(
            "Encounter {no} is in {enc_name}. Start it from an object, as the game starts its other encounters."
        ));
    }
    plan.notes.push(format!("Enemy type {etype} is in {setup_name}."));
    Ok(plan)
}

fn need_letter(c: char) -> Result<()> {
    if c.is_ascii_uppercase() {
        Ok(())
    } else {
        Err(Error::BadRequest(format!("entrance letter '{c}' must be A to Z")))
    }
}

/// Door object for one entrance letter: Kris touches the box, the game goes to `dre_room` (set in creation code).
fn door_object(plan: &mut Plan, letter: char) {
    let o = format!("obj_dre_door{letter}");
    let entrance = letter as u32 - 'A' as u32 + 1;
    let create = format!("dre_room = -1;\ndre_w = 40;\ndre_h = 40;\ndre_entrance = {entrance};\n");
    let step = "if (dre_room != -1 && global.interact == 0 && instance_exists(obj_mainchara))\n{\n    if (collision_rectangle(x, y, x + dre_w - 1, y + dre_h - 1, obj_mainchara, false, true) != noone)\n    {\n        global.entrance = dre_entrance;\n        dre_next = dre_room;\n        dre_room = -1;\n        room_goto(dre_next);\n    }\n}\n";
    add_object(plan, &o, |ev| object(None, None, false, false, ev), create, step.into());
}

pub fn room(store: &ChapterStore, p: &RoomParams) -> Result<Plan> {
    need_ident("room", &p.name)?;
    let rooms = ty("rooms");
    if store.has(rooms, &p.name) {
        return Err(Error::Conflict(format!("room '{}' exists already", p.name)));
    }
    let template = if p.template.is_empty() {
        store
            .list(rooms)
            .into_iter()
            .next()
            .map(|e| e.name)
            .ok_or_else(|| Error::BadRequest("no room to copy the layout from".into()))?
    } else {
        p.template.clone()
    };
    let mut plan = Plan::default();
    need_asset(store, &plan, "rooms", &template, "template room")?;
    let mut data = store.read(rooms, &template)?["data"].clone();
    {
        let r = data
            .as_object_mut()
            .ok_or_else(|| Error::Other("room is not a JSON object".into()))?;
        r.insert("GameObjects".into(), json!([]));
        r.insert("Tiles".into(), json!([]));
        r.insert("CreationCodeEntry".into(), Value::Null);
        if r.get("InstanceCreationOrderIDs").is_some_and(Value::is_array) {
            r.insert("InstanceCreationOrderIDs".into(), json!([]));
        }
        if let Some(w) = p.width {
            r.insert("Width".into(), json!(w));
        }
        if let Some(h) = p.height {
            r.insert("Height".into(), json!(h));
        }
        for l in r.get_mut("Layers").and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(o) = l.as_object_mut() {
                for (k, empty) in [("InstanceIndices", json!([])), ("Sprites", json!([])), ("LegacyTiles", json!([]))] {
                    if o.contains_key(k) {
                        o.insert(k.into(), empty);
                    }
                }
                if let Some(td) = o.get_mut("TileData") {
                    clear_tiles(td);
                }
            }
        }
    }
    plan.asset("rooms", &p.name, true, data);
    if !p.creation_code.trim().is_empty() {
        let cc = format!("gml_RoomCC_{}_Create", p.name);
        plan.code(cc.clone(), p.creation_code.clone());
        plan.writes[0].data.as_mut().unwrap()["CreationCodeEntry"] = json!(cc);
    }
    for m in &p.markers {
        need_letter(m.letter)?;
        let o = format!("obj_marker{}", m.letter);
        if !store.has(ty("objects"), &o) && !plan.planned("objects", &o) {
            plan.asset("objects", &o, true, object(None, None, false, false, &[]));
            plan.notes.push(format!("{o} did not exist: made an empty one."));
        }
        let pl = Place {
            room: p.name.clone(),
            x: m.x,
            y: m.y,
        };
        place(store, &mut plan, &pl, &o, None)?;
    }
    for d in &p.doors {
        need_letter(d.letter)?;
        if d.target != p.name {
            need_asset(store, &plan, "rooms", &d.target, "door target room")?;
        }
        let o = format!("obj_dre_door{}", d.letter);
        if !store.has(ty("objects"), &o) && !plan.planned("objects", &o) {
            door_object(&mut plan, d.letter);
        }
        let pl = Place {
            room: p.name.clone(),
            x: d.x,
            y: d.y,
        };
        let cc = format!("dre_room = {};\ndre_w = {};\ndre_h = {};\n", d.target, n(d.w), n(d.h));
        place(store, &mut plan, &pl, &o, Some(cc))?;
    }
    plan.notes.push(
        "Doors set global.entrance (A = 1, B = 2, ...) and go to the target room. Kris comes out at the marker with the same letter there."
            .into(),
    );
    plan.notes.push(format!(
        "To come into {} from another room, put an obj_dre_door instance there with creation code `dre_room = {};`.",
        p.name, p.name
    ));
    Ok(plan)
}

/// Sets all tile numbers to 0 (keeps the shape of nested lists).
fn clear_tiles(v: &mut Value) {
    match v {
        Value::Array(a) => a.iter_mut().for_each(clear_tiles),
        Value::Number(_) => *v = json!(0),
        _ => {}
    }
}

pub fn cutscene(store: &ChapterStore, p: &CutsceneParams) -> Result<Plan> {
    let mut plan = check_new_object(store, &p.object, None)?;
    let o = &p.object;
    let mut steps = if p.steps.is_empty() {
        vec![
            StepIn {
                func: "c_sel".into(),
                args: vec!["kr".into()],
            },
            StepIn {
                func: "c_facing".into(),
                args: vec!["\"d\"".into()],
            },
            StepIn {
                func: "c_wait".into(),
                args: vec!["30".into()],
            },
        ]
    } else {
        p.steps.clone()
    };
    if !steps.iter().any(|s| s.func == "c_terminatekillactors") {
        steps.push(StepIn {
            func: "c_terminatekillactors".into(),
            args: vec![],
        });
    }
    let body = cutscene::render_steps(&steps, "    ")?;
    let mut create = format!("con = 0;\ndre_w = {};\ndre_h = {};\n", n(p.w), n(p.h));
    if let Some(f) = p.flag {
        let _ = writeln!(create, "if (global.flag[{f}] != 0)\n    instance_destroy();");
    }
    let test = match p.trigger {
        Trigger::Touch => {
            "global.interact == 0 && instance_exists(obj_mainchara) && collision_rectangle(x, y, x + dre_w - 1, y + dre_h - 1, obj_mainchara, false, true) != noone".to_owned()
        }
        Trigger::Talk => talk_test(24.0),
        Trigger::Auto => "global.interact == 0".to_owned(),
    };
    let mut step = format!(
        "if (con == 0 && {test})\n{{\n    con = 1;\n    global.interact = 1;\n    cutscene_master = scr_cutscene_make();\n    scr_maincharacters_actors();\n}}\nif (con == 1)\n{{\n    con = 2;\n    {body}\n}}\nif (con == 2 && !instance_exists(obj_cutscene_master))\n{{\n    con = 3;\n    global.interact = 0;\n"
    );
    if let Some(f) = p.flag {
        let _ = writeln!(step, "    global.flag[{f}] = 1;");
    }
    step.push_str("}\n");
    add_object(&mut plan, o, |ev| object(None, None, false, false, ev), create, step);
    place(store, &mut plan, &p.place, o, None)?;
    plan.notes.push(format!("Edit the steps in Cutscenes › {}.", step_code(o)));
    Ok(plan)
}

/// Makes the plan of one wizard from its JSON parameters.
pub fn plan(store: &ChapterStore, kind: &str, params: Value) -> Result<Plan> {
    fn p<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
        serde_json::from_value(v).map_err(|e| Error::BadRequest(format!("parameters: {e}")))
    }
    match kind {
        "npc" => npc(store, &p(params)?),
        "shop" => shop(store, &p(params)?),
        "enemy" => enemy(store, &p(params)?),
        "room" => room(store, &p(params)?),
        "cutscene" => cutscene(store, &p(params)?),
        _ => Err(Error::NotFound(format!("wizard '{kind}' (use {})", KINDS.join(", ")))),
    }
}

/// Writes a plan into `mod/`. All or nothing: on an error, the assets that were written go back.
pub fn apply(store: &ChapterStore, plan: &Plan) -> Result<()> {
    for w in &plan.writes {
        let t = ty(w.ty);
        if w.new && store.has(t, &w.name) {
            return Err(Error::Conflict(format!("{}/{} exists already", w.ty, w.name)));
        }
        if !w.new && !store.has(t, &w.name) {
            return Err(Error::NotFound(format!("{}/{}", w.ty, w.name)));
        }
    }
    // Old state of each asset: None = not in mod/ (revert), Some = the mod copy to write back.
    let mut done: Vec<(&PlannedWrite, Option<Value>)> = Vec::new();
    for w in &plan.writes {
        let t = ty(w.ty);
        let old = store.in_mod(t, &w.name).then(|| store.read(t, &w.name)).transpose()?;
        if let Err(e) = store.write(t, &w.name, w.data.clone(), w.source.clone()) {
            // The failed write can have copied the asset into mod/ already.
            done.push((w, old));
            for (w, old) in done.into_iter().rev() {
                let t = ty(w.ty);
                let _ = match old {
                    Some(o) => store
                        .write(
                            t,
                            &w.name,
                            (w.ty != "code").then(|| o["data"].clone()),
                            o["source"].as_str().map(str::to_owned),
                        )
                        .map(|_| ()),
                    None if store.in_mod(t, &w.name) => store.revert(t, &w.name),
                    None => Ok(()),
                };
            }
            return Err(e);
        }
        done.push((w, old));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETUP: &str = "function scr_monstersetup()\n{\n    switch (global.monstertype[myself])\n    {\n        case 1:\n            global.monstername[myself] = stringsetloc(\"Dummy\", \"k\");\n            global.monstermaxhp[myself] = 90;\n            break;\n        case 3:\n            global.monstermaxhp[myself] = 5;\n            break;\n        default:\n            global.monstermaxhp[myself] = 1;\n    }\n}\n";
    const ENC: &str = "switch (argument0)\n{\n    case 1:\n        global.monsterinstancetype[0] = obj_dummy;\n        global.monstertype[0] = 1;\n        global.monstermakex[0] = xx + 420;\n        global.monstermakey[0] = yy + 160;\n        break;\n}\n";

    fn setup() -> (tempfile::TempDir, ChapterStore) {
        let tmp = tempfile::tempdir().unwrap();
        let st = ChapterStore::new(tmp.path(), "chapter1");
        let base = st.base();
        for d in ["code", "objects", "rooms", "sprites/spr_npc"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::write(base.join("project.json"), r#"{"Name":"T"}"#).unwrap();
        std::fs::write(base.join("code/gml_GlobalScript_scr_monstersetup.gml"), SETUP).unwrap();
        std::fs::write(base.join("code/gml_GlobalScript_scr_encountersetup.gml"), ENC).unwrap();
        std::fs::write(base.join("objects/obj_monsterparent.json"), r#"{"$type":"GameObject"}"#).unwrap();
        std::fs::write(base.join("objects/obj_markerA.json"), r#"{"$type":"GameObject"}"#).unwrap();
        std::fs::write(base.join("sprites/spr_npc/spr_npc.json"), r#"{"$type":"Sprite"}"#).unwrap();
        let room = json!({ "$type": "Room", "Width": 320, "Height": 240, "GameObjects": [{ "ObjectName": "obj_markerA", "X": 1, "Y": 2 }],
            "Layers": [{ "$type": "InstancesLayer", "Name": "Instances", "Depth": 0, "InstanceIndices": [0] },
                       { "$type": "TilesLayer", "Name": "T", "Depth": 100, "TileData": [[5, 6], [7, 8]] }],
            "InstanceCreationOrderIDs": [0], "CreationCodeEntry": "gml_RoomCC_room_a_Create" });
        std::fs::write(base.join("rooms/room_a.json"), serde_json::to_vec(&room).unwrap()).unwrap();
        (tmp, st)
    }

    fn src<'a>(plan: &'a Plan, name: &str) -> &'a str {
        plan.writes
            .iter()
            .find(|w| w.name == name)
            .and_then(|w| w.source.as_deref())
            .unwrap()
    }

    #[test]
    fn switch_insert_before_default() {
        let sw = find_switch(SETUP, "monstermaxhp").unwrap();
        assert_eq!(sw.max_case, Some(3));
        assert_eq!(sw.labels, ["1", "3"]);
        assert_eq!((sw.case_indent.as_str(), sw.body_indent.as_str()), ("        ", "            "));
        let out = insert_case(SETUP, &sw, 4, &["x = 1;".into()]);
        assert!(
            out.contains("            break;\n        case 4:\n            x = 1;\n            break;\n        default:"),
            "{out}"
        );
        // One-line switch: the case goes before the closing brace.
        let one = "switch (a) { case 1: global.monstermaxhp[0] = 1; break; }";
        let sw = find_switch(one, "monstermaxhp").unwrap();
        let out = insert_case(one, &sw, 2, &["y = 2;".into()]);
        assert_eq!(gml::tokenize(&out).iter().filter(|t| t.ident() == Some("case")).count(), 2, "{out}");
        assert!(find_switch("x = 1;", "monstermaxhp").is_none());
    }

    #[test]
    fn npc_plan_and_apply() {
        let (_t, st) = setup();
        let p: NpcParams = serde_json::from_value(json!({
            "object": "obj_npc_a", "sprite": "spr_npc", "speaker": "susie", "lines": ["* Hi.", "* Bye./%"],
            "place": { "room": "room_a", "x": 40, "y": 50 },
        }))
        .unwrap();
        let plan = npc(&st, &p).unwrap();
        let step = src(&plan, "gml_Object_obj_npc_a_Step_0");
        assert!(step.contains("msgsetloc(0, \"* Hi./\", \"obj_npc_a_dre_0\");"), "{step}");
        assert!(step.contains("msgnextloc(\"* Bye./%\", \"obj_npc_a_dre_1\");"));
        assert!(step.contains("scr_speaker(\"susie\");"));
        // The dialogue index sees the new lines with their speaker.
        let lines = crate::dialogue::scan_source("x", step);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].speaker.as_deref(), Some("susie"));
        let room = plan.writes.iter().find(|w| w.name == "room_a").unwrap();
        assert!(!room.new);
        let d = room.data.as_ref().unwrap();
        assert_eq!(d["GameObjects"][1]["ObjectName"], "obj_npc_a");
        assert_eq!(d["Layers"][0]["InstanceIndices"], json!([0, 1]));
        assert_eq!(d["InstanceCreationOrderIDs"], json!([0, 1]));

        apply(&st, &plan).unwrap();
        let obj = st.read(ty("objects"), "obj_npc_a").unwrap();
        assert_eq!(obj["data"]["Sprite"], "spr_npc");
        assert_eq!(obj["data"]["Events"][1]["CodeEntry"], "gml_Object_obj_npc_a_Step_0");
        assert_eq!(obj["data"]["PhysicsVertices"], json!([]));
        // Again: the object exists now.
        assert!(matches!(npc(&st, &p), Err(Error::Conflict(_))));
        assert!(matches!(apply(&st, &plan), Err(Error::Conflict(_))));
        // Bad input.
        let bad: NpcParams = serde_json::from_value(json!({ "object": "obj_b", "sprite": "spr_none", "lines": ["x"] })).unwrap();
        assert!(npc(&st, &bad).is_err());
        let bad: NpcParams = serde_json::from_value(json!({ "object": "obj b", "lines": ["x"] })).unwrap();
        assert!(npc(&st, &bad).is_err());
    }

    #[test]
    fn apply_rolls_back() {
        let (_t, st) = setup();
        let mut plan = Plan::default();
        plan.code("gml_Script_new_a", "x = 1;\n".into());
        plan.writes.push(PlannedWrite {
            ty: "code",
            name: "gml_GlobalScript_scr_encountersetup".into(),
            new: false,
            data: None,
            source: Some("changed".into()),
        });
        // Bad data: wrong $type, so this write fails after the first two.
        plan.asset("objects", "obj_bad", true, json!({ "$type": "Room" }));
        assert!(apply(&st, &plan).is_err());
        assert!(!st.has(ty("code"), "gml_Script_new_a"));
        assert!(!st.in_mod(ty("code"), "gml_GlobalScript_scr_encountersetup"));
        assert!(!st.has(ty("objects"), "obj_bad"));
    }

    #[test]
    fn shop_gml() {
        let (_t, st) = setup();
        let p: ShopParams = serde_json::from_value(json!({
            "object": "obj_shop_a", "items": [{ "name": "Candy", "item": 1, "price": 25 }, { "name": "Tea", "item": 2, "price": 40 }],
        }))
        .unwrap();
        let plan = shop(&st, &p).unwrap();
        let step = src(&plan, "gml_Object_obj_shop_a_Step_0");
        assert!(step.contains("global.choicemsg[0] = stringsetloc(\"#Candy (25G)\""), "{step}");
        assert!(step.contains("global.choicemsg[2] = stringsetloc(\"#Leave\""));
        assert!(step.contains("msgnextloc(\"\\\\C3 \""), "{step}");
        assert!(step.contains("else if (global.choice == 1)"));
        assert!(step.contains("global.gold -= 40;") && step.contains("scr_itemget(2);"));
        assert!(step.contains("    else\n        msgsetloc(0, \"* Come again!/%\""), "{step}");
        assert_eq!(step.matches("instance_create(0, 0, obj_dialoguer);").count(), 2);
        // Brackets match in the made code.
        let toks = gml::tokenize(step);
        assert_eq!(
            toks.iter().filter(|t| t.is_punct('{')).count(),
            toks.iter().filter(|t| t.is_punct('}')).count()
        );
        let none: ShopParams = serde_json::from_value(json!({ "object": "obj_s", "items": [] })).unwrap();
        assert!(shop(&st, &none).is_err());
    }

    #[test]
    fn enemy_cases() {
        let (_t, st) = setup();
        let p: EnemyParams = serde_json::from_value(json!({
            "object": "obj_dre_enemy_a", "name": "Blob", "hp": 120, "at": 4, "df": 1, "wave": "w1",
            "encounter": true, "x": 400, "y": 120,
        }))
        .unwrap();
        let plan = enemy(&st, &p).unwrap();
        let setup = src(&plan, "gml_GlobalScript_scr_monstersetup");
        assert!(
            setup.contains("        case 4:\n            global.monstername[myself] = stringsetloc(\"Blob\", \"obj_dre_enemy_a_dre_name\");\n            global.monstermaxhp[myself] = 120;"),
            "{setup}"
        );
        assert!(setup.find("case 4:").unwrap() < setup.find("default:").unwrap());
        let recs = enemies::scan_source("s", setup);
        assert!(
            recs.iter()
                .any(|r| r.case.as_deref() == Some("4") && r.fields["monsterat"].expr == "4")
        );
        let enc = src(&plan, "gml_GlobalScript_scr_encountersetup");
        assert!(
            enc.contains("    case 2:\n        global.monsterinstancetype[0] = obj_dre_enemy_a;"),
            "{enc}"
        );
        assert!(enc.contains("global.monstermakex[0] = xx + 400;") && enc.contains("global.monstertype[0] = 4;"));
        assert!(enc.contains("global.battlemsg[0] = \"* Blob appears.\";"));
        let step = src(&plan, "gml_Object_obj_dre_enemy_a_Step_0");
        assert!(step.starts_with("event_inherited();\n") && step.contains("instance_create(0, 0, obj_dre_wave_w1);"));
        assert!(plan.notes.iter().any(|n| n.contains("Install the bullet pattern")));
        apply(&st, &plan).unwrap();
        // Same type again is refused.
        let mut p2 = p.clone();
        p2.object = "obj_dre_enemy_b".into();
        p2.enemy_type = Some(4);
        assert!(matches!(enemy(&st, &p2), Err(Error::Conflict(_))));
        p2.enemy_type = None;
        let plan2 = enemy(&st, &p2).unwrap();
        assert!(src(&plan2, "gml_GlobalScript_scr_monstersetup").contains("case 5:"));
    }

    #[test]
    fn room_with_doors() {
        let (_t, st) = setup();
        let p: RoomParams = serde_json::from_value(json!({
            "name": "room_dre_new", "template": "room_a", "width": 640,
            "markers": [{ "letter": "A", "x": 100, "y": 100 }, { "letter": "C", "x": 10, "y": 10 }],
            "doors": [{ "letter": "B", "x": 0, "y": 200, "w": 20, "target": "room_a" }],
            "creation_code": "global.currentsong[0] = 0;",
        }))
        .unwrap();
        let plan = room(&st, &p).unwrap();
        let r = plan.writes.iter().find(|w| w.name == "room_dre_new").unwrap();
        assert!(r.new);
        let d = r.data.as_ref().unwrap();
        assert_eq!(d["Width"], 640);
        assert_eq!(d["Height"], 240);
        assert_eq!(d["CreationCodeEntry"], "gml_RoomCC_room_dre_new_Create");
        assert_eq!(d["Layers"][1]["TileData"], json!([[0, 0], [0, 0]]));
        let names: Vec<&str> = d["GameObjects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["ObjectName"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["obj_markerA", "obj_markerC", "obj_dre_doorB"]);
        assert_eq!(d["Layers"][0]["InstanceIndices"], json!([0, 1, 2]));
        let cc = d["GameObjects"][2]["CreationCodeEntry"].as_str().unwrap();
        assert_eq!(src(&plan, cc), "dre_room = room_a;\ndre_w = 20;\ndre_h = 40;\n");
        assert!(src(&plan, "gml_Object_obj_dre_doorB_Create_0").contains("dre_entrance = 2;"));
        assert!(plan.writes.iter().any(|w| w.name == "obj_markerC" && w.new));
        apply(&st, &plan).unwrap();
        assert!(st.has(ty("rooms"), "room_dre_new"));
        // A second room reuses the door object.
        let p2: RoomParams =
            serde_json::from_value(json!({ "name": "room_dre_2", "doors": [{ "letter": "B", "x": 0, "y": 0, "target": "room_dre_new" }] }))
                .unwrap();
        let plan2 = room(&st, &p2).unwrap();
        assert!(!plan2.planned("objects", "obj_dre_doorB"));
        let bad: RoomParams =
            serde_json::from_value(json!({ "name": "room_x", "doors": [{ "letter": "B", "x": 0, "y": 0, "target": "room_none" }] }))
                .unwrap();
        assert!(room(&st, &bad).is_err());
    }

    #[test]
    fn cutscene_object() {
        let (_t, st) = setup();
        let p: CutsceneParams = serde_json::from_value(json!({
            "object": "obj_dre_cs_a", "flag": 300, "trigger": "touch",
            "steps": [{ "func": "c_sel", "args": ["kr"] }, { "func": "c_msgsetloc", "args": ["0", "\"* Wow./%\"", "\"k1\""] }, { "func": "c_talk_wait" }],
        }))
        .unwrap();
        let plan = cutscene(&st, &p).unwrap();
        let step = src(&plan, "gml_Object_obj_dre_cs_a_Step_0");
        let blocks = cutscene::blocks(step);
        assert_eq!(blocks.len(), 1, "{step}");
        let funcs: Vec<&str> = blocks[0].steps.iter().map(|s| s.func.as_str()).collect();
        assert_eq!(funcs, ["c_sel", "c_msgsetloc", "c_talk_wait", "c_terminatekillactors"]);
        let secs = cutscene::sections(step);
        assert_eq!(
            secs.iter().map(|s| (s.con.as_str(), s.next.clone())).collect::<Vec<_>>(),
            [
                ("0", vec!["1".to_owned()]),
                ("1", vec!["2".to_owned()]),
                ("2", vec!["3".to_owned()])
            ]
        );
        assert!(step.contains("global.flag[300] = 1;"));
        assert!(src(&plan, "gml_Object_obj_dre_cs_a_Create_0").contains("if (global.flag[300] != 0)"));
        let bad: CutsceneParams =
            serde_json::from_value(json!({ "object": "obj_c", "steps": [{ "func": "c_wait", "args": ["1); x = (2"] }] })).unwrap();
        assert!(cutscene(&st, &bad).is_err());
    }
}
