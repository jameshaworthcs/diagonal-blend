//! Embedded ICC profiles: find them, tell sRGB from Display P3, and write
//! small matrix/TRC profiles of our own for exports.

use crate::color::{D50_XYZ, Space, bradford_d65_to_d50, d50_colorants};

/// What an image's embedded profile says about its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detected {
    /// No profile: the web's default, sRGB.
    Untagged,
    Known(Space),
    /// A profile we do not handle ourselves (Adobe RGB, ProPhoto, CMYK...).
    /// The browser converts these to sRGB while decoding.
    Other,
}

impl Detected {
    pub fn code(self) -> i32 {
        match self {
            Detected::Untagged | Detected::Known(Space::Srgb) => 0,
            Detected::Known(Space::DisplayP3) => 1,
            Detected::Other => -1,
        }
    }
}

pub fn detect(file: &[u8]) -> Detected {
    match extract(file) {
        None => Detected::Untagged,
        Some(p) => classify(&p),
    }
}

/// Pull the ICC profile out of a JPEG, PNG or WebP file.
pub fn extract(b: &[u8]) -> Option<Vec<u8>> {
    if b.starts_with(&[0xff, 0xd8]) {
        jpeg(b)
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(b)
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        webp(b)
    } else {
        None
    }
}

fn jpeg(b: &[u8]) -> Option<Vec<u8>> {
    // APP2 "ICC_PROFILE\0" segments, each carrying (sequence, count).
    let mut parts: Vec<(u8, &[u8])> = Vec::new();
    let mut i = 2;
    while i + 4 <= b.len() && b[i] == 0xff {
        let marker = b[i + 1];
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        let end = (i + 2 + len).min(b.len());
        let body = &b[(i + 4).min(end)..end];
        if marker == 0xe2 && body.len() > 14 && body.starts_with(b"ICC_PROFILE\0") {
            parts.push((body[12], &body[14..]));
        }
        i += 2 + len.max(2);
    }
    if parts.is_empty() {
        return None;
    }
    parts.sort_by_key(|p| p.0);
    Some(parts.into_iter().flat_map(|p| p.1.iter().copied()).collect())
}

fn png(b: &[u8]) -> Option<Vec<u8>> {
    let mut i = 8;
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes(b[i..i + 4].try_into().ok()?) as usize;
        let kind = &b[i + 4..i + 8];
        let body = b.get(i + 8..(i + 8).checked_add(len)?)?;
        match kind {
            b"iCCP" => {
                let nul = body.iter().position(|&c| c == 0)?;
                let data = body.get(nul + 2..)?;
                return miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(data, 4 << 20).ok();
            }
            b"IDAT" | b"IEND" => return None,
            _ => {}
        }
        i += 12 + len;
    }
    None
}

fn webp(b: &[u8]) -> Option<Vec<u8>> {
    let mut i = 12;
    while i + 8 <= b.len() {
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().ok()?) as usize;
        let body = b.get(i + 8..(i + 8).checked_add(len)?)?;
        if &b[i..i + 4] == b"ICCP" {
            return Some(body.to_vec());
        }
        i += 8 + len + (len & 1);
    }
    None
}

fn be32(p: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(p.get(i..i + 4)?.try_into().ok()?))
}

fn s15f16(p: &[u8], i: usize) -> Option<f64> {
    Some(be32(p, i)? as i32 as f64 / 65536.0)
}

fn tag<'a>(p: &'a [u8], sig: &[u8; 4]) -> Option<&'a [u8]> {
    let count = be32(p, 128)? as usize;
    for k in 0..count.min(256) {
        let e = 132 + k * 12;
        if p.get(e..e + 4)? == sig {
            let off = be32(p, e + 4)? as usize;
            let len = be32(p, e + 8)? as usize;
            return p.get(off..off.checked_add(len)?);
        }
    }
    None
}

fn xyz_tag(p: &[u8], sig: &[u8; 4]) -> Option<[f64; 3]> {
    let t = tag(p, sig)?;
    if t.get(0..4)? != b"XYZ " {
        return None;
    }
    Some([s15f16(t, 8)?, s15f16(t, 12)?, s15f16(t, 16)?])
}

