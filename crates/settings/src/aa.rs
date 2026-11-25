//! The anti-aliasing modes an app offers, and their defaults.

use serde::{Deserialize, Serialize};

/// One entry of the anti-aliasing menu.
///
/// Serialized as its storage id (`"off"`, `"taa-quality"`, …), which is also
/// the `<option>` value, the `?aa=` value and the protocol's `mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AaMode {
    /// No anti-aliasing.
    Off,
    /// SMAA, a post-process (no MSAA).
    Smaa,
    /// 4× MSAA (the only count the renderer supports).
    Msaa,
    /// TAA at native resolution (`TaaConfig::native()`).
    Taa,
    /// TAA upscaling from 0.67 of the width and height (`TaaPreset::Quality`).
    TaaQuality,
    /// TAA upscaling from 0.58 (`TaaPreset::Balanced`).
    TaaBalanced,
    /// TAA upscaling from 0.5 (`TaaPreset::Performance`).
    TaaPerformance,
}

impl AaMode {
    /// Every mode, in menu order. MSAA and SMAA together is not offered: on
    /// top of a 4× MSAA resolve SMAA adds almost nothing and costs a
    /// full-screen pass (the renderer leaves it off by default for that
    /// reason). TAA combines with neither.
    pub const ALL: [AaMode; 7] = [
        AaMode::Off,
        AaMode::Smaa,
        AaMode::Msaa,
        AaMode::Taa,
        AaMode::TaaQuality,
        AaMode::TaaBalanced,
        AaMode::TaaPerformance,
    ];

    /// The storage / `<option>` / protocol id.
    pub const fn id(self) -> &'static str {
        match self {
            AaMode::Off => "off",
            AaMode::Smaa => "smaa",
            AaMode::Msaa => "msaa",
            AaMode::Taa => "taa",
            AaMode::TaaQuality => "taa-quality",
            AaMode::TaaBalanced => "taa-balanced",
            AaMode::TaaPerformance => "taa-performance",
        }
    }

    /// The mode with that id. Ids stored before TAA existed (`off`, `smaa`,
    /// `msaa`) are the same strings.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }

    /// What the menu calls it.
    pub const fn label(self) -> &'static str {
        match self {
            AaMode::Off => "Off",
            AaMode::Smaa => "SMAA",
            AaMode::Msaa => "MSAA 4×",
            AaMode::Taa => "TAA",
            AaMode::TaaQuality => "TAA Quality (67%)",
            AaMode::TaaBalanced => "TAA Balanced (58%)",
            AaMode::TaaPerformance => "TAA Performance (50%)",
        }
    }

    /// One sentence for a tooltip or a help line.
    pub const fn description(self) -> &'static str {
        match self {
            AaMode::Off => "No anti-aliasing: the fastest, with jagged edges.",
            AaMode::Smaa => "Smooths edges with a cheap post-process. Does not stop shimmer.",
            AaMode::Msaa => {
                "4× multisampling: smooth geometric edges, at a cost that grows with the screen."
            }
            AaMode::Taa => {
                "Temporal anti-aliasing at full resolution: the smoothest picture, \
                 edges, shading and fine detail alike."
            }
            AaMode::TaaQuality => {
                "TAA that renders 67% of the width and height and rebuilds full \
                 resolution: smooth, and much faster."
            }
            AaMode::TaaBalanced => "TAA rendering at 58%: faster again, a little softer.",
            AaMode::TaaPerformance => {
                "TAA rendering at 50%: the fastest TAA, softest on fine detail."
            }
        }
    }

    /// Whether this is one of the TAA modes.
    pub const fn is_taa(self) -> bool {
        matches!(
            self,
            AaMode::Taa | AaMode::TaaQuality | AaMode::TaaBalanced | AaMode::TaaPerformance
        )
    }

    /// The renderer's internal render scale for a TAA mode (render size /
    /// output size), `None` for the others. The canvas (output) size is the
    /// resolution setting's; this scales what is shaded inside it.
    pub const fn taa_render_scale(self) -> Option<f32> {
        match self {
            AaMode::Taa => Some(1.0),
            AaMode::TaaQuality => Some(0.67),
            AaMode::TaaBalanced => Some(0.58),
            AaMode::TaaPerformance => Some(0.5),
            _ => None,
        }
    }

    /// The menu entry as the protocol carries it.
    pub fn info(self) -> AaModeInfo {
        AaModeInfo {
            id: self,
            label: self.label().to_string(),
            description: self.description().to_string(),
        }
    }
}

