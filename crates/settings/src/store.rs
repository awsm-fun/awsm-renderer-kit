//! The page side: stored choices, URL flags, the canvas's size.

use awsm_renderer_kit_web::{storage, url};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, ResizeObserver, ResizeObserverEntry};

use crate::{migrate_legacy_aa, AaMode, ResolutionPolicy, SettingsConfig};

/// The kit's keys in `localStorage`, all under the app's prefix:
///
/// | Key | Value |
/// |---|---|
/// | `<prefix>res_scale` | the resolution scale, two decimals (`"0.80"`) |
/// | `<prefix>aa` | the mode id (`"taa"`) |
/// | `<prefix>stats` | the stats panel shown (`"1"` / `"0"`) |
/// | `<prefix>stats_open_<group>` | a stats group expanded |
///
/// localStorage is shared by everything on one origin, so each app needs a
/// prefix of its own. Values are only written when the user makes a choice;
/// a default is never stored.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    prefix: String,
    reset_keys: Vec<String>,
    legacy_aa_keys: Option<(String, String)>,
}

impl SettingsStore {
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            reset_keys: Vec::new(),
            legacy_aa_keys: None,
        }
    }

    /// Full keys of the app's own settings that `?reset` also clears (a
    /// game's options). Its other keys (a best score, progress) survive.
    pub fn with_reset_keys(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.reset_keys.extend(keys.into_iter().map(Into::into));
        self
    }

    /// The pre-0.42 Rust v1 keys (`"awsm_msaa"`, `"awsm_smaa"`), migrated
    /// by [`Self::migrate_legacy_aa`] and cleared by `?reset`.
    pub fn with_legacy_aa_keys(mut self, msaa: impl Into<String>, smaa: impl Into<String>) -> Self {
        self.legacy_aa_keys = Some((msaa.into(), smaa.into()));
        self
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// `<prefix><name>`.
    pub fn key(&self, name: &str) -> String {
        format!("{}{name}", self.prefix)
    }

    /// The stored resolution scale, clamped to the policy's range.
    pub fn res_scale(&self, policy: &ResolutionPolicy) -> Option<f64> {
        policy.parse_stored(&storage::get(&self.key("res_scale"))?)
    }

    pub fn set_res_scale(&self, scale: f64) {
        storage::set(
            &self.key("res_scale"),
            &ResolutionPolicy::format_stored(scale),
        );
    }

    /// The stored mode id, as stored (check it with
    /// [`SettingsConfig::parse`]).
    pub fn aa_id(&self) -> Option<String> {
        storage::get(&self.key("aa"))
    }

    /// The stored mode, if it is one `config` offers.
    pub fn aa(&self, config: &SettingsConfig) -> Option<AaMode> {
        config.parse(&self.aa_id()?)
    }

    pub fn set_aa(&self, mode: AaMode) {
        storage::set(&self.key("aa"), mode.id());
    }

    /// A stored `"1"` / `"0"` under `<prefix><name>`.
    pub fn get_bool(&self, name: &str) -> Option<bool> {
        storage::get_bool(&self.key(name))
    }

    pub fn set_bool(&self, name: &str, value: bool) {
        storage::set_bool(&self.key(name), value);
    }

    /// Carries a pre-0.42 choice over to `<prefix>aa` when that is unset
    /// ([`migrate_legacy_aa`]), then removes the legacy keys either way.
    /// Call it once at start-up, after [`Self::reset_if_asked`].
    pub fn migrate_legacy_aa(&self) {
        let Some((msaa_key, smaa_key)) = &self.legacy_aa_keys else {
            return;
        };
        let (msaa, smaa) = (storage::get(msaa_key), storage::get(smaa_key));
        if self.aa_id().is_none() {
            if let Some(mode) = migrate_legacy_aa(msaa.as_deref(), smaa.as_deref()) {
                self.set_aa(mode);
            }
        }
        storage::remove(msaa_key);
        storage::remove(smaa_key);
    }

    /// The keys `?reset` clears that are present now: the kit's own
    /// (`res_scale`, `aa`, `stats`, every `stats_open_*`), the legacy pair
    /// and the app's registered keys. Nothing else under the prefix.
    pub fn reset_keys(&self) -> Vec<String> {
        let mut keys = vec![self.key("res_scale"), self.key("aa"), self.key("stats")];
        let open = self.key("stats_open_");
        keys.extend(
            storage::keys()
                .into_iter()
                .filter(|key| key.starts_with(&open)),
        );
        if let Some((msaa, smaa)) = &self.legacy_aa_keys {
            keys.push(msaa.clone());
            keys.push(smaa.clone());
        }
        keys.extend(self.reset_keys.iter().cloned());
        keys
    }

    /// `?reset` in the URL: forgets the stored settings (see
    /// [`Self::reset_keys`]) so this load starts from the defaults. Returns
    /// whether it did. It must run before anything reads a stored setting.
    pub fn reset_if_asked(&self) -> bool {
        if !url::query_has("reset") {
            return false;
        }
        for key in self.reset_keys() {
            storage::remove(&key);
        }
        true
    }

    /// `?aa=<id>`: the mode for this session, not stored. `None` without the
    /// flag; an id that is not offered is ignored by
    /// [`SettingsConfig::initial_aa`].
    pub fn url_aa(&self) -> Option<String> {
        url::query_get("aa")
    }
}

