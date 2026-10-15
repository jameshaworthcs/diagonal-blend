//! The eye-pair similarity solve and the face midline.
//!
//! A similarity transform (uniform scale, rotation, translation) has four
//! degrees of freedom, and two point correspondences give exactly four
//! equations, so the eye pair pins it down. Treat points as complex numbers:
//! `S(p) = z·p + t`. From `S(b1) = a1` and `S(b2) = a2`,
//! `z = (a2 − a1) / (b2 − b1)` and `t = a1 − z·b1`.

use crate::geometry::{Placement, Split, Vec2, v2, wrap_degrees};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Similarity {
    /// `z` as (re, im): scale·(cos θ, sin θ).
    pub z: Vec2,
    pub t: Vec2,
}

fn cmul(a: Vec2, b: Vec2) -> Vec2 {
    v2(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x)
}

fn cdiv(a: Vec2, b: Vec2) -> Option<Vec2> {
    let d = b.dot(b);
    if d < 1e-18 {
        return None;
    }
    Some(v2((a.x * b.x + a.y * b.y) / d, (a.y * b.x - a.x * b.y) / d))
}

impl Similarity {
    /// The transform taking `b1 -> a1` and `b2 -> a2`.
    pub fn solve(a1: Vec2, a2: Vec2, b1: Vec2, b2: Vec2) -> Option<Similarity> {
        let z = cdiv(a2 - a1, b2 - b1)?;
        if z.len() < 1e-9 {
            return None;
        }
        Some(Similarity { z, t: a1 - cmul(z, b1) })
    }
    pub fn apply(&self, p: Vec2) -> Vec2 {
        cmul(self.z, p) + self.t
    }
    pub fn scale(&self) -> f64 {
        self.z.len()
    }
    pub fn rotation(&self) -> f64 {
        self.z.angle()
    }
    /// `S ∘ P`: the placement that draws the image where `S` moves it.
    pub fn compose(&self, p: &Placement) -> Placement {
        let c = self.apply(p.centre());
        Placement {
            cx: c.x,
            cy: c.y,
            zoom: p.zoom * self.scale(),
            rot_deg: wrap_degrees(p.rot_deg + self.rotation().to_degrees()),
        }
    }
}

/// How far the midline is pulled towards vertical: 0 keeps the true
/// perpendicular bisector of the eyes, 1 makes it exactly vertical.
pub const VERTICAL_PULL: f64 = 0.5;

/// The perpendicular bisector of the eye pair, nudged towards vertical.
///
/// The split's normal points along the eye line, from `e1` to `e2`, so the
/// first eye's half of the face comes from A.
pub fn midline(e1: Vec2, e2: Vec2) -> Split {
    let roll = (e2 - e1).angle().to_degrees();
    let upright = (roll / 180.0).round() * 180.0;
    let angle = upright + (roll - upright) * (1.0 - VERTICAL_PULL);
    Split::through((e1 + e2) * 0.5, angle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Frame;

    fn close2(a: Vec2, b: Vec2) -> bool {
        (a - b).len() < 1e-9
    }

    #[test]
    fn solve_maps_both_eyes_exactly() {
        let (a1, a2) = (v2(-0.12, -0.05), v2(0.11, -0.03));
        let (b1, b2) = (v2(0.3, 0.2), v2(0.35, 0.31));
        let s = Similarity::solve(a1, a2, b1, b2).unwrap();
        assert!(close2(s.apply(b1), a1));
        assert!(close2(s.apply(b2), a2));
        assert!(((a2 - a1).len() / (b2 - b1).len() - s.scale()).abs() < 1e-12);
    }

    #[test]
    fn solve_recovers_a_known_transform() {
        let truth = Similarity { z: v2(1.7, 0.0).rotate(-0.4), t: v2(0.25, -0.1) };
        let (b1, b2) = (v2(0.1, 0.2), v2(-0.3, 0.05));
        let s = Similarity::solve(truth.apply(b1), truth.apply(b2), b1, b2).unwrap();
        assert!(close2(s.z, truth.z) && close2(s.t, truth.t));
        assert!((s.rotation() + 0.4).abs() < 1e-12);
    }

    #[test]
    fn coincident_eyes_have_no_solution() {
        let p = v2(0.1, 0.1);
        assert!(Similarity::solve(v2(0.0, 0.0), v2(1.0, 0.0), p, p).is_none());
        assert!(Similarity::solve(p, p, v2(0.0, 0.0), v2(1.0, 0.0)).is_none());
    }

    #[test]
    fn composed_placement_moves_the_eyes_onto_a() {
        let f = Frame::new(1080.0, 1350.0);
        let size = v2(1200.0, 1600.0);
        let b = Placement { cx: 0.05, cy: -0.02, zoom: 1.2, rot_deg: 8.0 };
        let m = b.image_to_frame(&f, size);
        // Eyes in B's own pixels, and where A's eyes sit in the frame.
        let (eb1, eb2) = (v2(480.0, 700.0), v2(760.0, 690.0));
        let (a1, a2) = (v2(-0.1, -0.08), v2(0.09, -0.07));
        let s = Similarity::solve(a1, a2, m.apply(eb1), m.apply(eb2)).unwrap();
        let moved = s.compose(&b).image_to_frame(&f, size);
        assert!(close2(moved.apply(eb1), a1));
        assert!(close2(moved.apply(eb2), a2));
    }

    #[test]
    fn midline_bisects_the_eyes() {
        let (e1, e2) = (v2(-0.1, 0.0), v2(0.1, 0.0));
        let s = midline(e1, e2);
        assert!(s.angle_deg.abs() < 1e-9 && s.offset.abs() < 1e-9);
        assert!(s.distance(e1) < 0.0 && s.distance(e2) > 0.0);
    }

    #[test]
    fn midline_is_nudged_towards_vertical() {
        // A head tilted by 20 degrees.
        let e1 = v2(0.0, 0.0);
        let e2 = v2(20f64.to_radians().cos(), 20f64.to_radians().sin()) * 0.2;
        let s = midline(e1, e2);
        assert!((s.angle_deg - 20.0 * (1.0 - VERTICAL_PULL)).abs() < 1e-9);
        assert!(s.distance((e1 + e2) * 0.5).abs() < 1e-12);
        // An upside-down face still gets a near-vertical line.
        let s = midline(e2, e1);
        assert!((s.angle_deg.abs() - (180.0 - 10.0)).abs() < 1e-9);
    }
}