/// A menu entry: id, label and description. The worker sends the table in
/// [`crate::SettingsOptions`], so a JS page builds its menu from it and the
/// words live only here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AaModeInfo {
    pub id: AaMode,
    pub label: String,
    pub description: String,
}

/// What a device is, as far as the defaults care.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceClass {
    /// `(pointer: coarse)`: a phone or tablet.
    pub coarse: bool,
    /// A software ("fallback") WebGPU adapter.
    pub software: bool,
}

/// The default mode per device class. A stored choice and `?aa=` always win
/// over these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AaDefaults {
    /// A desktop (fine pointer, hardware GPU).
    pub desktop: AaMode,
    /// A touch device. `Off`: like its lower resolution, its defaults are all
    /// about frame rate (and TAA's 32 B/sample path has not been measured on
    /// a phone yet).
    pub coarse: AaMode,
    /// A software GPU: it really cannot push pixels.
    pub software: AaMode,
}

impl Default for AaDefaults {
    fn default() -> Self {
        Self {
            desktop: AaMode::Taa,
            coarse: AaMode::Off,
            software: AaMode::Off,
        }
    }
}

impl AaDefaults {
    /// The default for `device`: software first (the weakest), then coarse,
    /// then desktop.
    pub fn pick(&self, device: DeviceClass) -> AaMode {
        if device.software {
            self.software
        } else if device.coarse {
            self.coarse
        } else {
            self.desktop
        }
    }
}

/// The pre-0.42 per-toggle keys of the Rust v1 projects (`awsm_msaa` /
/// `awsm_smaa`, `"1"` / `"0"`, no app prefix), as a mode. They were only
/// ever written by a user toggle, so they are real choices: MSAA on maps to
/// MSAA (SMAA alongside it is not offered any more), MSAA off to SMAA if SMAA
/// was on, else Off. SMAA on with MSAA never stored maps to MSAA, exactly as
/// those projects did. `None` when there is nothing to migrate.
pub fn migrate_legacy_aa(msaa: Option<&str>, smaa: Option<&str>) -> Option<AaMode> {
    match (msaa, smaa) {
        (Some("1"), _) => Some(AaMode::Msaa),
        (Some("0"), Some("1")) => Some(AaMode::Smaa),
        (Some("0"), _) => Some(AaMode::Off),
        (None, Some("1")) => Some(AaMode::Msaa),
        _ => None,
    }
}

/// Which lever a resolution governor (an auto-tuner such as Dreidel
/// Derby's) should move for a mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RetuneLever {
    /// Without TAA there is no renderer-side scale below 1: resize the
    /// canvas (the resolution setting).
    CanvasScale,
    /// Under TAA, change `AwsmRenderer::set_taa_render_scale`: no compile, no
    /// history reset. A canvas resize changes the output size, which resets
    /// TAA's history (one frame of aliasing over the whole picture).
    TaaRenderScale,
}

/// The lever a governor should move under `mode` (see [`RetuneLever`]).
pub fn retune_target(mode: AaMode) -> RetuneLever {
    if mode.is_taa() {
        RetuneLever::TaaRenderScale
    } else {
        RetuneLever::CanvasScale
    }
}

#[cfg(feature = "renderer")]
mod renderer {
    use super::AaMode;
    use awsm_renderer::anti_alias::AntiAliasing;
    use awsm_renderer::taa::{TaaConfig, TaaPreset};

    impl AaMode {
        /// The renderer's configuration for this mode.
        pub fn to_anti_aliasing(self) -> AntiAliasing {
            match self {
                AaMode::Off => AntiAliasing::none(),
                AaMode::Smaa => AntiAliasing::smaa(),
                AaMode::Msaa => AntiAliasing::msaa4(),
                AaMode::Taa => AntiAliasing::taa(TaaConfig::native()),
                AaMode::TaaQuality => AntiAliasing::taa(TaaConfig::preset(TaaPreset::Quality)),
                AaMode::TaaBalanced => AntiAliasing::taa(TaaConfig::preset(TaaPreset::Balanced)),
                AaMode::TaaPerformance => {
                    AntiAliasing::taa(TaaConfig::preset(TaaPreset::Performance))
                }
            }
        }

