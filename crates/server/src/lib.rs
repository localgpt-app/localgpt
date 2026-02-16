mod http;
pub mod telegram;
mod websocket;

#[cfg(feature = "egui-web")]
pub mod web;

pub use http::Server;

// WASM entry point for egui web UI
#[cfg(all(target_arch = "wasm32", feature = "egui-web"))]
use wasm_bindgen::prelude::*;

#[cfg(all(target_arch = "wasm32", feature = "egui-web"))]
#[wasm_bindgen]
pub async fn start_web_ui(canvas_id: &str) -> Result<(), wasm_bindgen::JsValue> {
    // Redirect `log` message to `console.log` and friends:
    console_error_panic_hook::set_once();
    
    let web_options = eframe::WebOptions::default();

    let document = web_sys::window()
        .ok_or("No window")?
        .document()
        .ok_or("No document")?;

    let canvas = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| format!("Failed to find canvas with id: {}", canvas_id))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| format!("{} was not a HtmlCanvasElement", canvas_id))?;

    eframe::WebRunner::new()
        .start(
            canvas,
            web_options,
            Box::new(|cc| Ok(Box::new(web::WebApp::new(cc)))),
        )
        .await
}
