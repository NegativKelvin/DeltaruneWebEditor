//! Bullet patterns and battle box: stored per chapter in `<workspace>/<chapter>/patterns/<name>.json`
//! (not in `mod/`, so the build does not see them), and made into GML for a wave object.
//!
//! The webpage simulator (`web/js/editors/battlesim.js`) uses the same rules as the GML below, step by step:
//! 1. box keyframes, 2. update the bullets of this wave (life, acceleration, turn), 3. emitters fire,
//! 4. GameMaker motion (gravity, then position).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::store::{self, ChapterStore};

/// Prefix of the wave objects that `install` makes.
pub const WAVE_PREFIX: &str = "obj_dre_wave_";
const MAX_FRAMES: u32 = 30 * 60 * 5;
const MAX_EMITTERS: usize = 64;
const MAX_COUNT: u32 = 256;
/// After `duration`, the wave keeps updating its bullets until none are left, for at most this many steps.
const CLEANUP_FRAMES: u32 = 30 * 30;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Pattern {
    /// Wave length in steps (30 per second).
    pub duration: u32,
    /// Object made for each bullet. `obj_collidebullet` is the Deltarune bullet that hurts the soul.
    pub bullet_object: String,
    /// Battle box in screen pixels (640×480), centre point.
    #[serde(rename = "box")]
    pub bbox: BoxKey,
    /// More box keyframes. Between two keys the box changes linearly.
    pub box_keys: Vec<BoxKey>,
    /// Make GML that moves and resizes `obj_growtangle` (the battle box) by the keyframes.
    pub box_code: bool,
    pub soul: Soul,
    pub emitters: Vec<Emitter>,
    pub notes: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BoxKey {
    pub frame: u32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Soul settings for the test only (the game uses its own).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Soul {
    pub speed: f64,
    pub slow: f64,
    pub radius: f64,
    pub graze: f64,
    pub iframes: u32,
    pub hp: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Box centre.
    #[default]
    Box,
    /// Soul centre (at the time of the shot).
    Soul,
    /// Top-left corner of the screen (camera).
    Screen,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Emitter {
    pub name: String,
    pub enabled: bool,
    /// First and last step that can fire, and steps between shots.
    pub start: u32,
    pub end: u32,
    pub interval: u32,
    pub origin: Origin,
    /// Offset from the origin.
    pub x: f64,
    pub y: f64,
    /// Random offset, ± this value.
    pub jitter_x: f64,
    pub jitter_y: f64,
    /// Bullets start this far from the point, in their direction (rings).
    pub radius: f64,
    /// Bullets per shot.
    pub count: u32,
    /// Direction in degrees (0 right, 90 up), spread of the shot, and change per shot.
    pub angle: f64,
    pub spread: f64,
    pub spin: f64,
    /// Add the direction to the soul.
    pub aim: bool,
    /// Random direction change, ± this value.
    pub jitter_angle: f64,
    pub speed: f64,
    /// Speed added for each next bullet in the same shot.
    pub speed_step: f64,
    /// Speed change per step.
    pub accel: f64,
    pub gravity: f64,
    pub gravity_dir: f64,
    /// Direction change per step (curves).
    pub turn: f64,
    /// Steps before the bullet goes away. 0: until it is far off screen.
    pub life: u32,
    /// Sprite of the bullet. Empty: the sprite of the bullet object.
    pub sprite: String,
    pub scale: f64,
    /// Turn the sprite to the direction of motion.
    pub rotate: bool,
    /// Hit radius in the test, and colour when there is no sprite.
    pub size: f64,
    pub color: String,
    /// Sets `damage` on the bullet when more than 0.
    pub damage: i32,
    /// GML run with the new bullet (`with (bullet) { ... }`).
    pub on_spawn: String,
}

impl Default for Pattern {
    fn default() -> Self {
        Self {
            duration: 240,
            bullet_object: "obj_collidebullet".into(),
            bbox: BoxKey::default(),
            box_keys: Vec::new(),
            box_code: false,
            soul: Soul::default(),
            emitters: vec![Emitter::default()],
            notes: String::new(),
        }
    }
}

impl Default for BoxKey {
    fn default() -> Self {
        Self {
            frame: 0,
            x: 320.0,
            y: 170.0,
            w: 150.0,
            h: 150.0,
        }
    }
}

impl Default for Soul {
    fn default() -> Self {
        Self {
            speed: 4.0,
            slow: 2.0,
            radius: 4.0,
            graze: 24.0,
            iframes: 30,
            hp: 100,
        }
    }
}

impl Default for Emitter {
    fn default() -> Self {
        Self {
            name: "ring".into(),
            enabled: true,
            start: 10,
            end: 200,
            interval: 20,
            origin: Origin::Box,
            x: 0.0,
            y: -140.0,
            jitter_x: 0.0,
            jitter_y: 0.0,
            radius: 0.0,
            count: 8,
            angle: 270.0,
            spread: 90.0,
            spin: 7.0,
            aim: false,
            jitter_angle: 0.0,
            speed: 3.0,
            speed_step: 0.0,
            accel: 0.0,
            gravity: 0.0,
            gravity_dir: 270.0,
            turn: 0.0,
            life: 0,
            sprite: String::new(),
            scale: 1.0,
            rotate: false,
            size: 6.0,
            color: "#ffffff".into(),
            damage: 0,
            on_spawn: String::new(),
        }
    }
}

/// GML identifier: letter or `_` first, then letters, digits, `_`.
fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|x| x.is_ascii_alphanumeric() || x == '_') && s.len() <= 120
}

/// Pattern names become part of an object name.
pub fn validate_pattern_name(name: &str) -> Result<()> {
    if !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(())
    } else {
        Err(Error::BadRequest(format!(
            "invalid pattern name '{name}': use 1-64 letters, digits, or _"
        )))
    }
}

