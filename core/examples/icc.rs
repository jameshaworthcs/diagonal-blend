//! Write one of the core's ICC profiles to stdout: `cargo run --example icc -- p3`.

use std::io::Write;

use diagonal_blend_core::color::Space;
use diagonal_blend_core::icc::profile;

fn main() {
    let space = match std::env::args().nth(1).as_deref() {
        Some("p3") => Space::DisplayP3,
        _ => Space::Srgb,
    };
    std::io::stdout().write_all(&profile(space)).unwrap();
}