/// The profile's description as plain text (v2 `desc` or v4 `mluc`).
fn description(p: &[u8]) -> Option<String> {
    let t = tag(p, b"desc")?;
    match t.get(0..4)? {
        b"desc" => {
            let n = be32(t, 8)? as usize;
            let s = t.get(12..12 + n)?;
            Some(String::from_utf8_lossy(s).trim_end_matches('\0').to_string())
        }
        b"mluc" => {
            let len = be32(t, 20)? as usize;
            let off = be32(t, 24)? as usize;
            let s = t.get(off..off + len)?;
            let units: Vec<u16> = s.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            Some(String::from_utf16_lossy(&units))
        }
        _ => None,
    }
}

pub fn classify(p: &[u8]) -> Detected {
    if p.len() < 132 || p.get(36..40) != Some(b"acsp") || p.get(16..20) != Some(b"RGB ") {
        return Detected::Other;
    }
    if let (Some(r), Some(g), Some(b)) = (xyz_tag(p, b"rXYZ"), xyz_tag(p, b"gXYZ"), xyz_tag(p, b"bXYZ")) {
        let found = [r, g, b];
        for space in [Space::Srgb, Space::DisplayP3] {
            let want = d50_colorants(space);
            let ok = (0..3).all(|c| (0..3).all(|k| (found[c][k] - want[c][k]).abs() < 0.006));
            if ok {
                return Detected::Known(space);
            }
        }
        return Detected::Other;
    }
    // LUT-based profiles carry no colorants; fall back to the name.
    match description(p) {
        Some(d) if d.contains("P3") => Detected::Known(Space::DisplayP3),
        Some(d) if d.contains("sRGB") => Detected::Known(Space::Srgb),
        _ => Detected::Other,
    }
}

fn push_s15f16(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&((v * 65536.0).round() as i32).to_be_bytes());
}

fn xyz(v: [f64; 3]) -> Vec<u8> {
    let mut t = b"XYZ \0\0\0\0".to_vec();
    for c in v {
        push_s15f16(&mut t, c);
    }
    t
}

fn mluc(text: &str) -> Vec<u8> {
    let utf16: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
    let mut t = b"mluc\0\0\0\0".to_vec();
    t.extend_from_slice(&1u32.to_be_bytes());
    t.extend_from_slice(&12u32.to_be_bytes());
    t.extend_from_slice(b"enUS");
    t.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
    t.extend_from_slice(&28u32.to_be_bytes());
    t.extend(utf16);
    t
}

/// The sRGB transfer curve as an ICC parametric curve (function type 3).
fn srgb_curve() -> Vec<u8> {
    let mut t = b"para\0\0\0\0\0\x03\0\0".to_vec();
    for v in [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045] {
        push_s15f16(&mut t, v);
    }
    t
}

