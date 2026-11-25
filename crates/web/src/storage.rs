//! `localStorage`, main thread only. Every call is wrapped so a missing or
//! throwing storage behaves like an empty one: nothing is remembered and the
//! defaults apply on every load.

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// The stored string, or `None` (no key, no storage).
pub fn get(key: &str) -> Option<String> {
    local_storage()?.get_item(key).ok().flatten()
}

/// Stores `value` under `key` (silently does nothing without storage).
pub fn set(key: &str, value: &str) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(key, value);
    }
}

/// Removes `key`.
pub fn remove(key: &str) {
    if let Some(storage) = local_storage() {
        let _ = storage.remove_item(key);
    }
}

/// Every key currently stored (empty without storage).
pub fn keys() -> Vec<String> {
    let Some(storage) = local_storage() else {
        return Vec::new();
    };
    let count = storage.length().unwrap_or(0);
    (0..count)
        .filter_map(|index| storage.key(index).ok().flatten())
        .collect()
}

/// A stored `"1"` / `"0"`, or `None` if there is none (or it is something
/// else).
pub fn get_bool(key: &str) -> Option<bool> {
    match get(key)?.as_str() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// Stores a boolean as `"1"` / `"0"`.
pub fn set_bool(key: &str, value: bool) {
    set(key, if value { "1" } else { "0" });
}
