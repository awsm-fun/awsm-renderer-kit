//! The worker side: counts presented frames, windows them, smooths every
//! figure and writes the report.
//!
//! **Windows.** Every presented frame goes in ([`StatsCollector::end_frame`]).
//! A frame interval longer than [`GAP_MS`] is a pause (a hidden tab, a
//! debugger stop, an anti-aliasing compile), not a slow frame: it is left out
//! with its frame, so pauses never count as missed frames. A window closes
//! after [`StatsConfig::report_span_ms`] of presented frames, and is trusted
//! only if it lasted [`MIN_WINDOW_MS`] to [`MAX_WINDOW_MS`] of wall time;
//! otherwise the last figures stay up.
//!
//! **Start-up.** Nothing is counted until the scene is on screen
//! ([`StatsCollector::mark_ready`]), and then only after [`WARMUP_FRAMES`]
//! more frames: start-up (cold caches, the browser still optimizing the
//! wasm) reads several times the settled cost. Counted in frames, not time,
//! so a stalled renderer keeps waiting instead of "warming up" through the
//! stall.
//!
//! **Cost.** A collapsed panel costs nothing extra. Expanding the renderer
//! group times the app's frame phases and switches awsm-renderer's CPU
//! timing tier to `SubFrame` (`Off` otherwise); expanding the GPU group
//! passes on the per-pass GPU times, which the `Frame` tier behind the
//! headline already measures. While the panel is hidden every group counts
//! as collapsed. The page says which groups are open (`stats-view`).
//!
//! **Missed frames.** A window of presented frames spanned
//! `round(span / period)` refreshes; every refresh beyond the frames
//! presented (and those a paced renderer skipped on purpose) showed the
//! previous picture again. The total is kept signed, so a window that
//! counts one early and the next one late even out.

use crate::ema::{smooth, NamedMeans, Smoother};
use crate::format::{duration, fixed, ordinal, per_frame, share, tenths};
use crate::protocol::{PagePart, StatsToWorker};
use crate::refresh::RefreshEstimator;
use crate::report::{AppSection, FrameReport, StatGroup, StatRow, Summary};

/// A frame interval longer than this is a pause (ms).
pub const GAP_MS: f64 = 250.0;
/// A window is trusted only between these wall-clock lengths (ms).
pub const MIN_WINDOW_MS: f64 = 200.0;
pub const MAX_WINDOW_MS: f64 = 3000.0;
/// Frames presented after `ready` before anything is counted.
pub const WARMUP_FRAMES: u32 = 60;
/// The app's late frames turn red after this many windows in a row with
/// any: a steady condition, not a one-off such as a tab switch.
pub const LATE_WINDOWS: u32 = 3;
/// Trusted windows without a GPU sample before the GPU headline says `n/a`
/// (the device has no timestamp queries).
pub const GPU_EMPTY_WINDOWS: u32 = 3;
/// Renderer CPU scopes faster than this (ms) fold into one "N more" row.
pub const FOLD_UNDER_MS: f64 = 0.001;
/// A lane's share of the frame budget from which its bar warns (the view's
/// CSS: `stats-warn`), and over which it is over (`stats-over`).
pub const SHARE_WARN: f64 = 0.8;

/// Group ids of the standard groups.
pub const BUDGET: &str = "budget";
pub const DISPLAY: &str = "display";
pub const GPU: &str = "gpu";
pub const RENDERER: &str = "renderer";
pub const PAGE: &str = "page";

/// What an app configures.
#[derive(Clone, Debug, PartialEq)]
pub struct StatsConfig {
    /// The renderer group's label (and its budget lane's).
    pub renderer_label: String,
    /// The page group's label (and its budget lane's).
    pub page_label: String,
    /// The app's frame phases, in the order they run, as the renderer
    /// group lists them ([`StatsCollector::lap`] takes an index into this).
    pub phases: Vec<String>,
    /// The phase that is `AwsmRenderer::render`: awsm-renderer's own CPU
    /// scopes are listed under it, as `prepare` / `passes` / `submit` /
    /// `hooks`, with what is left over.
    pub render_phase: Option<usize>,
    /// The app's sections, in order: their place and their "starting…"
    /// headers before the first report.
    pub sections: Vec<SectionSpec>,
    /// The display group's `pacing` row (every refresh, or every Nth by
    /// choice).
    pub pacing_row: bool,
    /// Whether the renderer is a lane of the frame budget (a project whose
    /// renderer shares a thread with the app may count both in one app
    /// section instead).
    pub renderer_in_budget: bool,
    /// Presented-frame time per report (ms).
    pub report_span_ms: f64,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            renderer_label: "renderer (render worker)".to_string(),
            page_label: "page (main thread)".to_string(),
            phases: Vec::new(),
            render_phase: None,
            sections: Vec::new(),
            pacing_row: true,
            renderer_in_budget: true,
            report_span_ms: 1000.0,
        }
    }
}

/// An app section's place and header.
#[derive(Clone, Debug, PartialEq)]
pub struct SectionSpec {
    pub id: String,
    pub label: String,
    pub lane: Option<String>,
}

impl SectionSpec {
    pub fn new(id: impl Into<String>, label: impl Into<String>, lane: Option<&str>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            lane: lane.map(str::to_string),
        }
    }

    /// The section's group, empty.
    pub fn group(&self) -> AppSection {
        StatGroup {
            id: self.id.clone(),
            label: self.label.clone(),
            lane: self.lane.clone(),
            ..StatGroup::default()
        }
    }
}

/// The kind of an awsm-renderer CPU scope inside `render()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    Prep,
    Pass,
    Submit,
    Hook,
}

impl ScopeKind {
    /// Subgroups under the render phase, in the order they run, with the
    /// prefix the renderer gives that kind's names.
    pub const GROUPS: [(ScopeKind, &'static str, &'static str); 4] = [
        (ScopeKind::Prep, "prepare", "Prep: "),
        (ScopeKind::Pass, "passes", ""),
        (ScopeKind::Submit, "submit", "Submit: "),
        (ScopeKind::Hook, "hooks", ""),
    ];
}

/// A CPU scope's running total.
#[derive(Clone, Debug, PartialEq)]
pub struct ScopeTotal {
    pub name: String,
    pub kind: ScopeKind,
    pub total_ms: f64,
}

