//! The web-sys stats panel: the twin of `js/stats.js`'s `installStats`, with
//! the same DOM (`button.stats-group` headers with `aria-controls`
//! `stats-rows-<id>`, `div.stats-rows`, `div.stats-row` with `.stats-nested`
//! / `.stats-nested-2` / `.stats-bad`, `span.stats-name`, `span.stats-value`
//! split into `.stats-num` / `.stats-unit`, `span.stats-dot[data-lane]`,
//! `span.stats-bar` with `.stats-warn` / `.stats-over`, `button.stats-help`),
//! so one stylesheet serves both families.
//!
//! A view only: the worker measures and formats ([`crate::StatsCollector`]).
//! The panel sends back `stats-view` (shown, open groups) on every change and
//! `stats-page` (its own frame gaps and work) once a second.

use std::cell::RefCell;
use std::rc::Rc;

use awsm_renderer_kit_web::dom::{append, attr, el, el_text, is_shown, on, set_shown};
use awsm_renderer_kit_web::{coarse_pointer, storage};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element, HtmlElement};

use crate::format::value_pieces;
use crate::{
    AppSection, FrameReport, PagePart, StatGroup, StatRow, StatsToPage, StatsToWorker, SHARE_WARN,
};

/// Builds page-side app sections from a report.
pub type SectionsHook = Box<dyn Fn(&FrameReport) -> Vec<AppSection>>;

/// Page-side rows for a group the worker sent: `(group id, report)` → rows
/// appended after the worker's. Called only for expanded groups.
pub type SectionRowsHook = Box<dyn Fn(&str, &FrameReport) -> Vec<StatRow>>;

/// A self-rescheduling animation-frame callback.
type FrameLoop = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

/// How often the page sends what it measured (ms).
const SAMPLE_INTERVAL_MS: i32 = 1000;

/// Where the panel's messages go, and what the app adds.
pub struct StatsHooks {
    /// Sends a message to the worker that runs the collector.
    pub post: Box<dyn Fn(&StatsToWorker)>,
    /// Opens an explanation of the panel ("what do these mean?"); `None`:
    /// no such button.
    pub on_help: Option<Box<dyn Fn()>>,
    /// The name of the page's work in the page group.
    pub page_part: String,
    /// Groups only the page knows, shown after the worker's app sections
    /// (a MuJoCo worker's sim stats reaching the page directly). They do not
    /// join the frame budget; give the worker the numbers for that.
    pub app_sections: Option<SectionsHook>,
    /// Rows only the page knows, added to the end of a group the worker
    /// sent (the C family's game rows and "last cue" under the app's group).
    /// Called for expanded groups only, on every report. Hooks must not call
    /// back into the panel.
    pub section_rows: Option<SectionRowsHook>,
    /// The panel's initial state; `None`: the stored choice, never on a
    /// touch device.
    pub shown: Option<bool>,
}

struct GroupView {
    id: String,
    open: bool,
    header: HtmlElement,
    label: HtmlElement,
    arrow: HtmlElement,
    headline: HtmlElement,
    rows: HtmlElement,
    look: Option<(Option<String>, String)>,
}

struct Inner {
    prefix: String,
    panel: HtmlElement,
    button: Option<HtmlElement>,
    post: Box<dyn Fn(&StatsToWorker)>,
    page_part: String,
    app_sections: Option<SectionsHook>,
    section_rows: Option<SectionRowsHook>,
    groups: Vec<GroupView>,
    order: Vec<String>,
    report: Option<FrameReport>,
    gaps: Vec<f64>,
    last_frame: Option<f64>,
    page: Vec<(String, f64)>,
    page_frames: u32,
}

/// See the module docs. Cheap to clone (a handle). Lives as long as the
/// page.
#[derive(Clone)]
pub struct StatsPanel {
    inner: Rc<RefCell<Inner>>,
}

