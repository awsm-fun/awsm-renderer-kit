//! What the worker sends the page: every group of the panel, ready to show.

use serde::{Deserialize, Serialize};

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

/// One report, about once a second: the standard groups, the app's sections,
/// and the raw figures behind them (for a governor). A view renders it
/// generically, in [`Self::groups`] order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameReport {
    /// `"starting…"` / `"warming up…"` while there are no numbers yet (every
    /// headline says so too).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub budget: StatGroup,
    pub display: StatGroup,
    pub gpu: StatGroup,
    pub renderer: StatGroup,
    /// The app's sections (physics, poses, a sim, a C app), between the
    /// renderer and the page.
    #[serde(default)]
    pub app: Vec<AppSection>,
    pub page: StatGroup,
    #[serde(default)]
    pub summary: Summary,
}

impl FrameReport {
    /// Every group, top to bottom.
    pub fn groups(&self) -> impl Iterator<Item = &StatGroup> {
        [&self.budget, &self.display, &self.gpu, &self.renderer]
            .into_iter()
            .chain(self.app.iter())
            .chain(std::iter::once(&self.page))
    }
}

/// A group of the panel: a header (label, headline) and the rows it shows
/// while expanded. Rows are only sent for expanded groups.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatGroup {
    /// Stable: the storage key `<prefix>stats_open_<id>`, the element id
    /// `stats-rows-<id>`, and what the page reports as open.
    pub id: String,
    pub label: String,
    /// The colour lane (`gpu`, `renderer`, `sim`, `page`: the CSS
    /// `--lane-*` tokens), shown as a dot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    pub headline: String,
    /// Red header.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bad: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<StatRow>,
    /// An app section's time per frame drawn (ms): a lane of the frame
    /// budget, labelled with the section's label (or `budget_label`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_ms: Option<f64>,
    /// The budget lane's label when it is not the group's (one lane for a
    /// thread that two sections share: `"app worker (app + Box3D step)"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_label: Option<String>,
}

/// An app's section: a group like any other.
pub type AppSection = StatGroup;

impl StatGroup {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            ..Self::default()
        }
    }

    pub fn lane(mut self, lane: impl Into<String>) -> Self {
        self.lane = Some(lane.into());
        self
    }

    pub fn headline(mut self, headline: impl Into<String>) -> Self {
        self.headline = headline.into();
        self
    }

    pub fn bad(mut self, bad: bool) -> Self {
        self.bad = bad;
        self
    }

    pub fn rows(mut self, rows: Vec<StatRow>) -> Self {
        self.rows = rows;
        self
    }

    pub fn row(mut self, row: StatRow) -> Self {
        self.rows.push(row);
        self
    }

    /// Joins the frame budget with this time per frame.
    pub fn budget_ms(mut self, ms: Option<f64>) -> Self {
        self.budget_ms = ms;
        self
    }

    /// Labels this section's budget lane (default: the group's label).
    pub fn budget_label(mut self, label: impl Into<String>) -> Self {
        self.budget_label = Some(label.into());
        self
    }
}

/// A row: label left, value right. The value is the whole display string
/// (`"1.23 ms / frame"`); the view splits numbers from units for styling
/// (`format::value_pieces`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatRow {
    pub label: String,
    pub value: String,
    /// 0 for a group's own rows; 1 and 2 for a breakdown of a row above.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub depth: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bad: bool,
    /// A lane of the frame budget: its dot...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    /// ...and its share of the budget, drawn as a bar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share: Option<f64>,
}

impl StatRow {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            ..Self::default()
        }
    }

    pub fn depth(mut self, depth: u8) -> Self {
        self.depth = depth.min(2);
        self
    }

    pub fn bad(mut self, bad: bool) -> Self {
        self.bad = bad;
        self
    }

    pub fn budget(mut self, lane: impl Into<String>, share: f64) -> Self {
        self.lane = Some(lane.into());
        self.share = Some(share);
        self
    }
}

/// The raw figures behind a report (smoothed), for code that acts on them:
/// an auto-tuner reads these instead of re-deriving them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub refresh_ms: Option<f64>,
    pub fps: Option<f64>,
    /// The render worker's CPU per frame.
    pub cpu_ms: Option<f64>,
    /// The GPU per sampled frame.
    pub gpu_ms: Option<f64>,
    pub page_ms: Option<f64>,
    /// Refreshes per presented frame the renderer paces at (1: every one).
    pub frame_interval: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wire_format_is_compact() {
        let group = StatGroup::new("gpu", "GPU")
            .lane("gpu")
            .headline("2.00 ms / frame")
            .row(StatRow::new("Geometry", "1.00 ms / frame"))
            .row(
                StatRow::new("GPU", "2.0 / 16.7 ms (12%)")
                    .depth(1)
                    .budget("gpu", 0.12),
            );
        assert_eq!(
            serde_json::to_value(&group).unwrap(),
            json!({
                "id": "gpu", "label": "GPU", "lane": "gpu", "headline": "2.00 ms / frame",
                "rows": [
                    {"label": "Geometry", "value": "1.00 ms / frame"},
                    {"label": "GPU", "value": "2.0 / 16.7 ms (12%)", "depth": 1, "lane": "gpu", "share": 0.12}
                ]
            })
        );
    }

    #[test]
    fn report_round_trips_and_orders_groups() {
        let report = FrameReport {
            status: None,
            budget: StatGroup::new("budget", "frame budget"),
            display: StatGroup::new("display", "display"),
            gpu: StatGroup::new("gpu", "GPU"),
            renderer: StatGroup::new("renderer", "renderer"),
            app: vec![StatGroup::new("app", "app").budget_ms(Some(1.5)).bad(true)],
            page: StatGroup::new("page", "page"),
            summary: Summary {
                refresh_ms: Some(16.7),
                frame_interval: 1,
                ..Summary::default()
            },
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: FrameReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back, report);
        let ids: Vec<_> = report.groups().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["budget", "display", "gpu", "renderer", "app", "page"]);
    }
}
