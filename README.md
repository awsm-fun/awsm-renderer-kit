# awsm-renderer-kit

The settings and stats code every [awsm-renderer](https://github.com/awsm-fun/awsm-renderer)
app used to copy by hand, in one place:

| Piece | What it is |
|---|---|
| `awsm-renderer-settings` (crate) | Anti-aliasing modes (Off, SMAA, MSAA 4×, TAA, TAA Quality / Balanced / Performance), defaults per device class, the resolution policy, storage and URL flags, applying a mode to the renderer behind a "compiling pipelines" overlay, the GPU probe, the settings messages, and a web-sys Settings panel |
| `awsm-renderer-stats` (crate) | The stats panel's numbers: a worker-side collector (frame windows, warm-up, EMA, refresh rate, missed frames, frame budget, renderer CPU scopes and GPU passes), a serde `FrameReport` with app sections, and a web-sys panel that shows it |
| `awsm-renderer-kit-web` (crate, internal) | localStorage, URL flags, `postMessage` and DOM helpers both crates share |
| `js/` | The page-side twins for apps whose page is plain JS (the C family): `settings.js`, `stats.js`, `kit-web.js`. Synced into a project by `scripts/sync-js.sh` |

Built for awsm-renderer **0.46.0**. MIT OR Apache-2.0.

Contents: [The shape](#the-shape) · [API](#api) · [Protocol](#protocol) ·
[Depending on it](#depending-on-it) · [How to adopt](#how-to-adopt) ·
[An auto-tuner](#an-auto-tuner) ·
[TAA and the resolution setting](#taa-and-the-resolution-setting) ·
[Decisions](#decisions) · [Working on the kit](#working-on-the-kit)

## The shape

Three levels:

- **The core** (no features, unit-tested natively) holds the policy and the
  tables: the modes and their words, the defaults, the resolution numbers, the
  stats windows, smoothing and formatting, the wire types.
- **Thin adapters** on each side: a worker adapter that drives `AwsmRenderer`
  (`renderer` feature), a page adapter and panels in Rust (`web`, `ui`), and
  the JS twins for a JS page.
- **Hooks** where the app plugs in: where the panels mount and the buttons
  that open them, the app's own Settings rows, the app's stats sections, and
  how messages travel (the app posts them; the kit only serializes).

Who computes what:

```
render worker (Rust, awsm-renderer)                page (Rust or JS)
──────────────────────────────────                 ─────────────────
probe_gpu  ─┐
            └─ settings-options ─────────────────▶  Settings: stored choice / ?aa= / device default,
               (menu, defaults, policy, GPU)        canvas size from the policy (ResizeObserver)
                                    ◀───────────── init {width, height, aa}   (the app's message)
apply_anti_aliasing                 ◀───────────── anti-alias {mode}
  aa-compile-start/-progress/-done ──────────────▶  compile overlay
canvas resize                       ◀───────────── resize {width, height}
StatsCollector (every frame)        ◀───────────── stats-view {shown, open}   (on change)
                                    ◀───────────── stats-page {gaps, page, frames}   (1 Hz)
  stats {FrameReport} (≈1 Hz) ───────────────────▶  StatsPanel / stats.js: shows it
```

The worker owns the words and numbers; the JS page is a view. The one piece
of policy the page runs itself is the resolution formula (the canvas size is
a page fact), and its JS twin is tested against the same fixture as the Rust
(`fixtures/resolution.json`, generated from the policy the templates shipped).

## API

### `awsm-renderer-settings`

Core:

| Item | |
|---|---|
| `AaMode` | `Off`, `Smaa`, `Msaa`, `Taa`, `TaaQuality` (0.67), `TaaBalanced` (0.58), `TaaPerformance` (0.5). `ALL`, `id()` / `from_id()` (`off`, `smaa`, `msaa`, `taa`, `taa-quality`, `taa-balanced`, `taa-performance`; serde uses the id), `label()`, `description()`, `is_taa()`, `taa_render_scale()`, `info()` |
| `AaDefaults { desktop, coarse, software }` | default `Taa` / `Off` / `Off`; `pick(DeviceClass)` (software, then coarse, then desktop) |
| `DeviceClass { coarse, software }` | |
| `SettingsConfig { modes, defaults, resolution }` | `Default` = all seven modes, the defaults above, the shared policy; `offers`, `parse`, `default_for`, `initial_aa(device, stored, url) -> (AaMode, AaSource)` |
| `ResolutionPolicy` | `min_scale` 0.5, `max_scale` 1.0, `step` 0.05, `pixel_budget` 2.8 MP, `min_effective_dpr` 1, `coarse_start` 0.6, `software_start` `Some(0.5)`, `max_texture` 8192; `cap`, `auto_scale`, `default_scale(css_w, css_h, dpr, device)`, `backing_size(native_w, native_h, scale, max_texture)`, `parse_stored` / `format_stored` (`"0.80"`), `percent`, `from_percent` |
| `ResolutionState` | the page's state: `new`, `resized`, `gpu_info`, `set_percent`, `backing`, `scale`, `percent`, `is_user`, `auto_scale`, `dpr`, `cap`; for an app's auto-tuner `set_auto_scale`, `clear_auto_scale`, `scale_for`, `is_auto_tuned` ([below](#an-auto-tuner)) |
| `migrate_legacy_aa(msaa, smaa)` | the Rust v1 projects' pre-0.42 `awsm_msaa` / `awsm_smaa` keys, as they migrated them |
| `show_quality_notice(device, stored_scale, stored_aa)` | |
| `retune_target(mode) -> RetuneLever { CanvasScale, TaaRenderScale }` | which lever an auto-tuner moves |
| `SettingsToPage`, `SettingsToWorker`, `SettingsOptions`, `GpuInfo`, `InitSettings`, `KIT_VERSION` | the protocol, below |

`renderer` feature (worker side):

| Item | |
|---|---|
| `AaMode::to_anti_aliasing()` / `AaMode::from_anti_aliasing(&AntiAliasing)` | the renderer's constructors (`none`, `smaa`, `msaa4`, `taa(TaaConfig::native() / preset(..))`); back to the closest mode |
| `apply_anti_aliasing(renderer, mode, on_event) -> Result<(), String>` | `set_anti_aliasing`, then `commit_load` with `LoadingStats::phase_label()` as progress when the structure changed (none between TAA presets; also after the renderer's own TAA → MSAA fallback, whose pipelines need it); `on_event(AaCompileEvent::{Start {mode, compiles}, Progress {message}, Done {ok, error, mode, taa_fallback}})`, `Done` always. `SettingsToPage::from(event)` is the message |
| `probe_gpu() -> GpuInfo` | the worker's own lightweight `requestAdapter`: `is_fallback`, `max_texture_dim` |
| `post_to_page(&msg)` | serialize and post from a dedicated worker |
| `SettingsToWorker::from_js(&JsValue)` | `None` for a message that is not the kit's |

`scene` feature: `scene_anti_aliasing(&AntiAliasing) -> AntiAliasingConfig`, so
a scene load keeps the app's mode (`scene.post_process.anti_aliasing = …`
before `load_scene_for_player`).

`web` feature (page side): `SettingsStore::new(prefix)` with
`with_reset_keys(..)`, `with_legacy_aa_keys(..)`, `res_scale` /
`set_res_scale`, `aa` / `aa_id` / `set_aa`, `get_bool` / `set_bool`,
`migrate_legacy_aa`, `reset_if_asked`, `reset_keys`, `url_aa`;
`observe_canvas(canvas, FnMut(CanvasSize))`, `CanvasSize::of(canvas)`;
`SettingsToPage::from_js`.

`ui` feature: `SettingsPanel::new(container, canvas, store, &options, SettingsUi, log)`,
`.start(canvas, SettingsHooks { on_resize, on_anti_alias, open_button, extra_rows })`,
`.backing()`, `.aa()`, `.scale()`, `.open()`, `.close()`, `.ready()`,
`.handle(&SettingsToPage)`, `.handle_js(&JsValue) -> bool`,
`.is_user_scale()`, `.dpr()`, `.set_auto_scale(scale)`, `.clear_auto_scale()`. `SettingsUi`
holds the words and knobs (`title`, `hint`, `close_class`, notice and
overlay text, `show_aa`, `escape_stops`).

### `awsm-renderer-stats`

Core:

| Item | |
|---|---|
| `StatsCollector::new(StatsConfig)` | the worker-side collector |
| per frame | `begin_frame(now)`, `lap(phase, now)`, `end_frame(FrameInput, &impl TimingSource) -> Option<Window>`, then `report(&window, app_sections) -> FrameReport` |
| lifecycle | `mark_ready()` (scene on screen: warm-up starts), `reset()` (scene reload), `restart()` (a new app instance), `placeholder()`; each returns the report to post |
| from the page | `handle(StatsToWorker)`, `set_view(shown, open)`, `page_sample(gaps, parts, frames)`, `is_open(id)`, `renderer_detail()`, `refresh_ms()` |
| app figures | `smooth(key, value)`, `smoothed(key)` |
| `StatsConfig` | `renderer_label`, `page_label`, `phases`, `render_phase`, `sections: Vec<SectionSpec>`, `pacing_row`, `renderer_in_budget`, `report_span_ms` |
| `FrameInput { t_ms, cpu_ms, late, interval, paced_skips }` | one presented frame |
| `Window { seconds, frames, span_ms, late: LateStats }` | `LateStats::row(label)` is the shared "late frames" row |
| `FrameReport { status, budget, display, gpu, renderer, app, page, summary }` | `groups()` in display order; `summary` has the raw `refresh_ms`, `fps`, `cpu_ms`, `gpu_ms`, `page_ms`, `frame_interval` for a governor |
| `StatGroup` (= `AppSection`) | `id`, `label`, `lane`, `headline`, `bad`, `rows`, `budget_ms` (joins the frame budget), `budget_label` (the lane's label when not the group's, e.g. one lane for a thread two sections share); builder methods |
| `StatRow` | `label`, `value` (the display string), `depth`, `bad`, `lane` + `share` (a budget bar) |
| `TimingSource` | the renderer's running totals (`()` = none) |
| helpers | `format::{duration, per_frame, per_step, tenths, share, ordinal, fixed, value_pieces}`, `step_headline`, `frame_headline`, `duration_row`, `Smoother`, `NamedMeans`, `RefreshEstimator`, the constants (`EMA_ALPHA`, `WARMUP_FRAMES`, `GAP_MS`, `MIN/MAX_WINDOW_MS`, `LATE_WINDOWS`, …) |

`renderer` feature: `RendererTimings` (awsm-renderer's `profiling` as a
`TimingSource`), `StatsCollector::begin_renderer_frame(&mut renderer, now)`
(also switches `renderer.logging.cpu` between `SubFrame` and `Off` with the
renderer group), `stats_logging()` (build with it: GPU `Frame` tier),
`post_report(report)`.

`ui` feature: `StatsPanel::mount(prefix, panel, button, StatsHooks { post, on_help, page_part, app_sections, section_rows, shown })`
(`section_rows(group_id, &report) -> Vec<StatRow>`: page-side rows added to the end of an expanded group the worker sent),
`.handle(&StatsToPage)`, `.handle_js(&JsValue)`, `.page_frame(ms, part, count_frame)`,
`.set_open(bool)`, `.is_open()`, `.last_report()`.

### The JS twins (`js/`)

```js
import { Settings, resetSettingsIfAsked } from './kit/settings.js';
import { installStats } from './kit/stats.js';

resetSettingsIfAsked({ prefix, extraKeys: [], legacyAaKeys: null });   // → bool
const settings = new Settings({ prefix, canvas, container, options, ui: {}, log });
settings.backing(); settings.aa(); settings.scale();
settings.start({ onResize({ width, height }), onAntiAlias(id), openButton, extraRows: () => [els] });
settings.handleMessage(msg);   // → true for the kit's settings messages
settings.ready(); settings.open(); settings.close();
settings.isUserScale(); settings.dpr(); settings.setAutoScale(scale); settings.clearAutoScale();

const stats = installStats({ prefix, panel, button, post, onHelp, pagePart, appSections, sectionRows, shown });
stats.handleMessage(msg);      // → true for `stats`
stats.pageFrame(ms, part, { frame });
stats.setOpen(open); stats.isOpen(); stats.lastReport();
```

`settings.js` also exports the policy functions (`ResolutionState`,
`autoScale`, `defaultScale`, `backingSize`, `initialAa`, …) and
`SETTINGS_UI`, the default words. `kit-web.js` exports `KIT_VERSION`;
`Settings` warns in the console when the worker's `kitVersion` (in
`settings-options`) differs, i.e. when the crates' tag and the synced JS
are out of step.

**Showing and hiding.** Every element the kit shows and hides (overlays,
the anti-aliasing row, the stats panel and its groups' rows) gets the
`hidden` attribute *and* an inline `display: none`, removed again when shown
(`setShown` / `dom::set_shown`). The attribute alone loses to an app rule
such as `.settings-overlay { display: flex }`, which the Rust templates
have. An app that shows or hides a kit element itself should do the same,
or call the kit (`open()`, `close()`, `setOpen()`).

## Protocol

Every message is a plain object with a kebab-case `type` and camelCase
fields (the C family's envelope; Rust deserializes the same objects with
serde-wasm-bindgen).

| Direction | `type` | Fields |
|---|---|---|
| worker → page | `settings-options` | `modes: [{id, label, description}]`, `defaults: {desktop, coarse, software}`, `resolution: {minScale, maxScale, step, pixelBudget, minEffectiveDpr, coarseStart, softwareStart, maxTexture}`, `gpu: {isFallback, maxTextureDim} \| null`, `kitVersion` |
| worker → page | `gpu-info` | `isFallback`, `maxTextureDim` (for apps that probe later; also inside `settings-options`) |
| worker → page | `aa-compile-start` | `mode`, `label`, `compiles` (false between TAA presets: no overlay) |
| worker → page | `aa-compile-progress` | `message` |
| worker → page | `aa-compile-done` | `ok`, `error`, `mode` (what runs now), `taaFallback` (why TAA was replaced, if it was) |
| worker → page | `stats` | a `FrameReport` |
| page → worker | `anti-alias` | `mode` |
| page → worker | `resize` | `width`, `height` |
| page → worker | `stats-view` | `shown`, `open: [groupId]` |
| page → worker | `stats-page` | `gaps: [ms]`, `page: [{name, ms}]`, `frames` |

The app's `init` carries the canvas and its own fields; it adds `width`,
`height` and `aa` (the mode id) — `InitSettings`.

Storage (all under the app's prefix): `res_scale` (`"0.80"`), `aa` (the id),
`stats` (`"1"`/`"0"`), `stats_open_<group>`. URL: `?reset` clears exactly
those (plus registered keys); `?aa=<id>` overrides the mode for the session,
unstored.

## Depending on it

```toml
[dependencies]
awsm-renderer-settings = { git = "https://github.com/awsm-fun/awsm-renderer-kit", tag = "v0.1.1", features = ["renderer", "scene"] }
awsm-renderer-stats = { git = "https://github.com/awsm-fun/awsm-renderer-kit", tag = "v0.1.1", features = ["renderer"] }

# While iterating on the kit next to the project:
[patch."https://github.com/awsm-fun/awsm-renderer-kit"]
awsm-renderer-settings = { path = "../awsm-renderer-kit/crates/settings" }
awsm-renderer-stats = { path = "../awsm-renderer-kit/crates/stats" }
```

- Cargo still resolves the git source when a `[patch]` replaces it, so the
  tag must exist on the remote (or be in Cargo's cache) even while patched.
  Before a tag exists, use plain `path =` dependencies.
- Features: worker crate `renderer` (+ `scene` if it loads scenes); a Rust
  page `web` + `ui`; a Rust crate that is both (the Rust family's single
  crate) enables both sets — they only compile web-sys bindings.
- The kit needs what awsm-renderer needs: `--cfg=web_sys_unstable_apis` in
  `.cargo/config.toml` under `[target.wasm32-unknown-unknown] rustflags`
  (an env `RUSTFLAGS` replaces it; add the cfg there too), and a wasm-capable
  clang for `meshopt` (`CC_wasm32_unknown_unknown` etc.).
- awsm-renderer 0.46.0 does not build with `default-features = false`
  (`animation` is referenced unconditionally), so the kit uses its defaults.

### Syncing the JS twins

```sh
../awsm-renderer-kit/scripts/sync-js.sh <project-dir>            # copy into <project>/dom/runtime/kit/
../awsm-renderer-kit/scripts/sync-js.sh <project-dir> --check    # CI: fail if they differ from this kit
../awsm-renderer-kit/scripts/sync-js.sh <project-dir> --dest web/kit
```

Each copy starts with a header naming the kit version. Never edit the copies;
re-sync after bumping the kit tag in the project's Cargo.toml, so the worker
and the page speak the same version.

In a project's CI, without a kit checkout next to it: fetch the pinned
tag's tarball and run its script (a shallow `git clone --branch <tag>` of an
annotated tag prints "… is not a commit!" warnings):

```sh
tag=v0.1.1   # the tag in renderer/Cargo.toml
tmp=$(mktemp -d)
curl -sL "https://codeload.github.com/awsm-fun/awsm-renderer-kit/tar.gz/refs/tags/$tag" | tar -xz -C "$tmp"
"$tmp"/awsm-renderer-kit-*/scripts/sync-js.sh . --check
```

## How to adopt

The pilot, `templates/simple-multithreaded-c`, is the worked example for the
C family. Everything below is what it took there, plus what differs for the
others.

### Every project

1. Bump to awsm-renderer 0.46.0 (`AntiAliasing { .. }` and
   `AntiAliasingConfig { .. }` literals need `taa: None`; the kit replaces
   most of them anyway). Bump the Basis transcoder cache key if the project
   has one (the vendored files are unchanged in 0.46).
2. Add the two crates (above). Keep the storage prefix: stored choices carry
   over (`aa` ids `off`/`smaa`/`msaa` are the same strings; `res_scale` and
   the stats keys are the same keys). Users who never chose keep the new
   desktop default, TAA.
3. Worker: at start, `probe_gpu().await`, then post
   `SettingsToPage::SettingsOptions(SettingsOptions::new(&config, Some(gpu)))`
   and `post_report(collector.placeholder())`. Build the renderer with
   `mode.to_anti_aliasing()` and `stats_logging()`. Before a scene load,
   `scene.post_process.anti_aliasing = scene_anti_aliasing(&renderer.anti_aliasing)`.
   On an `anti-alias`, take the renderer out of the frame loop and run
   `apply_anti_aliasing(.., |e| post_to_page(&SettingsToPage::from(e)))`.
4. Stats, per frame: `begin_renderer_frame`, `lap` between your phases
   (only read the clock when `renderer_detail()`), then
   `end_frame(FrameInput { late, interval, paced_skips, ..FrameInput::new(t, cpu_ms) }, &RendererTimings)`;
   on `Some(window)`, build your sections and `post_report(collector.report(&window, sections))`.
   `mark_ready()` with your `ready`, `reset()` when a scene reload starts,
   `restart()` when the app's own counters restart. Route `stats-view` and
   `stats-page` to `collector.handle`.
5. Delete the project's own AA enum, resolution code, `?reset`, GPU probe,
   compile overlay, stats panel and profiling code.

### The C family (PMC, DD)

As the pilot did:

- `renderer/web/worker.js`: `init(canvas, baseUrl, width, height, aa)`;
  forward `anti-alias`, `stats-view` and `stats-page` whole to one
  `kit(message)` export (`SettingsToWorker::from_js` /
  `StatsToWorker::from_js` dispatch).
- The page waits for `settings-options` before it sizes the canvas,
  transfers it and posts `init` (one message round trip; the worker's wasm
  load dominates anyway).
- `dom/runtime/settings.js` and `stats.js` are replaced by the synced
  `dom/runtime/kit/`. The static Settings modal, notice and compile overlay
  leave `index.html` (the kit builds them into `#ui` with the same ids and
  classes); the buttons and `#stats` stay.
- **Stats move into the render worker.** The page no longer samples the
  bridge block: the worker builds the C app's group from it (`step_work_us`,
  `latest_step`, the timing table's `APP_PHASE` entries) as an app section,
  with a budget lane, and writes `bridge_stats.detail` itself from the open
  groups (the C app keeps reading it). See the pilot's `renderer/src/stats.rs`
  and `Bridge::{set_detail, step_work_us, app_timings}`. The renderer's
  bridge counters (`frame_count`, `gpu_frame_us`, …) are still written, so
  nothing in `bridge.h` changes; the renderer's own timing-table entries are
  gone (they are in the report).
- **PMC**: a second section `physics` (Box3D stages, solve parts, the
  thread-count label: set `label` per report) from `PHYSICS_STAGE` /
  `PHYSICS_SOLVE` entries and `physics_step_us`. The budget had one lane
  for the app worker's whole step: put `budget_ms` on the app section only,
  with `.budget_label("app worker (app + Box3D step)")`. The page's game
  rows ("balls dropped" from `active_poses`) and "last cue" (audio runs on
  the page) go under the app group with `installStats({ sectionRows })`.
  The page row is "frame marks + sound cues".
- **DD**: `ResolutionPolicy { pixel_budget: 2_000_000.0, ..Default::default() }`
  in the worker's `SettingsConfig`. Its game keys (`muted`, `best`,
  `learned`) are **not** cleared by `?reset` any more unless registered
  (`resetSettingsIfAsked({ prefix, extraKeys: [...] })`): register only
  game *options* (probably `muted`), never the score. The Settings checkbox
  for stats is an `extraRows` row calling `stats.setOpen`. `settings.open()`
  serves the title and pause menus; `ui: { escapeStops: true, noticeText: … }`
  replaces its modal.js patch and notice wording. Its renderer `Pacing`
  feeds `FrameInput { interval, paced_skips }` (the display group's pacing
  row and the budget follow). The **auto-tuner**: see
  [An auto-tuner](#an-auto-tuner): `setAutoScale` / `clearAutoScale` replace
  its `tune()` internals (`#tunedPixels`, `#scaleFor`, `#forgetTuning`);
  read GPU ms and the refresh period from `stats.lastReport().summary`
  instead of the bridge and `measureRefresh`; under TAA modes it should
  move `set_taa_render_scale` in the worker, not the canvas (a canvas resize
  resets TAA history) — a small worker message of the game's own.

### The Rust family

- **Page**: `SettingsPanel` and `StatsPanel` are web-sys, no dominator, and
  build the same DOM the v1 dominator code did (`settings-overlay`,
  `settings-modal`, `settings-row`, `res-slider`, `settings-select`,
  `stats-group`, …), so the existing inline CSS keeps working. v2 (Isaac,
  MuJoCo) uses `settings-close` for the × (`SettingsUi { close_class }`) and
  the hint "Lower the resolution if your framerate is low."; both have a
  fixed MSAA 4× and no menu: `SettingsConfig { modes: vec![AaMode::Msaa], .. }`
  and `SettingsUi { show_aa: false, .. }` (or offer the menu now: the kit
  makes it free). v2 has no software-GPU seed: keep
  `software_start: None` to stay identical, or adopt it. Give the panels a
  container element; mount order sets the Tab order as before.
- A Rust main thread has the config itself: `SettingsOptions::new(&config, None)`
  and a later `gpu-info` from the worker (`SettingsPanel::handle`), or wait
  for `settings-options` like the C family.
- **Messages**: the Rust projects tag their own enums `msg` / `quality` /
  `resize`. Post the kit's messages as their own objects and dispatch with
  `SettingsToPage::from_js` / `StatsToPage::from_js` before your own enum
  (they return `None` for anything not the kit's).
- **MT (shared memory)**: the collector runs in the render worker and posts
  `stats` about once a second (measured in the pilot: building a report
  ≈0.07 ms, serializing and posting ≈0.15 ms with every group open, ~5 KB).
  The physics worker's figures (step time, late steps, pool split) still
  live in the `BodyMotion` atomics: read them in the render worker at window
  close and build the `physics` section there, as the C pilot does with the
  bridge. Don't lock the renderer's `profiling` tables from the main thread
  (an `Atomics.wait` trap risk); the collector reads them on the render
  thread. `RenderMsg::Timings` and the `DETAIL_*` bits go away (the worker
  knows the open groups).
- **ST**: one game worker runs renderer and physics: `renderer_label:
  "renderer (game worker)"`, and either keep `renderer_in_budget` with a
  physics section that has no `budget_ms`, or put the whole worker in one
  section. Six phases (`post sound cues`). No COOP/COEP, which the kit does
  not need.
- **Your CSS**: the panels hide elements with an inline `display: none`
  besides `hidden` (see "Showing and hiding"), so the existing
  `.settings-overlay { display: flex }` rules keep working unchanged.
- **Isaac**: `poses` section (`samples/s per robot`, late via
  `FrameInput::late`, dropped stream frames), no budget lane. **MuJoCo**:
  the sim stats come from the MuJoCo JS worker to the main thread: either
  forward them to the render worker and build the `sim` section there (it
  then joins the budget), or use `StatsHooks::app_sections` on the page
  (no budget lane). `{kind:"profile"}` to the MuJoCo worker follows
  `stats-view` (the page knows the open groups).
- **v1 legacy keys**: `SettingsStore::with_legacy_aa_keys("awsm_msaa", "awsm_smaa")`
  and `migrate_legacy_aa()` once at start; `?reset` clears them too.
- The About modal's Stats tab is the app's own text: keep it in step with
  the group labels.

## An auto-tuner

An app that adjusts the resolution itself (Dreidel Derby's GPU auto-tuner)
asks `retune_target(mode)` which lever applies:

- **`CanvasScale`** (no TAA): `settings.setAutoScale(scale)` /
  `SettingsPanel::set_auto_scale`. Ignored while the scale is the user's
  (`isUserScale()`); clamped to the range and the device ceiling and taken
  as given otherwise (the tuner keeps its own stepping and floors);
  remembered as **canvas pixels**, so a window resize keeps about as many
  (`scaleFor`, rounded down to the step, not below the minimum effective
  pixel ratio); forgotten on a new pixel ratio (another display) and when
  the user moves the slider; never stored. `clearAutoScale()` goes back to
  the policy's default (call it when the cost per pixel changes, e.g. on an
  anti-aliasing change). Feed the tuner from `stats.lastReport().summary`
  (`gpuMs`, `fps`, `refreshMs`, `frameInterval`).
- **`TaaRenderScale`** (TAA): `AwsmRenderer::set_taa_render_scale` in the
  worker, through a message of the app's own (below).

## TAA and the resolution setting

- The resolution slider keeps its meaning: the **output** canvas size. TAA
  presets scale the renderer's internal render size on top of it (TAA
  Quality at 80% shades 0.8 × 0.67 of the width).
- A canvas size change changes TAA's output size, which resets its history
  (one frame without anti-aliasing). Fine for a user's drag; an auto-tuner
  under TAA should move `AwsmRenderer::set_taa_render_scale` instead (sync,
  no compile, no history reset; step every few seconds at most, with
  hysteresis). `retune_target(mode)` says which lever applies.
- Switching between TAA presets needs no pipeline (`compiles: false`): no
  overlay. Switching into or out of TAA compiles once per direction; the
  renderer caches the pipelines afterwards.
- If TAA cannot run on a device, the renderer falls back to MSAA 4× once and
  says why: `aa-compile-done` carries `mode: "msaa"` and `taaFallback`; the
  panels disable the TAA entries (the stored choice is kept).

## Decisions

The spec left these to the kit; this is what it does and why.

- **Internal web crate**: yes, `awsm-renderer-kit-web`, so storage, URL,
  `postMessage` and the modal behaviour exist once for both crates.
- **Envelope**: `{type: "<kebab-case>", …camelCase}`, the C family's. The
  options message is `settings-options` rather than `options`, so it cannot
  collide with an app's generic message names.
- **The GPU probe runs before `init`** and travels inside
  `settings-options`, so the page knows a software GPU before it picks the
  first size and mode (the software default, Off, could not apply otherwise).
  `gpu-info` remains for apps that probe later.
- **Stats are computed and formatted in the worker**, including the frame
  budget, the display group (from the page's raw frame gaps) and the page
  group (from the page's raw work times): every number and string comes
  from one Rust implementation, and the JS is a view.
- **The C family's app section is built in the render worker** from the
  bridge block, not on the page as the spec sketched: the worker already
  maps the block, this keeps every figure in Rust (no EMA or windowing in
  JS), and it lets the app join the frame budget. The page-side section hook
  still exists (`appSections` / `StatsHooks::app_sections`) for sources only
  the page sees; those don't join the budget.
- **Rows carry the whole display string** (`"1.23 ms / frame"`), no separate
  `unit`: the view splits numbers from units with the rule every project
  used (`format::value_pieces`, `/<?\d[\d.]*%?/`).
- **Windows** are the v2 model (the worker windows presented frames, about
  one second; intervals over 250 ms are pauses) plus the C/v1 trust rule
  (a window counts only if it lasted 200–3000 ms of wall time) and the
  60-frame warm-up after `ready`.
- **Compile overlay line**: the mode's label (`TAA Quality (67%)`) instead
  of `MSAA 4× on · SMAA off`, which no longer describes seven modes. Mode
  descriptions are tooltips (`title`), so the modal looks as before.
- **Inline z-index** on the notice (25) and the compile overlay (30), as the
  Rust v1 code had, so the overlays stack right without app CSS.
- **`?aa=` that names no offered mode** is ignored with a log line.
- **Coarse-pointer defaults** are unchanged (Off at 60%); software GPU is
  Off at 50%; desktop is TAA native.

## Working on the kit

```sh
scripts/check.sh                       # fmt, cargo test, node tests, clippy (wasm + native), wasm check per feature set
node --test js/test/*.test.mjs         # the JS twins against fixtures/resolution.json
scripts/ui-harness.sh <out-dir> [port] # the Rust panels on a page with fake data (examples/ui-harness)
```

Changing the policy: change the Rust, regenerate or extend
`fixtures/resolution.json`, mirror the change in `js/settings.js`, run both
test suites, then bump the version and the CHANGELOG.
