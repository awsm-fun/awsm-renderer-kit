//! `postMessage` in both directions, with serde.
//!
//! The kit's messages are plain objects with a `type` field
//! (`{type: 'aa-compile-start', …}`), so the C family's JS page reads them
//! as they are. They are serialized with serde-wasm-bindgen's JSON-compatible
//! serializer: maps become objects and `None` becomes `null`, never a `Map`
//! or `undefined`.

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::{JsCast, JsValue};

/// `value` as a plain JS object.
pub fn to_js<T: Serialize + ?Sized>(value: &T) -> Result<JsValue, String> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|err| err.to_string())
}

/// A JS value back into `T`.
pub fn from_js<T: DeserializeOwned>(value: JsValue) -> Result<T, String> {
    serde_wasm_bindgen::from_value(value).map_err(|err| err.to_string())
}

/// The `type` field of a message, if it has a string one.
pub fn message_type(value: &JsValue) -> Option<String> {
    js_sys::Reflect::get(value, &JsValue::from_str("type"))
        .ok()?
        .as_string()
}

/// Posts `message` from a dedicated worker to the page that started it.
/// Errors (not in a worker, not serializable) are logged to the console.
pub fn post_to_page<T: Serialize + ?Sized>(message: &T) {
    let result = to_js(message).and_then(|value| {
        js_sys::global()
            .dyn_into::<web_sys::DedicatedWorkerGlobalScope>()
            .map_err(|_| "post_to_page: not in a dedicated worker".to_string())?
            .post_message(&value)
            .map_err(|err| format!("post_to_page: {err:?}"))
    });
    if let Err(message) = result {
        web_sys::console::error_1(&JsValue::from_str(&message));
    }
}

/// Posts `message` from the page to `worker`. Errors are logged.
pub fn post_to_worker<T: Serialize + ?Sized>(worker: &web_sys::Worker, message: &T) {
    let result = to_js(message).and_then(|value| {
        worker
            .post_message(&value)
            .map_err(|err| format!("post_to_worker: {err:?}"))
    });
    if let Err(message) = result {
        web_sys::console::error_1(&JsValue::from_str(&message));
    }
}