        /// The menu mode closest to a renderer configuration: what is
        /// actually running (after a TAA fallback to MSAA, after a scene
        /// applied its own). TAA at a scale that is not a preset reads as the
        /// nearest preset at or above it; MSAA with SMAA as MSAA.
        pub fn from_anti_aliasing(aa: &AntiAliasing) -> Self {
            if let Some(taa) = &aa.taa {
                let scale = taa.clamped_render_scale();
                return [
                    AaMode::TaaPerformance,
                    AaMode::TaaBalanced,
                    AaMode::TaaQuality,
                ]
                .into_iter()
                .find(|mode| mode.taa_render_scale().is_some_and(|s| scale <= s + 1e-3))
                .unwrap_or(AaMode::Taa);
            }
            match (aa.msaa_sample_count.is_some(), aa.smaa) {
                (true, _) => AaMode::Msaa,
                (false, true) => AaMode::Smaa,
                (false, false) => AaMode::Off,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for mode in AaMode::ALL {
            assert_eq!(AaMode::from_id(mode.id()), Some(mode));
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, format!("\"{}\"", mode.id()));
            assert_eq!(serde_json::from_str::<AaMode>(&json).unwrap(), mode);
        }
        assert_eq!(AaMode::from_id("MSAA"), None);
        assert_eq!(AaMode::from_id(""), None);
    }

    #[test]
    fn stored_ids_from_before_taa_still_parse() {
        assert_eq!(AaMode::from_id("off"), Some(AaMode::Off));
        assert_eq!(AaMode::from_id("smaa"), Some(AaMode::Smaa));
        assert_eq!(AaMode::from_id("msaa"), Some(AaMode::Msaa));
    }

    #[test]
    fn ids_are_unique_and_labels_present() {
        let mut ids: Vec<_> = AaMode::ALL.iter().map(|m| m.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), AaMode::ALL.len());
        for mode in AaMode::ALL {
            assert!(!mode.label().is_empty());
            assert!(mode.description().ends_with('.'));
        }
    }

    #[test]
    fn taa_scales_are_the_renderer_presets() {
        assert_eq!(AaMode::Taa.taa_render_scale(), Some(1.0));
        assert_eq!(AaMode::TaaQuality.taa_render_scale(), Some(0.67));
        assert_eq!(AaMode::TaaBalanced.taa_render_scale(), Some(0.58));
        assert_eq!(AaMode::TaaPerformance.taa_render_scale(), Some(0.5));
        assert_eq!(AaMode::Msaa.taa_render_scale(), None);
        assert!(AaMode::ALL
            .iter()
            .all(|m| m.is_taa() == m.taa_render_scale().is_some()));
    }

    #[test]
    fn defaults_per_device() {
        let defaults = AaDefaults::default();
        assert_eq!(defaults.pick(DeviceClass::default()), AaMode::Taa);
        let coarse = DeviceClass {
            coarse: true,
            software: false,
        };
        assert_eq!(defaults.pick(coarse), AaMode::Off);
        let software = DeviceClass {
            coarse: false,
            software: true,
        };
        assert_eq!(defaults.pick(software), AaMode::Off);
        let custom = AaDefaults {
            desktop: AaMode::Msaa,
            coarse: AaMode::Smaa,
            software: AaMode::TaaPerformance,
        };
        assert_eq!(
            custom.pick(DeviceClass {
                coarse: true,
                software: true
            }),
            AaMode::TaaPerformance
        );
        assert_eq!(custom.pick(coarse), AaMode::Smaa);
    }

    #[test]
    fn legacy_migration_matches_the_v1_projects() {
        assert_eq!(migrate_legacy_aa(Some("1"), None), Some(AaMode::Msaa));
        assert_eq!(migrate_legacy_aa(Some("1"), Some("1")), Some(AaMode::Msaa));
        assert_eq!(migrate_legacy_aa(Some("1"), Some("0")), Some(AaMode::Msaa));
        assert_eq!(migrate_legacy_aa(Some("0"), Some("1")), Some(AaMode::Smaa));
        assert_eq!(migrate_legacy_aa(Some("0"), Some("0")), Some(AaMode::Off));
        assert_eq!(migrate_legacy_aa(Some("0"), None), Some(AaMode::Off));
        // MSAA never toggled, SMAA on: MSAA, as physics-multithreaded did.
        assert_eq!(migrate_legacy_aa(None, Some("1")), Some(AaMode::Msaa));
        assert_eq!(migrate_legacy_aa(None, Some("0")), None);
        assert_eq!(migrate_legacy_aa(None, None), None);
        assert_eq!(migrate_legacy_aa(Some("yes"), None), None);
    }

    #[test]
    fn retune_lever() {
        assert_eq!(retune_target(AaMode::Msaa), RetuneLever::CanvasScale);
        assert_eq!(retune_target(AaMode::Off), RetuneLever::CanvasScale);
        assert_eq!(retune_target(AaMode::Taa), RetuneLever::TaaRenderScale);
        assert_eq!(
            retune_target(AaMode::TaaPerformance),
            RetuneLever::TaaRenderScale
        );
    }
}
