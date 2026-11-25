# Changelog

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
