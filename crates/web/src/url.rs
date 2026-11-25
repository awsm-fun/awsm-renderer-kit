//! The page's URL query (`?reset`, `?aa=…`). Main thread only: in a worker
//! these see no flags.

fn params() -> Option<web_sys::UrlSearchParams> {
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search).ok()
}

/// Whether the query has `name`, with or without a value (`?reset`).
pub fn query_has(name: &str) -> bool {
    params().is_some_and(|params| params.has(name))
}

/// The value of `name` in the query (`?aa=taa` → `Some("taa")`).
pub fn query_get(name: &str) -> Option<String> {
    params()?.get(name)
}