/// A compact ICC v4 display profile for `space` (about 550 bytes).
pub fn profile(space: Space) -> Vec<u8> {
    let [r, g, b] = d50_colorants(space);
    let chad = bradford_d65_to_d50();
    let mut chad_t = b"sf32\0\0\0\0".to_vec();
    for row in chad {
        for v in row {
            push_s15f16(&mut chad_t, v);
        }
    }
    let curve = srgb_curve();
    let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", mluc(space.name())),
        (b"cprt", mluc("No copyright, use freely")),
        (b"wtpt", xyz(D50_XYZ)),
        (b"chad", chad_t),
        (b"rXYZ", xyz(r)),
        (b"gXYZ", xyz(g)),
        (b"bXYZ", xyz(b)),
        (b"rTRC", curve.clone()),
        (b"gTRC", curve.clone()),
        (b"bTRC", curve),
    ];

    let mut table = Vec::new();
    let mut data: Vec<u8> = Vec::new();
    let data_start = 128 + 4 + 12 * tags.len();
    let mut curve_at: Option<(u32, u32)> = None;
    for (sig, body) in &tags {
        // The three TRC tags share one curve, as profiles usually do.
        let (off, len) = match (sig[1..] == *b"TRC", curve_at) {
            (true, Some(at)) => at,
            _ => {
                while data.len() % 4 != 0 {
                    data.push(0);
                }
                let at = ((data_start + data.len()) as u32, body.len() as u32);
                data.extend_from_slice(body);
                if sig[1..] == *b"TRC" {
                    curve_at = Some(at);
                }
                at
            }
        };
        table.extend_from_slice(*sig);
        table.extend_from_slice(&off.to_be_bytes());
        table.extend_from_slice(&len.to_be_bytes());
    }
    while data.len() % 4 != 0 {
        data.push(0);
    }

    let size = (data_start + data.len()) as u32;
    let mut p = Vec::with_capacity(size as usize);
    p.extend_from_slice(&size.to_be_bytes());
    p.extend_from_slice(&[0; 4]); // preferred CMM
    p.extend_from_slice(&[0x04, 0x30, 0, 0]); // version 4.3
    p.extend_from_slice(b"mntrRGB XYZ ");
    for v in [2026u16, 10, 13, 22, 0, 0] {
        p.extend_from_slice(&v.to_be_bytes());
    }
    p.extend_from_slice(b"acsp");
    p.extend_from_slice(&[0; 4 + 4 + 4 + 4 + 8 + 4]); // platform, flags, maker, model, attributes, intent
    for c in D50_XYZ {
        push_s15f16(&mut p, c);
    }
    p.extend_from_slice(&[0; 4 + 16 + 28]); // creator, profile id, reserved
    debug_assert_eq!(p.len(), 128);
    p.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    p.extend(table);
    p.extend(data);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_profiles_classify_as_themselves() {
        for space in [Space::Srgb, Space::DisplayP3] {
            let p = profile(space);
            assert_eq!(p.len() % 4, 0);
            assert_eq!(be32(&p, 0).unwrap() as usize, p.len());
            assert_eq!(classify(&p), Detected::Known(space));
            assert_eq!(description(&p).unwrap(), space.name());
        }
    }

    #[test]
    fn trc_tags_share_one_curve() {
        let p = profile(Space::DisplayP3);
        assert_eq!(tag(&p, b"rTRC").unwrap().as_ptr(), tag(&p, b"bTRC").unwrap().as_ptr());
        assert_eq!(&tag(&p, b"gTRC").unwrap()[0..4], b"para");
    }

    #[test]
    fn other_primaries_are_other() {
        let mut p = profile(Space::Srgb);
        // Swap in Adobe RGB's red colorant.
        let off = (0..10).map(|k| 132 + k * 12).find(|&e| &p[e..e + 4] == b"rXYZ").unwrap();
        let at = be32(&p, off + 4).unwrap() as usize + 8;
        p[at..at + 4].copy_from_slice(&((0.6097f64 * 65536.0) as i32).to_be_bytes());
        assert_eq!(classify(&p), Detected::Other);
        assert_eq!(classify(b"garbage"), Detected::Other);
    }

    fn jpeg_with_profile(p: &[u8], chunk: usize) -> Vec<u8> {
        let mut j = vec![0xff, 0xd8];
        let parts: Vec<&[u8]> = p.chunks(chunk).collect();
        // Write the chunks out of order; the reader must sort them.
        for (seq, part) in parts.iter().enumerate().rev() {
            j.extend_from_slice(&[0xff, 0xe2]);
            j.extend_from_slice(&((part.len() + 16) as u16).to_be_bytes());
            j.extend_from_slice(b"ICC_PROFILE\0");
            j.extend_from_slice(&[(seq + 1) as u8, parts.len() as u8]);
            j.extend_from_slice(part);
        }
        j.extend_from_slice(&[0xff, 0xda, 0, 2]);
        j
    }

    #[test]
    fn finds_a_profile_split_across_jpeg_segments() {
        let p = profile(Space::DisplayP3);
        let j = jpeg_with_profile(&p, 100);
        assert_eq!(extract(&j).unwrap(), p);
        assert_eq!(detect(&j), Detected::Known(Space::DisplayP3));
        assert_eq!(detect(&[0xff, 0xd8, 0xff, 0xda, 0, 2]), Detected::Untagged);
    }
}
