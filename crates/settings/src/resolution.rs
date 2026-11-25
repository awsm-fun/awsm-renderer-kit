//! The resolution policy: how big the canvas's backing store is.
//!
//! The resolution setting is a fraction of native resolution, applied to the
//! **canvas size** (the output size). Under TAA the presets lower the
//! renderer's internal scale on top of it (`TaaConfig::render_scale`), so
//! TAA Quality at 80% shades 0.8 × 0.67 of the width.
//!
//! The default is native resolution held to a **pixel budget**, and lower
//! again on a touch device or a software GPU. Native on every desktop is not
//! a safe default: a large high-DPI screen has several times the pixels of an
//! ordinary one, and the GPU's time per frame grows with them. So the budget
//! is in pixels, not in device pixel ratio, and a small window keeps native
//! resolution. The default never goes below one device pixel per CSS pixel
//! (softer than that reads as blurry), so a display with a ratio of 1 keeps
//! native resolution however large it is; its user has the slider.
//!
//! **Interaction with TAA.** A change of the canvas size changes TAA's output
//! size, which resets its history (one frame without anti-aliasing over the
//! whole picture). A user dragging the slider does not mind; an auto-tuner
//! stepping every few seconds would show it, so under TAA a governor should
//! move `set_taa_render_scale` instead ([`crate::retune_target`]).

use serde::{Deserialize, Serialize};

use crate::{DeviceClass, GpuInfo};

/// The numbers behind the default resolution and the slider. The defaults
/// are what every awsm-renderer app shipped with before the kit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionPolicy {
    /// The slider's floor (0.5). Below it the picture turns to mush for
    /// little more speed.
    pub min_scale: f64,
    /// The ceiling (1.0): native resolution, never supersampled.
    pub max_scale: f64,
    /// The slider's step (0.05). The default is rounded DOWN to it, so the
    /// thumb, its label and the real scale agree.
    pub step: f64,
    /// Canvas pixels the default allows (2.8 million; Dreidel Derby uses 2.0).
    /// A 1200 × 897 CSS-pixel window at a ratio of 2 (4.3 million native)
    /// defaults to 80%.
    pub pixel_budget: f64,
    /// The default never goes below this many device pixels per CSS pixel
    /// (1).
    pub min_effective_dpr: f64,
    /// Where a touch device starts (0.6): a high pixel ratio on a mobile GPU
    /// is the case native resolution suits worst.
    pub coarse_start: f64,
    /// Where a software GPU starts (0.5), once the worker has reported the
    /// adapter. `None`: no software-GPU seed.
    pub software_start: Option<f64>,
    /// The largest canvas side assumed until the worker reports the GPU's
    /// real limit (8192: every WebGPU device supports at least this).
    pub max_texture: u32,
}

impl Default for ResolutionPolicy {
    fn default() -> Self {
        Self {
            min_scale: 0.5,
            max_scale: 1.0,
            step: 0.05,
            pixel_budget: 2_800_000.0,
            min_effective_dpr: 1.0,
            coarse_start: 0.6,
            software_start: Some(0.5),
            max_texture: 8192,
        }
    }
}

impl ResolutionPolicy {
    /// `scale` within the slider's range (NaN becomes the ceiling).
    pub fn clamp(&self, scale: f64) -> f64 {
        if scale.is_nan() {
            return self.max_scale;
        }
        scale.clamp(self.min_scale, self.max_scale)
    }

    /// Steps per unit of scale (20 for 0.05).
    fn steps(&self) -> f64 {
        (1.0 / self.step).round().max(1.0)
    }

    /// The device-class ceiling for the default scale: the coarse start on a
    /// touch device, lowered to the software start on a software GPU.
    pub fn cap(&self, device: DeviceClass) -> f64 {
        let mut cap = if device.coarse {
            self.coarse_start
        } else {
            self.max_scale
        };
        if device.software {
            if let Some(start) = self.software_start {
                cap = cap.min(start);
            }
        }
        cap
    }

    /// The default scale for a canvas of `native_w × native_h` device pixels
    /// at `dpr` (at least 1), under the ceiling `cap`: native, held to the
    /// pixel budget, not below the minimum effective ratio, rounded down to
    /// the step and within the range.
    pub fn auto_scale(&self, native_w: f64, native_h: f64, dpr: f64, cap: f64) -> f64 {
        let native_pixels = native_w * native_h;
        let budget = if native_pixels > self.pixel_budget {
            (self.pixel_budget / native_pixels)
                .sqrt()
                .max(self.min_effective_dpr / dpr.max(1.0))
        } else {
            self.max_scale
        };
        let steps = self.steps();
        let stepped = (budget.min(cap) * steps).floor() / steps;
        self.clamp(stepped)
    }

    /// [`Self::auto_scale`] for a CSS size and pixel ratio on `device`.
    pub fn default_scale(&self, css_w: f64, css_h: f64, dpr: f64, device: DeviceClass) -> f64 {
        let dpr = dpr.max(1.0);
        self.auto_scale(
            css_w.max(1.0) * dpr,
            css_h.max(1.0) * dpr,
            dpr,
            self.cap(device),
        )
    }

