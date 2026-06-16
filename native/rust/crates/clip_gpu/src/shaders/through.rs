pub(crate) const THROUGH_GROUP_RESOLVE_SHADER: &str = r#"
@group(0) @binding(0)
var after_texture: texture_2d<f32>;

@group(0) @binding(1)
var before_texture: texture_2d<f32>;

@group(0) @binding(2)
var mask_texture: texture_2d<f32>;

struct SourceParams {
    color: vec4<f32>,
    opacity: f32,
    source_kind: u32,
    has_mask: u32,
    _pad0: u32,
    source_origin: vec2<i32>,
    target_origin: vec2<i32>,
};

@group(0) @binding(3)
var<uniform> source_params: SourceParams;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );
    var out: VertexOut;
    out.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(position.xy);
    let before = textureLoad(before_texture, texel, 0);
    let after_texel = texel + source_params.target_origin - source_params.source_origin;
    let after_size = textureDimensions(after_texture);
    var after = before;
    if (
        after_texel.x >= 0 &&
        after_texel.y >= 0 &&
        after_texel.x < i32(after_size.x) &&
        after_texel.y < i32(after_size.y)
    ) {
        after = textureLoad(after_texture, after_texel, 0);
    }
    var strength = clamp(source_params.opacity, 0.0, 1.0);
    if (source_params.has_mask == 1u) {
        strength = strength * textureLoad(mask_texture, texel + source_params.target_origin, 0).r;
    }

    let before_pm = before.rgb * before.a;
    let after_pm = after.rgb * after.a;
    let out_alpha = before.a * (1.0 - strength) + after.a * strength;
    let out_pm = before_pm * (1.0 - strength) + after_pm * strength;
    var out_rgb = vec3<f32>(1.0);
    if (out_alpha > 0.0) {
        out_rgb = out_pm / out_alpha;
    }
    return vec4<f32>(out_rgb, out_alpha);
}
"#;
