// Two photos, one line.
//
// Every output pixel is mapped into each photo through its own affine
// transform (pan, zoom, rotation and EXIF orientation, folded together on the
// CPU), and the two samples are mixed by the pixel's signed distance to the
// split line. Distances are in output pixels, so the edge is one pixel wide
// at any resolution and any angle.

struct Uniforms {
    // Rows of the 2x3 maps from output pixels to texture coordinates.
    uv_a: array<vec4f, 2>,
    uv_b: array<vec4f, 2>,
    // Rows of the 3x3 maps from each photo's linear RGB to the output's.
    color_a: array<vec4f, 3>,
    color_b: array<vec4f, 3>,
    // (nx, ny, k, half feather in px): signed distance d = n·p + k.
    line: vec4f,
    // Linear background colour; w > 0 draws stripes where a photo is missing.
    bg: vec4f,
    // (width, height, has A, has B) of the render target.
    target: vec4f,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var tex_a: texture_2d<f32>;
@group(0) @binding(2) var tex_b: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

struct VertexOut {
    @builtin(position) clip: vec4f,
    // Pixel coordinates, y down. Interpolated rather than taken from
    // @builtin(position) so WebGPU and the GL backend agree exactly.
    @location(0) px: vec2f,
}

// One triangle that covers the whole target: (-1,-1), (3,-1), (-1,3).
@vertex
fn vs(@builtin(vertex_index) i: u32) -> VertexOut {
    let xy = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VertexOut;
    out.clip = vec4f(xy * 2.0 - 1.0, 0.0, 1.0);
    out.px = vec2f(xy.x, 1.0 - xy.y) * u.target.xy;
    return out;
}

fn to_uv(m: array<vec4f, 2>, p: vec2f) -> vec2f {
    let h = vec3f(p, 1.0);
    return vec2f(dot(m[0].xyz, h), dot(m[1].xyz, h));
}

fn to_output(m: array<vec4f, 3>, c: vec3f) -> vec3f {
    return vec3f(dot(m[0].xyz, c), dot(m[1].xyz, c), dot(m[2].xyz, c));
}

// Coverage of the unit square in uv space, anti-aliased with the same
// footprint trick as the split so a rotated photo has clean borders.
fn inside(uv: vec2f) -> f32 {
    let fw = max(fwidth(uv), vec2f(1e-6));
    let d = min(uv, 1.0 - uv) / fw;
    return clamp(min(d.x, d.y) + 0.5, 0.0, 1.0);
}

// Soft diagonal stripes, standing in for a photo not loaded yet.
fn placeholder(p: vec2f) -> vec3f {
    let w = 0.5 + 0.5 * sin((p.x + p.y) * 0.18);
    return u.bg.rgb * mix(1.0, 0.82, w * u.bg.w);
}

fn srgb_encode(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4f {
    let p = in.px;
    let uva = to_uv(u.uv_a, p);
    let uvb = to_uv(u.uv_b, p);

    // Sample unconditionally: derivatives need uniform control flow.
    // The textures are sRGB-tagged, so these are linear values and the
    // hardware filters (and mips) in linear light.
    let ca = textureSample(tex_a, samp, uva).rgb;
    let cb = textureSample(tex_b, samp, uvb).rgb;
    let empty = placeholder(p);
    let a = mix(empty, to_output(u.color_a, ca), inside(uva) * u.target.z);
    let b = mix(empty, to_output(u.color_b, cb), inside(uvb) * u.target.w);

    // Signed distance to the line, in output pixels.
    let d = dot(u.line.xy, p) + u.line.z;
    // How much d changes across one pixel. d is already measured in pixels,
    // so this is 1, but taking it from the derivatives keeps the edge one
    // pixel wide if the mapping ever gains a scale.
    let footprint = length(vec2f(dpdx(d), dpdy(d)));
    // Half a pixel of anti-aliasing plus half the feather on each side.
    let half_width = 0.5 * footprint + u.line.w;
    let t = smoothstep(-half_width, half_width, d);

    let c = mix(a, b, t);
    return vec4f(srgb_encode(clamp(c, vec3f(0.0), vec3f(1.0))), 1.0);
}