impl StatsPanel {
    /// Installs the panel into `panel` (hidden while the panel is), with
    /// `button` showing and hiding it (it gets `.active`). `prefix`: the
    /// app's storage prefix (`<prefix>stats`, `<prefix>stats_open_<id>`).
    pub fn mount(
        prefix: &str,
        panel: &HtmlElement,
        button: Option<&HtmlElement>,
        hooks: StatsHooks,
    ) -> Self {
        if let Some(on_help) = hooks.on_help {
            let help = el_text("button", "stats-help", "what do these mean?");
            attr(&help, "type", "button");
            on(&help, "click", move |_| on_help());
            append(panel, &help);
        }
        let this = Self {
            inner: Rc::new(RefCell::new(Inner {
                prefix: prefix.to_string(),
                panel: panel.clone(),
                button: button.cloned(),
                post: hooks.post,
                page_part: hooks.page_part,
                app_sections: hooks.app_sections,
                section_rows: hooks.section_rows,
                groups: Vec::new(),
                order: Vec::new(),
                report: None,
                gaps: Vec::new(),
                last_frame: None,
                page: Vec::new(),
                page_frames: 0,
            })),
        };

        // Off by default: on a phone the panel covers a lot of the screen.
        let shown = hooks.shown.unwrap_or_else(|| {
            !coarse_pointer() && storage::get_bool(&format!("{prefix}stats")) == Some(true)
        });
        this.show(shown);
        if let Some(button) = button {
            let panel = this.clone();
            on(button, "click", move |_| {
                let open = !panel.is_open();
                panel.set_open(open);
            });
        }
        this.start_sampling();
        this
    }

    /// A `stats` message from the worker.
    pub fn handle(&self, message: &StatsToPage) {
        match message {
            StatsToPage::Stats(report) => self.render(report.clone()),
        }
    }

    /// A raw worker message: handles it and returns `true` if it is a
    /// `stats` message.
    pub fn handle_js(&self, value: &JsValue) -> bool {
        match StatsToPage::from_js(value) {
            Some(Ok(message)) => {
                self.handle(&message);
                true
            }
            Some(Err(err)) => {
                web_sys::console::error_1(&format!("stats message: {err}").into());
                true
            }
            None => false,
        }
    }

