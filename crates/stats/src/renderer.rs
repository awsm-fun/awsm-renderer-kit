//! The worker side: awsm-renderer's profiling as a [`TimingSource`], and
//! the CPU timing tier that follows the panel.

use awsm_renderer::debug::{AwsmRendererLogging, TimingTier};
use awsm_renderer::profiling::{
    cpu_scope_kind, cpu_timing_stats, gpu_frame_timing, gpu_timing_stats, CpuScopeKind,
    GPU_FRAME_SCOPE,
};
use awsm_renderer::AwsmRenderer;

use crate::{FrameReport, ScopeKind, ScopeTotal, StatsCollector, StatsToPage, TimingSource};

/// The renderer's logging for the stats panel: GPU timestamps at the
/// `Frame` tier (cheap, and silently off on a device without
/// `timestamp-query`), CPU scopes off until the renderer group opens. Build
/// with `AwsmRendererBuilder::with_logging(stats_logging())`.
pub fn stats_logging() -> AwsmRendererLogging {
    AwsmRendererLogging {
        gpu: TimingTier::Frame,
        ..Default::default()
    }
}

/// awsm-renderer's running timing totals (`awsm_renderer::profiling`). They
/// are per thread: read them on the thread that renders.
#[derive(Clone, Copy, Debug, Default)]
pub struct RendererTimings;

impl TimingSource for RendererTimings {
    fn gpu_frame(&self) -> Option<(f64, u64)> {
        gpu_frame_timing().map(|stat| (stat.total_ms, stat.count))
    }

    fn gpu_passes(&self) -> Vec<(String, f64)> {
        gpu_timing_stats()
            .into_iter()
            .filter(|(name, _)| *name != GPU_FRAME_SCOPE)
            .map(|(name, stat)| (name.to_string(), stat.total_ms))
            .collect()
    }

    fn cpu_scopes(&self) -> Vec<ScopeTotal> {
        cpu_timing_stats()
            .into_iter()
            .filter_map(|(name, stat)| {
                let kind = match cpu_scope_kind(name) {
                    // The whole of render(): the app's render phase.
                    CpuScopeKind::Frame => return None,
                    CpuScopeKind::Prep => ScopeKind::Prep,
                    CpuScopeKind::Pass => ScopeKind::Pass,
                    CpuScopeKind::Submit => ScopeKind::Submit,
                    CpuScopeKind::Hook => ScopeKind::Hook,
                };
                Some(ScopeTotal {
                    name: name.to_string(),
                    kind,
                    total_ms: stat.total_ms,
                })
            })
            .collect()
    }
}

impl StatsCollector {
    /// [`StatsCollector::begin_frame`], plus the renderer's CPU timing tier:
    /// at least `SubFrame` while the renderer group is open (every pass,
    /// hook, prep and submit scope). Closing the group restores the app's own
    /// tier (whatever it set, e.g. from a `?trace=` flag; `Off` by default)
    /// rather than switching timing off.
    pub fn begin_renderer_frame(&mut self, renderer: &mut AwsmRenderer, now_ms: f64) {
        let current = tier_rank(renderer.logging.cpu);
        let detail = self.renderer_detail();
        let tier = rank_tier(
            self.cpu_tier
                .next(current, detail, tier_rank(TimingTier::SubFrame)),
        );
        if renderer.logging.cpu != tier {
            renderer.logging.cpu = tier;
        }
        self.begin_frame(now_ms);
    }
}

fn tier_rank(tier: TimingTier) -> u8 {
    match tier {
        TimingTier::Off => 0,
        TimingTier::Frame => 1,
        TimingTier::SubFrame => 2,
    }
}

fn rank_tier(rank: u8) -> TimingTier {
    match rank {
        0 => TimingTier::Off,
        1 => TimingTier::Frame,
        _ => TimingTier::SubFrame,
    }
}

/// Posts `report` to the page (`{type: 'stats', …}`) from a dedicated
/// worker.
pub fn post_report(report: FrameReport) {
    awsm_renderer_kit_web::message::post_to_page(&StatsToPage::Stats(report));
}
