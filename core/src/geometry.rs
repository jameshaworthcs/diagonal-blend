//! Frame space, the split line and the per-image transforms.
//!
//! Everything is resolution independent. A point in *frame space* is measured
//! from the centre of the output frame in units of its short side, with y
//! pointing down. The preview canvas and an 8K export of the same settings
//! therefore show exactly the same composition.

use std::ops::{Add, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

pub const fn v2(x: f64, y: f64) -> Vec2 {
    Vec2 { x, y }
}

impl Vec2 {
    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn rotate(self, rad: f64) -> Vec2 {
        let (s, c) = rad.sin_cos();
        v2(c * self.x - s * self.y, s * self.x + c * self.y)
    }
    /// Angle of the vector in radians, measured clockwise from +x (y is down).
    pub fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        v2(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        v2(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f64) -> Vec2 {
        v2(self.x * k, self.y * k)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        v2(-self.x, -self.y)
    }
}

/// A 2D affine map `p -> [a b; c d] p + [tx ty]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

    pub fn translate(t: Vec2) -> Affine {
        Affine { tx: t.x, ty: t.y, ..Affine::IDENTITY }
    }
    pub fn scale(sx: f64, sy: f64) -> Affine {
        Affine { a: sx, d: sy, ..Affine::IDENTITY }
    }
    pub fn rotate(rad: f64) -> Affine {
        let (s, c) = rad.sin_cos();
        Affine { a: c, b: -s, c: s, d: c, tx: 0.0, ty: 0.0 }
    }
    pub fn apply(&self, p: Vec2) -> Vec2 {
        v2(self.a * p.x + self.b * p.y + self.tx, self.c * p.x + self.d * p.y + self.ty)
    }
    /// `self.then(o)` applies `self` first, then `o`.
    pub fn then(&self, o: &Affine) -> Affine {
        Affine {
            a: o.a * self.a + o.b * self.c,
            b: o.a * self.b + o.b * self.d,
            c: o.c * self.a + o.d * self.c,
            d: o.c * self.b + o.d * self.d,
            tx: o.a * self.tx + o.b * self.ty + o.tx,
            ty: o.c * self.tx + o.d * self.ty + o.ty,
        }
    }
    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-300 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine { a, b, c, d, tx: -(a * self.tx + b * self.ty), ty: -(c * self.tx + d * self.ty) })
    }
}

/// The output frame: only its aspect ratio matters to the composition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub aspect: f64,
}

impl Frame {
    pub fn new(width: f64, height: f64) -> Frame {
        Frame { aspect: (width / height).max(1e-6) }
    }
    /// Half extents in frame units (the short side is 1).
    pub fn half(&self) -> Vec2 {
        if self.aspect >= 1.0 { v2(self.aspect / 2.0, 0.5) } else { v2(0.5, 0.5 / self.aspect) }
    }
    /// Map from pixels of a `w` x `h` target (with this aspect) to frame space.
    pub fn from_pixels(w: f64, h: f64) -> Affine {
        let s = 1.0 / w.min(h);
        Affine { a: s, b: 0.0, c: 0.0, d: s, tx: -w / 2.0 * s, ty: -h / 2.0 * s }
    }
    pub fn contains(&self, q: Vec2) -> bool {
        let h = self.half();
        q.x.abs() <= h.x + 1e-12 && q.y.abs() <= h.y + 1e-12
    }
}

/// The split line, stored as an angle and a signed offset.
///
/// `angle_deg = 0` is a vertical line with A on the left. The unit normal
/// `n = (cos θ, sin θ)` points into B, and the line is `{q : q·n = offset}`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Split {
    pub angle_deg: f64,
    pub offset: f64,
}

impl Default for Split {
    fn default() -> Split {
        Split { angle_deg: 28.0, offset: 0.0 }
    }
}

