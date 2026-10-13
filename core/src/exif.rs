//! EXIF orientation, read straight from the file bytes.
//!
//! Browsers disagree about when they honour the orientation tag, and some
//! cannot be told not to, so the app reads it here, compares it with what the
//! decoder produced and applies whatever rotation is still owed in the shader.

use crate::geometry::{Affine, Vec2, v2};

/// The eight EXIF orientations. `1` is upright.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Orientation(pub u8);

impl Orientation {
    pub const UP: Orientation = Orientation(1);

    pub fn from_tag(v: u16) -> Orientation {
        if (1..=8).contains(&v) { Orientation(v as u8) } else { Orientation::UP }
    }

    /// Orientations 5 to 8 turn the image on its side.
    pub fn swaps_axes(self) -> bool {
        self.0 >= 5
    }

    /// Size of the image as displayed, given its stored size.
    pub fn display_size(self, stored: Vec2) -> Vec2 {
        if self.swaps_axes() { v2(stored.y, stored.x) } else { stored }
    }

    /// Normalised display coordinates -> normalised stored coordinates.
    pub fn display_to_stored(self) -> Affine {
        let (a, b, c, d, tx, ty) = match self.0 {
            2 => (-1.0, 0.0, 0.0, 1.0, 1.0, 0.0),
            3 => (-1.0, 0.0, 0.0, -1.0, 1.0, 1.0),
            4 => (1.0, 0.0, 0.0, -1.0, 0.0, 1.0),
            5 => (0.0, 1.0, 1.0, 0.0, 0.0, 0.0),
            6 => (0.0, 1.0, -1.0, 0.0, 0.0, 1.0),
            7 => (0.0, -1.0, -1.0, 0.0, 1.0, 1.0),
            8 => (0.0, -1.0, 1.0, 0.0, 1.0, 0.0),
            _ => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        };
        Affine { a, b, c, d, tx, ty }
    }

    /// The rotation still owed after a decoder that produced `decoded` pixels
    /// from an image stored as `stored`. A decoder that already rotated a
    /// sideways image gives itself away by swapping the dimensions; for the
    /// orientations that keep them, `auto_oriented` (probed once per browser)
    /// decides.
    pub fn residual(self, stored: Vec2, decoded: Vec2, auto_oriented: bool) -> Orientation {
        if self == Orientation::UP {
            return self;
        }
        if self.swaps_axes() && stored.x != stored.y {
            let rotated = (decoded.x - stored.y).abs() < 0.5 && (decoded.y - stored.x).abs() < 0.5;
            return if rotated { Orientation::UP } else { self };
        }
        if auto_oriented { Orientation::UP } else { self }
    }
}

/// What the header of an image file says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Header {
    pub orientation: Option<u16>,
    pub width: u32,
    pub height: u32,
}

pub fn read_header(bytes: &[u8]) -> Header {
    if bytes.starts_with(&[0xff, 0xd8]) {
        jpeg(bytes)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(bytes)
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        webp(bytes)
    } else {
        Header::default()
    }
}

fn be16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn le24(b: &[u8], i: usize) -> Option<u32> {
    let s = b.get(i..i + 3)?;
    Some(s[0] as u32 | (s[1] as u32) << 8 | (s[2] as u32) << 16)
}

fn jpeg(b: &[u8]) -> Header {
    let mut h = Header::default();
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xff {
            break;
        }
        let marker = b[i + 1];
        if marker == 0xff {
            i += 1;
            continue;
        }
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        let Some(len) = be16(b, i + 2) else { break };
        let len = len as usize;
        let body = i + 4;
        let end = (i + 2 + len).min(b.len());
        if len < 2 {
            break;
        }
        match marker {
            0xe1 if h.orientation.is_none() && b.get(body..body + 6) == Some(b"Exif\0\0") => {
                h.orientation = tiff_orientation(&b[(body + 6).min(end)..end]);
            }
            // SOF0..SOF15, except DHT (c4), JPG (c8) and DAC (cc).
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                if let (Some(hh), Some(ww)) = (be16(b, body + 1), be16(b, body + 3)) {
                    h.width = ww as u32;
                    h.height = hh as u32;
                }
            }
            _ => {}
        }
        i += 2 + len;
    }
    h
}

