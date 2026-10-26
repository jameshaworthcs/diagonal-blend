//! AVIF from one AV1 key frame.
//!
//! Browsers cannot encode AVIF from a canvas, but they can encode AV1 with
//! WebCodecs. This module wraps one encoded key frame in the HEIF boxes that
//! make it a still image: the item, its size, its AV1 configuration (read
//! back out of the frame's sequence header), its colour, and an ICC profile.

use crate::color::Space;
use crate::icc;

const OBU_SEQUENCE_HEADER: u8 = 1;
const OBU_TEMPORAL_DELIMITER: u8 = 2;
const OBU_PADDING: u8 = 15;

/// One OBU: its type and its bytes, header included.
struct Obu<'a> {
    kind: u8,
    bytes: &'a [u8],
    payload: &'a [u8],
}

fn leb128(b: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut v = 0u64;
    for k in 0..8 {
        let byte = *b.get(i)?;
        v |= ((byte & 0x7f) as u64) << (7 * k);
        i += 1;
        if byte & 0x80 == 0 {
            return Some((v, i));
        }
    }
    None
}

/// Split a low-overhead bitstream into OBUs. Every OBU must carry a size.
fn obus(data: &[u8]) -> Option<Vec<Obu<'_>>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let start = i;
        let header = data[i];
        let kind = (header >> 3) & 0x0f;
        let has_extension = header & 0x04 != 0;
        if header & 0x02 == 0 {
            return None;
        }
        i += 1 + has_extension as usize;
        let (size, at) = leb128(data, i)?;
        let end = at.checked_add(size as usize)?;
        if end > data.len() {
            return None;
        }
        out.push(Obu { kind, bytes: &data[start..end], payload: &data[at..end] });
        i = end;
    }
    Some(out)
}

struct Bits<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Bits<'_> {
    fn f(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            let byte = *self.b.get(self.pos / 8)?;
            v = (v << 1) | ((byte >> (7 - self.pos % 8)) & 1) as u32;
            self.pos += 1;
        }
        Some(v)
    }
    fn flag(&mut self) -> Option<bool> {
        Some(self.f(1)? == 1)
    }
    fn uvlc(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while !self.flag()? {
            zeros += 1;
            if zeros >= 32 {
                return Some(u32::MAX);
            }
        }
        Some(self.f(zeros)? + (1u32 << zeros) - 1)
    }
}

/// The fields of an AV1 sequence header that `av1C` and `colr` repeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Sequence {
    pub profile: u8,
    pub level: u8,
    pub tier: u8,
    pub high_bitdepth: bool,
    pub twelve_bit: bool,
    pub monochrome: bool,
    pub subsampling_x: bool,
    pub subsampling_y: bool,
    pub chroma_sample_position: u8,
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
    pub full_range: bool,
}

