//! Hot reload: send changes into the running game without a rebuild.
//!
//! GameMaker cannot load new code at run time (there is no `eval`), so code, objects, and room layouts
//! still need a build. What can change at run time is data: global variables and flags, instance
//! variables, the current room, and sprite images (`sprite_replace` loads a PNG strip from disk).
//!
//! How it works: `install` adds a persistent object `obj_dre_live` to the mod and one line to a host
//! code entry that runs every step (`obj_time` Step in Deltarune), which makes the object. After one
//! build, the object reads `dre_live/cmd.txt` in the save folder 3 times per second, runs new
//! commands, and writes `dre_live/ack.txt` (sequence, room, plot) once per second.
//!
//! `cmd.txt`: first line is the sequence; then each command is 6 lines: op and 5 arguments.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::patterns;
use crate::store::{self, ChapterStore, validate_name};

pub const OBJECT: &str = "obj_dre_live";
const CREATE_CODE: &str = "gml_Object_obj_dre_live_Create_0";
const STEP_CODE: &str = "gml_Object_obj_dre_live_Step_0";
/// Host code entries to try, in order, when the request names none.
pub const DEFAULT_HOSTS: &[&str] = &["gml_Object_obj_time_Step_0", "gml_Object_obj_time_Create_0"];
/// Put at the start of the host code (an `exit` later in the host does not skip it).
pub const HOOK: &str = "// dre-live: make the hot reload object\nvar dre_live_o = asset_get_index(\"obj_dre_live\");\nif (dre_live_o >= 0 && !instance_exists(dre_live_o))\n    instance_create_depth(0, 0, 0, dre_live_o);\n";
/// Folder in the save folder.
pub const DIR: &str = "dre_live";
const ARGS: usize = 5;
/// The game wrote `ack.txt` this recently: it runs with the hook.
const CONNECTED: Duration = Duration::from_secs(5);
const MAX_PENDING: usize = 500;

pub const CREATE_GML: &str = r#"dre_live_t = 0;
dre_live_beat = 3;
dre_live_done = 0;
dre_live_seq = "";
if (!directory_exists("dre_live"))
    directory_create("dre_live");
// Commands from an earlier session do not run again.
if (file_exists("dre_live/cmd.txt"))
{
    var f = file_text_open_read("dre_live/cmd.txt");
    dre_live_seq = file_text_read_string(f);
    file_text_close(f);
}
"#;

pub const STEP_GML: &str = r#"dre_live_t += 1;
if (dre_live_t < 10)
    exit;