/// Where the renderer's running timing totals come from. The `renderer`
/// feature reads awsm-renderer's profiling (`RendererTimings`); tests feed
/// their own. Read only when a window opens or closes.
pub trait TimingSource {
    /// The GPU whole-frame totals: (total ms, sampled frames).
    fn gpu_frame(&self) -> Option<(f64, u64)>;
    /// Each GPU pass's total ms (not the whole-frame entry).
    fn gpu_passes(&self) -> Vec<(String, f64)>;
    /// Each CPU scope inside `render()` (not the whole-`render()` scope).
    fn cpu_scopes(&self) -> Vec<ScopeTotal>;
}

/// No renderer timings (a renderer built without GPU timing, or a test).
impl TimingSource for () {
    fn gpu_frame(&self) -> Option<(f64, u64)> {
        None
    }
    fn gpu_passes(&self) -> Vec<(String, f64)> {
        Vec::new()
    }
    fn cpu_scopes(&self) -> Vec<ScopeTotal> {
        Vec::new()
    }
}

/// One presented frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameInput {
    /// The frame's `requestAnimationFrame` timestamp (ms).
    pub t_ms: f64,
    /// The CPU time the frame took on this thread (ms).
    pub cpu_ms: f64,
    /// The app had not published what this frame shows (an app's notion;
    /// see [`Window::late`]).
    pub late: bool,
    /// Refreshes per frame the renderer paces at (1: every refresh).
    pub interval: u32,
    /// Refreshes the renderer skipped on purpose since the last frame.
    pub paced_skips: u32,
}

impl FrameInput {
    pub fn new(t_ms: f64, cpu_ms: f64) -> Self {
        Self {
            t_ms,
            cpu_ms,
            late: false,
            interval: 1,
            paced_skips: 0,
        }
    }
}

/// A closed, trusted window, for the app to build its sections from before
/// [`StatsCollector::report`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    /// Its wall-clock length: rates are per these seconds.
    pub seconds: f64,
    /// Frames presented in it.
    pub frames: u32,
    /// The presented frames' span (ms, pauses excluded).
    pub span_ms: f64,
    pub late: LateStats,
}

/// The app's late frames (`FrameInput::late`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LateStats {
    /// In this window.
    pub frames: u32,
    /// Per second, in this window.
    pub rate: f64,
    /// Since counting started.
    pub total: u64,
    /// Late in [`LATE_WINDOWS`] windows in a row.
    pub bad: bool,
}

impl LateStats {
    /// The row every project shows: `"N / s · T total"`, red when bad.
    pub fn row(&self, label: &str) -> StatRow {
        StatRow::new(
            label,
            format!("{} / s · {} total", fixed(self.rate, 0), self.total),
        )
        .bad(self.bad)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    /// Before `ready`.
    Starting,
    /// Frames since `ready`.
    Warming(u32),
    Live,
}

/// The phase clock of one frame (only while the renderer group is open).
struct Clock {
    t: f64,
    ms: Vec<f64>,
}

/// See the module docs.
pub struct StatsCollector {
    config: StatsConfig,
    stage: Stage,
    shown: bool,
    open: Vec<String>,

    // The window being filled.
    window_start: f64,
    prev_t: Option<f64>,
    frames: u32,
    span_ms: f64,
    cpu_ms: f64,
    late: u32,
    paced_skips: u32,
    interval: u32,

    // The renderer group's breakdown.
    clock: Option<Clock>,
    phase_ms: Vec<f64>,
    phase_frames: u32,
    /// The renderer's scope totals when this breakdown started (`None`: take
    /// them at the next frame).
    scope_base: Option<Vec<ScopeTotal>>,
    /// The GPU whole-frame totals at the window's start.
    gpu_base: Option<(f64, u64)>,
    /// The GPU group's pass totals and sampled-frame count at its start.
    gpu_pass_base: Option<(u64, Vec<(String, f64)>)>,

    // From the page.
    refresh: RefreshEstimator,
    page_parts: Vec<(String, f64)>,
    page_frames: u32,

    // Smoothed.
    fps: Option<f64>,
    cpu: Option<f64>,
    gpu: Option<f64>,
    page: Option<f64>,
    page_means: NamedMeans,
    phase_means: NamedMeans,
    scope_means: Vec<NamedMeans>,
    gpu_pass_means: NamedMeans,
    gpu_empty: u32,
    missed_rate: Option<f64>,
    missed_total: i64,
    late_total: u64,
    late_windows: u32,
    app: Smoother,

    /// The renderer's CPU timing tier: the app's own, raised while the
    /// renderer group is open (`renderer` feature).
    #[cfg_attr(not(feature = "renderer"), allow(dead_code))]
    pub(crate) cpu_tier: crate::tier::TierFollower,
}

impl StatsCollector {
    pub fn new(config: StatsConfig) -> Self {
        let phases = config.phases.len();
        Self {
            config,
            stage: Stage::Starting,
            shown: false,
            open: Vec::new(),
            window_start: 0.0,
            prev_t: None,
            frames: 0,
            span_ms: 0.0,
            cpu_ms: 0.0,
            late: 0,
            paced_skips: 0,
            interval: 1,
            clock: None,
            phase_ms: vec![0.0; phases],
            phase_frames: 0,
            scope_base: None,
            gpu_base: None,
            gpu_pass_base: None,
            refresh: RefreshEstimator::default(),
            page_parts: Vec::new(),
            page_frames: 0,
            fps: None,
            cpu: None,
            gpu: None,
            page: None,
            page_means: NamedMeans::default(),
            phase_means: NamedMeans::default(),
            scope_means: vec![NamedMeans::default(); ScopeKind::GROUPS.len()],
            gpu_pass_means: NamedMeans::default(),
            gpu_empty: 0,
            missed_rate: None,
            missed_total: 0,
            late_total: 0,
            late_windows: 0,
            cpu_tier: crate::tier::TierFollower::default(),
            app: Smoother::default(),
        }
    }

    pub fn config(&self) -> &StatsConfig {
        &self.config
    }

    // ── What the page says ─────────────────────────────────────────────────

    /// A `stats-view` or `stats-page` message.
    pub fn handle(&mut self, message: StatsToWorker) {
        match message {
            StatsToWorker::StatsView { shown, open } => self.set_view(shown, open),
            StatsToWorker::StatsPage { gaps, page, frames } => {
                self.page_sample(&gaps, &page, frames)
            }
        }
    }

