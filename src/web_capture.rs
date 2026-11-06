//! Opt-in GPU readback for browser validation, independent of desktop capture.
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(
    inline_js = "export function take_capture_request() { const requested = window.__parkourCaptureRequested === true; window.__parkourCaptureRequested = false; return requested; } export function save_capture(bytes) { window.__parkourCapture = {state:'uploading', bytes:bytes.length}; fetch('./__capture', {method:'POST',body:bytes}).then(r => { if (!r.ok) throw new Error('capture HTTP '+r.status); window.__parkourCapture.state='saved'; }).catch(e => {window.__parkourCapture.state='failed';window.__parkourCapture.error=String(e);}); } export function capture_error(error) { window.__parkourCapture = {state:'failed',error}; }"
)]
extern "C" {
    fn take_capture_request() -> bool;
    fn save_capture(bytes: &[u8]);
    fn capture_error(error: &str);
}

pub struct WebCapturePlugin;
impl Plugin for WebCapturePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, capture);
    }
}

fn capture(mut commands: Commands) {
    if !take_capture_request() {
        return;
    }
    commands
        .spawn(Screenshot::primary_window())
        .observe(|event: On<ScreenshotCaptured>| {
            let result = event
                .image
                .clone()
                .try_into_dynamic()
                .map_err(|e| format!("{e:?}"))
                .and_then(|image| {
                    let mut png = std::io::Cursor::new(Vec::new());
                    image
                        .write_to(&mut png, image::ImageFormat::Png)
                        .map_err(|e| e.to_string())?;
                    Ok(png.into_inner())
                });
            match result {
                Ok(png) => save_capture(&png),
                Err(error) => capture_error(&error),
            }
        });
}