dre_live_t = 0;
dre_live_beat += 1;
var d = "dre_live/";
if (file_exists(d + "cmd.txt"))
{
    var f = file_text_open_read(d + "cmd.txt");
    var seq = file_text_read_string(f);
    file_text_readln(f);
    if (seq != dre_live_seq)
    {
        dre_live_seq = seq;
        dre_live_beat = 3;
        while (!file_text_eof(f))
        {
            var op = file_text_read_string(f);
            file_text_readln(f);
            var a0 = file_text_read_string(f);
            file_text_readln(f);
            var a1 = file_text_read_string(f);
            file_text_readln(f);
            var a2 = file_text_read_string(f);
            file_text_readln(f);
            var a3 = file_text_read_string(f);
            file_text_readln(f);
            var a4 = file_text_read_string(f);
            file_text_readln(f);
            if (op == "flag")
                global.flag[real(a0)] = real(a1);
            else if (op == "flagstr")
                global.flag[real(a0)] = a1;
            else if (op == "global")
                variable_global_set(a0, real(a1));
            else if (op == "globalstr")
                variable_global_set(a0, a1);
            else if (op == "goto")
            {
                var r = asset_get_index(a0);
                if (r >= 0 && room_exists(r))
                    room_goto(r);
            }
            else if (op == "restart")
                room_restart();
            else if (op == "var")
            {
                var o = asset_get_index(a0);
                if (o >= 0)
                {
                    with (o)
                        variable_instance_set(id, a1, real(a2));
                }
            }
            else if (op == "sprite")
            {
                var s = asset_get_index(a0);
                if (s >= 0 && sprite_exists(s))
                    sprite_replace(s, d + a1, real(a2), false, false, real(a3), real(a4));
            }
            else if (op == "dumpflags" && variable_global_exists("flag"))
            {
                var w1 = file_text_open_write(d + "flags.txt");
                // array_length_1d: known to every GameMaker version (array_length only to 2.3+).
                var n = array_length_1d(global.flag);
                for (var i = 0; i < n; i++)
                {
                    file_text_write_string(w1, string(global.flag[i]));
                    file_text_writeln(w1);
                }
                file_text_close(w1);
            }
            if (op != "")
                dre_live_done += 1;
        }
    }
    file_text_close(f);
}
if (dre_live_beat >= 3)
{
    dre_live_beat = 0;
    var plot = "";
    if (variable_global_exists("plot"))
        plot = string(global.plot);
    var w2 = file_text_open_write(d + "ack.txt");
    file_text_write_string(w2, dre_live_seq);
    file_text_writeln(w2);
    file_text_write_string(w2, room_get_name(room));
    file_text_writeln(w2);
    file_text_write_string(w2, plot);
    file_text_writeln(w2);
    file_text_write_string(w2, string(dre_live_done));
    file_text_writeln(w2);
    file_text_write_string(w2, game_display_name);
    file_text_writeln(w2);
    file_text_close(w2);
}
"#;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Command {
    pub op: String,
    #[serde(default)]
    pub args: Vec<String>,
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A number that GML `real()` reads (it stops the game on bad text).
fn is_number(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (a, b) = s.split_once('.').unwrap_or((s, "0"));
    !a.is_empty() && !b.is_empty() && s.len() <= 30 && a.chars().chain(b.chars()).all(|c| c.is_ascii_digit())
}

fn is_index(s: &str) -> bool {
    !s.is_empty() && s.len() <= 6 && s.chars().all(|c| c.is_ascii_digit())
}

impl Command {
    /// Checks the arguments of each op, so the game never reads a bad value.
    pub fn validate(&self) -> Result<()> {
        let a = |i: usize| self.args.get(i).map(String::as_str).unwrap_or("");
        let text = |i: usize| !a(i).contains(['\n', '\r']) && a(i).len() <= 1000;
        let ok = match self.op.as_str() {
            "flag" => is_index(a(0)) && is_number(a(1)),
            "flagstr" => is_index(a(0)) && text(1),
            "global" => is_ident(a(0)) && is_number(a(1)),
            "globalstr" => is_ident(a(0)) && text(1),
            "goto" => is_ident(a(0)),
            "restart" | "dumpflags" => true,
            "var" => is_ident(a(0)) && is_ident(a(1)) && is_number(a(2)),
            "sprite" => {
                is_ident(a(0))
                    && validate_name(a(1)).is_ok()
                    && a(1).ends_with(".png")
                    && is_index(a(2))
                    && a(2) != "0"
                    && is_number(a(3))
                    && is_number(a(4))
            }
            _ => return Err(Error::BadRequest(format!("unknown live command '{}'", self.op))),
        };
        if !ok || self.args.len() > ARGS {
            return Err(Error::BadRequest(format!(
                "bad arguments for live command '{}': {:?}",
                self.op, self.args
            )));
        }
        Ok(())
    }

    fn lines(&self, out: &mut String) {
        out.push_str(&self.op);
        out.push('\n');
        for i in 0..ARGS {
            out.push_str(self.args.get(i).map(String::as_str).unwrap_or(""));
            out.push('\n');
        }
    }
}

/// What the game wrote last.
#[derive(Debug, Clone, Serialize)]
pub struct Ack {
    pub seq: String,
    pub room: String,
    pub plot: String,
    pub done: String,
    pub game: String,
    /// Milliseconds since the game wrote it.
    pub age_ms: u64,
}

pub fn read_ack(dir: &Path) -> Option<Ack> {
    let path = dir.join("ack.txt");
    let age = std::fs::metadata(&path).ok()?.modified().ok()?.elapsed().unwrap_or_default();
    let text = std::fs::read_to_string(&path).ok()?;
    let mut l = text.lines().map(|x| x.trim_end_matches('\r').to_owned());
    let mut next = || l.next().unwrap_or_default();
    Some(Ack {
        seq: next(),
        room: next(),
        plot: next(),
        done: next(),
        game: next(),
        age_ms: age.as_millis() as u64,
    })
}