    /// The backing-store size in device pixels: native × scale, each side
    /// rounded and between 1 and `max_texture`.
    pub fn backing_size(
        &self,
        native_w: f64,
        native_h: f64,
        scale: f64,
        max_texture: u32,
    ) -> (u32, u32) {
        let max = f64::from(max_texture.max(1));
        let side = |native: f64| (native * scale).round().max(1.0).min(max) as u32;
        (side(native_w), side(native_h))
    }

    /// A stored scale (`"0.80"`), clamped to the range; `None` if it is not
    /// a number.
    pub fn parse_stored(&self, value: &str) -> Option<f64> {
        let value: f64 = value.trim().parse().ok()?;
        value.is_finite().then(|| self.clamp(value))
    }

    /// A scale as it is stored: two decimals (`0.8` → `"0.80"`).
    pub fn format_stored(scale: f64) -> String {
        format!("{scale:.2}")
    }

    /// A scale as the slider shows it: 0.6 → 60.
    pub fn percent(scale: f64) -> u32 {
        (scale * 100.0).round().max(0.0) as u32
    }

    /// The slider's percentage as a scale, clamped (NaN → the ceiling).
    pub fn from_percent(&self, percent: f64) -> f64 {
        self.clamp(percent / 100.0)
    }
}

/// The page's resolution state: the canvas's native size, the device class
/// ceiling, and whether the scale is the user's. The JS twin
/// (`js/settings.js`) runs the same steps; `fixtures/resolution.json` holds
/// cases both are tested against.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolutionState {
    pub policy: ResolutionPolicy,
    native_w: f64,
    native_h: f64,
    dpr: f64,
    cap: f64,
    user: bool,
    scale: f64,
    max_texture: u32,
}

impl ResolutionState {
    /// The state for a canvas of `native_w × native_h` device pixels at
    /// `dpr`, on `device`, with the stored scale if there is one (it is the
    /// user's and is never overridden).
    pub fn new(
        policy: ResolutionPolicy,
        native_w: f64,
        native_h: f64,
        dpr: f64,
        device: DeviceClass,
        stored: Option<f64>,
    ) -> Self {
        let mut state = Self {
            policy,
            native_w: native_w.max(1.0),
            native_h: native_h.max(1.0),
            dpr: dpr.max(1.0),
            cap: policy.cap(device),
            user: stored.is_some(),
            scale: 1.0,
            max_texture: policy.max_texture,
        };
        state.scale = match stored {
            Some(scale) => policy.clamp(scale),
            None => state.auto_scale(),
        };
        state
    }

    /// The current scale.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// The current scale as the slider shows it.
    pub fn percent(&self) -> u32 {
        ResolutionPolicy::percent(self.scale)
    }

    /// Whether the scale is the user's choice.
    pub fn is_user(&self) -> bool {
        self.user
    }

    /// The largest canvas side in effect.
    pub fn max_texture(&self) -> u32 {
        self.max_texture
    }

    /// The default for the current size and device.
    pub fn auto_scale(&self) -> f64 {
        self.policy
            .auto_scale(self.native_w, self.native_h, self.dpr, self.cap)
    }

    /// The backing-store size for the current state.
    pub fn backing(&self) -> (u32, u32) {
        self.policy
            .backing_size(self.native_w, self.native_h, self.scale, self.max_texture)
    }

    /// The canvas's layout size or the pixel ratio changed. The default
    /// depends on the size (the pixel budget), so it is worked out again; a
    /// scale the user chose stays.
    pub fn resized(&mut self, native_w: f64, native_h: f64, dpr: f64) {
        self.native_w = native_w.max(1.0);
        self.native_h = native_h.max(1.0);
        self.dpr = dpr.max(1.0);
        if !self.user {
            self.scale = self.auto_scale();
        }
    }

    /// The worker's report on the GPU: its largest texture caps the canvas,
    /// and a software adapter lowers the default. Returns whether the scale
    /// was lowered (the page may say so in its loading log).
    pub fn gpu_info(&mut self, info: &GpuInfo) -> bool {
        self.max_texture = info.max_texture_dim.max(1);
        if info.is_fallback {
            if let Some(start) = self.policy.software_start {
                self.cap = self.cap.min(start);
                let auto = self.auto_scale();
                if !self.user && self.scale > auto {
                    self.scale = auto;
                    return true;
                }
            }
        }
        false
    }