impl Pattern {
    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(Error::BadRequest(m));
        if self.duration == 0 || self.duration > MAX_FRAMES {
            return bad(format!("duration must be 1..={MAX_FRAMES}"));
        }
        if !is_ident(&self.bullet_object) {
            return bad(format!("bullet_object '{}' is not a GML name", self.bullet_object));
        }
        if self.emitters.len() > MAX_EMITTERS {
            return bad(format!("at most {MAX_EMITTERS} emitters"));
        }
        for k in std::iter::once(&self.bbox).chain(&self.box_keys) {
            if ![k.x, k.y, k.w, k.h].iter().all(|v| v.is_finite()) || k.w < 0.0 || k.h < 0.0 {
                return bad("box values must be numbers; w and h 0 or more".into());
            }
        }
        for (i, e) in self.emitters.iter().enumerate() {
            let at = |m: &str| Error::BadRequest(format!("emitter {i} ({}): {m}", e.name));
            if e.interval == 0 {
                return Err(at("interval must be 1 or more"));
            }
            if e.count == 0 || e.count > MAX_COUNT {
                return Err(at(&format!("count must be 1..={MAX_COUNT}")));
            }
            if e.end < e.start {
                return Err(at("end is before start"));
            }
            if !e.sprite.is_empty() && !is_ident(&e.sprite) {
                return Err(at("sprite is not a GML name"));
            }
            let nums = [
                e.x,
                e.y,
                e.jitter_x,
                e.jitter_y,
                e.radius,
                e.angle,
                e.spread,
                e.spin,
                e.jitter_angle,
                e.speed,
                e.speed_step,
                e.accel,
                e.gravity,
                e.gravity_dir,
                e.turn,
                e.scale,
                e.size,
            ];
            if !nums.iter().all(|v| v.is_finite()) {
                return Err(at("all numbers must be finite"));
            }
        }
        Ok(())
    }

    /// Box keyframes sorted by frame, the main box first (frame 0).
    pub fn keys(&self) -> Vec<BoxKey> {
        let mut k = vec![BoxKey { frame: 0, ..self.bbox }];
        k.extend(self.box_keys.iter().copied().filter(|x| x.frame > 0));
        k.sort_by_key(|x| x.frame);
        k
    }
}