/// Parse a sequence header OBU's payload (AV1 specification, 5.5).
pub fn parse_sequence_header(payload: &[u8]) -> Option<Sequence> {
    let mut r = Bits { b: payload, pos: 0 };
    let mut s = Sequence { primaries: 2, transfer: 2, matrix: 2, ..Default::default() };
    s.profile = r.f(3)? as u8;
    let _still_picture = r.flag()?;
    let reduced = r.flag()?;
    if reduced {
        s.level = r.f(5)? as u8;
    } else {
        let mut decoder_model = false;
        let mut buffer_delay_len = 0;
        if r.flag()? {
            // timing_info
            r.f(32)?;
            r.f(32)?;
            if r.flag()? {
                r.uvlc()?;
            }
            decoder_model = r.flag()?;
            if decoder_model {
                buffer_delay_len = r.f(5)? + 1;
                r.f(32)?;
                r.f(5)?;
                r.f(5)?;
            }
        }
        let initial_display_delay = r.flag()?;
        let points = r.f(5)? + 1;
        for i in 0..points {
            r.f(12)?;
            let level = r.f(5)? as u8;
            let tier = if level > 7 { r.f(1)? as u8 } else { 0 };
            if i == 0 {
                s.level = level;
                s.tier = tier;
            }
            if decoder_model && r.flag()? {
                r.f(buffer_delay_len)?;
                r.f(buffer_delay_len)?;
                r.f(1)?;
            }
            if initial_display_delay && r.flag()? {
                r.f(4)?;
            }
        }
    }
    let wbits = r.f(4)? + 1;
    let hbits = r.f(4)? + 1;
    r.f(wbits)?;
    r.f(hbits)?;
    if !reduced && r.flag()? {
        r.f(4)?;
        r.f(3)?;
    }
    r.f(3)?; // 128x128 superblocks, filter intra, intra edge filter
    if !reduced {
        r.f(4)?; // interintra, masked compound, warped motion, dual filter
        let order_hint = r.flag()?;
        if order_hint {
            r.f(2)?; // jnt comp, ref frame mvs
        }
        let force_screen_content = if r.flag()? { 2 } else { r.f(1)? };
        if force_screen_content > 0 && !r.flag()? {
            r.f(1)?;
        }
        if order_hint {
            r.f(3)?;
        }
    }
    r.f(3)?; // superres, cdef, restoration

    // color_config
    s.high_bitdepth = r.flag()?;
    if s.profile == 2 && s.high_bitdepth {
        s.twelve_bit = r.flag()?;
    }
    s.monochrome = if s.profile == 1 { false } else { r.flag()? };
    if r.flag()? {
        s.primaries = r.f(8)? as u8;
        s.transfer = r.f(8)? as u8;
        s.matrix = r.f(8)? as u8;
    }
    if s.monochrome {
        s.full_range = r.flag()?;
        s.subsampling_x = true;
        s.subsampling_y = true;
        return Some(s);
    }
    if s.primaries == 1 && s.transfer == 13 && s.matrix == 0 {
        s.full_range = true;
        return Some(s);
    }
    s.full_range = r.flag()?;
    match s.profile {
        0 => {
            s.subsampling_x = true;
            s.subsampling_y = true;
        }
        1 => {}
        _ => {
            if s.twelve_bit {
                s.subsampling_x = r.flag()?;
                s.subsampling_y = s.subsampling_x && r.flag()?;
            } else {
                s.subsampling_x = true;
            }
        }
    }
    if s.subsampling_x && s.subsampling_y {
        s.chroma_sample_position = r.f(2)? as u8;
    }
    Some(s)
}

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut b = vec![version];
    b.extend_from_slice(&flags.to_be_bytes()[1..]);
    b.extend_from_slice(body);
    bx(kind, &b)
}

fn av1c(s: &Sequence, sequence_header: &[u8]) -> Vec<u8> {
    let mut b = vec![
        0x81,
        (s.profile << 5) | (s.level & 0x1f),
        (s.tier << 7)
            | ((s.high_bitdepth as u8) << 6)
            | ((s.twelve_bit as u8) << 5)
            | ((s.monochrome as u8) << 4)
            | ((s.subsampling_x as u8) << 3)
            | ((s.subsampling_y as u8) << 2)
            | (s.chroma_sample_position & 3),
        0,
    ];
    b.extend_from_slice(sequence_header);
    bx(b"av1C", &b)
}

