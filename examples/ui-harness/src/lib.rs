//! See Cargo.toml. Everything the panels send is recorded in
//! `window.__harness` for a test to read.

use std::cell::RefCell;
use std::rc::Rc;

use awsm_renderer_settings::{
    AaMode, GpuInfo, SettingsConfig, SettingsHooks, SettingsOptions, SettingsPanel, SettingsStore,
    SettingsToPage, SettingsUi,
};
use awsm_renderer_stats::{
    FrameInput, SectionSpec, StatRow, StatsCollector, StatsConfig, StatsHooks, StatsPanel,
    StatsToPage,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

fn record(kind: &str, detail: JsValue) {
    let window = web_sys::window().unwrap();
    let log = js_sys::Reflect::get(&window, &"__harness".into()).unwrap();
    let log: js_sys::Array = if log.is_undefined() {
        let array = js_sys::Array::new();
        let _ = js_sys::Reflect::set(&window, &"__harness".into(), &array);
        array
    } else {
        log.unchecked_into()
    };
    let entry = js_sys::Array::of2(&kind.into(), &detail);
    log.push(&entry);
}

thread_local! {
    static SETTINGS: RefCell<Option<SettingsPanel>> = const { RefCell::new(None) };
}

#[wasm_bindgen(start)]
pub fn start() {
    let document = web_sys::window().unwrap().document().unwrap();
    let by_id = |id: &str| document.get_element_by_id(id).unwrap();
    let canvas: web_sys::HtmlCanvasElement = by_id("view").unchecked_into();
    let ui = by_id("ui");

    let store = SettingsStore::new("kit_harness_");
    if store.reset_if_asked() {
        record("log", "?reset".into());
    }
    let options = SettingsOptions::new(&SettingsConfig::default(), Some(GpuInfo::default()));
    let settings = SettingsPanel::new(
        &ui,
        &canvas,
        store,
        &options,
        SettingsUi::default(),
        |line| record("log", line.into()),
    );
    let (w, h) = settings.backing();
    record("init", format!("{w}x{h} {}", settings.aa().id()).into());
    let row = document.create_element("div").unwrap();
    row.set_class_name("settings-row harness-extra");
    row.set_text_content(Some("an app row"));
    settings.start(
        &canvas,
        SettingsHooks {
            on_resize: Box::new(|w, h| record("resize", format!("{w}x{h}").into())),
            on_anti_alias: Box::new(|mode| record("anti-alias", mode.id().into())),
            open_button: Some(by_id("settings-btn")),
            extra_rows: vec![row],
        },
    );
    SETTINGS.with_borrow_mut(|s| *s = Some(settings));

    // The stats: a collector here, fed fake 60 Hz frames.
    let collector = Rc::new(RefCell::new(StatsCollector::new(StatsConfig {
        phases: vec!["read".into(), "render (encode + submit)".into()],
        render_phase: Some(1),
        sections: vec![SectionSpec::new("app", "app (harness)", Some("sim"))],
        ..StatsConfig::default()
    })));
    let posted = collector.clone();
    let panel = StatsPanel::mount(
        "kit_harness_",
        &by_id("stats").unchecked_into(),
        Some(&by_id("stats-btn").unchecked_into()),
        StatsHooks {
            post: Box::new(move |message| {
                record("post", message.to_js().unwrap_or(JsValue::NULL));
                match posted.try_borrow_mut() {
                    Ok(mut collector) => collector.handle(message.clone()),
                    Err(_) => record("reentrant-post", JsValue::NULL),
                }
            }),
            on_help: Some(Box::new(|| record("help", JsValue::NULL))),
            page_part: "harness frame".into(),
            app_sections: None,
            shown: None,
        },
    );
    let frames = Rc::new(RefCell::new(0u32));
    type FrameLoop = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;
    let tick: FrameLoop = Rc::new(RefCell::new(None));
    let again = tick.clone();
    *tick.borrow_mut() = Some(Closure::new(move |t: f64| {
        // The collector is released before the panel sees a report: the
        // panel's `post` hook reaches the same collector.
        let report = {
            let mut stats = collector.borrow_mut();
            let mut n = frames.borrow_mut();
            *n += 1;
            let ready = (*n == 3).then(|| stats.mark_ready());
            stats.begin_frame(t);
            stats.lap(0, t + 0.2);
            stats.lap(1, t + 0.7);
            ready.or_else(|| {
                stats.end_frame(FrameInput::new(t, 0.9), &()).map(|window| {
                    let section = SectionSpec::new("app", "app (harness)", Some("sim"))
                        .group()
                        .headline("12 µs / step")
                        .budget_ms(Some(0.1))
                        .row(StatRow::new("steps", "120 steps/s"))
                        .row(window.late.row("late frames"));
                    stats.report(&window, vec![section])
                })
            })
        };
        if let Some(report) = report {
            panel.handle(&StatsToPage::Stats(report));
        }
        panel.page_frame(0.05, None, true);
        if let Some(callback) = again.borrow().as_ref() {
            let _ = web_sys::window()
                .unwrap()
                .request_animation_frame(callback.as_ref().unchecked_ref());
        }
    }));
    let first = tick.borrow();
    if let Some(callback) = first.as_ref() {
        let _ = web_sys::window()
            .unwrap()
            .request_animation_frame(callback.as_ref().unchecked_ref());
    }
}

/// What a worker would send around a mode change; the test drives it.
#[wasm_bindgen]
pub fn compile(mode: &str, phase: &str) {
    let mode = AaMode::from_id(mode).unwrap_or(AaMode::Msaa);
    let message = match phase {
        "start" => SettingsToPage::AaCompileStart {
            mode,
            label: mode.label().into(),
            compiles: true,
        },
        "progress" => SettingsToPage::AaCompileProgress {
            message: "Compiling pipelines (3 remaining)".into(),
        },
        "fallback" => SettingsToPage::AaCompileDone {
            ok: false,
            error: Some("test".into()),
            mode: AaMode::Msaa,
            taa_fallback: Some("a test fallback".into()),
        },
        _ => SettingsToPage::AaCompileDone {
            ok: true,
            error: None,
            mode,
            taa_fallback: None,
        },
    };
    SETTINGS.with_borrow(|s| s.as_ref().unwrap().handle(&message));
}

/// The scene is on screen (the notice, on a touch device).
#[wasm_bindgen]
pub fn ready() {
    SETTINGS.with_borrow(|s| s.as_ref().unwrap().ready());
}
