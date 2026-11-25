//! The web-sys settings panel: the Settings modal (resolution slider,
//! anti-aliasing menu, the app's own rows), the reduced-quality notice and
//! the "compiling pipelines" overlay. No framework. The twin of
//! `js/settings.js`'s `Settings`: the same DOM (ids `settings`, `res-label`,
//! `res-slider`, `aa-select`, `quality-notice`, `aa-compile`,
//! `aa-compile-line`; classes `settings-overlay`, `settings-modal`,
//! `settings-row`, `settings-label`, `res-slider`, `settings-select`,
//! `settings-hint`, `notice-text`, `notice-ok`, `compile-overlay`), so one
//! stylesheet serves both families.
//!
//! The app owns where the panel lives (`container`), the button that opens
//! it, and the CSS.

use std::cell::RefCell;
use std::rc::Rc;

use awsm_renderer_kit_web::coarse_pointer;
use awsm_renderer_kit_web::dom::{append, attr, el, el_text, on, Modal, ModalOptions};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element, HtmlCanvasElement, HtmlElement, HtmlInputElement, HtmlSelectElement};

use crate::store::{observe_canvas, CanvasSize};
use crate::{
    show_quality_notice, AaMode, AaSource, DeviceClass, GpuInfo, ResolutionPolicy, ResolutionState,
    SettingsConfig, SettingsOptions, SettingsStore, SettingsToPage,
};

/// The words and DOM knobs. `Default` is what the templates shipped.
#[derive(Clone, Debug)]
pub struct SettingsUi {
    pub title: String,
    /// The line under the controls (`None`: no line).
    pub hint: Option<String>,
    /// The class of the × buttons (`about-close` in the C family and the
    /// Rust v1 projects, `settings-close` in v2).
    pub close_class: String,
    pub notice_title: String,
    pub notice_text: String,
    pub compile_title: String,
    pub compile_text: String,
    /// `false`: no anti-aliasing menu (an app with a fixed mode).
    pub show_aa: bool,
    /// Escape that closes the modal or the notice stops there (a game that
    /// pauses on Escape).
    pub escape_stops: bool,
}

impl Default for SettingsUi {
    fn default() -> Self {
        Self {
            title: "Settings".into(),
            hint: Some("Adjust these settings if your framerate is low.".into()),
            close_class: "about-close".into(),
            notice_title: "Display Quality Reduced".into(),
            notice_text: "Display quality was reduced to keep the frame rate up on touch \
                          devices. You can adjust the resolution and anti-aliasing any time \
                          in Settings (bottom right)."
                .into(),
            compile_title: "Compiling Pipelines".into(),
            compile_text: "Rebuilding the render pipelines for the new anti-aliasing \
                           settings. The first switch in each direction compiles shaders; \
                           after that it's cached and instant."
                .into(),
            show_aa: true,
            escape_stops: false,
        }
    }
}

/// Where the panel's changes go.
pub struct SettingsHooks {
    /// Every new backing-store size (device pixels): post `resize`.
    pub on_resize: Box<dyn FnMut(u32, u32)>,
    /// Every mode the user picks: post `anti-alias`.
    pub on_anti_alias: Box<dyn FnMut(AaMode)>,
    /// An element that opens the modal on click.
    pub open_button: Option<Element>,
    /// The app's own rows (audio, a stats toggle, game options), inserted
    /// after the anti-aliasing row.
    pub extra_rows: Vec<Element>,
}

struct Dom {
    settings: Modal,
    notice: Modal,
    compiling: Modal,
    compiling_line: HtmlElement,
    res_label: HtmlElement,
    res_slider: HtmlInputElement,
    aa_select: HtmlSelectElement,
    hint: Option<HtmlElement>,
    card: HtmlElement,
}

struct Inner {
    store: SettingsStore,
    config: SettingsConfig,
    resolution: ResolutionState,
    aa: AaMode,
    show_notice: bool,
    log: Rc<dyn Fn(&str)>,
    dom: Dom,
    on_resize: Option<Box<dyn FnMut(u32, u32)>>,
    on_anti_alias: Option<Box<dyn FnMut(AaMode)>>,
}

/// See the module docs. Cheap to clone (a handle).
#[derive(Clone)]
pub struct SettingsPanel {
    inner: Rc<RefCell<Inner>>,
}

