//! The stats messages between the page and the worker (the same envelope as
//! the settings ones: kebab-case `type`, camelCase fields).

use serde::{Deserialize, Serialize};

use crate::FrameReport;

/// Worker → page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum StatsToPage {
    /// `stats`: a report, about once a second while the scene runs (and the
    /// placeholders before).
    Stats(FrameReport),
}

/// Page → worker.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum StatsToWorker {
    /// `stats-view`: whether the panel is shown and which groups are
    /// expanded (their ids). Sent at start-up and on every change: the
    /// costly breakdowns are measured only for these.
    StatsView { shown: bool, open: Vec<String> },
    /// `stats-page`: about once a second, what only the page can measure:
    /// the gaps between its own animation frames (the display's refresh
    /// rate), and its own work since the last one (`page`, named ms totals
    /// over `frames` frames).
    StatsPage {
        gaps: Vec<f64>,
        page: Vec<PagePart>,
        frames: u32,
    },
}

impl StatsToPage {
    pub const TYPES: [&'static str; 1] = ["stats"];
}

impl StatsToWorker {
    pub const TYPES: [&'static str; 2] = ["stats-view", "stats-page"];
}

/// A named part of the page's work (`"camera frame loop"`), in ms.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PagePart {
    pub name: String,
    pub ms: f64,
}

#[cfg(any(feature = "renderer", feature = "web"))]
mod js {
    use super::{StatsToPage, StatsToWorker};
    use awsm_renderer_kit_web::message::{from_js, message_type, to_js};
    use wasm_bindgen::JsValue;

    impl StatsToWorker {
        /// A page message as this enum: `None` when its `type` is not one of
        /// [`Self::TYPES`], `Some(Err)` when it is but does not parse.
        pub fn from_js(value: &JsValue) -> Option<Result<Self, String>> {
            let kind = message_type(value)?;
            Self::TYPES
                .contains(&kind.as_str())
                .then(|| from_js(value.clone()))
        }

        pub fn to_js(&self) -> Result<JsValue, String> {
            to_js(self)
        }
    }

    impl StatsToPage {
        /// A worker message as this enum (see [`StatsToWorker::from_js`]).
        pub fn from_js(value: &JsValue) -> Option<Result<Self, String>> {
            let kind = message_type(value)?;
            Self::TYPES
                .contains(&kind.as_str())
                .then(|| from_js(value.clone()))
        }

        pub fn to_js(&self) -> Result<JsValue, String> {
            to_js(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wire_format() {
        let msg: StatsToWorker = serde_json::from_value(
            json!({"type": "stats-view", "shown": true, "open": ["gpu", "app"]}),
        )
        .unwrap();
        assert_eq!(
            msg,
            StatsToWorker::StatsView {
                shown: true,
                open: vec!["gpu".into(), "app".into()]
            }
        );
        let msg: StatsToWorker = serde_json::from_value(json!({
            "type": "stats-page", "gaps": [16.6, 16.7],
            "page": [{"name": "app events + HUD", "ms": 1.5}], "frames": 2
        }))
        .unwrap();
        assert!(matches!(msg, StatsToWorker::StatsPage { frames: 2, .. }));
        let value = serde_json::to_value(StatsToPage::Stats(FrameReport::default())).unwrap();
        assert_eq!(value["type"], "stats");
        assert!(value["budget"].is_object());
    }
}