    /// Once per animation frame: the time the page spent on it (ms), under
    /// `part` (`None`: the hooks' `page_part`). `count_frame`: false for a
    /// second part of the same frame.
    pub fn page_frame(&self, ms: f64, part: Option<&str>, count_frame: bool) {
        let mut inner = self.inner.borrow_mut();
        let name = part.map_or_else(|| inner.page_part.clone(), str::to_string);
        match inner.page.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 += ms,
            None => inner.page.push((name, ms)),
        }
        if count_frame {
            inner.page_frames += 1;
        }
    }

    /// Shows or hides the panel, and stores the choice.
    pub fn set_open(&self, open: bool) {
        let key = format!("{}stats", self.inner.borrow().prefix);
        storage::set_bool(&key, open);
        self.show(open);
    }

    pub fn is_open(&self) -> bool {
        is_shown(&self.inner.borrow().panel)
    }

    /// The latest report (its `summary` holds the raw figures).
    pub fn last_report(&self) -> Option<FrameReport> {
        self.inner.borrow().report.clone()
    }

    fn show(&self, open: bool) {
        let report = {
            let inner = self.inner.borrow();
            set_shown(&inner.panel, open);
            if let Some(button) = &inner.button {
                let _ = button.class_list().toggle_with_force("active", open);
            }
            inner.report.clone()
        };
        self.publish_view();
        // A hidden panel only keeps the latest report: show it now.
        if let (true, Some(report)) = (open, report) {
            self.render(report);
        }
    }

    fn publish_view(&self) {
        let message = {
            let inner = self.inner.borrow();
            StatsToWorker::StatsView {
                shown: is_shown(&inner.panel),
                open: inner
                    .groups
                    .iter()
                    .filter(|g| g.open)
                    .map(|g| g.id.clone())
                    .collect(),
            }
        };
        let inner = self.inner.borrow();
        (inner.post)(&message);
    }

    fn render(&self, report: FrameReport) {
        // Hidden: nothing to draw (the report still arrives each second, for
        // `last_report`); it is drawn when the panel shows.
        if !self.is_open() {
            self.inner.borrow_mut().report = Some(report);
            return;
        }
        let extra = {
            let inner = self.inner.borrow();
            inner.app_sections.as_ref().map(|f| f(&report))
        };
        let mut groups: Vec<StatGroup> = [
            report.budget.clone(),
            report.display.clone(),
            report.gpu.clone(),
            report.renderer.clone(),
        ]
        .into_iter()
        .chain(report.app.iter().cloned())
        .chain(extra.unwrap_or_default())
        .chain(std::iter::once(report.page.clone()))
        .collect();

        // Every group's view (a new one brings its stored open state).
        let mut created = false;
        let open: Vec<bool> = {
            let mut inner = self.inner.borrow_mut();
            groups
                .iter()
                .map(
                    |group| match inner.groups.iter().find(|g| g.id == group.id) {
                        Some(view) => view.open,
                        None => {
                            created = true;
                            let prefix = inner.prefix.clone();
                            let view = self.group_view(&prefix, &group.id);
                            let open = view.open;
                            inner.groups.push(view);
                            open
                        }
                    },
                )
                .collect()
        };
        // The page's own rows, for the expanded groups.
        {
            let inner = self.inner.borrow();
            if let Some(hook) = &inner.section_rows {
                for (group, _) in groups.iter_mut().zip(&open).filter(|(_, open)| **open) {
                    let rows = hook(&group.id, &report);
                    group.rows.extend(rows);
                }
            }
        }

        {
            let mut inner = self.inner.borrow_mut();
            inner.report = Some(report);
            for group in &groups {
                if let Some(view) = inner.groups.iter_mut().find(|g| g.id == group.id) {
                    show_group(view, group);
                }
            }
            let ids: Vec<String> = groups.iter().map(|g| g.id.clone()).collect();
            // Only re-append when the order changed: moving a focused header
            // would take the focus away.
            if ids != inner.order {
                for id in &ids {
                    if let Some(view) = inner.groups.iter().find(|g| g.id == *id) {
                        append(&inner.panel, &view.header);
                        append(&inner.panel, &view.rows);
                    }
                }
                for view in inner.groups.iter().filter(|g| !ids.contains(&g.id)) {
                    view.header.remove();
                    view.rows.remove();
                }
                inner.order = ids;
            }
        }
        // New groups bring their stored open state: tell the worker.
        if created {
            self.publish_view();
        }
    }

    fn group_view(&self, prefix: &str, id: &str) -> GroupView {
        let key = format!("{prefix}stats_open_{id}");
        let header = el("button", "stats-group");
        attr(&header, "type", "button");
        let label = el("span", "stats-name");
        let arrow = el("span", "");
        let headline = el("span", "stats-value");
        append(&header, &label);
        let _ = header.append_with_str_1(" "); // for a screen reader
        append(&header, &headline);
        let rows = el("div", "stats-rows");
        rows.set_id(&format!("stats-rows-{id}"));
        attr(&header, "aria-controls", &rows.id());
        let open = storage::get_bool(&key) == Some(true);
        let view = GroupView {
            id: id.to_string(),
            open,
            header: header.clone(),
            label,
            arrow,
            headline,
            rows,
            look: None,
        };
        set_open(&view, open);

        let panel = self.clone();
        let id = id.to_string();
        on(&header, "click", move |_| {
            {
                let mut inner = panel.inner.borrow_mut();
                let Some(view) = inner.groups.iter_mut().find(|g| g.id == id) else {
                    return;
                };
                view.open = !view.open;
                set_open(view, view.open);
                storage::set_bool(&key, view.open);
            }
            panel.publish_view();
            // The rows of a group just opened fill in with the next report.
            let report = panel.inner.borrow().report.clone();
            if let Some(report) = report {
                panel.render(report);
            }
        });
        view
    }

    /// The page's own animation frames (their gaps are the display's
    /// refresh) and the once-a-second `stats-page`.
    fn start_sampling(&self) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let tick: FrameLoop = Rc::new(RefCell::new(None));
        let panel = self.clone();
        let again = tick.clone();
        let frame_window = window.clone();
        *tick.borrow_mut() = Some(Closure::new(move |t: f64| {
            {
                let mut inner = panel.inner.borrow_mut();
                if let Some(last) = inner.last_frame.replace(t) {
                    inner.gaps.push(t - last);
                }
            }
            if let Some(callback) = again.borrow().as_ref() {
                let _ = frame_window.request_animation_frame(callback.as_ref().unchecked_ref());
            }
        }));
        if let Some(callback) = tick.borrow().as_ref() {
            let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
        }

        // A hidden tab runs no frames: the gap across it is not a refresh.
        if let Some(document) = window.document() {
            let panel = self.clone();
            on(&document, "visibilitychange", move |_| {
                panel.inner.borrow_mut().last_frame = None;
            });
        }

        let panel = self.clone();
        let sample = Closure::<dyn FnMut()>::new(move || {
            let message = {
                let mut inner = panel.inner.borrow_mut();
                StatsToWorker::StatsPage {
                    gaps: std::mem::take(&mut inner.gaps),
                    page: std::mem::take(&mut inner.page)
                        .into_iter()
                        .map(|(name, ms)| PagePart { name, ms })
                        .collect(),
                    frames: std::mem::take(&mut inner.page_frames),
                }
            };
            let inner = panel.inner.borrow();
            (inner.post)(&message);
        });
        let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
            sample.as_ref().unchecked_ref(),
            SAMPLE_INTERVAL_MS,
        );
        sample.forget();
    }
}