pub fn connected(ack: Option<&Ack>) -> bool {
    ack.is_some_and(|a| a.age_ms < CONNECTED.as_millis() as u64)
}

/// Commands that the game did not run yet are sent again with the next batch.
#[derive(Debug, Default)]
pub struct Queue {
    pub seq: u64,
    pub pending: Vec<Command>,
}

impl Queue {
    /// Writes the commands (and the ones not acknowledged yet) to `cmd.txt`. Returns the new sequence.
    pub fn send(&mut self, dir: &Path, cmds: Vec<Command>) -> Result<u64> {
        for c in &cmds {
            c.validate()?;
        }
        if read_ack(dir).is_some_and(|a| a.seq == self.seq.to_string()) {
            self.pending.clear();
        }
        self.pending.extend(cmds);
        if self.pending.len() > MAX_PENDING {
            let extra = self.pending.len() - MAX_PENDING;
            self.pending.drain(..extra);
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let seq = now.max(self.seq + 1);
        let mut text = format!("{seq}\n");
        for c in &self.pending {
            c.lines(&mut text);
        }
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("cmd.tmp");
        std::fs::write(&tmp, text)?;
        // The game can hold the file open for a moment (Windows): try again.
        let mut last = None;
        for _ in 0..10 {
            match std::fs::rename(&tmp, dir.join("cmd.txt")) {
                Ok(()) => {
                    self.seq = seq;
                    return Ok(seq);
                }
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        Err(Error::Conflict(format!(
            "cannot write cmd.txt (the game holds it?): {}",
            last.map(|e| e.to_string()).unwrap_or_default()
        )))
    }
}

/// Saved install state: `<workspace>/<chapter>/live.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Installed {
    pub host: String,
}

fn state_path(store: &ChapterStore) -> PathBuf {
    store.root.join("live.json")
}

pub fn installed(store: &ChapterStore) -> Option<Installed> {
    serde_json::from_slice(&std::fs::read(state_path(store)).ok()?).ok()
}

/// Install state of a chapter: host, and whether the object and the hook are in the mod.
pub fn status(store: &ChapterStore) -> Value {
    let code = store::asset_type("code").unwrap();
    let objects = store::asset_type("objects").unwrap();
    let inst = installed(store);
    let hooked = inst.as_ref().is_some_and(|i| {
        store
            .read(code, &i.host)
            .ok()
            .and_then(|v| v["source"].as_str().map(|s| s.contains(HOOK)))
            .unwrap_or(false)
    });
    let object = store.list(objects).iter().any(|e| e.name == OBJECT && e.modified);
    let hosts: Vec<&str> = DEFAULT_HOSTS.iter().copied().filter(|h| store.read(code, h).is_ok()).collect();
    json!({
        "installed": hooked && object,
        "host": inst.map(|i| i.host),
        "object": object,
        "hooked": hooked,
        "default_hosts": hosts,
    })
}

/// Writes `obj_dre_live` and its code into the mod and puts the hook into the host code.
pub fn install(store: &ChapterStore, host: Option<&str>) -> Result<Value> {
    let code = store::asset_type("code")?;
    let objects = store::asset_type("objects")?;
    let host = match host.filter(|h| !h.is_empty()) {
        Some(h) => {
            validate_name(h)?;
            if store.read(code, h).is_err() {
                return Err(Error::NotFound(format!("code '{h}'")));
            }
            h.to_owned()
        }
        None => DEFAULT_HOSTS
            .iter()
            .find(|h| store.read(code, h).is_ok())
            .map(|h| (*h).to_owned())
            .ok_or_else(|| {
                Error::Conflict(format!(
                    "no host code found ({}); name a code entry that runs every step",
                    DEFAULT_HOSTS.join(", ")
                ))
            })?,
    };
    // Move the hook when the host changes.
    if let Some(old) = installed(store).filter(|o| o.host != host) {
        unhook(store, &old.host)?;
    }
    store.write(code, CREATE_CODE, None, Some(CREATE_GML.into()))?;
    store.write(code, STEP_CODE, None, Some(STEP_GML.into()))?;
    let mut data = store
        .read(objects, OBJECT)
        .ok()
        .and_then(|v| v.get("data").cloned())
        .unwrap_or_else(patterns::new_object);
    data["Persistent"] = json!(true);
    data["Visible"] = json!(false);
    data["Events"] = json!([
        { "Category": "Create", "Subtype": "", "CodeEntry": CREATE_CODE },
        { "Category": "Step", "Subtype": "Step", "CodeEntry": STEP_CODE },
    ]);
    store.write(objects, OBJECT, Some(data), None)?;
    let src = store.read(code, &host)?["source"].as_str().unwrap_or_default().to_owned();
    if !src.contains(HOOK) {
        store.write(code, &host, None, Some(format!("{HOOK}{src}")))?;
    }
    std::fs::write(state_path(store), serde_json::to_vec_pretty(&Installed { host: host.clone() })?)?;
    Ok(json!({ "object": OBJECT, "host": host, "code": [CREATE_CODE, STEP_CODE] }))
}

/// Takes the hook out of a host code entry. The entry is reverted when nothing else changed in it.
fn unhook(store: &ChapterStore, host: &str) -> Result<()> {
    let code = store::asset_type("code")?;
    let Ok(cur) = store.read(code, host) else { return Ok(()) };
    let src = cur["source"].as_str().unwrap_or_default();
    if !src.contains(HOOK) {
        return Ok(());
    }
    let new = src.replacen(HOOK, "", 1);
    let base = std::fs::read_to_string(store.base().join("code").join(format!("{host}.gml"))).ok();
    if base.as_deref() == Some(new.as_str()) {
        store.revert(code, host)
    } else {
        store.write(code, host, None, Some(new)).map(|_| ())
    }
}

/// Removes what `install` wrote. Returns the names that were removed or changed.
pub fn uninstall(store: &ChapterStore) -> Result<Vec<String>> {
    let code = store::asset_type("code")?;
    let objects = store::asset_type("objects")?;
    let mut done = Vec::new();
    if let Some(i) = installed(store) {
        unhook(store, &i.host)?;
        done.push(i.host);
    }
    for c in [CREATE_CODE, STEP_CODE] {
        if store.list(code).iter().any(|e| e.name == c && e.modified) {
            store.revert(code, c)?;
            done.push(c.to_owned());
        }
    }
    if store.list(objects).iter().any(|e| e.name == OBJECT && e.modified) {
        store.revert(objects, OBJECT)?;
        done.push(OBJECT.to_owned());
    }
    let _ = std::fs::remove_file(state_path(store));
    Ok(done)
}

/// Values from the last `dumpflags` command.
pub fn read_flags(dir: &Path) -> Option<(Vec<String>, u64)> {
    let path = dir.join("flags.txt");
    let age = std::fs::metadata(&path).ok()?.modified().ok()?.elapsed().unwrap_or_default();
    let text = std::fs::read_to_string(path).ok()?;
    Some((
        text.lines().map(|l| l.trim_end_matches('\r').to_owned()).collect(),
        age.as_millis() as u64,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(op: &str, args: &[&str]) -> Command {
        Command {
            op: op.into(),
            args: args.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn validation() {
        assert!(cmd("flag", &["12", "1"]).validate().is_ok());
        assert!(cmd("flag", &["12", "-0.5"]).validate().is_ok());
        assert!(cmd("flag", &["12", "abc"]).validate().is_err());
        assert!(cmd("flag", &["x", "1"]).validate().is_err());
        assert!(cmd("flagstr", &["1", "a\nb"]).validate().is_err());
        assert!(cmd("global", &["plot", "40"]).validate().is_ok());
        assert!(cmd("global", &["plot; x", "40"]).validate().is_err());
        assert!(cmd("goto", &["room_town"]).validate().is_ok());
        assert!(cmd("sprite", &["spr_kris", "spr_kris.png", "4", "10", "20"]).validate().is_ok());
        assert!(cmd("sprite", &["spr_kris", "../x.png", "4", "0", "0"]).validate().is_err());
        assert!(cmd("sprite", &["spr_kris", "a.png", "0", "0", "0"]).validate().is_err());
        assert!(cmd("rm", &[]).validate().is_err());
        assert!(cmd("restart", &[]).validate().is_ok());
    }

    #[test]
    fn queue_resends_until_ack() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(DIR);
        let mut q = Queue::default();
        let s1 = q.send(&dir, vec![cmd("flag", &["1", "2"])]).unwrap();
        let t = std::fs::read_to_string(dir.join("cmd.txt")).unwrap();
        assert_eq!(t, format!("{s1}\nflag\n1\n2\n\n\n\n"));
        // No ack yet: the first command goes again.
        let s2 = q.send(&dir, vec![cmd("restart", &[])]).unwrap();
        assert!(s2 > s1);
        let t = std::fs::read_to_string(dir.join("cmd.txt")).unwrap();
        assert_eq!(t.lines().count(), 13);
        // The game ran s2.
        std::fs::write(dir.join("ack.txt"), format!("{s2}\nroom_town\n40\n2\nDELTARUNE\n")).unwrap();
        let ack = read_ack(&dir).unwrap();
        assert_eq!((ack.room.as_str(), ack.plot.as_str()), ("room_town", "40"));
        assert!(connected(Some(&ack)));
        q.send(&dir, vec![cmd("goto", &["room_a"])]).unwrap();
        let t = std::fs::read_to_string(dir.join("cmd.txt")).unwrap();
        assert_eq!(t.lines().count(), 7);
        assert!(q.send(&dir, vec![cmd("flag", &["a", "1"])]).is_err());
    }

    #[test]
    fn install_and_uninstall() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ChapterStore::new(tmp.path(), "chapter1");
        let base = store.base();
        std::fs::create_dir_all(base.join("code")).unwrap();
        std::fs::create_dir_all(base.join("objects")).unwrap();
        std::fs::write(base.join("project.json"), "{}").unwrap();
        std::fs::write(base.join("code/gml_Object_obj_time_Step_0.gml"), "global.time += 1;\n").unwrap();
        std::fs::write(base.join("code/gml_Object_obj_b_Step_0.gml"), "y = 1;\n").unwrap();
        assert_eq!(status(&store)["installed"], false);
        let out = install(&store, None).unwrap();
        assert_eq!(out["host"], "gml_Object_obj_time_Step_0");
        let st = status(&store);
        assert_eq!(st["installed"], true, "{st}");
        let objects = store::asset_type("objects").unwrap();
        let obj = store.read(objects, OBJECT).unwrap()["data"].clone();
        assert_eq!(obj["Persistent"], true);
        assert_eq!(obj["PhysicsVertices"], json!([]));
        let code = store::asset_type("code").unwrap();
        let host = store.read(code, "gml_Object_obj_time_Step_0").unwrap();
        assert_eq!(host["source"], format!("{HOOK}global.time += 1;\n"));
        // Install again: one hook only. Other host: the hook moves.
        install(&store, None).unwrap();
        install(&store, Some("gml_Object_obj_b_Step_0")).unwrap();
        assert!(
            !store
                .list(code)
                .iter()
                .any(|e| e.name == "gml_Object_obj_time_Step_0" && e.modified)
        );
        assert_eq!(
            store.read(code, "gml_Object_obj_b_Step_0").unwrap()["source"]
                .as_str()
                .unwrap()
                .matches(HOOK)
                .count(),
            1
        );
        assert!(install(&store, Some("gml_nope")).is_err());
        let removed = uninstall(&store).unwrap();
        assert_eq!(removed.len(), 4, "{removed:?}");
        assert!(store.list(code).iter().all(|e| !e.modified));
        assert!(store.list(objects).is_empty());
        assert_eq!(status(&store)["installed"], false);
    }

    #[test]
    fn gml_reads_six_lines_per_command() {
        // The Step code reads op + ARGS lines; keep it in step with `Command::lines`.
        assert_eq!(STEP_GML.matches("file_text_read_string(f)").count(), 2 + ARGS);
    }
}