    /// The user moved the slider to `percent`. Returns the new scale (what
    /// to store).
    pub fn set_percent(&mut self, percent: f64) -> f64 {
        let percent = if percent.is_finite() && percent != 0.0 {
            percent
        } else {
            100.0
        };
        self.scale = self.policy.from_percent(percent);
        self.user = true;
        self.scale
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    const FIXTURE: &str = include_str!("../../../fixtures/resolution.json");

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Fixture {
        default_scale: Vec<DefaultCase>,
        backing: Vec<BackingCase>,
        state: Vec<StateCase>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DefaultCase {
        css_w: f64,
        css_h: f64,
        dpr: f64,
        #[serde(default)]
        coarse: bool,
        #[serde(default)]
        software: bool,
        #[serde(default)]
        pixel_budget: Option<f64>,
        scale: f64,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct BackingCase {
        native_w: f64,
        native_h: f64,
        scale: f64,
        max_texture: u32,
        size: (u32, u32),
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct StateCase {
        name: String,
        native_w: f64,
        native_h: f64,
        dpr: f64,
        #[serde(default)]
        coarse: bool,
        stored: Option<f64>,
        steps: Vec<Step>,
    }

    #[derive(Deserialize)]
    #[serde(tag = "do", rename_all = "camelCase")]
    enum Step {
        #[serde(rename_all = "camelCase")]
        Expect { scale: f64, size: (u32, u32) },
        #[serde(rename_all = "camelCase")]
        Resize {
            native_w: f64,
            native_h: f64,
            dpr: f64,
        },
        #[serde(rename_all = "camelCase")]
        GpuInfo {
            is_fallback: bool,
            max_texture_dim: u32,
            lowered: bool,
        },
        #[serde(rename_all = "camelCase")]
        Slider { percent: f64 },
    }

    fn fixture() -> Fixture {
        serde_json::from_str(FIXTURE).unwrap()
    }

    #[test]
    fn default_scale_matches_the_fixture() {
        for case in fixture().default_scale {
            let policy = ResolutionPolicy {
                pixel_budget: case.pixel_budget.unwrap_or(2_800_000.0),
                ..Default::default()
            };
            let device = DeviceClass {
                coarse: case.coarse,
                software: case.software,
            };
            let got = policy.default_scale(case.css_w, case.css_h, case.dpr, device);
            assert!(
                (got - case.scale).abs() < 1e-9,
                "{}×{} @{} coarse {} software {}: {got} != {}",
                case.css_w,
                case.css_h,
                case.dpr,
                case.coarse,
                case.software,
                case.scale
            );
        }
    }

    #[test]
    fn backing_matches_the_fixture() {
        let policy = ResolutionPolicy::default();
        for case in fixture().backing {
            assert_eq!(
                policy.backing_size(case.native_w, case.native_h, case.scale, case.max_texture),
                case.size,
                "{}×{} × {} (max {})",
                case.native_w,
                case.native_h,
                case.scale,
                case.max_texture
            );
        }
    }

    #[test]
    fn state_sequences_match_the_fixture() {
        for case in fixture().state {
            let device = DeviceClass {
                coarse: case.coarse,
                software: false,
            };
            let mut state = ResolutionState::new(
                ResolutionPolicy::default(),
                case.native_w,
                case.native_h,
                case.dpr,
                device,
                case.stored,
            );
            for (index, step) in case.steps.iter().enumerate() {
                let at = format!("{} step {index}", case.name);
                match *step {
                    Step::Expect { scale, size } => {
                        assert!((state.scale() - scale).abs() < 1e-9, "{at}: scale");
                        assert_eq!(state.backing(), size, "{at}: size");
                    }
                    Step::Resize {
                        native_w,
                        native_h,
                        dpr,
                    } => state.resized(native_w, native_h, dpr),
                    Step::GpuInfo {
                        is_fallback,
                        max_texture_dim,
                        lowered,
                    } => {
                        let got = state.gpu_info(&GpuInfo {
                            is_fallback,
                            max_texture_dim,
                        });
                        assert_eq!(got, lowered, "{at}: lowered");
                    }
                    Step::Slider { percent } => {
                        state.set_percent(percent);
                    }
                }
            }
        }
    }

    #[test]
    fn stored_values() {
        let policy = ResolutionPolicy::default();
        assert_eq!(policy.parse_stored("0.80"), Some(0.8));
        assert_eq!(policy.parse_stored("0.3"), Some(0.5));
        assert_eq!(policy.parse_stored("7"), Some(1.0));
        assert_eq!(policy.parse_stored("x"), None);
        assert_eq!(policy.parse_stored(""), None);
        assert_eq!(policy.parse_stored("NaN"), None);
        assert_eq!(ResolutionPolicy::format_stored(0.8), "0.80");
        assert_eq!(ResolutionPolicy::format_stored(0.65), "0.65");
        assert_eq!(ResolutionPolicy::percent(0.6), 60);
        assert_eq!(ResolutionPolicy::percent(0.55), 55);
    }

    #[test]
    fn the_documented_example() {
        // 1200 × 897 CSS pixels at a ratio of 2 → 4.3 million native → 80%.
        let policy = ResolutionPolicy::default();
        let scale = policy.default_scale(1200.0, 897.0, 2.0, DeviceClass::default());
        assert!((scale - 0.8).abs() < 1e-12);
        // Dreidel Derby's 2.0 MP budget.
        let dreidel = ResolutionPolicy {
            pixel_budget: 2_000_000.0,
            ..policy
        };
        let scale = dreidel.default_scale(1200.0, 897.0, 2.0, DeviceClass::default());
        assert!((scale - 0.65).abs() < 1e-12);
    }
}
