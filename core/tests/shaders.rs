//! The shaders must validate, and must translate to GLSL ES 3.00, which is
//! what the WebGL2 backend runs.

use naga::back::glsl;
use naga::valid::{Capabilities, ValidationFlags, Validator};

fn check(name: &str, src: &str, entries: &[(naga::ShaderStage, &str)]) {
    let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(src)));
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    for &(stage, entry) in entries {
        let options = glsl::Options {
            version: glsl::Version::Embedded { version: 300, is_webgl: true },
            ..Default::default()
        };
        let pipeline = glsl::PipelineOptions { shader_stage: stage, entry_point: entry.into(), multiview: None };
        let mut out = String::new();
        glsl::Writer::new(&mut out, &module, &info, &options, &pipeline, naga::proc::BoundsCheckPolicies::default())
            .and_then(|mut w| w.write())
            .unwrap_or_else(|e| panic!("{name} {entry}: {e:?}"));
        assert!(out.starts_with("#version 300 es"));
    }
}

#[test]
fn blend_shader_validates_and_translates_to_webgl2() {
    let src = include_str!("../src/blend.wgsl");
    check("blend", src, &[(naga::ShaderStage::Vertex, "vs"), (naga::ShaderStage::Fragment, "fs")]);
}

#[test]
fn mip_shader_validates_and_translates_to_webgl2() {
    let src = include_str!("../src/mip.wgsl");
    check("mip", src, &[(naga::ShaderStage::Vertex, "vs"), (naga::ShaderStage::Fragment, "fs")]);
}

#[test]
fn shaders_avoid_wgsl_reserved_words() {
    // naga accepts some words that browsers' WGSL front ends reserve.
    for src in [include_str!("../src/blend.wgsl"), include_str!("../src/mip.wgsl")] {
        for word in ["target", "sampler_state", "texture_state", "filter", "module", "namespace", "self", "type"] {
            let code: String = src.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n");
            let found = code
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|w| w == word);
            assert!(!found, "reserved word `{word}` in a shader");
        }
    }
}
