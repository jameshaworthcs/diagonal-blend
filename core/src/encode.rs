//! Still-image containers: PNG written here, WebP from the browser with our
//! ICC profile spliced in.

use crate::color::Space;
use crate::exif::read_header;
use crate::icc;

/// Encode RGBA8 pixels as a PNG tagged with `space`'s ICC profile.
pub fn png(rgba: &[u8], width: u32, height: u32, space: Space) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut info = png::Info::with_size(width, height);
    info.color_type = png::ColorType::Rgba;
    info.bit_depth = png::BitDepth::Eight;
    info.icc_profile = Some(icc::profile(space).into());
    let mut enc = png::Encoder::with_info(&mut out, info).map_err(|e| e.to_string())?;
    enc.set_compression(png::Compression::Fast);
    let opaque = rgba.chunks_exact(4).all(|p| p[3] == 255);
    if opaque {
        // Drop the alpha channel: a quarter smaller and nothing lost.
        enc.set_color(png::ColorType::Rgb);
        let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&rgb).map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
    } else {
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(rgba).map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(kind);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
}

/// Rewrite a WebP so that it carries `space`'s ICC profile. Simple-format
/// files (a lone VP8 or VP8L chunk) are promoted to the extended format,
/// which is the only one that can hold a profile.
pub fn webp_with_icc(webp: &[u8], space: Space) -> Result<Vec<u8>, String> {
    if webp.len() < 20 || &webp[0..4] != b"RIFF" || &webp[8..12] != b"WEBP" {
        return Err("not a WebP file".into());
    }
    let mut chunks: Vec<([u8; 4], &[u8])> = Vec::new();
    let mut i = 12;
    while i + 8 <= webp.len() {
        let kind: [u8; 4] = webp[i..i + 4].try_into().unwrap();
        let len = u32::from_le_bytes(webp[i + 4..i + 8].try_into().unwrap()) as usize;
        let body = webp.get(i + 8..i + 8 + len).ok_or("truncated WebP chunk")?;
        chunks.push((kind, body));
        i += 8 + len + (len & 1);
    }

    let mut vp8x: [u8; 10] = match chunks.first() {
        Some((k, b)) if k == b"VP8X" && b.len() >= 10 => b[0..10].try_into().unwrap(),
        Some((k, b)) if k == b"VP8 " || k == b"VP8L" => {
            let h = read_header(webp);
            if h.width == 0 {
                return Err("unreadable WebP frame header".into());
            }
            let mut x = [0u8; 10];
            if k == b"VP8L" && b.len() >= 5 && (b[4] >> 4) & 1 == 1 {
                x[0] |= 0x10; // alpha
            }
            x[4..7].copy_from_slice(&(h.width - 1).to_le_bytes()[0..3]);
            x[7..10].copy_from_slice(&(h.height - 1).to_le_bytes()[0..3]);
            x
        }
        _ => return Err("unexpected first WebP chunk".into()),
    };
    vp8x[0] |= 0x20; // ICC profile present

    let mut body = b"WEBP".to_vec();
    chunk(&mut body, b"VP8X", &vp8x);
    chunk(&mut body, b"ICCP", &icc::profile(space));
    for (kind, data) in chunks.iter().filter(|(k, _)| k != b"VP8X" && k != b"ICCP") {
        chunk(&mut body, kind, data);
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(body);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icc::{Detected, detect};

    #[test]
    fn png_round_trips_pixels_and_profile() {
        let px: Vec<u8> = (0..4 * 6).map(|i| (i * 11) as u8).collect();
        let out = png(&px, 3, 2, Space::DisplayP3).unwrap();
        assert_eq!(detect(&out), Detected::Known(Space::DisplayP3));
        let dec = png::Decoder::new(std::io::Cursor::new(&out));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut buf).unwrap();
        assert_eq!(buf, px);
    }

    #[test]
    fn opaque_png_drops_alpha() {
        let px = [10u8, 20, 30, 255, 40, 50, 60, 255];
        let out = png(&px, 2, 1, Space::Srgb).unwrap();
        let r = png::Decoder::new(std::io::Cursor::new(&out)).read_info().unwrap();
        assert_eq!(r.info().color_type, png::ColorType::Rgb);
    }

    /// A minimal lossy WebP: 3-byte frame tag, start code, 14-bit sizes.
    fn simple_webp(w: u16, h: u16) -> Vec<u8> {
        let mut vp8 = vec![0x30, 0x01, 0x00, 0x9d, 0x01, 0x2a];
        vp8.extend_from_slice(&w.to_le_bytes());
        vp8.extend_from_slice(&h.to_le_bytes());
        vp8.extend_from_slice(&[1, 2, 3]); // odd length, so it gets padded
        let mut body = b"WEBP".to_vec();
        chunk(&mut body, b"VP8 ", &vp8);
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    #[test]
    fn simple_webp_is_promoted_and_tagged() {
        let src = simple_webp(1080, 1350);
        let out = webp_with_icc(&src, Space::DisplayP3).unwrap();
        assert_eq!(&out[12..16], b"VP8X");
        assert_eq!(out[20] & 0x20, 0x20);
        let h = read_header(&out);
        assert_eq!((h.width, h.height), (1080, 1350));
        assert_eq!(detect(&out), Detected::Known(Space::DisplayP3));
        assert_eq!(u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize, out.len() - 8);
        // Tagging again replaces the profile rather than adding a second one.
        let again = webp_with_icc(&out, Space::Srgb).unwrap();
        assert_eq!(detect(&again), Detected::Known(Space::Srgb));
        assert_eq!(again.windows(4).filter(|w| w == b"ICCP").count(), 1);
    }

    #[test]
    fn rejects_non_webp() {
        assert!(webp_with_icc(b"RIFF\0\0\0\0WAVEfmt ", Space::Srgb).is_err());
    }
}
