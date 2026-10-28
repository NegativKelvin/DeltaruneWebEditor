# Deltarune Web Editor

A modding tool for Deltarune that runs in a webpage. A local Rust server (`server/`) runs
[UndertaleModTool](https://github.com/UnderminersTeam/UndertaleModTool) CLI to decompile each chapter,
serves every asset to the webpage (`web/`), stores edits, and builds a patched `data.win`.

Status: step 1 (backend) and step 2 (editors) are done. Live room preview, bullet pattern and battle box editors, and one-click launch come next.

## Requirements

- Deltarune (Steam install: `data.win` plus `chapter1_windows/` ... `chapter4_windows/`).
- UndertaleModCli **0.9.2.0 or later** (from the UTMT release zip, `UndertaleModCli.exe`). Earlier versions have no project export/build.
- Rust 1.85+ to build the server.

## Run

```sh
cd server
cargo run --release -- --game "C:/Program Files (x86)/Steam/steamapps/common/DELTARUNE" --utmt "C:/tools/UTMT/UndertaleModCli.exe" --workspace ../workspace --web ../web
```

Open <http://127.0.0.1:7878>. You can also set the paths later with `PUT /api/settings`. Settings are kept in `workspace/settings.json`.

| Flag | Env | Default |
|---|---|---|
| `--game` | `DRE_GAME_DIR` | from `settings.json` |
| `--utmt` | `DRE_UTMT_CLI` | `settings.json`, then `./`, `./tools/`, next to the server, `PATH` |
| `--workspace` | `DRE_WORKSPACE` | `workspace` |
| `--web` | `DRE_WEB_DIR` | `web` |
| `--listen` | `DRE_LISTEN` | `127.0.0.1:7878` |

The server accepts only requests addressed to `localhost`/`127.0.0.1` (DNS-rebinding guard), and it sends no CORS headers.

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

Shortcuts: Ctrl+S saves. Room: V select, A add, T paint, X erase, Del delete, Ctrl+D duplicate, arrows nudge, Space or middle button pans, wheel zooms.
Sprite: B pencil, E eraser, G fill, I colour picker, O origin, arrows change frame.

Known limits:
- Windows Deltarune draws shaders with the compiled HLSL11 data. After you change GLSL ES, compile HLSL 11 (`fxc`, `vs_4_0` / `ps_4_0`) and replace `HLSL11_*.bin` in the shader editor.
- Translations in `lang/*.json` are read-only here (they are not in `data.win`). Step 4 (deploy) copies changed language files.
- The enemy and cutscene views read decompiled GML. Values that the decompiler writes as numbers (for example an object index) show as numbers.

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
| GET/PUT | `/api/settings` | `{game_dir, utmt_cli}` |
| GET | `/api/asset-types` | the 12 editable types: slug, `$type`, own-directory flag |
| GET | `/api/chapters` | chapters found in `game_dir`: `launcher`, `chapter1`, ... with `extracted` and `meta` |
| POST | `/api/chapters/{ch}/extract` | `202 {job}` |
| POST | `/api/chapters/{ch}/build` | `202 {job}` |
| GET | `/api/jobs`, `/api/jobs/{id}` | status `running`/`succeeded`/`failed`, error, log lines |
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
DRE_TEST_UTMT=/path/to/UndertaleModCli cargo test  # plus the end-to-end test: extract, edit, build, verify
```

The end-to-end test makes a small data file with [tests/fixtures/make_fixture.csx](server/tests/fixtures/make_fixture.csx). CI builds UTMT 0.9.2.0 from source and runs all tests.