/// A canvas's size: device pixels at full resolution, and the ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasSize {
    pub native_w: f64,
    pub native_h: f64,
    /// `devicePixelRatio`, at least 1.
    pub dpr: f64,
}

impl CanvasSize {
    /// The canvas's current layout size (before any resize observation).
    pub fn of(canvas: &HtmlCanvasElement) -> Self {
        let dpr = awsm_renderer_kit_web::device_pixel_ratio();
        let rect = canvas.get_bounding_client_rect();
        Self {
            native_w: rect.width().max(1.0) * dpr,
            native_h: rect.height().max(1.0) * dpr,
            dpr,
        }
    }
}

/// Calls `on_size` whenever the canvas's layout size or the pixel ratio
/// changes (the window resized, or moved to another display). Where the
/// browser reports the exact size in device pixels, that is used: CSS size ×
/// ratio can be a pixel off at a fractional ratio. But only if the two agree
/// to within rounding; if they do not, the box is not in device pixels
/// (Chrome's DevTools device emulation reports CSS pixels there), and the
/// product is the one to believe. The observer lives as long as the page.
pub fn observe_canvas(canvas: &HtmlCanvasElement, mut on_size: impl FnMut(CanvasSize) + 'static) {
    let callback = Closure::<dyn FnMut(js_sys::Array)>::new(move |entries: js_sys::Array| {
        let Some(entry) = entries.get(0).dyn_into::<ResizeObserverEntry>().ok() else {
            return;
        };
        let dpr = awsm_renderer_kit_web::device_pixel_ratio();
        let rect = entry.content_rect();
        let mut size = CanvasSize {
            native_w: rect.width().max(1.0) * dpr,
            native_h: rect.height().max(1.0) * dpr,
            dpr,
        };
        let boxes = entry.device_pixel_content_box_size();
        if let Some(device) = boxes.get(0).dyn_into::<web_sys::ResizeObserverSize>().ok() {
            let (w, h) = (device.inline_size(), device.block_size());
            if (w - size.native_w).abs() < 2.0 && (h - size.native_h).abs() < 2.0 {
                size.native_w = w.max(1.0);
                size.native_h = h.max(1.0);
            }
        }
        on_size(size);
    });
    let Ok(observer) = ResizeObserver::new(callback.as_ref().unchecked_ref()) else {
        return;
    };
    callback.forget();
    let options = web_sys::ResizeObserverOptions::new();
    options.set_box(web_sys::ResizeObserverBoxOptions::DevicePixelContentBox);
    // Browsers without device-pixel boxes throw on that option, so the call
    // goes through `Function::call2`, which catches.
    let observe = js_sys::Reflect::get(&observer, &"observe".into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
    let observed = observe.is_some_and(|f| f.call2(&observer, canvas, &options).is_ok());
    if !observed {
        observer.observe(canvas);
    }
}
