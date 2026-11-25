//! The settings messages between the page and the worker that runs the
//! renderer.
//!
//! One envelope for both families: a plain object with a kebab-case `type`
//! and camelCase fields, which is what the C family's page already used
//! (`{type: 'gpu-info', isFallback, maxTextureDim}`). Rust pages
//! deserialize the same objects with serde-wasm-bindgen; JS pages read
//! them as they are.
//!
//! The kit does not define the app's `init` message (it carries the canvas
//! and app fields): an app puts [`InitSettings`]' two fields (`width`,
//! `height`) and `aa` in its own.

use serde::{Deserialize, Serialize};

use crate::{AaDefaults, AaMode, AaModeInfo, ResolutionPolicy, SettingsConfig};

/// The kit's version, stamped on [`SettingsOptions`] (and on the JS twins
/// by `scripts/sync-js.sh`).
pub const KIT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Worker → page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SettingsToPage {
    /// `settings-options`: the menu, the defaults and the resolution policy,
    /// posted once at start-up, before the page sends `init`. A JS page
    /// builds its Settings from this, so the words and numbers live only in
    /// Rust.
    SettingsOptions(SettingsOptions),
    /// `gpu-info`: the adapter facts the canvas is sized with. Also inside
    /// `settings-options`; posted on its own by apps that probe later.
    GpuInfo(GpuInfo),
    /// `aa-compile-start`: a mode change is being applied. `compiles` is
    /// false for a change that needs no pipeline (between TAA presets): the
    /// page need not raise its overlay for that.
    AaCompileStart {
        mode: AaMode,
        label: String,
        compiles: bool,
    },
    /// `aa-compile-progress`: a line for the overlay (deduplicated).
    AaCompileProgress { message: String },
    /// `aa-compile-done`: always follows a start, whether it worked or not.
    /// `mode` is what runs now (MSAA after a TAA fallback); `taaFallback`
    /// is why TAA was replaced, if it was (sticky for the renderer's life:
    /// a page stops offering TAA).
    AaCompileDone {
        ok: bool,
        error: Option<String>,
        mode: AaMode,
        taa_fallback: Option<String>,
    },
}

/// Page → worker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SettingsToWorker {
    /// `anti-alias`: the user picked a mode.
    AntiAlias { mode: AaMode },
    /// `resize`: the canvas's backing store should be this many device
    /// pixels.
    Resize { width: u32, height: u32 },
}

impl SettingsToPage {
    /// The `type` strings of this enum, for a page dispatching messages by
    /// hand.
    pub const TYPES: [&'static str; 5] = [
        "settings-options",
        "gpu-info",
        "aa-compile-start",
        "aa-compile-progress",
        "aa-compile-done",
    ];
}

impl SettingsToWorker {
    /// The `type` strings of this enum.
    pub const TYPES: [&'static str; 2] = ["anti-alias", "resize"];
}

/// What the page needs from the GPU to size the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    /// A software ("fallback") adapter.
    pub is_fallback: bool,
    /// The largest 2D texture side: the canvas can be no larger.
    pub max_texture_dim: u32,
}

impl Default for GpuInfo {
    /// What a page assumes when the adapter cannot be asked: hardware, and
    /// the minimum every WebGPU device supports.
    fn default() -> Self {
        Self {
            is_fallback: false,
            max_texture_dim: 8192,
        }
    }
}

/// The `settings-options` payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsOptions {
    pub modes: Vec<AaModeInfo>,
    pub defaults: AaDefaults,
    pub resolution: ResolutionPolicy,
    /// The worker's GPU probe, when it ran before this was posted.
    pub gpu: Option<GpuInfo>,
    pub kit_version: String,
}

impl SettingsOptions {
    pub fn new(config: &SettingsConfig, gpu: Option<GpuInfo>) -> Self {
        Self {
            modes: config.modes.iter().map(|mode| mode.info()).collect(),
            defaults: config.defaults,
            resolution: config.resolution,
            gpu,
            kit_version: KIT_VERSION.to_string(),
        }
    }

    /// Back to a [`SettingsConfig`] (a Rust page builds its panel from this
    /// the way the JS twin does).
    pub fn config(&self) -> SettingsConfig {
        SettingsConfig {
            modes: self.modes.iter().map(|info| info.id).collect(),
            defaults: self.defaults,
            resolution: self.resolution,
        }
    }
}