    /// Whether the panel is shown, and which groups are expanded.
    pub fn set_view(&mut self, shown: bool, open: Vec<String>) {
        let renderer_was = self.is_open(RENDERER);
        let gpu_was = self.is_open(GPU);
        self.shown = shown;
        self.open = open;
        if self.is_open(RENDERER) != renderer_was {
            // A fresh breakdown: totals from before are not ours to count.
            self.scope_base = None;
            self.phase_ms.iter_mut().for_each(|ms| *ms = 0.0);
            self.phase_frames = 0;
            self.phase_means.clear();
            self.scope_means.iter_mut().for_each(NamedMeans::clear);
        }
        if self.is_open(GPU) != gpu_was {
            self.gpu_pass_base = None;
            self.gpu_pass_means.clear();
        }
    }

    /// Whether the group `id` is expanded on a shown panel: measure its
    /// costly detail only then (an app section gates its own on this).
    pub fn is_open(&self, id: &str) -> bool {
        self.shown && self.open.iter().any(|open| open == id)
    }

    /// The page's own frame gaps (for the refresh rate) and its work since
    /// its last sample (`parts`: named ms totals over `frames` frames).
    pub fn page_sample(&mut self, gaps: &[f64], parts: &[PagePart], frames: u32) {
        self.refresh.push(gaps.iter().copied());
        for part in parts {
            if !part.ms.is_finite() || part.ms < 0.0 {
                continue;
            }
            match self.page_parts.iter_mut().find(|(n, _)| *n == part.name) {
                Some(entry) => entry.1 += part.ms,
                None => self.page_parts.push((part.name.clone(), part.ms)),
            }
        }
        self.page_frames += frames;
    }

    /// The refresh period the page measured (ms), once there is one.
    pub fn refresh_ms(&self) -> Option<f64> {
        self.refresh.period_ms()
    }

    // ── Lifecycle ──────────────────────────────────────────────────────────

    /// The scene is on screen: start warming up. Returns the "warming up…"
    /// report to post.
    pub fn mark_ready(&mut self) -> FrameReport {
        self.restart_counting();
        self.stage = Stage::Warming(0);
        self.placeholder()
    }

    /// Back to before `ready` (a scene reload starts): returns the
    /// "starting…" report to post.
    pub fn reset(&mut self) -> FrameReport {
        self.restart_counting();
        self.stage = Stage::Starting;
        self.placeholder()
    }

    /// Everything measured so far is dropped and counting starts over after a
    /// warm-up (a new app instance whose counters start from zero). Before
    /// `ready` this is [`Self::reset`].
    pub fn restart(&mut self) -> FrameReport {
        if self.stage == Stage::Starting {
            return self.reset();
        }
        self.mark_ready()
    }

    fn restart_counting(&mut self) {
        let config = std::mem::take(&mut self.config);
        let (shown, open) = (self.shown, std::mem::take(&mut self.open));
        let refresh = std::mem::take(&mut self.refresh);
        *self = Self::new(config);
        self.shown = shown;
        self.open = open;
        self.refresh = refresh;
    }

    /// The report while there are no numbers: every headline says so.
    pub fn placeholder(&self) -> FrameReport {
        let status = match self.stage {
            Stage::Starting => "starting…",
            Stage::Warming(_) | Stage::Live => "warming up…",
        };
        self.placeholder_with(status)
    }

    fn placeholder_with(&self, status: &str) -> FrameReport {
        let group = |group: StatGroup| group.headline(status);
        FrameReport {
            status: Some(status.to_string()),
            budget: group(StatGroup::new(BUDGET, "frame budget")),
            display: StatGroup::new(DISPLAY, "display").headline("—"),
            gpu: group(StatGroup::new(GPU, "GPU").lane("gpu")),
            renderer: group(StatGroup::new(RENDERER, &self.config.renderer_label).lane("renderer")),
            app: self
                .config
                .sections
                .iter()
                .map(|spec| spec.group().headline(status))
                .collect(),
            page: group(StatGroup::new(PAGE, &self.config.page_label).lane("page")),
            summary: Summary {
                refresh_ms: self.refresh.period_ms(),
                frame_interval: self.interval.max(1),
                ..Summary::default()
            },
        }
    }

    // ── Per frame ──────────────────────────────────────────────────────────

    /// Whether the renderer group's breakdown is being measured: the app's
    /// phases are timed and awsm-renderer's CPU tier should be `SubFrame`.
    pub fn renderer_detail(&self) -> bool {
        self.is_open(RENDERER)
    }

    /// Starts a frame at `now_ms` (`performance.now()`). With the
    /// `renderer` feature, `begin_renderer_frame` also sets the renderer's
    /// CPU timing tier.
    pub fn begin_frame(&mut self, now_ms: f64) {
        self.clock = self.renderer_detail().then(|| Clock {
            t: now_ms,
            ms: vec![0.0; self.config.phases.len()],
        });
    }

    /// Charges the time since the last lap (or the frame's start) to
    /// `phase` (an index into [`StatsConfig::phases`]). A no-op while the
    /// renderer group is collapsed. An index outside the phases charges
    /// nothing (see [`Self::skip_lap`]).
    pub fn lap(&mut self, phase: usize, now_ms: f64) {
        if let Some(clock) = &mut self.clock {
            if let Some(ms) = clock.ms.get_mut(phase) {
                *ms += now_ms - clock.t;
            }
            clock.t = now_ms;
        }
    }

    /// Moves the phase clock to `now_ms` without charging any phase: time
    /// inside the frame that is not the renderer's (a single-threaded app's
    /// physics steps, which its own section reports).
    pub fn skip_lap(&mut self, now_ms: f64) {
        if let Some(clock) = &mut self.clock {
            clock.t = now_ms;
        }
    }

