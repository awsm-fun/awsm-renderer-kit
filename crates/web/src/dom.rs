//! Small DOM helpers for the kit's web-sys panels (no framework).
//!
//! Event closures are leaked with `Closure::forget`: the panels are built
//! once and live as long as the page.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, Event, HtmlElement};

/// The page's document. Panics in a worker: the DOM panels are main-thread
/// only, and building one in a worker is a programming error.
pub fn document() -> Document {
    web_sys::window()
        .and_then(|window| window.document())
        .expect("the kit's DOM panels need a window and a document (main thread only)")
}

/// A new `<tag class="class">`. An empty `class` sets none.
pub fn el(tag: &str, class: &str) -> HtmlElement {
    let element: HtmlElement = document()
        .create_element(tag)
        .expect("create_element with a literal tag name")
        .unchecked_into();
    if !class.is_empty() {
        element.set_class_name(class);
    }
    element
}

/// [`el`] with text content.
pub fn el_text(tag: &str, class: &str, text: &str) -> HtmlElement {
    let element = el(tag, class);
    element.set_text_content(Some(text));
    element
}

/// Sets an attribute (ignoring the error a valid literal name cannot give).
pub fn attr(element: &Element, name: &str, value: &str) {
    let _ = element.set_attribute(name, value);
}

/// Appends `child` to `parent`.
pub fn append(parent: &Element, child: &Element) {
    let _ = parent.append_child(child);
}

/// Adds an event listener that lives as long as the page.
pub fn on(target: &web_sys::EventTarget, event: &str, handler: impl FnMut(Event) + 'static) {
    let closure = Closure::<dyn FnMut(Event)>::new(handler);
    let _ = target.add_event_listener_with_callback(event, closure.as_ref().unchecked_ref());
    closure.forget();
}

/// The same, as a capturing or bubbling listener with options.
pub fn on_with_capture(
    target: &web_sys::EventTarget,
    event: &str,
    capture: bool,
    handler: impl FnMut(Event) + 'static,
) {
    let closure = Closure::<dyn FnMut(Event)>::new(handler);
    let _ = target.add_event_listener_with_callback_and_bool(
        event,
        closure.as_ref().unchecked_ref(),
        capture,
    );
    closure.forget();
}

/// Shows or hides `element`: its `hidden` attribute, and an inline
/// `display: none` while hidden. The attribute alone loses to any author
/// rule that sets `display` (the Rust templates' `.settings-overlay {
/// display: flex }`, `.settings-row { display: flex }`), so the inline
/// style is what really hides it; showing removes it again, so the
/// stylesheet's own `display` applies. Every kit element is shown and
/// hidden through this (and `js/kit-web.js`'s `setShown`).
pub fn set_shown(element: &HtmlElement, shown: bool) {
    element.set_hidden(!shown);
    let style = element.style();
    if shown {
        let _ = style.remove_property("display");
    } else {
        let _ = style.set_property("display", "none");
    }
}

/// Whether `element` is shown (by [`set_shown`]).
pub fn is_shown(element: &HtmlElement) -> bool {
    !element.hidden()
}

/// The behaviour every kit overlay shares (the twin of the C family's
/// `modal.js` and `js/kit-web.js`'s `modal()`): `open()` / `close()` by
/// [`set_shown`]. A dismissable overlay also closes on any of its
/// `[data-close]` buttons, on a click on the backdrop itself (not on the card
/// in it) and on Escape. Keys pressed inside the overlay stay inside it
/// (except Escape), so a page that sends keys to a game from a window
/// listener does not see the arrows that move a slider or a select.
#[derive(Clone)]
pub struct Modal {
    pub overlay: HtmlElement,
}

/// How an overlay closes.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModalOptions {
    /// Closes on `[data-close]`, the backdrop and Escape.
    pub dismissable: bool,
    /// Escape that closes this overlay stops there
    /// (`stopImmediatePropagation`), so a game that also listens for Escape
    /// (pause) does not see it.
    pub escape_stops: bool,
}

impl Modal {
    /// Wires up `overlay` (hidden until opened).
    pub fn new(overlay: HtmlElement, options: ModalOptions) -> Self {
        set_shown(&overlay, false);
        on(&overlay, "keydown", |event| {
            let escape = event
                .dyn_ref::<web_sys::KeyboardEvent>()
                .is_some_and(|key| key.key() == "Escape");
            if !escape {
                event.stop_propagation();
            }
        });
        if options.dismissable {
            if let Ok(buttons) = overlay.query_selector_all("[data-close]") {
                for index in 0..buttons.length() {
                    if let Some(button) = buttons.item(index) {
                        let overlay = overlay.clone();
                        on(&button, "click", move |_| set_shown(&overlay, false));
                    }
                }
            }
            let backdrop = overlay.clone();
            on(&overlay, "click", move |event| {
                let target: Option<JsValue> = event.target().map(Into::into);
                if target.as_ref() == Some(backdrop.as_ref()) {
                    set_shown(&backdrop, false);
                }
            });
            if let Some(window) = web_sys::window() {
                let overlay = overlay.clone();
                on(&window, "keydown", move |event| {
                    let escape = event
                        .dyn_ref::<web_sys::KeyboardEvent>()
                        .is_some_and(|key| key.key() == "Escape");
                    if escape && is_shown(&overlay) {
                        set_shown(&overlay, false);
                        if options.escape_stops {
                            event.stop_immediate_propagation();
                        }
                    }
                });
            }
        }
        Self { overlay }
    }

    pub fn open(&self) {
        set_shown(&self.overlay, true);
    }

    pub fn close(&self) {
        set_shown(&self.overlay, false);
    }

    pub fn is_open(&self) -> bool {
        is_shown(&self.overlay)
    }
}