impl SettingsPanel {
    /// Reads the stored choices and works out the defaults, and builds the
    /// modal, the notice and the overlay into `container` (hidden). Call it
    /// after [`SettingsStore::reset_if_asked`] and before the canvas goes to
    /// the renderer: the first size and mode come from here ([`Self::backing`],
    /// [`Self::aa`]). `options`: the worker's `settings-options`, or
    /// `SettingsOptions::new(&config, None)` on a page that has the config
    /// itself (then a later `gpu-info` goes to [`Self::handle`]). `log`: a
    /// line for the loading log.
    pub fn new(
        container: &Element,
        canvas: &HtmlCanvasElement,
        store: SettingsStore,
        options: &SettingsOptions,
        ui: SettingsUi,
        log: impl Fn(&str) + 'static,
    ) -> Self {
        let log: Rc<dyn Fn(&str)> = Rc::new(log);
        let config = options.config();
        let policy = config.resolution;
        let device = DeviceClass {
            coarse: coarse_pointer(),
            software: options.gpu.is_some_and(|gpu| gpu.is_fallback),
        };
        let size = CanvasSize::of(canvas);
        let stored_scale = store.res_scale(&policy);
        let mut resolution = ResolutionState::new(
            policy,
            size.native_w,
            size.native_h,
            size.dpr,
            DeviceClass {
                coarse: device.coarse,
                software: false,
            },
            stored_scale,
        );
        // The software GPU is known already: taken like a late report, so it
        // logs the same line.
        if let Some(gpu) = options.gpu {
            if resolution.gpu_info(&gpu) {
                log(&format!(
                    "software GPU detected — starting at {}% resolution",
                    resolution.percent()
                ));
            }
        }
        let stored_aa = store.aa_id();
        let url_aa = store.url_aa();
        let (aa, source) = config.initial_aa(device, stored_aa.as_deref(), url_aa.as_deref());
        if let (Some(id), false) = (&url_aa, source == AaSource::Url) {
            log(&format!("?aa={id} is not one of the modes; ignored"));
        }
        let stored_aa_valid = stored_aa
            .as_deref()
            .and_then(|id| config.parse(id))
            .is_some();
        let show_notice = show_quality_notice(device, stored_scale.is_some(), stored_aa_valid);
        let dom = build(container, options, &ui, &policy);
        Self {
            inner: Rc::new(RefCell::new(Inner {
                store,
                config,
                resolution,
                aa,
                show_notice,
                log,
                dom,
                on_resize: None,
                on_anti_alias: None,
            })),
        }
    }

    /// The canvas's backing-store size in device pixels.
    pub fn backing(&self) -> (u32, u32) {
        self.inner.borrow().resolution.backing()
    }

    /// The chosen mode.
    pub fn aa(&self) -> AaMode {
        self.inner.borrow().aa
    }

    /// The resolution scale in effect.
    pub fn scale(&self) -> f64 {
        self.inner.borrow().resolution.scale()
    }

    /// Starts following the canvas size and the controls.
    pub fn start(&self, canvas: &HtmlCanvasElement, hooks: SettingsHooks) {
        {
            let mut inner = self.inner.borrow_mut();
            inner.on_resize = Some(hooks.on_resize);
            inner.on_anti_alias = Some(hooks.on_anti_alias);
            for row in &hooks.extra_rows {
                match &inner.dom.hint {
                    Some(hint) => {
                        let _ = inner.dom.card.insert_before(row, Some(hint));
                    }
                    None => append(&inner.dom.card, row),
                }
            }
            inner.show_scale();
            inner.dom.aa_select.set_value(inner.aa.id());
            inner.show_mode();
        }
        if let Some(button) = &hooks.open_button {
            let panel = self.clone();
            on(button, "click", move |_| panel.open());
        }

        let panel = self.clone();
        observe_canvas(canvas, move |size| {
            {
                let mut inner = panel.inner.borrow_mut();
                inner
                    .resolution
                    .resized(size.native_w, size.native_h, size.dpr);
                inner.show_scale();
            }
            panel.emit_resize();
        });

        let (slider, select) = {
            let inner = self.inner.borrow();
            (inner.dom.res_slider.clone(), inner.dom.aa_select.clone())
        };
        let panel = self.clone();
        on(&slider.clone(), "input", move |_| {
            {
                let mut inner = panel.inner.borrow_mut();
                let percent = slider.value().parse::<f64>().unwrap_or(f64::NAN);
                let scale = inner.resolution.set_percent(percent);
                inner.store.set_res_scale(scale);
                inner.show_scale();
            }
            panel.emit_resize();
        });
        let panel = self.clone();
        on(&select.clone(), "change", move |_| {
            let mode = {
                let mut inner = panel.inner.borrow_mut();
                let Some(mode) = inner.config.parse(&select.value()) else {
                    return;
                };
                if mode == inner.aa {
                    return;
                }
                inner.aa = mode;
                inner.store.set_aa(mode);
                inner.show_mode();
                mode
            };
            let hook = panel.inner.borrow_mut().on_anti_alias.take();
            if let Some(mut hook) = hook {
                hook(mode);
                panel.inner.borrow_mut().on_anti_alias = Some(hook);
            }
        });
    }