    /// Ends a presented frame. Returns a window when one closed and was
    /// trusted: build the app's sections from it, then call
    /// [`Self::report`] and post the result.
    pub fn end_frame(&mut self, input: FrameInput, timings: &impl TimingSource) -> Option<Window> {
        let clock = self.clock.take();
        if self.renderer_detail() {
            if self.scope_base.is_none() {
                // The breakdown starts now. This frame's scopes are already
                // in the renderer's totals, so its phases are left out too.
                self.scope_base = Some(timings.cpu_scopes());
                self.phase_ms.iter_mut().for_each(|ms| *ms = 0.0);
                self.phase_frames = 0;
            } else if let Some(clock) = clock {
                for (sum, ms) in self.phase_ms.iter_mut().zip(clock.ms) {
                    *sum += ms;
                }
                self.phase_frames += 1;
            }
        }
        if self.is_open(GPU) && self.gpu_pass_base.is_none() {
            let sampled = timings.gpu_frame().map_or(0, |frame| frame.1);
            self.gpu_pass_base = Some((sampled, timings.gpu_passes()));
        }
        self.interval = input.interval.max(1);

        match self.stage {
            Stage::Starting => {
                self.prev_t = Some(input.t_ms);
                None
            }
            Stage::Warming(frames) => {
                self.prev_t = Some(input.t_ms);
                if frames + 1 >= WARMUP_FRAMES {
                    self.stage = Stage::Live;
                    self.start_window(input.t_ms, timings);
                    self.restart_breakdowns(timings);
                } else {
                    self.stage = Stage::Warming(frames + 1);
                }
                None
            }
            Stage::Live => {
                let prev = self.prev_t.replace(input.t_ms)?;
                let dt = input.t_ms - prev;
                if !(dt > 0.0 && dt <= GAP_MS) {
                    return None;
                }
                self.frames += 1;
                self.span_ms += dt;
                self.cpu_ms += input.cpu_ms.max(0.0);
                self.late += u32::from(input.late);
                self.paced_skips += input.paced_skips;
                if self.span_ms < self.config.report_span_ms {
                    return None;
                }
                self.close_window(input.t_ms, timings)
            }
        }
    }

    /// The breakdowns start over from the renderer's totals now (the first
    /// window, and after a window that was not trusted).
    fn restart_breakdowns(&mut self, timings: &impl TimingSource) {
        if self.renderer_detail() {
            self.scope_base = Some(timings.cpu_scopes());
        }
        self.phase_ms.iter_mut().for_each(|ms| *ms = 0.0);
        self.phase_frames = 0;
        if self.is_open(GPU) {
            let sampled = timings.gpu_frame().map_or(0, |frame| frame.1);
            self.gpu_pass_base = Some((sampled, timings.gpu_passes()));
        }
    }

    /// A new window from `t`: the GPU baseline and empty counters.
    fn start_window(&mut self, t: f64, timings: &impl TimingSource) {
        self.window_start = t;
        self.frames = 0;
        self.span_ms = 0.0;
        self.cpu_ms = 0.0;
        self.late = 0;
        self.paced_skips = 0;
        self.gpu_base = timings.gpu_frame();
    }

    fn close_window(&mut self, t: f64, timings: &impl TimingSource) -> Option<Window> {
        let wall = t - self.window_start;
        let gpu_now = timings.gpu_frame();
        let gpu_ms = match (self.gpu_base, gpu_now) {
            (Some((t0, c0)), Some((t1, c1))) if c1 > c0 => Some((t1 - t0) / (c1 - c0) as f64),
            _ => None,
        };
        let (frames, span_ms, cpu_ms, late, skips) = (
            self.frames,
            self.span_ms,
            self.cpu_ms,
            self.late,
            self.paced_skips,
        );
        let trusted = (MIN_WINDOW_MS..MAX_WINDOW_MS).contains(&wall);
        if !trusted {
            self.start_window(t, timings);
            self.restart_breakdowns(timings);
            return None;
        }
        let seconds = wall / 1000.0;

        self.fps = Some(smooth(self.fps, f64::from(frames) / seconds));
        if frames > 0 {
            self.cpu = Some(smooth(self.cpu, cpu_ms / f64::from(frames)));
        }
        match gpu_ms {
            Some(ms) => {
                self.gpu_empty = 0;
                self.gpu = Some(smooth(self.gpu, ms));
            }
            None => self.gpu_empty += 1,
        }

        if self.page_frames > 0 {
            let n = f64::from(self.page_frames);
            let total: f64 = self.page_parts.iter().map(|p| p.1).sum();
            self.page = Some(smooth(self.page, total / n));
            let parts = std::mem::take(&mut self.page_parts);
            self.page_means
                .update(parts.into_iter().map(|(name, ms)| (name, ms / n)));
            self.page_frames = 0;
        }

        if let Some(period) = self.refresh.period_ms() {
            if span_ms <= wall + GAP_MS {
                let missed =
                    (span_ms / period).round() as i64 - i64::from(frames) - i64::from(skips);
                self.missed_total += missed;
                self.missed_rate = Some(missed.max(0) as f64 / seconds);
            }
        }

        self.late_total += u64::from(late);
        self.late_windows = if late > 0 { self.late_windows + 1 } else { 0 };
        let late = LateStats {
            frames: late,
            rate: f64::from(late) / seconds,
            total: self.late_total,
            bad: self.late_windows >= LATE_WINDOWS,
        };

        self.close_breakdowns(timings);
        self.start_window(t, timings);

        Some(Window {
            seconds,
            frames,
            span_ms,
            late,
        })
    }

    /// The renderer group's phases and scopes, and the GPU group's passes,
    /// over the frames they were measured on.
    fn close_breakdowns(&mut self, timings: &impl TimingSource) {
        if self.renderer_detail() && self.phase_frames > 0 {
            let n = f64::from(self.phase_frames);
            let phases = self
                .config
                .phases
                .iter()
                .zip(&self.phase_ms)
                .map(|(name, ms)| (name.clone(), ms / n))
                .collect::<Vec<_>>();
            self.phase_means.update(phases);
            if let Some(base) = self.scope_base.take() {
                let now = timings.cpu_scopes();
                for (index, (kind, _, _)) in ScopeKind::GROUPS.iter().enumerate() {
                    let means = now
                        .iter()
                        .filter(|scope| scope.kind == *kind)
                        .filter_map(|scope| {
                            let before = base
                                .iter()
                                .find(|b| b.name == scope.name)
                                .map_or(0.0, |b| b.total_ms);
                            (scope.total_ms > before)
                                .then(|| (scope.name.clone(), (scope.total_ms - before) / n))
                        })
                        .collect::<Vec<_>>();
                    self.scope_means[index].update(means);
                }
                self.scope_base = Some(now);
            }
            self.phase_ms.iter_mut().for_each(|ms| *ms = 0.0);
            self.phase_frames = 0;
        } else if !self.renderer_detail() {
            self.phase_means.clear();
            self.scope_means.iter_mut().for_each(NamedMeans::clear);
        }

        if self.is_open(GPU) {
            if let Some((c0, base)) = self.gpu_pass_base.take() {
                let sampled = timings.gpu_frame().map_or(0, |frame| frame.1);
                let now = timings.gpu_passes();
                if sampled > c0 {
                    let n = (sampled - c0) as f64;
                    let means = now
                        .iter()
                        .filter_map(|(name, total)| {
                            let before = base.iter().find(|b| b.0 == *name).map_or(0.0, |b| b.1);
                            (*total > before).then(|| (name.clone(), (total - before) / n))
                        })
                        .collect::<Vec<_>>();
                    self.gpu_pass_means.update(means);
                }
                self.gpu_pass_base = Some((sampled, now));
            }
        } else {
            self.gpu_pass_means.clear();
        }
    }

