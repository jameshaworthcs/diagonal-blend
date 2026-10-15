//! The JavaScript-facing API.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::color::Space;
use crate::exif::{Orientation, read_header};
use crate::geometry::v2;
use crate::render;
use crate::scene::Scene;

#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}

fn bg3(bg: &[f64]) -> [f64; 3] {
    [0, 1, 2].map(|i| bg.get(i).copied().unwrap_or(0.0))
}

fn time(t: f64) -> Option<f64> {
    (t >= 0.0).then_some(t)
}

#[wasm_bindgen]
pub struct Renderer(Rc<RefCell<render::Renderer>>);

#[wasm_bindgen]
impl Renderer {
    /// Open a renderer on `canvas`: WebGPU if the browser has a working
    /// adapter (unless `force_webgl`), WebGL2 otherwise. `want_p3` asks for a
    /// Display P3 swapchain; `color_space()` says whether it was granted.
    pub async fn create(canvas: HtmlCanvasElement, force_webgl: bool, want_p3: bool) -> Result<Renderer, JsValue> {
        let r = render::Renderer::new(canvas, force_webgl, want_p3).await?;
        Ok(Renderer(Rc::new(RefCell::new(r))))
    }

    pub fn backend(&self) -> String {
        self.0.borrow().backend().into()
    }

    /// "display-p3" or "srgb".
    pub fn color_space(&self) -> String {
        match self.0.borrow().preview_space() {
            Space::DisplayP3 => "display-p3".into(),
            Space::Srgb => "srgb".into(),
        }
    }

    pub fn max_texture_size(&self) -> u32 {
        self.0.borrow().max_texture_size()
    }

    pub fn resize(&self, width: u32, height: u32) {
        self.0.borrow_mut().resize(width, height);
    }

    pub fn set_texture(&self, which: u32, rgba: &[u8], width: u32, height: u32) -> Result<(), JsValue> {
        self.0.borrow_mut().set_texture(which as usize, rgba, width, height)
    }

    pub fn clear_texture(&self, which: u32) {
        self.0.borrow_mut().clear_texture(which as usize);
    }

    pub fn swap(&self) {
        self.0.borrow_mut().swap();
    }

    /// Draw the preview. `bg` is the sRGB background; `t` is a time into the
    /// reveal in seconds, or negative for the scene's own split.
    pub fn render(&self, scene: &Scene, bg: &[f64], t: f64) {
        self.0.borrow_mut().render(scene, bg3(bg), time(t));
    }

    /// Render offscreen at full size and resolve to RGBA8 pixels in
    /// `space` (0 sRGB, 1 Display P3).
    pub fn render_pixels(&self, scene: &Scene, width: u32, height: u32, space: u32, bg: &[f64], t: f64) -> js_sys::Promise {
        let this = self.0.clone();
        let scene = scene.clone();
        let bg = bg3(bg);
        wasm_bindgen_futures::future_to_promise(async move {
            let px = render::Renderer::render_pixels(this, scene, width, height, Space::from_code(space), bg, time(t)).await?;
            Ok(js_sys::Uint8Array::from(&px[..]).into())
        })
    }
}

/// `[orientation (0 if absent), stored width, stored height]`.
#[wasm_bindgen]
pub fn read_image_header(bytes: &[u8]) -> Vec<f64> {
    let h = read_header(bytes);
    vec![h.orientation.unwrap_or(0) as f64, h.width as f64, h.height as f64]
}

/// The EXIF orientation still owed after decoding: see `Orientation::residual`.
#[wasm_bindgen]
pub fn residual_orientation(tag: u16, stored_w: f64, stored_h: f64, decoded_w: f64, decoded_h: f64, auto: bool) -> u8 {
    Orientation::from_tag(tag).residual(v2(stored_w, stored_h), v2(decoded_w, decoded_h), auto).0
}

/// 0 sRGB or untagged, 1 Display P3, -1 another profile.
#[wasm_bindgen]
pub fn detect_color_space(bytes: &[u8]) -> i32 {
    crate::icc::detect(bytes).code()
}

#[wasm_bindgen]
pub fn encode_png(rgba: &[u8], width: u32, height: u32, space: u32) -> Result<Vec<u8>, JsValue> {
    crate::encode::png(rgba, width, height, Space::from_code(space)).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub fn tag_webp(webp: &[u8], space: u32) -> Result<Vec<u8>, JsValue> {
    crate::encode::webp_with_icc(webp, Space::from_code(space)).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub fn icc_profile(space: u32) -> Vec<u8> {
    crate::icc::profile(Space::from_code(space))
}

#[wasm_bindgen]
pub fn ease_in_out(t: f64) -> f64 {
    crate::timeline::ease_in_out(t)
}
