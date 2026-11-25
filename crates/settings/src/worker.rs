//! The worker side: applying a mode to the renderer, and probing the GPU.

use awsm_renderer::AwsmRenderer;
use wasm_bindgen::JsCast;
use web_sys::GpuAdapter;

use crate::{AaMode, GpuInfo, SettingsToPage};

pub use awsm_renderer_kit_web::message::post_to_page;

/// What [`apply_anti_aliasing`] reports while it runs. Every project's
/// "compiling pipelines" overlay hangs off these: `Start` raises it,
/// `Progress` updates its line, `Done` (always delivered) lowers it.
#[derive(Clone, Debug, PartialEq)]
pub enum AaCompileEvent {
    /// The change starts. `compiles` is false when nothing needs a new
    /// pipeline (between TAA presets: a live value of the TAA pass).
    Start { mode: AaMode, compiles: bool },
    /// A progress line (`LoadingStats::phase_label`, deduplicated).
    Progress { message: String },
    /// Finished, whether it worked or not. `mode` is what runs now: after a
    /// TAA fallback that is MSAA 4×. `taa_fallback` is the renderer's reason
    /// for replacing TAA, if it ever did (sticky for its lifetime).
    Done {
        ok: bool,
        error: Option<String>,
        mode: AaMode,
        taa_fallback: Option<String>,
    },
}

impl From<AaCompileEvent> for SettingsToPage {
    fn from(event: AaCompileEvent) -> Self {
        match event {
            AaCompileEvent::Start { mode, compiles } => SettingsToPage::AaCompileStart {
                mode,
                label: mode.label().to_string(),
                compiles,
            },
            AaCompileEvent::Progress { message } => SettingsToPage::AaCompileProgress { message },
            AaCompileEvent::Done {
                ok,
                error,
                mode,
                taa_fallback,
            } => SettingsToPage::AaCompileDone {
                ok,
                error,
                mode,
                taa_fallback,
            },
        }
    }
}

/// Switches `renderer` to `mode`: `set_anti_aliasing`, then `commit_load`,
/// which compiles the material pipelines the new mode needs (the renderer
/// only flags them in `set_anti_aliasing`). Only the first switch to a mode
/// really compiles: the renderer keeps the pipelines of modes it has used.
///
/// - A change that is not structural (between TAA presets: render scale and
///   sharpening are live values of the TAA pass) needs no commit, and gets
///   none: `Start { compiles: false }`, no overlay, no hitch.
/// - If TAA cannot be set up, the renderer falls back to MSAA 4× by itself
///   and returns the error; the fallback's own pipelines still need the
///   commit, so it runs anyway. `Done` then says `ok: false`, `mode: Msaa`
///   and the renderer's `taa_fallback_reason`. (A TAA pipeline that fails
///   inside the commit falls back the same way there: `ok: true`, with the
///   reason.)
///
/// The caller owns the renderer for the whole call and draws no frames
/// meanwhile (the projects take it out of their frame loop's cell). Post
/// each event to the page with `post_to_page(&SettingsToPage::from(event))`.
/// `Err` carries a message for the page's error display; `Done` has already
/// been delivered by then.
pub async fn apply_anti_aliasing(
    renderer: &mut AwsmRenderer,
    mode: AaMode,
    mut on_event: impl FnMut(AaCompileEvent),
) -> Result<(), String> {
    let config = mode.to_anti_aliasing();
    let before = renderer.anti_aliasing.clone();
    let compiles = !before.same_structure(&config);
    on_event(AaCompileEvent::Start { mode, compiles });
    let set = renderer
        .set_anti_aliasing(config)
        .await
        .map_err(|err| format!("anti-aliasing change: {err}"));
    // Commit whenever the structure moved: the requested change, or the
    // renderer's fallback after a failed one.
    let structural = !before.same_structure(&renderer.anti_aliasing)
        || (set.is_err() && renderer.anti_aliasing != before);
    let committed = if structural {
        let mut last_line = String::new();
        renderer
            .commit_load(|stats| {
                let Some(line) = stats.phase_label() else {
                    return;
                };
                if line != last_line {
                    on_event(AaCompileEvent::Progress {
                        message: line.clone(),
                    });
                    last_line = line;
                }
            })
            .await
            .map(|_| ())
            .map_err(|err| format!("anti-aliasing pipelines: {err}"))
    } else {
        Ok(())
    };
    let result = set.and(committed);
    on_event(AaCompileEvent::Done {
        ok: result.is_ok(),
        error: result.as_ref().err().cloned(),
        mode: AaMode::from_anti_aliasing(&renderer.anti_aliasing),
        taa_fallback: renderer.taa_fallback_reason().map(str::to_owned),
    });
    result
}

/// Asks the GPU what the page needs to size the canvas: whether the adapter
/// is a software fallback, and its largest 2D texture. A separate,
/// lightweight adapter request, made before the renderer builds its own
/// device (so the page can settle the canvas size and the default mode
/// before the slow part). If it fails, [`GpuInfo::default`]: hardware, and
/// the minimum every WebGPU device supports.
pub async fn probe_gpu() -> GpuInfo {
    let Some(gpu) = awsm_renderer::web_global::navigator_gpu() else {
        return GpuInfo::default();
    };
    let adapter = gpu
        .request_adapter()
        .await
        .ok()
        .filter(|adapter| !adapter.is_null() && !adapter.is_undefined())
        .map(|adapter| adapter.unchecked_into::<GpuAdapter>());
    match adapter {
        Some(adapter) => GpuInfo {
            is_fallback: adapter.info().is_fallback_adapter(),
            max_texture_dim: adapter.limits().max_texture_dimension_2d(),
        },
        None => GpuInfo::default(),
    }
}