/// Number as GML text: no trailing `.0`, no exponent.
fn n(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// GML string literal (GMS2 escapes).
fn s(v: &str) -> String {
    let mut o = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => {}
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub struct WaveGml {
    pub object: String,
    pub create_code: String,
    pub step_code: String,
    pub create: String,
    pub step: String,
}

pub fn wave_object(name: &str) -> String {
    format!("{WAVE_PREFIX}{name}")
}

/// Makes the Create and Step GML of the wave object for a pattern.
pub fn to_gml(name: &str, p: &Pattern) -> Result<WaveGml> {
    validate_pattern_name(name)?;
    p.validate()?;
    let object = wave_object(name);
    let head = format!(
        "// Made by Deltarune Web Editor from bullet pattern {}. Edit the pattern, not this code: install writes it again.\n",
        s(name)
    );
    let mut create = head.clone();
    create.push_str("dre_t = 0;\n");

    let mut o = head;
    let b = &p.bbox;
    let bo = &p.bullet_object;
    let _ = write!(
        o,
        "var dre_cx = camerax();\nvar dre_cy = cameray();\n\
         var dre_bx = dre_cx + {bx};\nvar dre_by = dre_cy + {by};\n",
        bx = n(b.x),
        by = n(b.y)
    );
    if p.box_code {
        let keys = p.keys();
        let k0 = keys[0];
        let _ = writeln!(
            o,
            "// Battle box keyframes\nvar dre_kx = {}, dre_ky = {}, dre_kw = {}, dre_kh = {};",
            n(k0.x),
            n(k0.y),
            n(k0.w),
            n(k0.h)
        );
        for w in keys.windows(2) {
            let (a, c) = (w[0], w[1]);
            let _ = writeln!(
                o,
                "if (dre_t >= {f1} && dre_t < {f2})\n{{\n    var dre_k = (dre_t - {f1}) / {d};\n    \
                 dre_kx = lerp({}, {}, dre_k);\n    dre_ky = lerp({}, {}, dre_k);\n    \
                 dre_kw = lerp({}, {}, dre_k);\n    dre_kh = lerp({}, {}, dre_k);\n}}",
                n(a.x),
                n(c.x),
                n(a.y),
                n(c.y),
                n(a.w),
                n(c.w),
                n(a.h),
                n(c.h),
                f1 = a.frame,
                f2 = c.frame,
                d = c.frame - a.frame,
            );
        }
        if keys.len() > 1 {
            let l = keys[keys.len() - 1];
            let _ = writeln!(
                o,
                "if (dre_t >= {})\n{{\n    dre_kx = {};\n    dre_ky = {};\n    dre_kw = {};\n    dre_kh = {};\n}}",
                l.frame,
                n(l.x),
                n(l.y),
                n(l.w),
                n(l.h)
            );
        }
        let _ = writeln!(o, "if (dre_t <= {} && instance_exists(obj_growtangle))", p.duration);
        o.push_str(
            "{\n    with (obj_growtangle)\n    {\n        \
             x = dre_cx + dre_kx;\n        y = dre_cy + dre_ky;\n        \
             image_xscale = dre_kw / max(1, sprite_get_width(sprite_index));\n        \
             image_yscale = dre_kh / max(1, sprite_get_height(sprite_index));\n    }\n}\n\
             dre_bx = dre_cx + dre_kx;\ndre_by = dre_cy + dre_ky;\n",
        );
    } else {
        o.push_str("if (instance_exists(obj_growtangle))\n{\n    dre_bx = obj_growtangle.x;\n    dre_by = obj_growtangle.y;\n}\n");
    }
    o.push_str(
        "var dre_sx = dre_bx;\nvar dre_sy = dre_by;\nif (instance_exists(obj_heart))\n{\n    \
         dre_sx = obj_heart.x + (obj_heart.sprite_width / 2);\n    dre_sy = obj_heart.y + (obj_heart.sprite_height / 2);\n}\n",
    );
    let _ = write!(
        o,
        "// Bullets of this wave: life, speed change, turn. Far off screen: destroy.\n\
         var dre_left = 0;\n\
         with ({bo})\n{{\n    if (variable_instance_exists(id, \"dre_wave\") && dre_wave == other.id)\n    {{\n        \
         if (dre_life > 0)\n            dre_life -= 1;\n        \
         if (dre_life == 0 || x < (dre_cx - 320) || x > (dre_cx + 960) || y < (dre_cy - 320) || y > (dre_cy + 800))\n        {{\n            \
         instance_destroy();\n        }}\n        else\n        {{\n            \
         speed += dre_acc;\n            direction += dre_turn;\n            \
         if (dre_rot)\n                image_angle = direction;\n            \
         dre_left += 1;\n        }}\n    }}\n}}\n"
    );
    for (i, e) in p.emitters.iter().enumerate().filter(|(_, e)| e.enabled) {
        emitter_gml(&mut o, i, e, bo, p.duration);
    }
    // After the wave time, no more shots, but the bullets that are left keep their life, speed change, and turn.
    let _ = write!(
        o,
        "dre_t += 1;\nif (dre_t > {} && (dre_left == 0 || dre_t > {}))\n    instance_destroy();\n",
        p.duration,
        p.duration + CLEANUP_FRAMES
    );

    Ok(WaveGml {
        create_code: format!("gml_Object_{object}_Create_0"),
        step_code: format!("gml_Object_{object}_Step_0"),
        object,
        create,
        step: o,
    })
}

fn emitter_gml(o: &mut String, i: usize, e: &Emitter, bo: &str, duration: u32) {
    let ind = "    ";
    let _ = writeln!(o, "// Emitter {i}: {}", e.name.replace(['\n', '\r'], " "));
    let _ = writeln!(
        o,
        "if (dre_t >= {s} && dre_t <= {end} && ((dre_t - {s}) mod {iv}) == 0)\n{{",
        s = e.start,
        end = e.end.min(duration),
        iv = e.interval
    );
    let _ = writeln!(o, "{ind}var dre_n = (dre_t - {}) div {};", e.start, e.interval);
    let (ox, oy) = match e.origin {
        Origin::Box => ("dre_bx", "dre_by"),
        Origin::Soul => ("dre_sx", "dre_sy"),
        Origin::Screen => ("dre_cx", "dre_cy"),
    };
    let _ = writeln!(o, "{ind}var dre_ox = {ox} + {};\n{ind}var dre_oy = {oy} + {};", n(e.x), n(e.y));
    let _ = writeln!(o, "{ind}var dre_base = {} + ({} * dre_n);", n(e.angle), n(e.spin));
    if e.aim {
        let _ = writeln!(o, "{ind}dre_base += point_direction(dre_ox, dre_oy, dre_sx, dre_sy);");
    }
    let _ = writeln!(o, "{ind}for (var dre_i = 0; dre_i < {}; dre_i += 1)\n{ind}{{", e.count);
    let i2 = "        ";
    let off = if e.count <= 1 {
        "0".to_owned()
    } else if e.spread.abs() >= 360.0 {
        format!("({} * dre_i / {})", n(e.spread), e.count)
    } else {
        format!("({} + ({} * dre_i / {}))", n(-e.spread / 2.0), n(e.spread), e.count - 1)
    };
    let mut a = format!("dre_base + {off}");
    if e.jitter_angle != 0.0 {
        let _ = write!(a, " + random_range({}, {})", n(-e.jitter_angle), n(e.jitter_angle));
    }
    let _ = writeln!(o, "{i2}var dre_a = {a};");
    let jx = if e.jitter_x != 0.0 {
        format!(" + random_range({}, {})", n(-e.jitter_x), n(e.jitter_x))
    } else {
        String::new()
    };
    let jy = if e.jitter_y != 0.0 {
        format!(" + random_range({}, {})", n(-e.jitter_y), n(e.jitter_y))
    } else {
        String::new()
    };
    let _ = writeln!(
        o,
        "{i2}var dre_px = dre_ox + lengthdir_x({r}, dre_a){jx};\n{i2}var dre_py = dre_oy + lengthdir_y({r}, dre_a){jy};",
        r = n(e.radius)
    );
    let _ = writeln!(o, "{i2}var dre_b = instance_create(dre_px, dre_py, {bo});");
    let life = if e.life == 0 { "-1".to_owned() } else { e.life.to_string() };
    let _ = writeln!(
        o,
        "{i2}dre_b.dre_wave = id;\n{i2}dre_b.dre_life = {life};\n{i2}dre_b.dre_acc = {};\n{i2}dre_b.dre_turn = {};\n{i2}dre_b.dre_rot = {};",
        n(e.accel),
        n(e.turn),
        if e.rotate { 1 } else { 0 }
    );
    let sp = if e.speed_step != 0.0 {
        format!("{} + ({} * dre_i)", n(e.speed), n(e.speed_step))
    } else {
        n(e.speed)
    };
    let _ = writeln!(o, "{i2}dre_b.direction = dre_a;\n{i2}dre_b.speed = {sp};");
    if e.gravity != 0.0 {
        let _ = writeln!(
            o,
            "{i2}dre_b.gravity = {};\n{i2}dre_b.gravity_direction = {};",
            n(e.gravity),
            n(e.gravity_dir)
        );
    }
    if !e.sprite.is_empty() {
        let _ = writeln!(o, "{i2}dre_b.sprite_index = {};", e.sprite);
    }
    if e.scale != 1.0 {
        let _ = writeln!(
            o,
            "{i2}dre_b.image_xscale = {};\n{i2}dre_b.image_yscale = {};",
            n(e.scale),
            n(e.scale)
        );
    }
    if e.rotate {
        let _ = writeln!(o, "{i2}dre_b.image_angle = dre_a;");
    }
    if e.damage > 0 {
        let _ = writeln!(o, "{i2}dre_b.damage = {};", e.damage);
    }
    if !e.on_spawn.trim().is_empty() {
        let _ = writeln!(o, "{i2}with (dre_b)\n{i2}{{");
        for line in e.on_spawn.lines() {
            let _ = writeln!(o, "{i2}    {line}");
        }
        let _ = writeln!(o, "{i2}}}");
    }
    let _ = writeln!(o, "{ind}}}\n}}");
}

/// Pattern files of one chapter.
pub struct PatternStore {
    dir: PathBuf,
}

impl PatternStore {
    pub fn new(store: &ChapterStore) -> Self {
        Self {
            dir: store.root.join("patterns"),
        }
    }

    fn path(&self, name: &str) -> Result<PathBuf> {
        validate_pattern_name(name)?;
        Ok(self.dir.join(format!("{name}.json")))
    }

    pub fn list(&self) -> Vec<Value> {
        let mut out: Vec<Value> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let name = p.file_stem()?.to_str()?.to_owned();
                (p.extension()? == "json" && validate_pattern_name(&name).is_ok()).then_some(())?;
                let pat = read_pattern(&p).ok()?;
                Some(json!({ "name": name, "duration": pat.duration, "emitters": pat.emitters.len(), "object": wave_object(&name) }))
            })
            .collect();
        out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        out
    }

    pub fn read(&self, name: &str) -> Result<Pattern> {
        let p = self.path(name)?;
        if !p.is_file() {
            return Err(Error::NotFound(format!("pattern '{name}'")));
        }
        read_pattern(&p)
    }

    pub fn write(&self, name: &str, pat: &Pattern) -> Result<()> {
        pat.validate()?;
        let p = self.path(name)?;
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(p, serde_json::to_vec_pretty(pat)?)?;
        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        let p = self.path(name)?;
        if !p.is_file() {
            return Err(Error::NotFound(format!("pattern '{name}'")));
        }
        std::fs::remove_file(p)?;
        Ok(())
    }
}