impl Split {
    pub fn normal(&self) -> Vec2 {
        let t = self.angle_deg.to_radians();
        v2(t.cos(), t.sin())
    }
    /// Direction along the line, pointing "down" for small angles.
    pub fn direction(&self) -> Vec2 {
        let t = self.angle_deg.to_radians();
        v2(-t.sin(), t.cos())
    }
    /// Signed distance in frame units; positive on B's side.
    pub fn distance(&self, q: Vec2) -> f64 {
        q.dot(self.normal()) - self.offset
    }
    /// The point on the line closest to the frame centre.
    pub fn foot(&self) -> Vec2 {
        self.normal() * self.offset
    }
    /// The largest offset that still touches the frame, for this angle.
    pub fn extent(&self, frame: &Frame) -> f64 {
        let n = self.normal();
        let h = frame.half();
        n.x.abs() * h.x + n.y.abs() * h.y
    }
    /// Line through `p` at `angle_deg`.
    pub fn through(p: Vec2, angle_deg: f64) -> Split {
        let mut s = Split { angle_deg: wrap_degrees(angle_deg), offset: 0.0 };
        s.offset = p.dot(s.normal());
        s
    }
    /// Keep the line inside the frame.
    pub fn clamp(&mut self, frame: &Frame) {
        self.angle_deg = wrap_degrees(self.angle_deg);
        let e = self.extent(frame);
        self.offset = self.offset.clamp(-e, e);
    }
    /// The part of the line inside the frame, ordered along `direction()`.
    pub fn segment(&self, frame: &Frame) -> Option<(Vec2, Vec2)> {
        let p = self.foot();
        let d = self.direction();
        let h = frame.half();
        let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
        for (pc, dc, hc) in [(p.x, d.x, h.x), (p.y, d.y, h.y)] {
            if dc.abs() < 1e-12 {
                if pc.abs() > hc {
                    return None;
                }
                continue;
            }
            let (a, b) = ((-hc - pc) / dc, (hc - pc) / dc);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        if t0 > t1 {
            return None;
        }
        Some((p + d * t0, p + d * t1))
    }
}

/// Wrap an angle into (-180, 180].
pub fn wrap_degrees(a: f64) -> f64 {
    let mut a = a % 360.0;
    if a <= -180.0 {
        a += 360.0;
    } else if a > 180.0 {
        a -= 360.0;
    }
    a
}

/// Where an image sits in the frame.
///
/// `zoom = 1` is the "cover" fit, the smallest scale at which the image fills
/// the frame. The centre is in frame units and the rotation is about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub cx: f64,
    pub cy: f64,
    pub zoom: f64,
    pub rot_deg: f64,
}

impl Default for Placement {
    fn default() -> Placement {
        Placement { cx: 0.0, cy: 0.0, zoom: 1.0, rot_deg: 0.0 }
    }
}

pub const MIN_ZOOM: f64 = 0.05;
pub const MAX_ZOOM: f64 = 40.0;

impl Placement {
    pub fn centre(&self) -> Vec2 {
        v2(self.cx, self.cy)
    }

    /// Frame units per image pixel at `zoom = 1`.
    pub fn cover_scale(frame: &Frame, size: Vec2) -> f64 {
        let h = frame.half();
        (2.0 * h.x / size.x).max(2.0 * h.y / size.y)
    }

    /// Oriented image pixels -> frame space.
    pub fn image_to_frame(&self, frame: &Frame, size: Vec2) -> Affine {
        let k = Placement::cover_scale(frame, size) * self.zoom;
        Affine::translate(-(size * 0.5))
            .then(&Affine::scale(k, k))
            .then(&Affine::rotate(self.rot_deg.to_radians()))
            .then(&Affine::translate(self.centre()))
    }

    /// Pan by `delta` frame units.
    pub fn pan(&mut self, delta: Vec2) {
        self.cx += delta.x;
        self.cy += delta.y;
    }

    /// Scale by `k` about the frame point `about`, keeping it fixed.
    pub fn zoom_about(&mut self, k: f64, about: Vec2) {
        let k = (self.zoom * k).clamp(MIN_ZOOM, MAX_ZOOM) / self.zoom;
        let c = about + (self.centre() - about) * k;
        self.cx = c.x;
        self.cy = c.y;
        self.zoom *= k;
    }

