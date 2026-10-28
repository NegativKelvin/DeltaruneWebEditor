# Deltarune Web Editor

A modding tool for Deltarune that runs in a webpage. A local Rust server (`server/`) runs
[UndertaleModTool](https://github.com/UnderminersTeam/UndertaleModTool) CLI to decompile each chapter,
serves every asset to the webpage (`web/`), stores edits, and builds a patched `data.win`.

Status: step 1 (backend), step 2 (editors), step 3 (room preview, bullet patterns, battle box), and the game state tools (flags, save files, hot reload, mod conflicts) are done. Build and one-click test launch come later.

## Download and run (no other downloads)

1. Get `DeltaruneWebEditor-<os>` from [Releases](https://github.com/NegativKelvin/DeltaruneWebEditor/releases): `win-x64` (.zip), `linux-x64`, `osx-arm64`, `osx-x64` (.tar.gz).
2. Extract it and start `dre-server` (`dre-server.exe` on Windows).
3. Open <http://127.0.0.1:7878>, set the Deltarune folder in Setup, and extract the chapters.

The package holds UndertaleModCli 0.9.2.0 in `tools/UndertaleModCli/`, built self-contained (it includes the .NET runtime), so you do not install UTMT or .NET.
The server finds it automatically. macOS: the files are not signed; run `xattr -dr com.apple.quarantine DeltaruneWebEditor-osx-*` once.

Make a release: push a tag `v*`. The [release workflow](.github/workflows/release.yml) builds each package and runs the end-to-end test with the bundled CLI (Linux).

### UndertaleModTool license (GPL-3.0)

UTMT is GPL-3.0. The package ships it unmodified as a separate program that `dre-server` starts as a child process (no linking),
so the editor code keeps its own license. Each release also gives what GPL-3.0 needs for the CLI:
its license text (`tools/UndertaleModCli/LICENSE.txt`), the notice [THIRD-PARTY.md](packaging/THIRD-PARTY.md),
and the corresponding source `UndertaleModTool-0.9.2.0-src.tar.gz` as a release asset. Keep these when you share a package.

## Build from source

- Deltarune (Steam install: `data.win` plus `chapter1_windows/` ... `chapter4_windows/`).
- Rust 1.85+ to build the server.
- UndertaleModCli **0.9.2.0 or later**. Earlier versions have no project export/build. Build the pinned copy into `tools/` with git and the .NET 10 SDK:
  `scripts/build-utmt.sh <win-x64|linux-x64|osx-arm64|osx-x64> tools` (Windows: Git Bash). Or use `UndertaleModCli.exe` from a UTMT build.

```sh
cd server
cargo run --release -- --game "C:/Program Files (x86)/Steam/steamapps/common/DELTARUNE" --utmt "C:/tools/UTMT/UndertaleModCli.exe" --workspace ../workspace --web ../web
```

Open <http://127.0.0.1:7878>. You can also set the paths later with `PUT /api/settings`. Settings are kept in `workspace/settings.json`.

| Flag | Env | Default |
|---|---|---|
| `--game` | `DRE_GAME_DIR` | from `settings.json` |
| `--utmt` | `DRE_UTMT_CLI` | `settings.json` (if the file is still there), then next to the server: `tools/UndertaleModCli/`, `tools/`, the server folder; then `./tools/UndertaleModCli/`, `./tools/`, `./`, `PATH` |
| `--workspace` | `DRE_WORKSPACE` | `workspace` |
| `--web` | `DRE_WEB_DIR` | `web` (in the current folder, else next to the server) |
| `--listen` | `DRE_LISTEN` | `127.0.0.1:7878` |

The server accepts only requests addressed to `localhost`/`127.0.0.1` (DNS-rebinding guard), and it sends no CORS headers. POST, PUT, and DELETE from a page of another origin (another site or port) get `403` (CSRF guard); requests without `Origin` (curl, scripts) are accepted.

## Editors (webpage)

The webpage is plain HTML, CSS, and JavaScript modules in `web/` (no build step). The browser cannot run the Rust code,
and a WebAssembly build adds a toolchain for no gain here, so all parsing stays in the Rust server and the page only draws and edits.

| Editor | What you can do |
|---|---|
| Setup | Set the game folder and UndertaleModCli path. Extract each chapter. |
| Overview and build | Asset counts, list of changes (revert each), build `data.win` with the log. |
| Rooms and triggers | Draws background colour, backgrounds, tile layers, asset layers, legacy tiles, and instances (old and GMS2 layer rooms). Select, drag, nudge, add, duplicate, delete instances. Paint and erase tiles. Edit instance creation code (trigger settings) and room creation code inline. Triggers list: door, marker, event, and invisible objects, and instances with creation code. Layer, view, and room properties. Undo/redo. |
| Dialogue | All text lines from GML with speaker, key, translations, and a text box preview (`\cY` colours, `^1` pauses, `/` `%`). Edit text: only the string literal changes. |
| Cutscenes | Code entries with `c_*` calls, shown as blocks of steps. Change function and arguments, add, delete, reorder. Saving replaces only the changed blocks. |
| Sprites | Pixel editor (pencil, eraser, fill, colour picker, flip), frames (add, copy, delete, reorder, import PNG or strip, export strip), resize, origin, bounding box, collision kind, animation preview. Automatic bounding box and collision masks are made on save. |
| Enemies and encounters | Stats from `global.monster*[i] = ...` assignments (`scr_monstersetup`), encounters (`scr_encountersetup`: enemy object, type, position, battle text), links to enemy objects. |
| Shaders | Edit every shader text file. Live WebGL preview of the GLSL ES pair on a sprite, with a control for each uniform. Replace compiled `.bin` data. |
| All assets | Every type (rooms, objects, code, scripts, sprites, backgrounds, sounds, fonts, shaders, paths, sequences, animation curves): form editor for all fields, raw JSON, side files (preview, replace, add, remove), new, duplicate, revert. Objects: events with inline code, add events. Paths: drag points. Code: GML editor with highlighting. |
| Data lists | Read-only tree view of every list in the data file. |
| Search code | Plain-text search in all GML, links to the line. |

## Test without the game

| Tool | What you can do |
|---|---|
| Room preview | Walk Kris through a room in the browser. **▶ Play** (P) in the room editor opens it in a window over the editor: it uses the unsaved room, so a moved instance or painted tile shows at once. Also a page (`Test › Room preview`) for saved rooms. Solid objects (`obj_solid*` and their children, or the Solid flag) block. Triggers (doors, markers, events, invisible objects, instances with creation code) light up when Kris touches them, and the log shows their creation code. Doors: the room names in the door code (and in `room == <this room>` branches) give the next room; `obj_doorX` puts Kris on `obj_markerX` there. Z talks: the dialogue lines of the object show in a text box. Camera from the first view. Dark world toggle (2× Kris, `_dark` sprites), walk/run speed, show solids, no clip, sprite animation. |
| Bullet patterns and battle box | Emitters with timing (start, end, every), origin (box, soul, screen), offset, random offset, ring radius, count, angle, spread, spin per shot, aim at the soul, random angle, speed, speed per bullet, acceleration, turn, gravity, life, sprite, scale, rotation, damage, and GML to run on each new bullet. Presets: ring, aimed burst, rain, spiral, side wall, falling arcs. Battle box: centre and size, keyframes over time. Timeline: play, pause, step, scrub, speed, loop. **Test with soul** (T): arrows move, X or Shift slow, with hits, i-frames, grazes, and HP. Every edit shows at once. |
| GML export | **GML** shows the wave object code. **Install in mod** writes `obj_dre_wave_<name>` (Create and Step events) and its code into the mod. Start the wave from enemy attack code: `instance_create(0, 0, obj_dre_wave_<name>);`. Build the chapter after each install. |

The simulator (`web/js/editors/battlesim.js`) runs the same steps in the same order as the made GML: box keys, bullet update (life, speed, turn), emitters, then GameMaker motion (gravity, position).
The GML uses the Deltarune names `camerax()`, `cameray()`, `instance_create`, `obj_heart` (soul), `obj_growtangle` (box), and `obj_collidebullet` (default bullet object, you can change it).

Known limits of step 3:
- The preview does not run GML. Cutscenes, enemy AI, and object code that moves things do not run. Door targets that the game sets at run time (for example from global variables) show as "no target found".
- Kris collides with the lower part of the sprite box. The game uses the masks of `obj_mainchara`.
- The battle test uses circles for hits. The game uses sprite masks, its own damage rules, and its own soul speed.
- "Write box code" (move and resize `obj_growtangle` by the keyframes) is experimental: the box object can also change its own size.

Shortcuts: Ctrl+S saves. Room: V select, A add, T paint, X erase, Del delete, Ctrl+D duplicate, arrows nudge, Space or middle button pans, wheel zooms.
Sprite: B pencil, E eraser, G fill, I colour picker, O origin, arrows change frame.

Known limits:
- Windows Deltarune draws shaders with the compiled HLSL11 data. After you change GLSL ES, compile HLSL 11 (`fxc`, `vs_4_0` / `ps_4_0`) and replace `HLSL11_*.bin` in the shader editor.
- Translations in `lang/*.json` are read-only here (they are not in `data.win`). Step 4 (deploy) copies changed language files.
- The enemy and cutscene views read decompiled GML. Values that the decompiler writes as numbers (for example an object index) show as numbers.

## Game state and live tools

| Tool | What you can do |
|---|---|
| Flags and variables | Every `global.<name>` and `global.<name>[i]` in the chapter code (Deltarune keeps story state in `global.flag[n]` and `global.plot`), with read and write counts and the numbers that the code sets or compares. Click a row: each read and write, with a link to the code line and what it does (`= 1`, `>= 30`). Notes per flag (kept in `workspace/<chapter>/flag-notes.json`). With hot reload: read the flag values of the running game, set a value in the game. |
| Save files | Edit `filech<chapter>_<slot>` in the save folder line by line. Each line shows the variable that the game writes there: the server reads the order from the chapter's own save script (`scr_saveprocess`: `file_text_write_*` calls, `for`/`repeat`/`while` loops). Story point: plot (with the plot values found in the code), room, set flag N. Save as preset, load a preset or backup into a slot. Every write makes a backup first (`workspace/saves/`, 20 per file). "Send changes to game" sends changed flags and globals to the running game. |
| Hot reload | Install a small persistent object `obj_dre_live` in the mod (one line in a host code entry, default `obj_time` Step, makes it). After one build, the running game reads `dre_live/cmd.txt` in the save folder 3 times per second. From the page: go to a room, restart the room, set a flag, set a global, set a variable on all instances of an object, replace a sprite with its saved frames, read all flags. The game writes `dre_live/ack.txt` (room, plot) once per second; the page shows "game connected". |
| Mod conflicts | Compares two or more mods (UTMT project folders: this chapter's `mod/`, folders in `workspace/<chapter>/mods/`, and paths you give) with the original. Code: line diff; changes on different lines can go together ("different parts"), on the same or next lines they conflict. Other assets: by top-level field and by side file (sprite frames, sounds). |

Save folder: set it in Setup, or the server finds it: `%LOCALAPPDATA%\DELTARUNE` (Windows), the Proton prefix `steamapps/compatdata/1671210/.../AppData/Local/DELTARUNE` (Linux), `~/Library/Application Support/com.tobyfox.deltarune` (macOS).

Known limits of the game state tools:
- Hot reload changes data only. GameMaker has no way to load new code while the game runs, so changes to code, objects, rooms (layout, instances, creation code), dialogue text, sounds, and shaders need a build and a game restart.
- A hot reloaded sprite is replaced with `sprite_replace` from a PNG strip. The game forgets it at restart; build to keep it. This was tested with the GML compiler (end-to-end test), not yet in the real game.
- Hot reload values are numbers or plain text, not GML expressions. Commands that the game did not acknowledge go again with the next batch, so a command can run twice.
- Save labels come from the save script without running it. Conditions in the script (`if`) and loops whose bounds are not numbers are marked with a warning, and the page says when the line counts do not match. The room line holds what the game writes there (for example a room id), not a room name.
- The flag browser finds `global.<name>` text in decompiled GML. Access through other names (`variable_global_get`, `with (global)`) is not found.

## How data flows

1. **Extract** (`POST /api/chapters/{ch}/extract`) runs `UndertaleModCli load <data.win> -s export_all.csx`.
   The script ([server/utmt-scripts/export_all.csx](server/utmt-scripts/export_all.csx)) writes:
   - `workspace/{ch}/base/`: a UTMT project with **all** rooms, objects, code (GML), scripts, sprites (PNG frames), backgrounds,
     sounds, fonts, shaders, paths, sequences, and animation curves.
   - `workspace/{ch}/generic/`: a read-only JSON dump of **every** list in the data file (also strings, texture page items,
     timelines, extensions, audio groups, variables, functions, general info, options, ...).
2. **Edit** (`PUT .../assets/...`) writes to `workspace/{ch}/mod/` only. Reads look in `mod/` first, then `base/`.
   `DELETE` removes the change.
3. **Build** (`POST /api/chapters/{ch}/build`) runs `UndertaleModCli project build mod/project.json -s <data.win> -d workspace/{ch}/build/data.win`.
   Only the changed assets are imported. The original game files are not changed.

## API

All responses are JSON. Errors: `{"error": "..."}` with status 400/404/409/500.

| Method | Path | Result |
|---|---|---|
| GET | `/api/health` | version |
| GET/PUT | `/api/settings` | `{game_dir, utmt_cli, save_dir}`. `utmt_cli` must be outside the workspace. GET also gives `save_dir_found` (the folder in use) |
| GET | `/api/asset-types` | the 12 editable types: slug, `$type`, own-directory flag |
| GET | `/api/chapters` | chapters found in `game_dir`: `launcher`, `chapter1`, ... with `extracted` and `meta` |
| POST | `/api/chapters/{ch}/extract` | `202 {job}` |
| POST | `/api/chapters/{ch}/build` | `202 {job}` |
| GET | `/api/jobs`, `/api/jobs/{id}` | status `running`/`succeeded`/`failed`, error, log lines |
| POST | `/api/jobs/{id}/cancel` | stops the UndertaleModCli run of a running job (`409` if not running). A run also stops after 2 hours |
| GET | `/api/chapters/{ch}/assets` | count per type |
| GET | `/api/chapters/{ch}/assets/{type}?q=&offset=&limit=` | `{total, items:[{name, modified, added}]}` |
| GET | `/api/chapters/{ch}/assets/{type}/{name}` | `{data, files, modified}`; code also has `source` (GML) |
| PUT | `/api/chapters/{ch}/assets/{type}/{name}` | body `{data?, source?}`; makes a new asset if the name is new |
| DELETE | `/api/chapters/{ch}/assets/{type}/{name}` | revert to the original (or delete a new asset) |
| GET/PUT | `/api/chapters/{ch}/assets/{type}/{name}/files/{file}` | side files: sprite frames `*.png`, masks, shader sources, audio |
| GET | `/api/chapters/{ch}/changes` | all changed assets per type |
| GET | `/api/chapters/{ch}/generic` | `{list: count}` for every list in the data file |
| GET | `/api/chapters/{ch}/generic/{list}?q=&offset=&limit=` | items of one list (`GeneralInfo`, `Options`, `Language` return one object) |
| GET | `/api/chapters/{ch}/dialogue?q=&code=&offset=&limit=` | dialogue lines (see below) |
| GET | `/api/chapters/{ch}/search?q=&max=` | plain-text search in all GML |
| DELETE | `/api/chapters/{ch}/assets/{type}/{name}/files/{file}` | remove a side file from the mod copy |
| POST | `/api/chapters/{ch}/code/{name}/patch` | body `{rev, edits:[{start, end, text?, string?}]}`: replace byte ranges of the GML. `string` is quoted as a GML string. `409` if `rev` is old |
| GET | `/api/chapters/{ch}/index/{type}?fields=A,B` | `{name: {A, B}}` for every asset of a type (for example object sprites) |
| GET | `/api/chapters/{ch}/cutscenes?q=` | code entries with `c_*` steps and the step count |
| GET | `/api/chapters/{ch}/cutscenes/{code}` | `{rev, source, blocks:[{start, end, line, indent, context, steps:[{func, args, line, start, end}]}]}` |
| GET | `/api/chapters/{ch}/enemies` | `global.monster*[i] = v` and `global.battlemsg[i] = v` assignments: `[{code, case, index, rev, fields:{name: {expr, start, end, line}}}]` |
| GET | `/api/chapters/{ch}/patterns` | saved bullet patterns: `[{name, duration, emitters, object}]` |
| GET/PUT/DELETE | `/api/chapters/{ch}/patterns/{name}` | one pattern (JSON, missing fields get defaults). Kept in `workspace/{ch}/patterns/`, not in the mod. `DELETE ...?uninstall=true` also removes the installed wave object and its code from `mod/` |
| POST | `/api/chapters/{ch}/patterns/{name}/gml` | body: a pattern, or empty for the saved one. `{object, create:{code, source}, step:{code, source}}` |
| POST | `/api/chapters/{ch}/patterns/{name}/install` | saves the body (if any), writes the wave object and its two code entries into `mod/` |
| GET | `/api/chapters/{ch}/flags?q=&name=&offset=&limit=` | global variables: `{total, items:[{name, index, reads, writes, values}]}`. `index` is the index text or `null` |
| GET | `/api/chapters/{ch}/flags/refs?name=&index=` | each read and write: `[{code, line, name, index, write, detail}]`. No `index`: the plain variable |
| GET/PUT | `/api/chapters/{ch}/flag-notes` | `{"flag[12]": "note", ...}` |
| GET | `/api/chapters/{ch}/save-layout` | `{code, fields:[{line, label, kind}], warnings}` from the save script |
| GET | `/api/saves` | `{dir, files:[{name, chapter, slot, size, modified}], copies:[...]}` (`409` when no save folder) |
| GET/PUT | `/api/saves/{file}` | `{lines, eol}` / body `{lines}`: writes the file (line ending kept), returns `{backup}`. Only `filech<N>_<slot>` names |
| POST | `/api/saves/{file}/copy`, `/api/saves/{file}/load` | body `{name}`: save the file as a preset / put a preset or backup into the slot (with a backup) |
| DELETE | `/api/save-copies/{name}` | delete a preset or backup |
| GET/POST/DELETE | `/api/chapters/{ch}/live` | hot reload object: status `{installed, host, default_hosts}` / install, body `{host?}` / remove from `mod/` |
| GET | `/api/live` | `{dir, connected, ack:{seq, room, plot, done, game, age_ms}, seq, pending}` |
| POST | `/api/live/commands` | body `{commands:[{op, args}]}`. Ops: `flag n v`, `flagstr n text`, `global name v`, `globalstr name text`, `goto room`, `restart`, `var object variable v`, `dumpflags` |
| POST | `/api/live/sprite/{name}?frames=&xo=&yo=` | body: PNG strip. Writes it to `dre_live/` and sends `sprite_replace` |
| GET | `/api/live/flags` | `{values, age_ms}` from the last `dumpflags` |
| GET | `/api/chapters/{ch}/mods` | mods in `workspace/{ch}/mods/` |
| POST | `/api/chapters/{ch}/conflicts` | body `{paths?, own?}`: `{mods, findings:[{type, name, mods, kind, details}]}`. `kind`: `conflict`, `merge`, `same` |

Asset types (`{type}`): `rooms`, `objects`, `code`, `scripts`, `sprites`, `backgrounds`, `sounds`, `fonts`, `shaders`,
`paths`, `sequences`, `animcurves`. The `data` JSON is the UTMT project format, so every field that UTMT can import can be changed.

### Dialogue

The server scans all GML for `msgsetloc`, `msgnextloc`, `msgsetsubloc`, `msgnextsubloc`, `c_msg*` variants, `stringsetloc`,
`stringsetsubloc`, `msgset`, `msgnext`, `stringset`, and `scr_84_get_lang_string`. Each line has
`{code, line, func, text, span, key, speaker, rev, translations}`. `speaker` comes from the last `scr_speaker`/`c_speaker` call in the same code entry.
`translations` come from `<chapter dir>/lang/lang_*.json` (for example `ja`).
To change a line, `POST .../code/{code}/patch` with `{rev, edits:[{start: span[0], end: span[1], string: "new text"}]}`.
Code reads (`GET .../assets/code/{name}`) also return `rev`.

## Tests

```sh
cd server
cargo test                                         # unit tests
DRE_TEST_UTMT=/path/to/UndertaleModCli cargo test  # plus the end-to-end test: extract, edit, install a pattern and the hot reload object, build, verify
cd .. && node --test web/tests/*.test.mjs          # battle simulator and room preview rules
```

The end-to-end test makes a small data file with [tests/fixtures/make_fixture.csx](server/tests/fixtures/make_fixture.csx). CI builds UTMT 0.9.2.0 from source with `scripts/build-utmt.sh` and runs all tests.