    /// Smooths an app figure under `key` (cleared by [`Self::restart`]).
    pub fn smooth(&mut self, key: &str, value: f64) -> f64 {
        self.app.smooth(key, value)
    }

    /// An app figure's current average.
    pub fn smoothed(&self, key: &str) -> Option<f64> {
        self.app.get(key)
    }

    // ── The report ─────────────────────────────────────────────────────────

    /// The report for a window, with the app's sections (missing configured
    /// ones show `—`).
    pub fn report(&self, window: &Window, sections: Vec<AppSection>) -> FrameReport {
        let _ = window;
        let app = self.order_sections(sections);
        let period = self.refresh.period_ms();
        let hz = period.map(|p| format!("{} Hz", fixed(1000.0 / p, 0)));
        FrameReport {
            status: None,
            budget: self.budget_group(period, hz.as_deref(), &app),
            display: self.display_group(hz.as_deref()),
            gpu: self.gpu_group(),
            renderer: self.renderer_group(),
            app,
            page: self.page_group(),
            summary: Summary {
                refresh_ms: period,
                fps: self.fps,
                cpu_ms: self.cpu,
                gpu_ms: self.gpu,
                page_ms: self.page,
                frame_interval: self.interval,
            },
        }
    }

    fn order_sections(&self, mut sections: Vec<AppSection>) -> Vec<AppSection> {
        let mut ordered = Vec::with_capacity(sections.len() + self.config.sections.len());
        for spec in &self.config.sections {
            match sections.iter().position(|s| s.id == spec.id) {
                Some(index) => ordered.push(sections.remove(index)),
                None => ordered.push(spec.group().headline("—")),
            }
        }
        ordered.extend(sections);
        for section in &mut ordered {
            if !self.is_open(&section.id) {
                section.rows.clear();
            }
        }
        ordered
    }

    fn budget_group(&self, period: Option<f64>, hz: Option<&str>, app: &[AppSection]) -> StatGroup {
        let group = StatGroup::new(BUDGET, "frame budget");
        let mut lanes: Vec<(String, f64, String)> = Vec::new();
        if let Some(ms) = self.gpu {
            lanes.push(("GPU".into(), ms, "gpu".into()));
        }
        if let (true, Some(ms)) = (self.config.renderer_in_budget, self.cpu) {
            lanes.push((self.config.renderer_label.clone(), ms, "renderer".into()));
        }
        for section in app {
            if let Some(ms) = section.budget_ms {
                let lane = section.lane.clone().unwrap_or_else(|| "sim".into());
                let label = section.budget_label.as_ref().unwrap_or(&section.label);
                lanes.push((label.clone(), ms, lane));
            }
        }
        if let Some(ms) = self.page {
            lanes.push((self.config.page_label.clone(), ms, "page".into()));
        }
        let (Some(period), Some(hz)) = (period, hz) else {
            return group.headline("measuring…");
        };
        if lanes.is_empty() {
            return group.headline("measuring…");
        }
        // A paced renderer has N refreshes per frame: that is its budget.
        let frame_ms = period * f64::from(self.interval);
        let budget = fixed(frame_ms, 1);
        let largest = lanes.iter().map(|l| l.1).fold(f64::MIN, f64::max);
        let headroom = frame_ms - largest;
        let mut rows = Vec::new();
        if self.is_open(BUDGET) {
            rows = lanes
                .iter()
                .map(|(label, ms, lane)| {
                    StatRow::new(
                        label.clone(),
                        format!("{} / {budget} ms ({})", tenths(*ms), share(ms / frame_ms)),
                    )
                    .budget(lane.clone(), ms / frame_ms)
                })
                .collect();
            if let (Some(gpu), Some(cpu)) = (self.gpu, self.cpu) {
                rows.push(StatRow::new(
                    "latency (renderer + GPU), reference only",
                    format!("{} ms", fixed(cpu + gpu, 1)),
                ));
            }
        }
        let left = if headroom < 0.0 {
            format!("over by {} ms", fixed(-headroom, 1))
        } else {
            format!("headroom {} ms", fixed(headroom, 1))
        };
        let rate = if self.interval > 1 {
            format!("{} fps of {hz}", fixed(1000.0 / frame_ms, 0))
        } else {
            hz.to_string()
        };
        group
            .headline(format!("{budget} ms ({rate}) · {left}"))
            .bad(headroom < 0.0)
            .rows(rows)
    }

    fn display_group(&self, hz: Option<&str>) -> StatGroup {
        let fps = self.fps.map_or("—".to_string(), |fps| fixed(fps, 0));
        let hz_text = hz.unwrap_or("measuring…");
        let mut rows = Vec::new();
        if self.is_open(DISPLAY) {
            rows.push(StatRow::new("refresh rate", hz_text));
            rows.push(StatRow::new("frames presented", format!("{fps} / s")));
            rows.push(StatRow::new(
                "missed frames",
                match self.missed_rate {
                    None => "—".to_string(),
                    Some(rate) => format!(
                        "{} / s · {} total",
                        fixed(rate, 0),
                        self.missed_total.max(0)
                    ),
                },
            ));
            if self.config.pacing_row {
                rows.push(StatRow::new(
                    "pacing",
                    if self.interval > 1 {
                        format!("every {} refresh, by choice", ordinal(self.interval))
                    } else {
                        "every refresh".to_string()
                    },
                ));
            }
        }
        StatGroup::new(DISPLAY, "display")
            .headline(format!("{hz_text} · {fps} fps"))
            .rows(rows)
    }

