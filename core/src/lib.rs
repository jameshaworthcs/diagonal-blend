//! diagonal-blend core: geometry, colour, file parsing and the wgpu renderer.

pub mod align;
pub mod avif;
pub mod color;
pub mod encode;
pub mod exif;
pub mod geometry;
pub mod icc;
pub mod scene;
pub mod timeline;

#[cfg(target_arch = "wasm32")]
mod render;
#[cfg(target_arch = "wasm32")]
mod wasm;
