//! Graphics settings shared by awsm-renderer apps.
//!
//! - **Core** (no features, unit-tested natively): the anti-aliasing modes
//!   ([`AaMode`], seven with TAA), their defaults per device class
//!   ([`AaDefaults`]), the resolution policy ([`ResolutionPolicy`],
//!   [`ResolutionState`]), the legacy-key migration, and the protocol
//!   between the page and the render worker ([`SettingsToPage`],
//!   [`SettingsToWorker`]).
//! - **`renderer`** (worker side): [`AaMode::to_anti_aliasing`],
//!   [`apply_anti_aliasing`] (the "compiling pipelines" flow) and
//!   [`probe_gpu`].
//! - **`scene`**: [`scene_anti_aliasing`], so a scene load keeps the app's
//!   mode.
//! - **`web`** (page side): [`SettingsStore`] (localStorage, `?reset`,
//!   `?aa=`) and [`observe_canvas`].
//! - **`ui`**: [`SettingsPanel`], the web-sys settings modal, compile
//!   overlay and reduced-quality notice. The JS twin for pages without Rust
//!   is `js/settings.js` in the kit repo.

mod aa;
mod config;
mod protocol;
mod resolution;

#[cfg(feature = "renderer")]
mod worker;

#[cfg(feature = "scene")]
mod scene;

#[cfg(feature = "web")]
mod store;

#[cfg(feature = "ui")]
mod ui;

pub use aa::{
    migrate_legacy_aa, retune_target, AaDefaults, AaMode, AaModeInfo, DeviceClass, RetuneLever,
};
pub use config::{show_quality_notice, AaSource, SettingsConfig};
pub use protocol::{
    GpuInfo, InitSettings, SettingsOptions, SettingsToPage, SettingsToWorker, KIT_VERSION,
};
pub use resolution::{ResolutionPolicy, ResolutionState};

#[cfg(feature = "renderer")]
pub use worker::{apply_anti_aliasing, post_to_page, probe_gpu, AaCompileEvent};

#[cfg(feature = "scene")]
pub use scene::scene_anti_aliasing;

#[cfg(feature = "web")]
pub use store::{observe_canvas, CanvasSize, SettingsStore};

#[cfg(feature = "ui")]
pub use ui::{SettingsHooks, SettingsPanel, SettingsUi};
