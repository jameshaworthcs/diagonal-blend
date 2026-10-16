//! The composition: split, edge, the two images and where they sit. The
//! TypeScript side holds a `Scene`, forwards pointer and keyboard input to it
//! in CSS pixels, and hands it to the renderer.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

use crate::align::{Similarity, midline};
use crate::color::{Space, convert};
use crate::exif::Orientation;
use crate::geometry::{Affine, Frame, Placement, Split, Vec2, v2};
use crate::timeline::Reveal;

#[derive(Clone, Copy, Debug)]
pub struct Image {
    /// Size as displayed (after orientation), in pixels.
    pub size: Vec2,
    /// Orientation the shader still has to apply to the texture.
    pub residual: Orientation,
    /// Primaries of the texture's pixel values.
    pub space: Space,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Clone, Debug)]
pub struct Scene {
    frame: Frame,
    split: Split,
    /// Feather width as a fraction of the frame's short side (0 to 0.5).
    feather: f64,
    soft: bool,
    images: [Option<Image>; 2],
    place: [Placement; 2],
    /// Size of the preview canvas in CSS pixels.
    view: Vec2,
    /// While the rotation handle is dragged: the pivot and the pointer.
    rotating: Option<(Vec2, Vec2)>,
    reveal: Reveal,
}

impl Default for Scene {
    fn default() -> Scene {
        Scene {
            frame: Frame::new(1080.0, 1350.0),
            split: Split::default(),
            feather: 0.08,
            soft: false,
            images: [None, None],
            place: [Placement::default(); 2],
            view: v2(1080.0, 1350.0),
            rotating: None,
            reveal: Reveal { sweep: 3.0, hold: 0.6, round_trip: true },
        }
    }
}

/// Uniform block shared with `blend.wgsl`; thirteen vec4s.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    pub uv_a: [[f32; 4]; 2],
    pub uv_b: [[f32; 4]; 2],
    pub color_a: [[f32; 4]; 3],
    pub color_b: [[f32; 4]; 3],
    /// (nx, ny, k, half feather in px): signed distance is `n·p + k`.
    pub line: [f32; 4],
    /// Background, linear in the output space; w is 1 to draw stripes for
    /// a missing photo.
    pub bg: [f32; 4],
    /// (target width, target height, has A, has B).
    pub size: [f32; 4],
}

fn rows(m: &Affine) -> [[f32; 4]; 2] {
    [[m.a as f32, m.b as f32, m.tx as f32, 0.0], [m.c as f32, m.d as f32, m.ty as f32, 0.0]]
}

/// A tile of an output image: the full size and the tile's pixel rectangle.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
    pub tile_w: f64,
    pub tile_h: f64,
}

impl Target {
    pub fn whole(width: f64, height: f64) -> Target {
        Target { width, height, x: 0.0, y: 0.0, tile_w: width, tile_h: height }
    }
}

impl Scene {
    fn s(&self) -> f64 {
        self.view.x.min(self.view.y).max(1.0)
    }
    /// CSS pixels on the preview canvas -> frame space.
    fn to_frame(&self, x: f64, y: f64) -> Vec2 {
        Frame::from_pixels(self.view.x, self.view.y).apply(v2(x, y))
    }
    fn to_view(&self, q: Vec2) -> Vec2 {
        Frame::from_pixels(self.view.x, self.view.y).inverse().unwrap().apply(q)
    }
    fn which(i: u32) -> usize {
        (i as usize).min(1)
    }

    pub fn image(&self, i: usize) -> Option<&Image> {
        self.images[i].as_ref()
    }

    /// Output pixel -> normalised stored-texture coordinates for image `i`.
    pub fn pixel_to_texture(&self, i: usize, t: &Target) -> Affine {
        let Some(img) = self.images[i] else { return Affine::IDENTITY };
        let to_frame = Affine::translate(v2(t.x, t.y)).then(&Frame::from_pixels(t.width, t.height));
        let to_image = self.place[i].image_to_frame(&self.frame, img.size).inverse().unwrap_or(Affine::IDENTITY);
        to_frame
            .then(&to_image)
            .then(&Affine::scale(1.0 / img.size.x, 1.0 / img.size.y))
            .then(&img.residual.display_to_stored())
    }

