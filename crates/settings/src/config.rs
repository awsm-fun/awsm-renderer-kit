//! What an app configures, and the choices made from it on the page.

use serde::{Deserialize, Serialize};

use crate::{AaDefaults, AaMode, DeviceClass, ResolutionPolicy};

/// An app's settings: which modes its menu offers, the default per device
/// class, and the resolution policy. `Default` is the kit's policy: all
/// seven modes, TAA on a desktop, Off on a touch device and on a software
/// GPU, and the shared resolution numbers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsConfig {
    /// The menu, in order. An app with a fixed mode offers just that one
    /// (and hides the menu, `SettingsUi::show_aa`).
    pub modes: Vec<AaMode>,
    pub defaults: AaDefaults,
    pub resolution: ResolutionPolicy,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            modes: AaMode::ALL.to_vec(),
            defaults: AaDefaults::default(),
            resolution: ResolutionPolicy::default(),
        }
    }
}

impl SettingsConfig {
    /// Whether the menu offers `mode`.
    pub fn offers(&self, mode: AaMode) -> bool {
        self.modes.contains(&mode)
    }

    /// `id` as an offered mode.
    pub fn parse(&self, id: &str) -> Option<AaMode> {
        AaMode::from_id(id).filter(|mode| self.offers(*mode))
    }

    /// The default for `device`, or the first offered mode when the app
    /// configured a default its menu does not offer.
    pub fn default_for(&self, device: DeviceClass) -> AaMode {
        let mode = self.defaults.pick(device);
        if self.offers(mode) {
            mode
        } else {
            self.modes.first().copied().unwrap_or(mode)
        }
    }

    /// The mode this session starts with: `?aa=` (for this session only, not
    /// stored), else the stored choice, else the device default. Ids that
    /// are not offered are ignored.
    pub fn initial_aa(
        &self,
        device: DeviceClass,
        stored: Option<&str>,
        url: Option<&str>,
    ) -> (AaMode, AaSource) {
        if let Some(mode) = url.and_then(|id| self.parse(id)) {
            return (mode, AaSource::Url);
        }
        if let Some(mode) = stored.and_then(|id| self.parse(id)) {
            return (mode, AaSource::Stored);
        }
        (self.default_for(device), AaSource::Default)
    }
}

/// Where the session's starting mode came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AaSource {
    Url,
    Stored,
    Default,
}

/// Whether to show the one-time "display quality reduced" notice once the
/// scene is on screen: on a touch device while a device default is in effect
/// (no stored resolution, or no stored anti-aliasing). It shows on every
/// such load until the user makes their own choices in Settings.
pub fn show_quality_notice(device: DeviceClass, stored_scale: bool, stored_aa: bool) -> bool {
    device.coarse && (!stored_scale || !stored_aa)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESKTOP: DeviceClass = DeviceClass {
        coarse: false,
        software: false,
    };
    const PHONE: DeviceClass = DeviceClass {
        coarse: true,
        software: false,
    };

    #[test]
    fn initial_mode_precedence() {
        let config = SettingsConfig::default();
        assert_eq!(
            config.initial_aa(DESKTOP, None, None),
            (AaMode::Taa, AaSource::Default)
        );
        assert_eq!(
            config.initial_aa(PHONE, None, None),
            (AaMode::Off, AaSource::Default)
        );
        // Stored before the kit: still honoured.
        assert_eq!(
            config.initial_aa(DESKTOP, Some("msaa"), None),
            (AaMode::Msaa, AaSource::Stored)
        );
        assert_eq!(
            config.initial_aa(PHONE, Some("smaa"), None),
            (AaMode::Smaa, AaSource::Stored)
        );
        // ?aa= wins for the session.
        assert_eq!(
            config.initial_aa(DESKTOP, Some("msaa"), Some("taa-quality")),
            (AaMode::TaaQuality, AaSource::Url)
        );
        // Junk is ignored.
        assert_eq!(
            config.initial_aa(DESKTOP, Some("msaa8"), Some("fxaa")),
            (AaMode::Taa, AaSource::Default)
        );
    }

    #[test]
    fn modes_not_offered_are_ignored() {
        let config = SettingsConfig {
            modes: vec![AaMode::Msaa],
            ..Default::default()
        };
        assert_eq!(
            config.initial_aa(DESKTOP, Some("taa"), Some("off")),
            (AaMode::Msaa, AaSource::Default)
        );
        assert_eq!(config.default_for(PHONE), AaMode::Msaa);
    }

    #[test]
    fn quality_notice() {
        assert!(!show_quality_notice(DESKTOP, false, false));
        assert!(show_quality_notice(PHONE, false, false));
        assert!(show_quality_notice(PHONE, true, false));
        assert!(show_quality_notice(PHONE, false, true));
        assert!(!show_quality_notice(PHONE, true, true));
    }
}