/// Wrap one AV1 key frame (a low-overhead bitstream with a sequence header)
/// as an AVIF still, tagged with `space` both as CICP and as an ICC profile.
pub fn avif(av1: &[u8], width: u32, height: u32, space: Space) -> Result<Vec<u8>, String> {
    let units = obus(av1).ok_or("malformed AV1 bitstream")?;
    let seq_obu = units.iter().find(|o| o.kind == OBU_SEQUENCE_HEADER).ok_or("no AV1 sequence header")?;
    let seq = parse_sequence_header(seq_obu.payload).ok_or("unreadable AV1 sequence header")?;
    // An image item holds a sync sample, which has no temporal delimiters.
    let data: Vec<u8> = units
        .iter()
        .filter(|o| o.kind != OBU_TEMPORAL_DELIMITER && o.kind != OBU_PADDING)
        .flat_map(|o| o.bytes.iter().copied())
        .collect();

    let ftyp = bx(b"ftyp", b"avif\0\0\0\0avifmif1miaf");
    let hdlr = full(b"hdlr", 0, 0, b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0\0");
    let pitm = full(b"pitm", 0, 0, &1u16.to_be_bytes());
    let infe = full(b"infe", 2, 0, b"\0\x01\0\0av01\0");
    let mut iinf_body = 1u16.to_be_bytes().to_vec();
    iinf_body.extend(infe);
    let iinf = full(b"iinf", 0, 0, &iinf_body);

    let mut ispe_body = width.to_be_bytes().to_vec();
    ispe_body.extend_from_slice(&height.to_be_bytes());
    let ispe = full(b"ispe", 0, 0, &ispe_body);
    // The primaries and transfer are what the pixels are, whatever the
    // encoder wrote into the bitstream (Firefox's always says BT.709); the
    // matrix and range describe the YUV encoding, so they come from it.
    let primaries: u16 = if space == Space::DisplayP3 { 12 } else { 1 };
    let mut nclx = b"nclx".to_vec();
    for v in [primaries, 13, seq.matrix as u16] {
        nclx.extend_from_slice(&v.to_be_bytes());
    }
    nclx.push((seq.full_range as u8) << 7);
    let mut prof = b"prof".to_vec();
    prof.extend(icc::profile(space));
    let bits = if seq.twelve_bit { 12 } else if seq.high_bitdepth { 10 } else { 8 };
    let channels: u8 = if seq.monochrome { 1 } else { 3 };
    let mut pixi_body = vec![channels];
    pixi_body.extend(std::iter::repeat_n(bits, channels as usize));
    let properties = [ispe, av1c(&seq, seq_obu.bytes), bx(b"colr", &nclx), bx(b"colr", &prof), full(b"pixi", 0, 0, &pixi_body)];
    let ipco = bx(b"ipco", &properties.concat());
    // ispe, av1C (essential), colr nclx, colr prof, pixi.
    let ipma = full(b"ipma", 0, 0, &[0, 0, 0, 1, 0, 1, 5, 0x01, 0x82, 0x03, 0x04, 0x05]);
    let iprp = bx(b"iprp", &[ipco, ipma].concat());

    let meta = |offset: u32| {
        let mut iloc = vec![0x44, 0x00]; // offset and length are 4 bytes, no base offset
        iloc.extend_from_slice(&1u16.to_be_bytes()); // one item
        iloc.extend_from_slice(&1u16.to_be_bytes()); // item 1
        iloc.extend_from_slice(&0u16.to_be_bytes()); // this file
        iloc.extend_from_slice(&1u16.to_be_bytes()); // one extent
        iloc.extend_from_slice(&offset.to_be_bytes());
        iloc.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let iloc = full(b"iloc", 0, 0, &iloc);
        full(b"meta", 0, 0, &[hdlr.clone(), pitm.clone(), iloc, iinf.clone(), iprp.clone()].concat())
    };
    let offset = (ftyp.len() + meta(0).len() + 8) as u32;
    let mut out = ftyp;
    out.extend(meta(offset));
    out.extend(bx(b"mdat", &data));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write bits MSB first, for building sequence headers.
    struct Writer(Vec<bool>);
    impl Writer {
        fn put(&mut self, v: u32, n: u32) -> &mut Self {
            for k in (0..n).rev() {
                self.0.push((v >> k) & 1 == 1);
            }
            self
        }
        fn bytes(&self) -> Vec<u8> {
            self.0.chunks(8).map(|c| c.iter().enumerate().fold(0u8, |b, (i, &bit)| b | ((bit as u8) << (7 - i)))).collect()
        }
    }

    /// A full (not reduced) sequence header: profile 0, level 13, 4:2:0,
    /// 8-bit, with colour description (P3 primaries, sRGB transfer, BT.709
    /// matrix), full range.
    fn sequence_header() -> Vec<u8> {
        let mut w = Writer(Vec::new());
        w.put(0, 3).put(0, 1).put(0, 1); // profile, still, reduced
        w.put(0, 1).put(0, 1); // timing info, initial display delay
        w.put(0, 5); // one operating point
        w.put(0, 12).put(13, 5).put(0, 1); // idc, level 13 (> 7 so a tier bit)
        w.put(11, 4).put(11, 4).put(1079, 12).put(1349, 12); // sizes
        w.put(0, 1); // frame id numbers
        w.put(0, 3).put(0, 4).put(1, 1).put(0, 2); // sb, filters; order hint on
        w.put(1, 1); // choose screen content tools (so SELECT)
        w.put(1, 1); // choose integer mv
        w.put(6, 3); // order hint bits
        w.put(0, 3); // superres, cdef, restoration
        w.put(0, 1).put(0, 1); // high bitdepth, mono
        w.put(1, 1).put(12, 8).put(13, 8).put(1, 8); // colour description
        w.put(1, 1).put(0, 2); // full range, chroma sample position
        w.put(1, 1); // film grain params present (ignored)
        w.bytes()
    }

    fn obu(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut o = vec![(kind << 3) | 0x02, payload.len() as u8];
        o.extend_from_slice(payload);
        o
    }

    #[test]
    fn reads_the_sequence_header() {
        let s = parse_sequence_header(&sequence_header()).unwrap();
        assert_eq!((s.profile, s.level, s.tier), (0, 13, 0));
        assert_eq!((s.primaries, s.transfer, s.matrix, s.full_range), (12, 13, 1, true));
        assert!(s.subsampling_x && s.subsampling_y && !s.monochrome && !s.high_bitdepth);
    }

    #[test]
    fn reads_chrome_s_real_header() {
        // The sequence header Chrome's WebCodecs AV1 encoder produced for a
        // 64x48 Display P3 frame.
        let s = parse_sequence_header(&[0, 0, 0, 2, 175, 247, 128, 52, 67, 3, 64, 97]);
        let s = s.unwrap();
        assert_eq!(s.profile, 0);
        assert!(s.subsampling_x && s.subsampling_y);
        assert_eq!((s.primaries, s.transfer, s.matrix, s.full_range), (12, 13, 1, true));
    }

    fn find<'a>(b: &'a [u8], path: &[&[u8; 4]]) -> Option<&'a [u8]> {
        let (head, rest) = path.split_first()?;
        let mut i = 0;
        while i + 8 <= b.len() {
            let size = u32::from_be_bytes(b[i..i + 4].try_into().ok()?) as usize;
            if &b[i + 4..i + 8] == *head {
                let mut body = &b[i + 8..i + size];
                if [b"meta", b"iloc", b"ipma", b"pitm", b"hdlr", b"iinf", b"ispe", b"pixi"].contains(head) {
                    body = &body[4..];
                }
                return if rest.is_empty() { Some(body) } else { find(body, rest) };
            }
            i += size.max(8);
        }
        None
    }

    #[test]
    fn builds_a_well_formed_avif() {
        let frame = [obu(OBU_TEMPORAL_DELIMITER, &[]), obu(OBU_SEQUENCE_HEADER, &sequence_header()), obu(6, &[9, 9, 9, 9])].concat();
        let out = avif(&frame, 1080, 1350, Space::DisplayP3).unwrap();
        assert_eq!(&out[4..12], b"ftypavif");
        let ispe = find(&out, &[b"meta", b"iprp", b"ipco", b"ispe"]).unwrap();
        assert_eq!(ispe, [0, 0, 4, 56, 0, 0, 5, 70]);
        let av1c = find(&out, &[b"meta", b"iprp", b"ipco", b"av1C"]).unwrap();
        assert_eq!(av1c[0..3], [0x81, 13, 0x0c]);
        assert_eq!(av1c[4] >> 3, OBU_SEQUENCE_HEADER);
        // iloc points at the mdat payload, which has no temporal delimiter.
        let iloc = find(&out, &[b"meta", b"iloc"]).unwrap();
        let off = u32::from_be_bytes(iloc[10..14].try_into().unwrap()) as usize;
        let len = u32::from_be_bytes(iloc[14..18].try_into().unwrap()) as usize;
        let mdat = find(&out, &[b"mdat"]).unwrap();
        assert_eq!(&out[off..off + len], mdat);
        assert_eq!(mdat[0] >> 3, OBU_SEQUENCE_HEADER);
        assert_eq!(off + len, out.len());
        // Both colour boxes: CICP from the bitstream, and the ICC profile.
        let ipco = find(&out, &[b"meta", b"iprp", b"ipco"]).unwrap();
        let mut colr = Vec::new();
        let mut i = 0;
        while i + 8 <= ipco.len() {
            let size = u32::from_be_bytes(ipco[i..i + 4].try_into().unwrap()) as usize;
            if &ipco[i + 4..i + 8] == b"colr" {
                colr.push(&ipco[i + 8..i + size]);
            }
            i += size;
        }
        assert_eq!(colr.len(), 2);
        assert_eq!(colr[0], b"nclx\0\x0c\0\x0d\0\x01\x80");
        assert_eq!(&colr[1][..4], b"prof");
        assert_eq!(icc::classify(&colr[1][4..]), icc::Detected::Known(Space::DisplayP3));
    }

    #[test]
    fn container_colour_wins_over_the_bitstream() {
        let frame = obu(OBU_SEQUENCE_HEADER, &sequence_header());
        let out = avif(&frame, 1080, 1350, Space::Srgb).unwrap();
        let at = out.windows(4).position(|w| w == b"nclx").unwrap();
        assert_eq!(&out[at + 4..at + 11], b"\0\x01\0\x0d\0\x01\x80");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(avif(&[0xff, 0x00], 1, 1, Space::Srgb).is_err());
        assert!(avif(&obu(6, &[1, 2, 3]), 1, 1, Space::Srgb).is_err());
        assert!(parse_sequence_header(&[0]).is_none());
    }
}