fn read_pattern(p: &Path) -> Result<Pattern> {
    Ok(serde_json::from_slice(&std::fs::read(p)?)?)
}

/// Writes the wave object (Create and Step events) and its two code entries into `mod/`.
/// Keeps other fields of an object that is already there.
pub fn install(store: &ChapterStore, name: &str, pat: &Pattern) -> Result<Value> {
    let g = to_gml(name, pat)?;
    let code = store::asset_type("code")?;
    let objects = store::asset_type("objects")?;
    store.write(code, &g.create_code, None, Some(g.create.clone()))?;
    store.write(code, &g.step_code, None, Some(g.step.clone()))?;
    let mut data = store
        .read(objects, &g.object)
        .ok()
        .and_then(|v| v.get("data").cloned())
        .unwrap_or_else(new_object);
    let events = json!([
        { "Category": "Create", "Subtype": "", "CodeEntry": g.create_code },
        { "Category": "Step", "Subtype": "Step", "CodeEntry": g.step_code },
    ]);
    let obj = data
        .as_object_mut()
        .ok_or_else(|| Error::Other(format!("{} is not a JSON object", g.object)))?;
    // Keep events of other kinds that a user added.
    let ours = |e: &Value| {
        let cat = e.get("Category").and_then(Value::as_str);
        cat == Some("Create") || (cat == Some("Step") && e.get("Subtype").and_then(Value::as_str) == Some("Step"))
    };
    let mut all: Vec<Value> = events.as_array().unwrap().clone();
    if let Some(old) = obj.get("Events").and_then(Value::as_array) {
        all.extend(old.iter().filter(|e| !ours(e)).cloned());
    }
    obj.insert("Events".into(), Value::Array(all));
    store.write(objects, &g.object, Some(data), None)?;
    Ok(json!({ "object": g.object, "code": [g.create_code, g.step_code] }))
}