    pub fn open(&self) {
        self.inner.borrow().dom.settings.open();
    }

    pub fn close(&self) {
        self.inner.borrow().dom.settings.close();
    }

    /// The first frames are on screen (call it again after every scene
    /// reload; the notice shows once).
    pub fn ready(&self) {
        let mut inner = self.inner.borrow_mut();
        if std::mem::take(&mut inner.show_notice) {
            inner.dom.notice.open();
        }
    }

    /// A worker message: the kit's settings messages (`gpu-info`,
    /// `aa-compile-*`).
    pub fn handle(&self, message: &SettingsToPage) {
        match message {
            SettingsToPage::SettingsOptions(_) => {}
            SettingsToPage::GpuInfo(info) => self.gpu_info(info),
            SettingsToPage::AaCompileStart {
                label, compiles, ..
            } => {
                let inner = self.inner.borrow();
                inner.dom.compiling_line.set_text_content(Some(label));
                if *compiles {
                    inner.dom.compiling.open();
                }
            }
            SettingsToPage::AaCompileProgress { message } => {
                let inner = self.inner.borrow();
                inner.dom.compiling_line.set_text_content(Some(message));
            }
            SettingsToPage::AaCompileDone {
                mode, taa_fallback, ..
            } => {
                let mut inner = self.inner.borrow_mut();
                inner.dom.compiling.close();
                if let Some(reason) = taa_fallback {
                    inner.taa_unavailable(reason);
                }
                // What runs now (MSAA after a TAA fallback). Not stored.
                if *mode != inner.aa {
                    inner.aa = *mode;
                    inner.dom.aa_select.set_value(mode.id());
                    inner.show_mode();
                }
            }
        }
    }

    /// A raw worker message: handles it and returns `true` if it is one of
    /// the kit's settings messages.
    pub fn handle_js(&self, value: &JsValue) -> bool {
        match SettingsToPage::from_js(value) {
            Some(Ok(message)) => {
                self.handle(&message);
                true
            }
            Some(Err(err)) => {
                web_sys::console::error_1(&format!("settings message: {err}").into());
                true
            }
            None => false,
        }
    }

    fn gpu_info(&self, info: &GpuInfo) {
        let lowered = {
            let mut inner = self.inner.borrow_mut();
            let lowered = inner.resolution.gpu_info(info);
            if lowered {
                inner.show_scale();
            }
            lowered.then(|| (inner.log.clone(), inner.resolution.percent()))
        };
        if let Some((log, percent)) = lowered {
            log(&format!(
                "software GPU detected — starting at {percent}% resolution"
            ));
        }
        // Again with the real texture limit (and any lowered scale).
        self.emit_resize();
    }

    /// Calls `on_resize` with the current backing size, outside the borrow
    /// (the hook may call back into the panel).
    fn emit_resize(&self) {
        let (hook, (width, height)) = {
            let mut inner = self.inner.borrow_mut();
            (inner.on_resize.take(), inner.resolution.backing())
        };
        if let Some(mut hook) = hook {
            hook(width, height);
            self.inner.borrow_mut().on_resize = Some(hook);
        }
    }
}

impl Inner {
    fn show_scale(&self) {
        let percent = self.resolution.percent();
        self.dom.res_slider.set_value(&percent.to_string());
        self.dom
            .res_label
            .set_text_content(Some(&format!("Resolution — {percent}%")));
    }

    fn show_mode(&self) {
        self.dom.aa_select.set_title(self.aa.description());
    }

    fn taa_unavailable(&self, reason: &str) {
        let options = self.dom.aa_select.options();
        for index in 0..options.length() {
            let Some(option) = options
                .item(index)
                .and_then(|o| o.dyn_into::<web_sys::HtmlOptionElement>().ok())
            else {
                continue;
            };
            if option.value().starts_with("taa") {
                option.set_disabled(true);
                option.set_title(&format!("TAA is not available on this device: {reason}"));
            }
        }
    }
}