/// The settings fields an app's `init` message carries, so the renderer is
/// built with the page's choices instead of switching right after start-up.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitSettings {
    pub width: u32,
    pub height: u32,
    pub aa: AaMode,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn wire<T: Serialize>(value: &T) -> Value {
        serde_json::to_value(value).unwrap()
    }

    #[test]
    fn worker_to_page_wire_format() {
        assert_eq!(
            wire(&SettingsToPage::GpuInfo(GpuInfo {
                is_fallback: true,
                max_texture_dim: 4096
            })),
            json!({"type": "gpu-info", "isFallback": true, "maxTextureDim": 4096})
        );
        assert_eq!(
            wire(&SettingsToPage::AaCompileStart {
                mode: AaMode::TaaQuality,
                label: "TAA Quality (67%)".into(),
                compiles: true
            }),
            json!({"type": "aa-compile-start", "mode": "taa-quality", "label": "TAA Quality (67%)", "compiles": true})
        );
        assert_eq!(
            wire(&SettingsToPage::AaCompileProgress {
                message: "Compiling pipelines (3 remaining)".into()
            }),
            json!({"type": "aa-compile-progress", "message": "Compiling pipelines (3 remaining)"})
        );
        assert_eq!(
            wire(&SettingsToPage::AaCompileDone {
                ok: false,
                error: Some("boom".into()),
                mode: AaMode::Msaa,
                taa_fallback: Some("no".into())
            }),
            json!({"type": "aa-compile-done", "ok": false, "error": "boom", "mode": "msaa", "taaFallback": "no"})
        );
    }

    #[test]
    fn page_to_worker_wire_format() {
        let msg: SettingsToWorker =
            serde_json::from_value(json!({"type": "anti-alias", "mode": "taa"})).unwrap();
        assert_eq!(msg, SettingsToWorker::AntiAlias { mode: AaMode::Taa });
        let msg: SettingsToWorker =
            serde_json::from_value(json!({"type": "resize", "width": 800, "height": 600})).unwrap();
        assert_eq!(
            msg,
            SettingsToWorker::Resize {
                width: 800,
                height: 600
            }
        );
        assert!(serde_json::from_value::<SettingsToWorker>(
            json!({"type": "anti-alias", "msaa": true, "smaa": false})
        )
        .is_err());
    }

    #[test]
    fn options_round_trip() {
        let config = SettingsConfig::default();
        let options = SettingsOptions::new(&config, Some(GpuInfo::default()));
        let message = SettingsToPage::SettingsOptions(options.clone());
        let value = wire(&message);
        assert_eq!(value["type"], "settings-options");
        assert_eq!(value["modes"][4]["id"], "taa-quality");
        assert_eq!(value["defaults"]["desktop"], "taa");
        assert_eq!(value["resolution"]["pixelBudget"], 2_800_000.0);
        assert_eq!(value["resolution"]["softwareStart"], 0.5);
        assert_eq!(value["gpu"]["maxTextureDim"], 8192);
        let back: SettingsToPage = serde_json::from_value(value).unwrap();
        assert_eq!(back, message);
        assert_eq!(options.config(), config);
    }

    #[test]
    fn type_lists_match_the_enums() {
        let samples = [
            wire(&SettingsToPage::SettingsOptions(SettingsOptions::new(
                &SettingsConfig::default(),
                None,
            ))),
            wire(&SettingsToPage::GpuInfo(GpuInfo::default())),
            wire(&SettingsToPage::AaCompileStart {
                mode: AaMode::Off,
                label: String::new(),
                compiles: true,
            }),
            wire(&SettingsToPage::AaCompileProgress {
                message: String::new(),
            }),
            wire(&SettingsToPage::AaCompileDone {
                ok: true,
                error: None,
                mode: AaMode::Off,
                taa_fallback: None,
            }),
        ];
        for (sample, name) in samples.iter().zip(SettingsToPage::TYPES) {
            assert_eq!(sample["type"], name);
        }
        let samples = [
            wire(&SettingsToWorker::AntiAlias { mode: AaMode::Off }),
            wire(&SettingsToWorker::Resize {
                width: 1,
                height: 1,
            }),
        ];
        for (sample, name) in samples.iter().zip(SettingsToWorker::TYPES) {
            assert_eq!(sample["type"], name);
        }
    }
}

#[cfg(any(feature = "renderer", feature = "web"))]
mod js {
    use super::{SettingsToPage, SettingsToWorker};
    use awsm_renderer_kit_web::message::{from_js, message_type, to_js};
    use wasm_bindgen::JsValue;

    impl SettingsToWorker {
        /// A page message as this enum: `None` when its `type` is not one of
        /// [`Self::TYPES`] (an app message), `Some(Err)` when it is but does
        /// not parse.
        pub fn from_js(value: &JsValue) -> Option<Result<Self, String>> {
            let kind = message_type(value)?;
            Self::TYPES
                .contains(&kind.as_str())
                .then(|| from_js(value.clone()))
        }

        /// As a plain JS object.
        pub fn to_js(&self) -> Result<JsValue, String> {
            to_js(self)
        }
    }

    impl SettingsToPage {
        /// A worker message as this enum (see [`SettingsToWorker::from_js`]).
        pub fn from_js(value: &JsValue) -> Option<Result<Self, String>> {
            let kind = message_type(value)?;
            Self::TYPES
                .contains(&kind.as_str())
                .then(|| from_js(value.clone()))
        }

        /// As a plain JS object.
        pub fn to_js(&self) -> Result<JsValue, String> {
            to_js(self)
        }
    }
}