fn png(b: &[u8]) -> Header {
    let mut h = Header::default();
    let mut i = 8;
    while let (Some(len), Some(kind)) = (be32(b, i), b.get(i + 4..i + 8)) {
        let body = i + 8;
        let end = body.saturating_add(len as usize);
        if end > b.len() {
            break;
        }
        match kind {
            b"IHDR" => {
                h.width = be32(b, body).unwrap_or(0);
                h.height = be32(b, body + 4).unwrap_or(0);
            }
            b"eXIf" => h.orientation = tiff_orientation(&b[body..end]),
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        i = end + 4;
    }
    h
}

fn webp(b: &[u8]) -> Header {
    let mut h = Header::default();
    let mut i = 12;
    while let (Some(kind), Some(len)) = (b.get(i..i + 4), le32(b, i + 4)) {
        let body = i + 8;
        let end = body.saturating_add(len as usize);
        if end > b.len() {
            break;
        }
        match kind {
            b"VP8X" => {
                h.width = le24(b, body + 4).unwrap_or(0) + 1;
                h.height = le24(b, body + 7).unwrap_or(0) + 1;
            }
            b"VP8 " if h.width == 0 => {
                if let (Some(w), Some(hh)) = (be16(b, body + 6), be16(b, body + 8)) {
                    h.width = (w.swap_bytes() & 0x3fff) as u32;
                    h.height = (hh.swap_bytes() & 0x3fff) as u32;
                }
            }
            b"VP8L" if h.width == 0 => {
                if let Some(bits) = le32(b, body + 1) {
                    h.width = (bits & 0x3fff) + 1;
                    h.height = ((bits >> 14) & 0x3fff) + 1;
                }
            }
            b"EXIF" => {
                let mut d = &b[body..end];
                if d.starts_with(b"Exif\0\0") {
                    d = &d[6..];
                }
                h.orientation = tiff_orientation(d);
            }
            _ => {}
        }
        i = end + (len as usize & 1);
    }
    h
}

/// Find tag 0x0112 in IFD0 of a TIFF structure.
fn tiff_orientation(t: &[u8]) -> Option<u16> {
    let le = match t.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |i: usize| -> Option<u16> {
        let s: [u8; 2] = t.get(i..i + 2)?.try_into().ok()?;
        Some(if le { u16::from_le_bytes(s) } else { u16::from_be_bytes(s) })
    };
    let u32_at = |i: usize| -> Option<u32> {
        let s: [u8; 4] = t.get(i..i + 4)?.try_into().ok()?;
        Some(if le { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) })
    };
    if u16_at(2)? != 42 {
        return None;
    }
    let ifd = u32_at(4)? as usize;
    let count = u16_at(ifd)? as usize;
    for k in 0..count.min(512) {
        let e = ifd + 2 + k * 12;
        if u16_at(e)? == 0x0112 {
            // SHORT, count 1: the value sits in the first two bytes of the field.
            return (u16_at(e + 2)? == 3).then(|| u16_at(e + 8)).flatten();
        }
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A TIFF block holding only an orientation tag.
    pub fn tiff(orientation: u16, little_endian: bool) -> Vec<u8> {
        let w16 = |v: u16| if little_endian { v.to_le_bytes() } else { v.to_be_bytes() };
        let w32 = |v: u32| if little_endian { v.to_le_bytes() } else { v.to_be_bytes() };
        let mut t = Vec::new();
        t.extend_from_slice(if little_endian { b"II" } else { b"MM" });
        t.extend_from_slice(&w16(42));
        t.extend_from_slice(&w32(8));
        t.extend_from_slice(&w16(2));
        // An unrelated tag first (ImageDescription, ASCII) to check the walk.
        t.extend_from_slice(&w16(0x010e));
        t.extend_from_slice(&w16(2));
        t.extend_from_slice(&w32(4));
        t.extend_from_slice(b"abc\0");
        t.extend_from_slice(&w16(0x0112));
        t.extend_from_slice(&w16(3));
        t.extend_from_slice(&w32(1));
        t.extend_from_slice(&w16(orientation));
        t.extend_from_slice(&[0, 0]);
        t.extend_from_slice(&w32(0));
        t
    }

    pub fn jpeg_with(orientation: u16, little_endian: bool, w: u16, h: u16) -> Vec<u8> {
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend(tiff(orientation, little_endian));
        let mut j = vec![0xff, 0xd8];
        // An APP0 (JFIF) segment before the EXIF one, as cameras write them.
        j.extend_from_slice(&[0xff, 0xe0, 0, 16]);
        j.extend_from_slice(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        j.extend_from_slice(&[0xff, 0xe1]);
        j.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
        j.extend(app1);
        j.extend_from_slice(&[0xff, 0xc0, 0, 17, 8]);
        j.extend_from_slice(&h.to_be_bytes());
        j.extend_from_slice(&w.to_be_bytes());
        j.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        j.extend_from_slice(&[0xff, 0xda, 0, 2, 0xff, 0xd9]);
        j
    }

    #[test]
    fn reads_jpeg_orientation_in_both_byte_orders() {
        for le in [true, false] {
            for o in 1..=8 {
                let h = read_header(&jpeg_with(o, le, 640, 480));
                assert_eq!(h.orientation, Some(o));
                assert_eq!((h.width, h.height), (640, 480));
            }
        }
    }

    #[test]
    fn jpeg_without_exif_has_no_orientation() {
        let mut j = jpeg_with(6, true, 10, 20);
        j[2 + 4 + 14 + 4] = b'X'; // break the "Exif" signature
        assert_eq!(read_header(&j).orientation, None);
    }

    #[test]
    fn truncated_input_never_panics() {
        let j = jpeg_with(6, false, 4000, 3000);
        for n in 0..j.len() {
            let _ = read_header(&j[..n]);
        }
        let _ = read_header(&[0xff, 0xd8, 0xff, 0xe1, 0xff, 0xff]);
        assert_eq!(read_header(b"not an image"), Header::default());
    }

    #[test]
    fn reads_png_exif_chunk() {
        let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8], data: &[u8]| {
            p.extend_from_slice(&(data.len() as u32).to_be_bytes());
            p.extend_from_slice(kind);
            p.extend_from_slice(data);
            p.extend_from_slice(&[0; 4]);
        };
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&300u32.to_be_bytes());
        ihdr.extend_from_slice(&200u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(b"IHDR", &ihdr);
        chunk(b"eXIf", &tiff(8, false));
        chunk(b"IEND", &[]);
        assert_eq!(read_header(&p), Header { orientation: Some(8), width: 300, height: 200 });
    }

    #[test]
    fn reads_webp_exif_chunk() {
        let mut body = b"WEBP".to_vec();
        let mut vp8x = vec![0x08, 0, 0, 0];
        vp8x.extend_from_slice(&[0x3f, 0x01, 0x00, 0xc7, 0x00, 0x00]); // 320 x 200
        body.extend_from_slice(b"VP8X");
        body.extend_from_slice(&(vp8x.len() as u32).to_le_bytes());
        body.extend(vp8x);
        let t = tiff(3, true);
        body.extend_from_slice(b"EXIF");
        body.extend_from_slice(&(t.len() as u32).to_le_bytes());
        body.extend(t);
        let mut w = b"RIFF".to_vec();
        w.extend_from_slice(&(body.len() as u32).to_le_bytes());
        w.extend(body);
        assert_eq!(read_header(&w), Header { orientation: Some(3), width: 320, height: 200 });
    }

    /// Where the stored pixel at normalised `(sx, sy)` is displayed.
    fn displayed(o: u8, stored: Vec2) -> Vec2 {
        Orientation(o).display_to_stored().inverse().unwrap().apply(stored)
    }

    #[test]
    fn orientation_maps_the_stored_top_left_corner() {
        // Where the first stored pixel ends up, per the EXIF specification.
        let expect = [
            (1, v2(0.0, 0.0)),
            (2, v2(1.0, 0.0)),
            (3, v2(1.0, 1.0)),
            (4, v2(0.0, 1.0)),
            (5, v2(0.0, 0.0)),
            (6, v2(1.0, 0.0)),
            (7, v2(1.0, 1.0)),
            (8, v2(0.0, 1.0)),
        ];
        for (o, want) in expect {
            assert_eq!(displayed(o, v2(0.0, 0.0)), want, "orientation {o}");
        }
        // Orientation 6 sends the stored first row down the right-hand side.
        assert_eq!(displayed(6, v2(1.0, 0.0)), v2(1.0, 1.0));
        // Orientation 8 sends it up the left-hand side.
        assert_eq!(displayed(8, v2(1.0, 0.0)), v2(0.0, 0.0));
        // Orientation 5 sends it down the left-hand side.
        assert_eq!(displayed(5, v2(1.0, 0.0)), v2(0.0, 1.0));
    }

    #[test]
    fn display_size_swaps_for_sideways_images() {
        assert_eq!(Orientation(6).display_size(v2(4000.0, 3000.0)), v2(3000.0, 4000.0));
        assert_eq!(Orientation(3).display_size(v2(4000.0, 3000.0)), v2(4000.0, 3000.0));
        assert_eq!(Orientation::from_tag(0), Orientation::UP);
        assert_eq!(Orientation::from_tag(9), Orientation::UP);
    }

    #[test]
    fn residual_detects_a_decoder_that_already_rotated() {
        let stored = v2(4000.0, 3000.0);
        let o = Orientation(6);
        assert_eq!(o.residual(stored, v2(3000.0, 4000.0), false), Orientation::UP);
        assert_eq!(o.residual(stored, v2(4000.0, 3000.0), true), o);
        assert_eq!(Orientation(3).residual(stored, stored, true), Orientation::UP);
        assert_eq!(Orientation(3).residual(stored, stored, false), Orientation(3));
    }
}
