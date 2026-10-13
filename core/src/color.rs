//! sRGB and Display P3: primaries, matrices and the shared transfer curve.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Srgb = 0,
    DisplayP3 = 1,
}

impl Space {
    pub fn from_code(c: u32) -> Space {
        if c == 1 { Space::DisplayP3 } else { Space::Srgb }
    }
    /// CIE xy chromaticities of red, green and blue. Both use D65 white.
    pub fn primaries(self) -> [(f64, f64); 3] {
        match self {
            Space::Srgb => [(0.640, 0.330), (0.300, 0.600), (0.150, 0.060)],
            Space::DisplayP3 => [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)],
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Space::Srgb => "sRGB",
            Space::DisplayP3 => "Display P3",
        }
    }
}

pub const D65: (f64, f64) = (0.3127, 0.3290);
/// The ICC profile connection space white, as XYZ.
pub const D50_XYZ: [f64; 3] = [0.9642, 1.0, 0.8249];

pub type Mat3 = [[f64; 3]; 3];

pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

pub fn mul_vec(a: &Mat3, v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| a[i][0] * v[0] + a[i][1] * v[1] + a[i][2] * v[2])
}

pub fn inverse(m: &Mat3) -> Mat3 {
    let [[a, b, c], [d, e, f], [g, h, i]] = *m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    let k = 1.0 / det;
    [
        [(e * i - f * h) * k, (c * h - b * i) * k, (b * f - c * e) * k],
        [(f * g - d * i) * k, (a * i - c * g) * k, (c * d - a * f) * k],
        [(d * h - e * g) * k, (b * g - a * h) * k, (a * e - b * d) * k],
    ]
}

fn xy_to_xyz((x, y): (f64, f64)) -> [f64; 3] {
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// Linear RGB -> XYZ (D65), from the primaries.
pub fn rgb_to_xyz(space: Space) -> Mat3 {
    let p = space.primaries().map(xy_to_xyz);
    let m = [[p[0][0], p[1][0], p[2][0]], [p[0][1], p[1][1], p[2][1]], [p[0][2], p[1][2], p[2][2]]];
    let s = mul_vec(&inverse(&m), xy_to_xyz(D65));
    [0, 1, 2].map(|i| [m[i][0] * s[0], m[i][1] * s[1], m[i][2] * s[2]])
}

/// Bradford chromatic adaptation from D65 to D50, as used by ICC profiles.
pub fn bradford_d65_to_d50() -> Mat3 {
    let b: Mat3 = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]];
    let src = mul_vec(&b, xy_to_xyz(D65));
    let dst = mul_vec(&b, D50_XYZ);
    let scale = [[dst[0] / src[0], 0.0, 0.0], [0.0, dst[1] / src[1], 0.0], [0.0, 0.0, dst[2] / src[2]]];
    mul(&inverse(&b), &mul(&scale, &b))
}

/// The red, green and blue colorants of a profile, adapted to D50.
pub fn d50_colorants(space: Space) -> [[f64; 3]; 3] {
    let m = mul(&bradford_d65_to_d50(), &rgb_to_xyz(space));
    [0, 1, 2].map(|c| [m[0][c], m[1][c], m[2][c]])
}

/// Linear `from` -> linear `to`.
pub fn convert(from: Space, to: Space) -> Mat3 {
    if from == to {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    mul(&inverse(&rgb_to_xyz(to)), &rgb_to_xyz(from))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &Mat3, b: &Mat3, tol: f64) -> bool {
        (0..3).all(|i| (0..3).all(|j| (a[i][j] - b[i][j]).abs() < tol))
    }

    #[test]
    fn srgb_matrix_matches_the_standard() {
        let want = [[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]];
        assert!(close(&rgb_to_xyz(Space::Srgb), &want, 1e-3));
    }

    #[test]
    fn srgb_to_p3_matches_published_values() {
        let want = [[0.8225, 0.1774, 0.0], [0.0332, 0.9669, 0.0], [0.0171, 0.0724, 0.9108]];
        assert!(close(&convert(Space::Srgb, Space::DisplayP3), &want, 1e-3));
    }

    #[test]
    fn white_stays_white() {
        let m = convert(Space::DisplayP3, Space::Srgb);
        for v in mul_vec(&m, [1.0, 1.0, 1.0]) {
            assert!((v - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn d50_colorants_match_common_profiles() {
        // The colorants found in the widely used sRGB IEC61966-2.1 profile.
        let r = d50_colorants(Space::Srgb)[0];
        assert!((r[0] - 0.4361).abs() < 2e-4 && (r[1] - 0.2225).abs() < 2e-4 && (r[2] - 0.0139).abs() < 2e-4);
        // And in Apple's Display P3 profile.
        let g = d50_colorants(Space::DisplayP3)[1];
        assert!((g[0] - 0.2919).abs() < 2e-4 && (g[1] - 0.6922).abs() < 2e-4 && (g[2] - 0.0419).abs() < 2e-4);
    }
}
