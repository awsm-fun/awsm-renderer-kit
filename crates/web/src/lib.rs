//! Browser helpers shared by `awsm-renderer-settings` and
//! `awsm-renderer-stats`. Internal to the kit: an app uses those two crates,
//! which re-export what it needs.
//!
//! Everything here tolerates the browser saying no: `localStorage` can be
//! missing or throw (privacy modes, storage disabled), a worker has no
//! `window`, a message may not deserialize. Those cases come back as `None`,
//! `false` or an `Err(String)`, never as a panic.

pub mod dom;
pub mod message;
pub mod storage;
pub mod url;

use wasm_bindgen::JsCast;

/// `matchMedia("(pointer: coarse)")`: a touch device (phones, tablets), the
/// device class that starts with lower graphics defaults. `false` in a worker
/// or where the query is unsupported.
pub fn coarse_pointer() -> bool {
    web_sys::window()
        .and_then(|window| window.match_media("(pointer: coarse)").ok().flatten())
        .is_some_and(|query| query.matches())
}

/// `devicePixelRatio`, at least 1 (1 in a worker).
pub fn device_pixel_ratio() -> f64 {
    web_sys::window()
        .map(|window| window.device_pixel_ratio())
        .filter(|dpr| dpr.is_finite())
        .unwrap_or(1.0)
        .max(1.0)
}

/// `performance.now()` on the main thread or in a worker (0 if neither has a
/// `performance` object).
pub fn now_ms() -> f64 {
    if let Some(window) = web_sys::window() {
        if let Some(performance) = window.performance() {
            return performance.now();
        }
    }
    js_sys::global()
        .dyn_into::<web_sys::WorkerGlobalScope>()
        .ok()
        .and_then(|scope| scope.performance())
        .map_or(0.0, |performance| performance.now())
}