fn close_button(class: &str) -> HtmlElement {
    let button = el_text("button", class, "×");
    attr(&button, "type", "button");
    attr(&button, "aria-label", "Close");
    attr(&button, "data-close", "");
    button
}

fn build(
    container: &Element,
    options: &SettingsOptions,
    ui: &SettingsUi,
    policy: &ResolutionPolicy,
) -> Dom {
    // Settings: resolution, anti-aliasing, the app's rows, a hint.
    let settings = el("div", "settings-overlay");
    settings.set_id("settings");
    let card = el("div", "settings-modal");
    attr(&card, "role", "dialog");
    attr(&card, "aria-modal", "true");
    attr(&card, "aria-label", &ui.title);
    append(&card, &close_button(&ui.close_class));
    append(&card, &el_text("h2", "", &ui.title));

    let res_row = el("div", "settings-row");
    let res_label = el_text("label", "settings-label", "Resolution — 100%");
    res_label.set_id("res-label");
    attr(&res_label, "for", "res-slider");
    let res_slider: HtmlInputElement = el("input", "res-slider").unchecked_into();
    res_slider.set_id("res-slider");
    res_slider.set_type("range");
    res_slider.set_min(&ResolutionPolicy::percent(policy.min_scale).to_string());
    res_slider.set_max(&ResolutionPolicy::percent(policy.max_scale).to_string());
    res_slider.set_step(&((policy.step * 100.0).round().max(1.0) as u32).to_string());
    attr(&res_slider, "aria-label", "render resolution");
    append(&res_row, &res_label);
    append(&res_row, &res_slider);
    append(&card, &res_row);

    let aa_row = el("div", "settings-row");
    let aa_label = el_text("label", "settings-label", "Anti-aliasing");
    attr(&aa_label, "for", "aa-select");
    let aa_select: HtmlSelectElement = el("select", "settings-select").unchecked_into();
    aa_select.set_id("aa-select");
    attr(&aa_select, "aria-label", "anti-aliasing");
    for mode in &options.modes {
        let option = el_text("option", "", &mode.label);
        attr(&option, "value", mode.id.id());
        option.set_title(&mode.description);
        append(&aa_select, &option);
    }
    append(&aa_row, &aa_label);
    append(&aa_row, &aa_select);
    aa_row.set_hidden(!ui.show_aa);
    append(&card, &aa_row);

    let hint = ui.hint.as_ref().map(|hint| {
        let element = el_text("p", "settings-hint", hint);
        append(&card, &element);
        element
    });
    append(&settings, &card);

    // The reduced-quality notice (touch devices).
    let notice = el("div", "settings-overlay");
    notice.set_id("quality-notice");
    // Above the About modal, which may already be open when this shows.
    let _ = notice.style().set_property("z-index", "25");
    let notice_card = el("div", "settings-modal");
    append(&notice_card, &close_button(&ui.close_class));
    append(&notice_card, &el_text("h2", "", &ui.notice_title));
    append(&notice_card, &el_text("p", "notice-text", &ui.notice_text));
    let ok = el_text("button", "notice-ok", "OK");
    attr(&ok, "type", "button");
    attr(&ok, "data-close", "");
    append(&notice_card, &ok);
    append(&notice, &notice_card);

    // While the renderer recompiles: not dismissable.
    let compiling = el("div", "settings-overlay compile-overlay");
    compiling.set_id("aa-compile");
    let _ = compiling.style().set_property("z-index", "30");
    let compiling_card = el("div", "settings-modal");
    let compiling_line = el("p", "settings-hint");
    compiling_line.set_id("aa-compile-line");
    append(&compiling_card, &el_text("h2", "", &ui.compile_title));
    append(
        &compiling_card,
        &el_text("p", "notice-text", &ui.compile_text),
    );
    append(&compiling_card, &compiling_line);
    append(&compiling, &compiling_card);

    append(container, &settings);
    append(container, &notice);
    append(container, &compiling);
    let dismiss = ModalOptions {
        dismissable: true,
        escape_stops: ui.escape_stops,
    };
    Dom {
        settings: Modal::new(settings, dismiss),
        notice: Modal::new(notice, dismiss),
        compiling: Modal::new(compiling, ModalOptions::default()),
        compiling_line,
        res_label,
        res_slider,
        aa_select,
        hint,
        card,
    }
}