    /// Rotate by `rad` about the frame point `about`.
    pub fn rotate_about(&mut self, rad: f64, about: Vec2) {
        let c = about + (self.centre() - about).rotate(rad);
        self.cx = c.x;
        self.cy = c.y;
        self.rot_deg = wrap_degrees(self.rot_deg + rad.to_degrees());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }
    fn close2(a: Vec2, b: Vec2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    #[test]
    fn frame_half_uses_the_short_side() {
        assert_eq!(Frame::new(1920.0, 1080.0).half(), v2(1920.0 / 1080.0 / 2.0, 0.5));
        assert_eq!(Frame::new(1080.0, 1350.0).half(), v2(0.5, 0.625));
    }

    #[test]
    fn pixels_map_to_frame_units() {
        let m = Frame::from_pixels(200.0, 100.0);
        assert!(close2(m.apply(v2(100.0, 50.0)), v2(0.0, 0.0)));
        assert!(close2(m.apply(v2(0.0, 0.0)), v2(-1.0, -0.5)));
        assert!(close2(m.apply(v2(200.0, 100.0)), v2(1.0, 0.5)));
    }

    #[test]
    fn affine_inverse_round_trips() {
        let m = Affine::rotate(0.7).then(&Affine::scale(3.0, 0.5)).then(&Affine::translate(v2(4.0, -2.0)));
        let p = v2(1.25, -7.5);
        assert!(close2(m.inverse().unwrap().apply(m.apply(p)), p));
        assert!(Affine::scale(0.0, 1.0).inverse().is_none());
    }

    #[test]
    fn vertical_split_puts_b_on_the_right() {
        let s = Split { angle_deg: 0.0, offset: 0.0 };
        assert!(s.distance(v2(0.3, 0.1)) > 0.0);
        assert!(s.distance(v2(-0.3, 0.1)) < 0.0);
        assert!(close2(s.direction(), v2(0.0, 1.0)));
    }

    #[test]
    fn extent_reaches_the_far_corner() {
        let f = Frame::new(2.0, 1.0);
        let s = Split { angle_deg: 45.0, offset: 0.0 };
        let corner = f.half();
        assert!(close(s.extent(&f), corner.dot(s.normal())));
        let mut s = Split { angle_deg: 0.0, offset: 5.0 };
        s.clamp(&f);
        assert!(close(s.offset, 1.0));
    }

    #[test]
    fn segment_is_clipped_to_the_frame() {
        let f = Frame::new(1.0, 1.0);
        let (a, b) = Split { angle_deg: 0.0, offset: 0.25 }.segment(&f).unwrap();
        assert!(close2(a, v2(0.25, -0.5)));
        assert!(close2(b, v2(0.25, 0.5)));
        let (a, b) = Split { angle_deg: 45.0, offset: 0.0 }.segment(&f).unwrap();
        assert!(close2(a, v2(0.5, -0.5)));
        assert!(close2(b, v2(-0.5, 0.5)));
        assert!(Split { angle_deg: 0.0, offset: 0.75 }.segment(&f).is_none());
    }

    #[test]
    fn through_passes_through_the_point() {
        let p = v2(0.1, -0.2);
        let s = Split::through(p, 200.0);
        assert!(close(s.distance(p), 0.0));
        assert!(close(s.angle_deg, -160.0));
    }

    #[test]
    fn wrap_degrees_stays_in_range() {
        assert!(close(wrap_degrees(190.0), -170.0));
        assert!(close(wrap_degrees(-180.0), 180.0));
        assert!(close(wrap_degrees(540.0), 180.0));
        assert!(close(wrap_degrees(-30.0), -30.0));
    }

    #[test]
    fn cover_fit_fills_the_frame() {
        let f = Frame::new(1.0, 1.0);
        let size = v2(400.0, 200.0);
        let m = Placement::default().image_to_frame(&f, size);
        // The short side of the image spans the frame exactly.
        assert!(close2(m.apply(v2(200.0, 0.0)), v2(0.0, -0.5)));
        assert!(close2(m.apply(v2(200.0, 200.0)), v2(0.0, 0.5)));
        assert!(close2(m.apply(v2(0.0, 100.0)), v2(-1.0, 0.0)));
    }

    #[test]
    fn zoom_about_keeps_the_point_fixed() {
        let f = Frame::new(1.0, 1.0);
        let size = v2(100.0, 100.0);
        let mut p = Placement::default();
        let about = v2(0.2, 0.3);
        let before = p.image_to_frame(&f, size).inverse().unwrap().apply(about);
        p.zoom_about(2.5, about);
        let after = p.image_to_frame(&f, size).inverse().unwrap().apply(about);
        assert!(close2(before, after));
        assert!(close(p.zoom, 2.5));
    }

    #[test]
    fn zoom_is_clamped() {
        let mut p = Placement::default();
        p.zoom_about(1e9, v2(0.0, 0.0));
        assert!(close(p.zoom, MAX_ZOOM));
    }

    #[test]
    fn rotate_about_keeps_the_point_fixed() {
        let f = Frame::new(4.0, 3.0);
        let size = v2(640.0, 480.0);
        let mut p = Placement { cx: 0.1, cy: 0.0, zoom: 1.3, rot_deg: 10.0 };
        let about = v2(-0.4, 0.2);
        let before = p.image_to_frame(&f, size).inverse().unwrap().apply(about);
        p.rotate_about(0.5, about);
        let after = p.image_to_frame(&f, size).inverse().unwrap().apply(about);
        assert!(close2(before, after));
        assert!(close(p.rot_deg, 10.0 + 0.5f64.to_degrees()));
    }
}