    fn gpu_group(&self) -> StatGroup {
        let headline = match self.gpu {
            Some(ms) => per_frame(ms),
            None if self.gpu_empty >= GPU_EMPTY_WINDOWS => "n/a".to_string(),
            None => "—".to_string(),
        };
        let rows = if self.is_open(GPU) {
            self.gpu_pass_means
                .slowest_first()
                .into_iter()
                .map(|(name, ms)| StatRow::new(name, per_frame(ms)))
                .collect()
        } else {
            Vec::new()
        };
        StatGroup::new(GPU, "GPU")
            .lane("gpu")
            .headline(headline)
            .rows(rows)
    }

    fn renderer_group(&self) -> StatGroup {
        let headline = self.cpu.map_or("—".to_string(), per_frame);
        let mut rows = Vec::new();
        if self.renderer_detail() {
            // The phases in the order they run. Under render(), what
            // awsm-renderer's own scopes say it spends, in subgroups by
            // kind, and what is left over: the glue between them.
            let render = self
                .config
                .render_phase
                .and_then(|index| self.config.phases.get(index));
            for (name, ms) in self.phase_means.entries() {
                rows.push(StatRow::new(name.clone(), per_frame(*ms)));
                if Some(name) != render {
                    continue;
                }
                let mut in_scopes = 0.0;
                let mut subgroups = 0;
                for (index, (_, label, prefix)) in ScopeKind::GROUPS.iter().enumerate() {
                    let means = &self.scope_means[index];
                    if means.is_empty() {
                        continue;
                    }
                    subgroups += 1;
                    in_scopes += means.total();
                    rows.push(StatRow::new(*label, per_frame(means.total())).depth(1));
                    let scopes = means.slowest_first();
                    let mut small = (0, 0.0);
                    for (scope, ms) in scopes {
                        if ms < FOLD_UNDER_MS {
                            small.0 += 1;
                            small.1 += ms;
                            continue;
                        }
                        let name = scope.strip_prefix(prefix).unwrap_or(&scope).to_string();
                        rows.push(StatRow::new(name, per_frame(ms)).depth(2));
                    }
                    if small.0 > 0 {
                        rows.push(
                            StatRow::new(
                                format!("{} more under 1 µs", small.0),
                                per_frame(small.1),
                            )
                            .depth(2),
                        );
                    }
                }
                if subgroups > 0 {
                    rows.push(
                        StatRow::new("outside the scopes", per_frame((ms - in_scopes).max(0.0)))
                            .depth(1),
                    );
                }
            }
        }
        StatGroup::new(RENDERER, &self.config.renderer_label)
            .lane("renderer")
            .headline(headline)
            .rows(rows)
    }