    /// The split currently drawn: the reveal's when `time` is given.
    pub fn split_at(&self, time: Option<f64>) -> Split {
        match time {
            Some(t) => self.reveal.split_at(t, &self.split, &self.frame, self.feather_width()),
            None => self.split,
        }
    }

    fn feather_width(&self) -> f64 {
        if self.soft { self.feather } else { 0.0 }
    }

    /// `placeholder` draws soft stripes where a photo is missing or does not
    /// reach (the preview); exports get the plain background.
    pub fn uniforms(&self, t: &Target, out: Space, bg_srgb: [f64; 3], time: Option<f64>, placeholder: bool) -> Uniforms {
        let s = t.width.min(t.height);
        let split = self.split_at(time);
        let n = split.normal();
        let centre = v2(t.width / 2.0, t.height / 2.0);
        let k = (v2(t.x, t.y) - centre).dot(n) - split.offset * s;
        let color = |i: usize| {
            let m = convert(self.images[i].map_or(Space::Srgb, |im| im.space), out);
            m.map(|r| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0])
        };
        let to_linear = |c: f64| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let bg_lin = bg_srgb.map(to_linear);
        let bg = crate::color::mul_vec(&convert(Space::Srgb, out), bg_lin);
        Uniforms {
            uv_a: rows(&self.pixel_to_texture(0, t)),
            uv_b: rows(&self.pixel_to_texture(1, t)),
            color_a: color(0),
            color_b: color(1),
            line: [n.x as f32, n.y as f32, k as f32, (self.feather_width() * s / 2.0) as f32],
            bg: [bg[0] as f32, bg[1] as f32, bg[2] as f32, placeholder as u8 as f32],
            size: [
                t.tile_w as f32,
                t.tile_h as f32,
                self.images[0].is_some() as u8 as f32,
                self.images[1].is_some() as u8 as f32,
            ],
        }
    }

    pub fn set_image_info(&mut self, i: usize, image: Option<Image>) {
        self.images[i] = image;
    }

    pub fn frame(&self) -> Frame {
        self.frame
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Scene {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> Scene {
        Scene::default()
    }

    /// Set the output's size (only the aspect ratio matters).
    pub fn set_frame(&mut self, width: f64, height: f64) {
        self.frame = Frame::new(width, height);
        self.split.clamp(&self.frame);
    }
    pub fn aspect(&self) -> f64 {
        self.frame.aspect
    }
    /// Size of the preview canvas, in CSS pixels.
    pub fn set_view(&mut self, width: f64, height: f64) {
        self.view = v2(width.max(1.0), height.max(1.0));
    }

    /// Register photo `which` (0 = A, 1 = B): its displayed size, the EXIF
    /// orientation still to apply and its colour space code.
    pub fn set_image(&mut self, which: u32, width: f64, height: f64, residual: u8, space: u32) {
        self.images[Scene::which(which)] = Some(Image {
            size: v2(width, height),
            residual: Orientation::from_tag(residual as u16),
            space: Space::from_code(space),
        });
    }
    pub fn clear_image(&mut self, which: u32) {
        self.images[Scene::which(which)] = None;
    }
    pub fn has_image(&self, which: u32) -> bool {
        self.images[Scene::which(which)].is_some()
    }
    pub fn swap(&mut self) {
        self.images.swap(0, 1);
        self.place.swap(0, 1);
    }

    pub fn angle(&self) -> f64 {
        self.split.angle_deg
    }
    pub fn set_angle(&mut self, deg: f64) {
        // Rotate about the point of the line nearest the centre of the frame.
        let pivot = self.split.foot();
        self.split = Split::through(pivot, deg);
        self.split.clamp(&self.frame);
    }
    pub fn offset(&self) -> f64 {
        self.split.offset
    }
    pub fn set_offset(&mut self, offset: f64) {
        self.split.offset = offset;
        self.split.clamp(&self.frame);
    }
    /// The offset as a fraction of how far the line can travel (-1 to 1).
    pub fn position(&self) -> f64 {
        self.split.offset / self.split.extent(&self.frame).max(1e-9)
    }
    pub fn set_position(&mut self, p: f64) {
        self.set_offset(p.clamp(-1.0, 1.0) * self.split.extent(&self.frame));
    }

    pub fn feather(&self) -> f64 {
        self.feather
    }
    pub fn set_feather(&mut self, f: f64) {
        self.feather = f.clamp(0.0, 0.5);
    }
    pub fn soft(&self) -> bool {
        self.soft
    }
    pub fn set_soft(&mut self, soft: bool) {
        self.soft = soft;
    }

    /// `[cx, cy, zoom, rotation°]` of image `which`.
    pub fn placement(&self, which: u32) -> Vec<f64> {
        let p = self.place[Scene::which(which)];
        vec![p.cx, p.cy, p.zoom, p.rot_deg]
    }
    pub fn set_placement(&mut self, which: u32, cx: f64, cy: f64, zoom: f64, rot_deg: f64) {
        self.place[Scene::which(which)] = Placement {
            cx: cx.clamp(-4.0, 4.0),
            cy: cy.clamp(-4.0, 4.0),
            zoom: zoom.clamp(crate::geometry::MIN_ZOOM, crate::geometry::MAX_ZOOM),
            rot_deg: crate::geometry::wrap_degrees(rot_deg),
        };
    }
    pub fn reset_placement(&mut self, which: u32) {
        self.place[Scene::which(which)] = Placement::default();
    }

    /// Pan image `which` by a CSS-pixel delta.
    pub fn pan(&mut self, which: u32, dx: f64, dy: f64) {
        let s = self.s();
        self.place[Scene::which(which)].pan(v2(dx / s, dy / s));
    }
    /// Zoom image `which` by `k` about a CSS-pixel point.
    pub fn zoom_at(&mut self, which: u32, k: f64, x: f64, y: f64) {
        let q = self.to_frame(x, y);
        self.place[Scene::which(which)].zoom_about(k, q);
    }
    /// Rotate image `which` by `rad` about a CSS-pixel point.
    pub fn rotate_at(&mut self, which: u32, rad: f64, x: f64, y: f64) {
        let q = self.to_frame(x, y);
        self.place[Scene::which(which)].rotate_about(rad, q);
    }

    /// Which photo is under a CSS-pixel point: 0 for A, 1 for B.
    pub fn side_at(&self, x: f64, y: f64) -> u32 {
        (self.split.distance(self.to_frame(x, y)) > 0.0) as u32
    }

    /// Handle positions in CSS pixels: `[position x, y, rotation x, y,
    /// line start x, y, line end x, y]`, or empty when the line is outside
    /// the frame. `inset` keeps the rotation handle off the frame edge.
    pub fn handles(&self, inset: f64) -> Vec<f64> {
        let Some((p0, p1)) = self.split.segment(&self.frame) else { return vec![] };
        let s = self.s();
        let mid = self.rotating.map_or((p0 + p1) * 0.5, |(pivot, _)| pivot);
        let len = (p1 - p0).len();
        let d = self.split.direction();
        let rot = match self.rotating {
            // While rotating, the handle follows the finger along the line.
            Some((pivot, at)) => {
                let t = (at - pivot).dot(d).clamp(-len, len);
                pivot + d * t
            }
            None => mid + d * (len / 2.0 - (inset / s).min(len / 2.0)),
        };
        let [a, b, c, e] = [mid, rot, p0, p1].map(|q| self.to_view(q));
        vec![a.x, a.y, b.x, b.y, c.x, c.y, e.x, e.y]
    }

    /// Move the line so that it passes through a CSS-pixel point.
    pub fn drag_position(&mut self, x: f64, y: f64) {
        let q = self.to_frame(x, y);
        self.set_offset(q.dot(self.split.normal()));
    }

    pub fn begin_rotate(&mut self) {
        if let Some((p0, p1)) = self.split.segment(&self.frame) {
            let pivot = (p0 + p1) * 0.5;
            self.rotating = Some((pivot, pivot));
        }
    }
    /// Turn the line about the pivot fixed at `begin_rotate`, so that it
    /// points at a CSS-pixel point.
    pub fn drag_rotate(&mut self, x: f64, y: f64) {
        let Some((pivot, _)) = self.rotating else { return };
        let q = self.to_frame(x, y);
        let d = q - pivot;
        if d.len() * self.s() < 4.0 {
            return;
        }
        // direction() is (-sin θ, cos θ).
        let deg = (-d.x).atan2(d.y).to_degrees();
        self.split = Split::through(pivot, deg);
        self.split.clamp(&self.frame);
        self.rotating = Some((pivot, q));
    }
    pub fn end_rotate(&mut self) {
        self.rotating = None;
    }

    /// Map a point given in normalised displayed coordinates of image
    /// `which` to CSS pixels on the preview.
    pub fn image_point_to_view(&self, which: u32, u: f64, v: f64) -> Vec<f64> {
        let i = Scene::which(which);
        let Some(img) = self.images[i] else { return vec![] };
        let q = self.place[i].image_to_frame(&self.frame, img.size).apply(v2(u * img.size.x, v * img.size.y));
        let p = self.to_view(q);
        vec![p.x, p.y]
    }

    /// Map a point in normalised *stored* texture coordinates (what a
    /// detector sees when the decoder did not rotate) to displayed ones.
    pub fn stored_to_displayed(&self, which: u32, u: f64, v: f64) -> Vec<f64> {
        let Some(img) = self.images[Scene::which(which)] else { return vec![u, v] };
        let m = img.residual.display_to_stored().inverse().unwrap_or(Affine::IDENTITY);
        let p = m.apply(v2(u, v));
        vec![p.x, p.y]
    }

    /// Align B's eyes onto A's and put the split down the face's midline.
    /// Eyes are `[x1, y1, x2, y2]` in normalised displayed coordinates of
    /// each photo, listed in the same anatomical order.
    pub fn align(&mut self, eyes_a: &[f64], eyes_b: &[f64]) -> bool {
        let (Some(ia), Some(ib)) = (self.images[0], self.images[1]) else { return false };
        if eyes_a.len() < 4 || eyes_b.len() < 4 {
            return false;
        }
        let ma = self.place[0].image_to_frame(&self.frame, ia.size);
        let mb = self.place[1].image_to_frame(&self.frame, ib.size);
        let pa = |k: usize| ma.apply(v2(eyes_a[k] * ia.size.x, eyes_a[k + 1] * ia.size.y));
        let pb = |k: usize| mb.apply(v2(eyes_b[k] * ib.size.x, eyes_b[k + 1] * ib.size.y));
        let (a1, a2, b1, b2) = (pa(0), pa(2), pb(0), pb(2));
        let Some(sim) = Similarity::solve(a1, a2, b1, b2) else { return false };
        self.place[1] = sim.compose(&self.place[1]);
        self.split = midline(a1, a2);
        self.split.clamp(&self.frame);
        true
    }

    pub fn set_reveal(&mut self, sweep: f64, hold: f64, round_trip: bool) {
        self.reveal = Reveal { sweep: sweep.clamp(1.0, 6.0), hold: hold.clamp(0.0, 3.0), round_trip };
    }
    pub fn reveal_duration(&self) -> f64 {
        self.reveal.duration()
    }
    pub fn reveal_progress(&self, t: f64) -> f64 {
        self.reveal.progress(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        let mut s = Scene::new();
        s.set_frame(1000.0, 1000.0);
        s.set_view(500.0, 500.0);
        s.set_image(0, 800.0, 800.0, 1, 0);
        s.set_image(1, 1600.0, 800.0, 1, 1);
        s
    }

    #[test]
    fn side_follows_the_line() {
        let mut s = scene();
        s.set_angle(0.0);
        s.set_offset(0.0);
        assert_eq!(s.side_at(100.0, 250.0), 0);
        assert_eq!(s.side_at(400.0, 250.0), 1);
    }

    #[test]
    fn dragging_position_puts_the_line_under_the_pointer() {
        let mut s = scene();
        s.set_angle(30.0);
        s.drag_position(320.0, 140.0);
        let q = s.to_frame(320.0, 140.0);
        assert!(s.split.distance(q).abs() < 1e-12);
        let h = s.handles(20.0);
        assert_eq!(h.len(), 8);
    }

    #[test]
    fn rotation_handle_stays_under_the_finger() {
        let mut s = scene();
        s.set_angle(0.0);
        s.set_offset(0.0);
        s.begin_rotate();
        let finger = (380.0, 420.0);
        s.drag_rotate(finger.0, finger.1);
        let h = s.handles(20.0);
        assert!((h[2] - finger.0).abs() < 1e-6 && (h[3] - finger.1).abs() < 1e-6);
        // The pivot did not move.
        assert!((h[0] - 250.0).abs() < 1e-6 && (h[1] - 250.0).abs() < 1e-6);
        s.end_rotate();
    }

    #[test]
    fn position_is_relative_to_the_reach() {
        let mut s = scene();
        s.set_angle(45.0);
        s.set_position(1.0);
        assert!((s.offset() - s.split.extent(&s.frame)).abs() < 1e-12);
        s.set_position(-3.0);
        assert!((s.position() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn texture_mapping_covers_the_frame() {
        let s = scene();
        let t = Target::whole(1000.0, 1000.0);
        // Image A is square: the frame's corners are its corners.
        let m = s.pixel_to_texture(0, &t);
        let p = m.apply(v2(0.0, 0.0));
        assert!(p.len() < 1e-12);
        let p = m.apply(v2(1000.0, 1000.0));
        assert!((p - v2(1.0, 1.0)).len() < 1e-12);
        // Image B is 2:1, so its middle half is visible.
        let m = s.pixel_to_texture(1, &t);
        assert!((m.apply(v2(0.0, 500.0)) - v2(0.25, 0.5)).len() < 1e-12);
    }

    #[test]
    fn tiles_agree_with_the_whole() {
        let s = scene();
        let whole = Target::whole(1000.0, 1000.0);
        let tile = Target { x: 512.0, y: 256.0, tile_w: 256.0, tile_h: 256.0, ..whole };
        let a = s.pixel_to_texture(1, &whole).apply(v2(600.5, 300.5));
        let b = s.pixel_to_texture(1, &tile).apply(v2(88.5, 44.5));
        assert!((a - b).len() < 1e-12);
        let uw = s.uniforms(&whole, Space::Srgb, [0.0; 3], None, true);
        let ut = s.uniforms(&tile, Space::Srgb, [0.0; 3], None, true);
        let d = |u: &Uniforms, x: f32, y: f32| u.line[0] * x + u.line[1] * y + u.line[2];
        assert!((d(&uw, 600.5, 300.5) - d(&ut, 88.5, 44.5)).abs() < 1e-3);
    }

    #[test]
    fn feather_is_in_output_pixels() {
        let mut s = scene();
        s.set_soft(true);
        s.set_feather(0.1);
        let u = s.uniforms(&Target::whole(2000.0, 1000.0), Space::Srgb, [0.0; 3], None, true);
        assert!((u.line[3] - 50.0).abs() < 1e-4);
        s.set_soft(false);
        let u = s.uniforms(&Target::whole(2000.0, 1000.0), Space::Srgb, [0.0; 3], None, true);
        assert_eq!(u.line[3], 0.0);
    }

    #[test]
    fn align_puts_b_eyes_on_a_eyes() {
        let mut s = scene();
        s.set_placement(1, 0.1, 0.05, 1.3, 12.0);
        let ea = [0.4, 0.45, 0.6, 0.46];
        let eb = [0.3, 0.5, 0.42, 0.52];
        assert!(s.align(&ea, &eb));
        for k in [0, 2] {
            let a = s.image_point_to_view(0, ea[k], ea[k + 1]);
            let b = s.image_point_to_view(1, eb[k], eb[k + 1]);
            assert!((a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6);
        }
        // The split passes between the eyes.
        let mid = s.image_point_to_view(0, 0.5, 0.455);
        let q = s.to_frame(mid[0], mid[1]);
        assert!(s.split.distance(q).abs() < 1e-9);
        assert!(!Scene::new().align(&ea, &eb));
    }

    #[test]
    fn swap_exchanges_images_and_placements() {
        let mut s = scene();
        s.set_placement(0, 0.2, 0.0, 2.0, 0.0);
        s.swap();
        assert_eq!(s.placement(1)[2], 2.0);
        assert_eq!(s.image(0).unwrap().size, v2(1600.0, 800.0));
    }
}
