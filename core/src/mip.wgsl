// Box-filter one mip level from the one above it. Both views are sRGB, so
// the average is taken in linear light.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VertexOut {
    let xy = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VertexOut;
    out.clip = vec4f(xy * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2f(xy.x, 1.0 - xy.y);
    return out;
}

@fragment
fn fs(in: VertexOut) -> @location(0) vec4f {
    return textureSampleLevel(src, samp, in.uv, 0.0);
}