    fn page_group(&self) -> StatGroup {
        let headline = self.page.map_or("—".to_string(), per_frame);
        let rows = if self.is_open(PAGE) {
            self.page_means
                .entries()
                .iter()
                .map(|(name, ms)| StatRow::new(name.clone(), per_frame(*ms)))
                .collect()
        } else {
            Vec::new()
        };
        StatGroup::new(PAGE, &self.config.page_label)
            .lane("page")
            .headline(headline)
            .rows(rows)
    }
}

/// Formats an app's time per step (`"0.42 ms / step"`), `—` for none.
pub fn step_headline(ms: Option<f64>) -> String {
    ms.map_or("—".to_string(), crate::format::per_step)
}

/// Formats an app's time per frame, `—` for none.
pub fn frame_headline(ms: Option<f64>) -> String {
    ms.map_or("—".to_string(), per_frame)
}

/// A duration row of an app section.
pub fn duration_row(label: impl Into<String>, ms: f64, per: &str) -> StatRow {
    StatRow::new(label, format!("{} / {per}", duration(ms)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Fake renderer totals that advance as the test says.
    #[derive(Default)]
    struct Fake {
        gpu: RefCell<Option<(f64, u64)>>,
        passes: RefCell<Vec<(String, f64)>>,
        scopes: RefCell<Vec<ScopeTotal>>,
    }

    impl TimingSource for Fake {
        fn gpu_frame(&self) -> Option<(f64, u64)> {
            *self.gpu.borrow()
        }
        fn gpu_passes(&self) -> Vec<(String, f64)> {
            self.passes.borrow().clone()
        }
        fn cpu_scopes(&self) -> Vec<ScopeTotal> {
            self.scopes.borrow().clone()
        }
    }

    impl Fake {
        /// One frame's worth of GPU time (`ms`, every frame sampled), split
        /// over two passes, and renderer scopes.
        fn frame(&self, ms: f64) {
            let mut gpu = self.gpu.borrow_mut();
            let (t, c) = gpu.unwrap_or((0.0, 0));
            *gpu = Some((t + ms, c + 1));
            let mut passes = self.passes.borrow_mut();
            if passes.is_empty() {
                *passes = vec![("Geometry".into(), 0.0), ("Display".into(), 0.0)];
            }
            passes[0].1 += ms * 0.75;
            passes[1].1 += ms * 0.25;
            let mut scopes = self.scopes.borrow_mut();
            if scopes.is_empty() {
                *scopes = vec![
                    ScopeTotal {
                        name: "Prep: uniforms".into(),
                        kind: ScopeKind::Prep,
                        total_ms: 0.0,
                    },
                    ScopeTotal {
                        name: "Geometry".into(),
                        kind: ScopeKind::Pass,
                        total_ms: 0.0,
                    },
                    ScopeTotal {
                        name: "Tiny".into(),
                        kind: ScopeKind::Pass,
                        total_ms: 0.0,
                    },
                ];
            }
            scopes[0].total_ms += 0.2;
            scopes[1].total_ms += 0.5;
            scopes[2].total_ms += 0.0001;
        }
    }

    fn config() -> StatsConfig {
        StatsConfig {
            phases: vec!["read".into(), "render (encode + submit)".into()],
            render_phase: Some(1),
            sections: vec![SectionSpec::new("app", "app (C, app worker)", Some("sim"))],
            // 60 frames of 1000/60 ms add up to a hair under 1000.
            report_span_ms: 999.0,
            ..StatsConfig::default()
        }
    }

    const FRAME: f64 = 1000.0 / 60.0;

    /// Runs `n` frames of 60 Hz from `t`, with the renderer phases timed as
    /// 1 ms + 2 ms. Returns the windows that closed, and the next `t`.
    fn run(
        stats: &mut StatsCollector,
        fake: &Fake,
        mut t: f64,
        n: u32,
        late: bool,
    ) -> (Vec<Window>, f64) {
        let mut windows = Vec::new();
        for _ in 0..n {
            t += FRAME;
            stats.begin_frame(t);
            stats.lap(0, t + 1.0);
            stats.lap(1, t + 3.0);
            fake.frame(4.0);
            let input = FrameInput {
                late,
                ..FrameInput::new(t, 3.0)
            };
            if let Some(w) = stats.end_frame(input, fake) {
                windows.push(w);
            }
        }
        (windows, t)
    }

    fn refresh(stats: &mut StatsCollector) {
        stats.page_sample(
            &[FRAME; 60],
            &[PagePart {
                name: "app events + HUD".into(),
                ms: 6.0,
            }],
            60,
        );
    }

    #[test]
    fn placeholders_before_numbers() {
        let mut stats = StatsCollector::new(config());
        let report = stats.placeholder();
        assert_eq!(report.status.as_deref(), Some("starting…"));
        assert_eq!(report.budget.headline, "starting…");
        assert_eq!(report.display.headline, "—");
        assert_eq!(report.app[0].id, "app");
        assert_eq!(report.app[0].headline, "starting…");
        let fake = Fake::default();
        // Before ready: nothing.
        let (windows, t) = run(&mut stats, &fake, 0.0, 200, false);
        assert!(windows.is_empty());
        let report = stats.mark_ready();
        assert_eq!(report.status.as_deref(), Some("warming up…"));
        // The warm-up, then the first window needs a full second.
        let (windows, t) = run(&mut stats, &fake, t, WARMUP_FRAMES + 59, false);
        assert!(windows.is_empty());
        let (windows, _) = run(&mut stats, &fake, t, 1, false);
        assert_eq!(windows.len(), 1);
    }

    fn live(stats: &mut StatsCollector, fake: &Fake) -> f64 {
        stats.mark_ready();
        let (_, t) = run(stats, fake, 0.0, WARMUP_FRAMES, false);
        t
    }

    #[test]
    fn windows_smooth_and_report() {
        let mut stats = StatsCollector::new(config());
        stats.set_view(
            true,
            ["budget", "display", "gpu", "renderer", "app", "page"]
                .map(String::from)
                .to_vec(),
        );
        let fake = Fake::default();
        let t = live(&mut stats, &fake);
        refresh(&mut stats);
        let (windows, _) = run(&mut stats, &fake, t, 60, false);
        assert_eq!(windows.len(), 1);
        let w = windows[0];
        assert_eq!(w.frames, 60);
        assert!((w.seconds - 1.0).abs() < 1e-9);

        let section = SectionSpec::new("app", "app (C, app worker)", Some("sim"))
            .group()
            .headline("0.50 ms / step")
            .budget_ms(Some(1.0))
            .row(w.late.row("late frames"));
        let report = stats.report(&w, vec![section]);
        assert_eq!(report.status, None);
        assert_eq!(report.display.headline, "60 Hz · 60 fps");
        assert_eq!(report.display.rows[2].value, "0 / s · 0 total");
        assert_eq!(report.display.rows[3].value, "every refresh");
        assert_eq!(report.gpu.headline, "4.00 ms / frame");
        assert_eq!(report.gpu.rows[0].label, "Geometry");
        assert_eq!(report.gpu.rows[0].value, "3.00 ms / frame");
        assert_eq!(report.renderer.headline, "3.00 ms / frame");
        let labels: Vec<_> = report
            .renderer
            .rows
            .iter()
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "read",
                "render (encode + submit)",
                "prepare",
                "uniforms",
                "passes",
                "Geometry",
                "1 more under 1 µs",
                "outside the scopes"
            ]
        );
        assert_eq!(report.renderer.rows[0].value, "1.00 ms / frame");
        assert_eq!(report.renderer.rows[1].value, "2.00 ms / frame");
        assert_eq!(report.renderer.rows[3].depth, 2);
        assert_eq!(report.renderer.rows[3].value, "0.20 ms / frame");
        assert_eq!(report.page.headline, "0.10 ms / frame");
        assert_eq!(report.page.rows[0].label, "app events + HUD");
        // The budget: GPU (4), renderer (3), app (1), page (0.1) of 16.7.
        assert_eq!(report.budget.headline, "16.7 ms (60 Hz) · headroom 12.7 ms");
        assert!(!report.budget.bad);
        let lanes: Vec<_> = report
            .budget
            .rows
            .iter()
            .map(|r| (r.label.as_str(), r.lane.as_deref()))
            .collect();
        assert_eq!(
            lanes,
            [
                ("GPU", Some("gpu")),
                ("renderer (render worker)", Some("renderer")),
                ("app (C, app worker)", Some("sim")),
                ("page (main thread)", Some("page")),
                ("latency (renderer + GPU), reference only", None),
            ]
        );
        assert_eq!(report.budget.rows[0].value, "4.0 / 16.7 ms (24%)");
        assert_eq!(report.budget.rows[4].value, "7.0 ms");
        assert_eq!(report.summary.gpu_ms, Some(4.0));

        // A lane labelled apart from its group (a thread two sections share).
        let section = SectionSpec::new("app", "app (C, app worker)", Some("sim"))
            .group()
            .budget_ms(Some(1.0))
            .budget_label("app worker (app + Box3D step)");
        let report = stats.report(&w, vec![section]);
        assert_eq!(report.budget.rows[2].label, "app worker (app + Box3D step)");
        assert_eq!(report.app[0].label, "app (C, app worker)");
        let wire = serde_json::to_value(&report).unwrap();
        assert_eq!(
            wire["app"][0]["budgetLabel"],
            "app worker (app + Box3D step)"
        );
        assert!(wire["gpu"].get("budgetLabel").is_none());
    }

    #[test]
    fn closed_groups_send_no_rows_and_cost_nothing() {
        let mut stats = StatsCollector::new(config());
        stats.set_view(false, vec!["renderer".into(), "gpu".into()]);
        assert!(!stats.renderer_detail());
        let fake = Fake::default();
        let t = live(&mut stats, &fake);
        refresh(&mut stats);
        let (windows, _) = run(&mut stats, &fake, t, 60, false);
        let section = SectionSpec::new("app", "app", None)
            .group()
            .row(StatRow::new("x", "y"));
        let report = stats.report(&windows[0], vec![section]);
        assert!(report.groups().all(|g| g.rows.is_empty()));
        assert_eq!(report.gpu.headline, "4.00 ms / frame");
    }

    #[test]
    fn pauses_and_untrusted_windows() {
        let mut stats = StatsCollector::new(config());
        let fake = Fake::default();
        let mut t = live(&mut stats, &fake);
        // Half a window, a 10 s pause (a hidden tab), then the rest: the
        // window's wall time is over 3 s, so it is dropped.
        let (w, next) = run(&mut stats, &fake, t, 30, false);
        assert!(w.is_empty());
        t = next + 10_000.0;
        let (w, next) = run(&mut stats, &fake, t, 61, false);
        assert!(w.is_empty(), "the window with the pause is not trusted");
        let (w, _) = run(&mut stats, &fake, next, 60, false);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].frames, 60);
    }

    #[test]
    fn missed_frames_from_the_page_refresh() {
        let mut stats = StatsCollector::new(config());
        stats.set_view(true, vec!["display".into()]);
        let fake = Fake::default();
        // 30 fps on a 60 Hz display: every other refresh missed.
        stats.mark_ready();
        let mut t = 0.0;
        for _ in 0..WARMUP_FRAMES {
            t += 2.0 * FRAME;
            stats.end_frame(FrameInput::new(t, 1.0), &fake);
        }
        refresh(&mut stats);
        let mut window = None;
        for _ in 0..30 {
            t += 2.0 * FRAME;
            window = window.or(stats.end_frame(FrameInput::new(t, 1.0), &fake));
        }
        let window = window.expect("a window");
        let report = stats.report(&window, vec![]);
        assert_eq!(report.display.headline, "60 Hz · 30 fps");
        assert_eq!(report.display.rows[2].value, "30 / s · 30 total");
        // The same, paced on purpose: not missed.
        let mut stats = StatsCollector::new(config());
        stats.set_view(true, vec!["display".into()]);
        stats.mark_ready();
        for _ in 0..WARMUP_FRAMES {
            t += 2.0 * FRAME;
            let input = FrameInput {
                interval: 2,
                paced_skips: 1,
                ..FrameInput::new(t, 1.0)
            };
            stats.end_frame(input, &fake);
        }
        refresh(&mut stats);
        let mut window = None;
        for _ in 0..30 {
            t += 2.0 * FRAME;
            let input = FrameInput {
                interval: 2,
                paced_skips: 1,
                ..FrameInput::new(t, 1.0)
            };
            window = window.or(stats.end_frame(input, &fake));
        }
        let report = stats.report(&window.unwrap(), vec![]);
        assert_eq!(report.display.rows[2].value, "0 / s · 0 total");
        assert_eq!(report.display.rows[3].value, "every 2nd refresh, by choice");
        assert!(report
            .budget
            .headline
            .starts_with("33.3 ms (30 fps of 60 Hz)"));
    }

    #[test]
    fn late_frames_turn_red_after_three_windows() {
        let mut stats = StatsCollector::new(config());
        let fake = Fake::default();
        let mut t = live(&mut stats, &fake);
        let mut lates = Vec::new();
        for _ in 0..4 {
            let (w, next) = run(&mut stats, &fake, t, 60, true);
            t = next;
            lates.push(w[0].late);
        }
        assert_eq!(lates[0].frames, 60);
        assert!(!lates[1].bad);
        assert!(lates[2].bad);
        assert_eq!(lates[3].total, 240);
        assert_eq!(lates[3].row("late frames").value, "60 / s · 240 total");
        let (w, _) = run(&mut stats, &fake, t, 60, false);
        assert!(!w[0].late.bad);
    }

    #[test]
    fn gpu_without_timestamps_reads_na() {
        let mut stats = StatsCollector::new(config());
        let t = live(&mut stats, &Fake::default());
        let mut t = t;
        let mut headlines = Vec::new();
        for _ in 0..3 {
            let mut window = None;
            for _ in 0..60 {
                t += FRAME;
                window = window.or(stats.end_frame(FrameInput::new(t, 1.0), &()));
            }
            headlines.push(stats.report(&window.unwrap(), vec![]).gpu.headline);
        }
        assert_eq!(headlines, ["—", "—", "n/a"]);
    }

    #[test]
    fn opening_the_renderer_group_starts_a_fresh_breakdown() {
        let mut stats = StatsCollector::new(config());
        let fake = Fake::default();
        let t = live(&mut stats, &fake);
        // Scopes recorded while collapsed (as if an earlier expansion left
        // totals behind) are not counted.
        let (_, t) = run(&mut stats, &fake, t, 90, false);
        stats.set_view(true, vec!["renderer".into()]);
        let (windows, _) = run(&mut stats, &fake, t, 120, false);
        let report = stats.report(windows.last().unwrap(), vec![]);
        let geometry = report
            .renderer
            .rows
            .iter()
            .find(|r| r.label == "Geometry")
            .unwrap();
        assert_eq!(geometry.value, "0.50 ms / frame");
    }

    #[test]
    fn skip_lap_charges_no_phase() {
        let mut stats = StatsCollector::new(config());
        stats.set_view(true, vec!["renderer".into()]);
        stats.begin_frame(100.0);
        stats.lap(0, 101.0); // read: 1 ms
        stats.skip_lap(105.0); // 4 ms of physics: nobody's
        stats.lap(1, 107.0); // render: 2 ms
        stats.lap(usize::MAX, 108.0); // out of range: nobody's either
        let clock = stats.clock.as_ref().unwrap();
        assert_eq!(clock.ms, vec![1.0, 2.0]);
        assert_eq!(clock.t, 108.0);
        // Collapsed: no clock, nothing to move.
        stats.set_view(true, vec![]);
        stats.begin_frame(200.0);
        stats.skip_lap(201.0);
        assert!(stats.clock.is_none());
    }

    #[test]
    fn restart_drops_everything_but_the_view() {
        let mut stats = StatsCollector::new(config());
        stats.set_view(true, vec!["gpu".into()]);
        let fake = Fake::default();
        let t = live(&mut stats, &fake);
        let (_, _) = run(&mut stats, &fake, t, 60, false);
        stats.smooth("steps", 60.0);
        let report = stats.restart();
        assert_eq!(report.status.as_deref(), Some("warming up…"));
        assert!(stats.is_open("gpu"));
        assert_eq!(stats.smoothed("steps"), None);
        let report = stats.reset();
        assert_eq!(report.status.as_deref(), Some("starting…"));
    }
}
