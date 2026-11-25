//! Stats shared by awsm-renderer apps.
//!
//! - **Core** (no features, unit-tested natively): the collector
//!   ([`StatsCollector`]: presented-frame windows, the 60-frame warm-up, the
//!   200–3000 ms trust rule, EMA 0.35, the refresh-rate estimator, missed
//!   frames, the frame budget, renderer CPU scopes and GPU passes), the
//!   [`FrameReport`] it writes (standard groups plus the app's
//!   [`AppSection`]s), formatting, and the protocol ([`StatsToPage`],
//!   [`StatsToWorker`]).
//! - **`renderer`** (worker side): `RendererTimings` (awsm-renderer's
//!   profiling as a [`TimingSource`]), `StatsCollector::begin_renderer_frame`
//!   (switches the CPU timing tier with the panel), [`stats_logging`] and
//!   [`post_report`].
//! - **`web`**: the messages as JS values.
//! - **`ui`**: [`StatsPanel`], the web-sys panel. The JS twin for pages
//!   without Rust is `js/stats.js` in the kit repo.
//!
//! Transport is the app's choice: the kit serializes; the app moves it
//! (`postMessage` from a render worker, or anything else).

mod collector;
mod ema;
pub mod format;
mod protocol;
mod refresh;
mod report;

#[cfg(feature = "renderer")]
mod renderer;

#[cfg(feature = "ui")]
mod ui;

pub use collector::{
    duration_row, frame_headline, step_headline, FrameInput, LateStats, ScopeKind, ScopeTotal,
    SectionSpec, StatsCollector, StatsConfig, TimingSource, Window, BUDGET, DISPLAY, FOLD_UNDER_MS,
    GAP_MS, GPU, GPU_EMPTY_WINDOWS, LATE_WINDOWS, MAX_WINDOW_MS, MIN_WINDOW_MS, PAGE, RENDERER,
    SHARE_WARN, WARMUP_FRAMES,
};
pub use ema::{smooth, NamedMeans, Smoother, EMA_ALPHA};
pub use protocol::{PagePart, StatsToPage, StatsToWorker};
pub use refresh::{RefreshEstimator, REFRESH_INLIER, REFRESH_MIN_SAMPLES, REFRESH_SAMPLES};
pub use report::{AppSection, FrameReport, StatGroup, StatRow, Summary};

#[cfg(feature = "renderer")]
pub use renderer::{post_report, stats_logging, RendererTimings};

#[cfg(feature = "ui")]
pub use ui::{StatsHooks, StatsPanel};