/// A new object with every field that UTMT import reads (it fails on a missing `PhysicsVertices`).
/// Physics values are the GameMaker defaults.
fn new_object() -> Value {
    json!({
        "$type": "GameObject",
        "Sprite": null,
        "Visible": false,
        "Solid": false,
        "Depth": 0,
        "Persistent": false,
        "ParentObject": null,
        "MaskSprite": null,
        "UsesPhysics": false,
        "IsSensor": false,
        "CollisionShape": "Box",
        "Density": 0.5,
        "Restitution": 0.1,
        "Group": 0,
        "LinearDamping": 0.1,
        "AngularDamping": 0.1,
        "Friction": 0.2,
        "Awake": true,
        "Kinematic": false,
        "PhysicsVertices": [],
        "Events": [],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_and_partial_json() {
        let p: Pattern = serde_json::from_str(r#"{"duration": 90, "emitters": [{"count": 3}]}"#).unwrap();
        assert_eq!(p.duration, 90);
        assert_eq!(p.emitters[0].count, 3);
        assert_eq!(p.emitters[0].interval, 20);
        assert_eq!(p.bullet_object, "obj_collidebullet");
        let back: Pattern = serde_json::from_value(serde_json::to_value(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        assert!(serde_json::to_value(&p).unwrap().get("box").is_some());
    }

    /// Same keys at every level (numbers may be written as 320 or 320.0).
    fn same_shape(a: &Value, b: &Value, at: &str) {
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => {
                let kx: Vec<_> = x.keys().collect();
                let ky: Vec<_> = y.keys().collect();
                assert_eq!(kx, ky, "keys at {at}");
                for (k, v) in x {
                    same_shape(v, &y[k], &format!("{at}.{k}"));
                }
            }
            (Value::Array(x), Value::Array(y)) => {
                assert_eq!(x.len(), y.len(), "length at {at}");
                for (i, (u, v)) in x.iter().zip(y).enumerate() {
                    same_shape(u, v, &format!("{at}[{i}]"));
                }
            }
            (Value::Number(x), Value::Number(y)) => assert_eq!(x.as_f64(), y.as_f64(), "value at {at}"),
            _ => assert_eq!(a, b, "value at {at}"),
        }
    }

    /// The webpage simulator has the same defaults (web/js/editors/battlesim.js writes this file; its test checks it).
    #[test]
    fn defaults_match_webpage() {
        let fixture: Value = serde_json::from_str(include_str!("../tests/fixtures/default_pattern.json")).unwrap();
        same_shape(&serde_json::to_value(Pattern::default()).unwrap(), &fixture, "pattern");
    }

    #[test]
    fn validation() {
        let mut p = Pattern::default();
        assert!(p.validate().is_ok());
        p.emitters[0].interval = 0;
        assert!(p.validate().is_err());
        p.emitters[0].interval = 1;
        p.emitters[0].sprite = "spr x".into();
        assert!(p.validate().is_err());
        p.emitters[0].sprite = String::new();
        p.bullet_object = "obj;evil()".into();
        assert!(p.validate().is_err());
        assert!(validate_pattern_name("a/b").is_err());
        assert!(validate_pattern_name("ring_1").is_ok());
    }

    #[test]
    fn gml_shape() {
        let mut p = Pattern::default();
        p.emitters[0].aim = true;
        p.emitters[0].gravity = 0.25;
        p.emitters[0].sprite = "spr_bullet".into();
        p.emitters[0].on_spawn = "image_blend = c_red;".into();
        p.emitters.push(Emitter {
            enabled: false,
            name: "off".into(),
            ..Emitter::default()
        });
        let g = to_gml("ring", &p).unwrap();
        assert_eq!(g.object, "obj_dre_wave_ring");
        assert_eq!(g.step_code, "gml_Object_obj_dre_wave_ring_Step_0");
        assert!(g.create.contains("dre_t = 0;"));
        let st = &g.step;
        assert!(st.contains("if (dre_t >= 10 && dre_t <= 200 && ((dre_t - 10) mod 20) == 0)"));
        assert!(st.contains("var dre_base = 270 + (7 * dre_n);"));
        assert!(st.contains("dre_base += point_direction(dre_ox, dre_oy, dre_sx, dre_sy);"));
        assert!(st.contains("var dre_a = dre_base + (-45 + (90 * dre_i / 7));"));
        assert!(st.contains("instance_create(dre_px, dre_py, obj_collidebullet);"));
        assert!(st.contains("dre_b.gravity = 0.25;"));
        assert!(st.contains("dre_b.sprite_index = spr_bullet;"));
        assert!(st.contains("        with (dre_b)\n        {\n            image_blend = c_red;\n        }"));
        assert!(!st.contains("Emitter 1"));
        assert!(st.ends_with("dre_t += 1;\nif (dre_t > 240 && (dre_left == 0 || dre_t > 1140))\n    instance_destroy();\n"));
        assert!(st.contains("var dre_left = 0;"));
        assert_eq!(st.matches('{').count(), st.matches('}').count());
        assert!(!st.contains("obj_growtangle)\n    {\n        x ="));
    }

    #[test]
    fn gml_box_keys_and_numbers() {
        let mut p = Pattern {
            box_code: true,
            ..Pattern::default()
        };
        p.box_keys.push(BoxKey {
            frame: 60,
            x: 320.0,
            y: 170.0,
            w: 300.0,
            h: 75.5,
        });
        p.emitters[0].spread = 360.0;
        let g = to_gml("b", &p).unwrap();
        assert!(g.step.contains("if (dre_t >= 0 && dre_t < 60)"));
        assert!(g.step.contains("dre_kh = lerp(150, 75.5, dre_k);"));
        assert!(g.step.contains("if (dre_t >= 60)\n{\n    dre_kx = 320;"));
        assert!(g.step.contains("(360 * dre_i / 8)"));
        assert!(
            g.step
                .contains("if (dre_t <= 240 && instance_exists(obj_growtangle))\n{\n    with (obj_growtangle)")
        );
        assert_eq!(g.step.matches('{').count(), g.step.matches('}').count());
        assert_eq!(n(0.1 + 0.2), "0.3");
        assert_eq!(n(-3.0), "-3");
        assert_eq!(s("a\"b\\"), "\"a\\\"b\\\\\"");
    }

    /// After the wave time: no shots, no box moves, the wave stays until its bullets are gone.
    #[test]
    fn gml_after_duration() {
        let mut p = Pattern {
            duration: 100,
            ..Pattern::default()
        };
        p.emitters[0].end = 500;
        p.emitters[0].name = "a\r\nb".into();
        let g = to_gml("w", &p).unwrap();
        assert!(g.step.contains("dre_t >= 10 && dre_t <= 100 &&"));
        assert!(g.step.contains("// Emitter 0: a  b\n"));
        assert!(g.step.contains("            dre_left += 1;\n"));
        assert!(
            g.step
                .ends_with("if (dre_t > 100 && (dre_left == 0 || dre_t > 1000))\n    instance_destroy();\n")
        );
    }

    #[test]
    fn store_and_install() {
        let tmp = tempfile::tempdir().unwrap();
        let cs = ChapterStore::new(tmp.path(), "chapter1");
        std::fs::create_dir_all(cs.base().join("objects")).unwrap();
        std::fs::write(cs.base().join("project.json"), r#"{"Name":"D"}"#).unwrap();
        let ps = PatternStore::new(&cs);
        assert!(ps.list().is_empty());
        ps.write("ring", &Pattern::default()).unwrap();
        assert_eq!(ps.list()[0]["name"], "ring");
        assert_eq!(ps.read("ring").unwrap(), Pattern::default());
        let r = install(&cs, "ring", &Pattern::default()).unwrap();
        assert_eq!(r["object"], "obj_dre_wave_ring");
        let obj: Value = serde_json::from_slice(&std::fs::read(cs.mod_dir().join("objects/obj_dre_wave_ring.json")).unwrap()).unwrap();
        assert_eq!(obj["$type"], "GameObject");
        assert_eq!(obj["PhysicsVertices"], json!([]));
        assert_eq!(obj["Events"].as_array().unwrap().len(), 2);
        assert!(cs.mod_dir().join("code/gml_Object_obj_dre_wave_ring_Step_0.gml").is_file());
        // Install again: no doubled events.
        install(&cs, "ring", &Pattern::default()).unwrap();
        let obj: Value = serde_json::from_slice(&std::fs::read(cs.mod_dir().join("objects/obj_dre_wave_ring.json")).unwrap()).unwrap();
        assert_eq!(obj["Events"].as_array().unwrap().len(), 2);
        ps.delete("ring").unwrap();
        assert!(ps.read("ring").is_err());
    }
}
