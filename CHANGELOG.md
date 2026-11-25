# Changelog

## Unreleased

- `StatsCollector::skip_lap(now)`: moves the phase clock without charging a
  phase (a single-threaded app's physics steps inside its frame). `lap` with
  an index outside the phases did the same, undocumented; it still does.
- README: checking a project's synced JS in CI from the tag's tarball.

## 0.1.1

Review fixes, and what the C-family port asked for. No breaking change
(except for exhaustive struct literals of `StatsHooks` / `StatGroup`).

Fixes:
- **Panels in the Rust templates' CSS**: the kit hid its overlays and rows
  with the `hidden` attribute only, which `.settings-overlay { display: flex }`
  and `.settings-row { display: flex }` override: the Settings modal, the
  notice and the compile overlay would all have shown at once. Every kit
  element is now shown and hidden with `hidden` plus an inline
  `display: none` (`dom::set_shown`, `setShown`).
- **TAA fallback**: when `set_anti_aliasing` cannot set TAA up, the renderer
  falls back to MSAA 4× itself and returns the error; `apply_anti_aliasing`
  skipped the commit then, leaving the fallback's material pipelines
  uncompiled. It now commits whenever the structure moved.
- `apply_anti_aliasing` no longer commits for a change that is not
  structural (between TAA presets): no hitch, as `compiles: false` says.
- A quick second pick (between TAA presets, which raise no overlay) could
  move the menu back for a moment when the first pick's `aa-compile-done`
  arrived: `done` now updates the menu only when it answers the latest
  `aa-compile-start` for the current pick.
- JS `Settings.start({ extraRows })` threw without a hint line.
- The stats panels do no DOM work while hidden (the report is kept and drawn
  when the panel shows; `lastReport()` stays current).

Added:
- `StatGroup::budget_label` (`budgetLabel`): a budget lane labelled apart
  from its group (PMC's "app worker (app + Box3D step)").
- `StatsHooks::section_rows` / `installStats({ sectionRows })`: page-side
  rows at the end of an expanded group the worker sent (PMC's game rows and
  "last cue").
- An auto-tuner's canvas lever: `ResolutionState::{set_auto_scale,
  clear_auto_scale, scale_for, is_auto_tuned, dpr, cap}`, and on the panels
  `is_user_scale` / `isUserScale`, `dpr`, `set_auto_scale` /
  `setAutoScale`, `clear_auto_scale` / `clearAutoScale` (Dreidel Derby's
  `tune()` state, without its policy). Fixture-tested in both twins.
- `KIT_VERSION` in `js/kit-web.js`; `Settings` warns when the worker's kit
  version differs; `scripts/check.sh` keeps it equal to Cargo.toml's.

## 0.1.0

First release, for awsm-renderer 0.46.0. Replaces the settings and stats code
that seven awsm-renderer apps carried as copies.

- `awsm-renderer-settings`: seven anti-aliasing modes (Off, SMAA, MSAA 4×,
  TAA native, TAA Quality 0.67, Balanced 0.58, Performance 0.5) with ids,
  labels and descriptions; defaults per device class (desktop TAA, touch Off,
  software GPU Off); the shared resolution policy (0.5–1.0 in 0.05 steps,
  2.8 MP budget, effective DPR floor 1, touch start 0.6, software start 0.5,
  8192 max texture), tested against the policy the templates shipped;
  `SettingsStore` (localStorage, `?reset` of the kit's own keys only, `?aa=`),
  the Rust v1 legacy-key migration; `apply_anti_aliasing` and `probe_gpu` for
  the worker; `scene_anti_aliasing`; the settings protocol; a web-sys
  `SettingsPanel`.
- `awsm-renderer-stats`: `StatsCollector` (presented-frame windows, 60-frame
  warm-up, 200–3000 ms trust rule, EMA 0.35, refresh-rate estimate from the
  page's frame gaps, missed frames with pacing, frame budget, renderer CPU
  scopes at `SubFrame` and GPU passes while their groups are open);
  `FrameReport` with app sections; the stats protocol; a web-sys
  `StatsPanel`.
- `js/`: `settings.js`, `stats.js`, `kit-web.js`, the twins for a JS page;
  `scripts/sync-js.sh` copies them into a project and checks copies.
- Pilot: `templates/simple-multithreaded-c`.