fn set_open(view: &GroupView, open: bool) {
    set_shown(&view.rows, open);
    attr(
        &view.header,
        "aria-expanded",
        if open { "true" } else { "false" },
    );
    view.arrow
        .set_text_content(Some(if open { "▾ " } else { "▸ " }));
}

fn dot(lane: &str) -> HtmlElement {
    let element = el("span", "stats-dot");
    attr(&element, "data-lane", lane);
    element
}

/// Fills `element` with a value as it is shown: the numbers bright, the
/// units and words around them dim.
fn show_value(element: &Element, text: &str) {
    element.set_text_content(None);
    for (number, piece) in value_pieces(text) {
        append(
            element,
            &el_text(
                "span",
                if number { "stats-num" } else { "stats-unit" },
                piece,
            ),
        );
    }
}

fn show_group(view: &mut GroupView, group: &StatGroup) {
    let look = (group.lane.clone(), group.label.clone());
    if view.look.as_ref() != Some(&look) {
        view.label.set_text_content(None);
        append(&view.label, &view.arrow);
        if let Some(lane) = &group.lane {
            append(&view.label, &dot(lane));
        }
        let _ = view.label.append_with_str_1(&group.label);
        view.look = Some(look);
    }
    show_value(&view.headline, &group.headline);
    let _ = view
        .header
        .class_list()
        .toggle_with_force("stats-bad", group.bad);
    if view.open {
        view.rows.set_text_content(None);
        for row in &group.rows {
            append(&view.rows, &row_element(row));
        }
    }
}

fn row_element(row: &StatRow) -> HtmlElement {
    let nested =
        ["", " stats-nested", " stats-nested stats-nested-2"][usize::from(row.depth.min(2))];
    let bad = if row.bad { " stats-bad" } else { "" };
    let element = el("div", &format!("stats-row{nested}{bad}"));
    let name = el("span", "stats-name");
    if let Some(lane) = &row.lane {
        append(&name, &dot(lane));
    }
    let _ = name.append_with_str_1(&row.label);
    let figure = el("span", "stats-value");
    show_value(&figure, &row.value);
    if let (Some(_), Some(share)) = (&row.lane, row.share) {
        // The share as a bar too; its colour says how close to the budget.
        let state = if share > 1.0 {
            " stats-over"
        } else if share >= SHARE_WARN {
            " stats-warn"
        } else {
            ""
        };
        let bar = el("span", &format!("stats-bar{state}"));
        let fill = el("span", "");
        let _ = fill
            .style()
            .set_property("width", &format!("{}%", share.min(1.0) * 100.0));
        append(&bar, &fill);
        let _ = figure.prepend_with_node_1(&bar);
    }
    append(&element, &name);
    append(&element, &figure);
    element
}
