pub(crate) const COLOR_DODGE_SHADER: &str = r#"
@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var dest_texture: texture_2d<f32>;

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
    mask_origin: vec2<i32>,
    mask_fill: f32,
    _pad1: u32,
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

fn to_u8(value: f32) -> i32 {
    return i32(clamp(floor(value * 255.0 + 0.5), 0.0, 255.0));
}

fn div_round(numerator: i32, denominator: i32) -> i32 {
    return (numerator + denominator / 2) / denominator;
}

fn color_dodge_channel(dst: i32, src: i32) -> i32 {
    if (src >= 255) {
        return 255;
    }
    return min(255, (dst * 255) / max(255 - src, 1));
}

fn composite_channel(src: i32, dst: i32, blended: i32, src_a: i32, dst_a: i32, alpha_num: i32) -> i32 {
    let numerator =
        src * src_a * (255 - dst_a) +
        dst * dst_a * (255 - src_a) +
        blended * src_a * dst_a;
    return clamp(div_round(numerator, alpha_num), 0, 255);
}

fn load_source(texel: vec2<i32>) -> vec4<f32> {
    if (source_params.source_kind == 1u) {
        return source_params.color;
    }
    let source_texel = texel + source_params.target_origin - source_params.source_origin;
    let source_size = textureDimensions(source_texture);
    if (
        source_texel.x < 0 ||
        source_texel.y < 0 ||
        source_texel.x >= i32(source_size.x) ||
        source_texel.y >= i32(source_size.y)
    ) {
        return vec4<f32>(1.0, 1.0, 1.0, 0.0);
    }
    return textureLoad(source_texture, source_texel, 0);
}

fn load_mask(global_texel: vec2<i32>) -> f32 {
    let mask_texel = global_texel - source_params.mask_origin;
    let mask_size = textureDimensions(mask_texture);
    if (
        mask_texel.x < 0 ||
        mask_texel.y < 0 ||
        mask_texel.x >= i32(mask_size.x) ||
        mask_texel.y >= i32(mask_size.y)
    ) {
        return source_params.mask_fill;
    }
    return textureLoad(mask_texture, mask_texel, 0).r;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(position.xy);
    var src = load_source(texel);
    if (source_params.source_kind == 1u) {
        src = source_params.color;
    }
    let dst = textureLoad(dest_texture, texel, 0);

    var src_a = to_u8(src.a);
    if (source_params.has_mask == 1u) {
        src_a = (src_a * to_u8(load_mask(texel + source_params.target_origin))) / 255;
    }
    let opacity_u8 = i32(clamp(floor(source_params.opacity * 256.0 + 0.5), 0.0, 256.0));
    src_a = (src_a * opacity_u8) / 256;
    if (src_a == 0) {
        return dst;
    }

    let dst_a = to_u8(dst.a);
    let alpha_num = src_a * 255 + dst_a * (255 - src_a);
    let out_a = min(div_round(alpha_num, 255), 255);

    let src_r = to_u8(src.r);
    let src_g = to_u8(src.g);
    let src_b = to_u8(src.b);
    let dst_r = to_u8(dst.r);
    let dst_g = to_u8(dst.g);
    let dst_b = to_u8(dst.b);
    let out_r = composite_channel(src_r, dst_r, color_dodge_channel(dst_r, src_r), src_a, dst_a, alpha_num);
    let out_g = composite_channel(src_g, dst_g, color_dodge_channel(dst_g, src_g), src_a, dst_a, alpha_num);
    let out_b = composite_channel(src_b, dst_b, color_dodge_channel(dst_b, src_b), src_a, dst_a, alpha_num);

    return vec4<f32>(
        f32(out_r) / 255.0,
        f32(out_g) / 255.0,
        f32(out_b) / 255.0,
        f32(out_a) / 255.0,
    );
}
"#;

pub(crate) const COLOR_BURN_SHADER: &str = r#"
@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var dest_texture: texture_2d<f32>;

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
    mask_origin: vec2<i32>,
    mask_fill: f32,
    _pad1: u32,
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

fn to_u8(value: f32) -> i32 {
    return i32(clamp(floor(value * 255.0 + 0.5), 0.0, 255.0));
}

fn div_round(numerator: i32, denominator: i32) -> i32 {
    return (numerator + denominator / 2) / denominator;
}

fn color_burn_channel(dst: i32, src: i32) -> i32 {
    if (src <= 0) {
        return 0;
    }
    return 255 - min(255, ((255 - dst) * 255) / max(src, 1));
}

fn composite_channel(src: i32, dst: i32, blended: i32, src_a: i32, dst_a: i32, alpha_num: i32) -> i32 {
    let numerator =
        src * src_a * (255 - dst_a) +
        dst * dst_a * (255 - src_a) +
        blended * src_a * dst_a;
    return clamp(div_round(numerator, alpha_num), 0, 255);
}

fn load_source(texel: vec2<i32>) -> vec4<f32> {
    if (source_params.source_kind == 1u) {
        return source_params.color;
    }
    let source_texel = texel + source_params.target_origin - source_params.source_origin;
    let source_size = textureDimensions(source_texture);
    if (
        source_texel.x < 0 ||
        source_texel.y < 0 ||
        source_texel.x >= i32(source_size.x) ||
        source_texel.y >= i32(source_size.y)
    ) {
        return vec4<f32>(1.0, 1.0, 1.0, 0.0);
    }
    return textureLoad(source_texture, source_texel, 0);
}

fn load_mask(global_texel: vec2<i32>) -> f32 {
    let mask_texel = global_texel - source_params.mask_origin;
    let mask_size = textureDimensions(mask_texture);
    if (
        mask_texel.x < 0 ||
        mask_texel.y < 0 ||
        mask_texel.x >= i32(mask_size.x) ||
        mask_texel.y >= i32(mask_size.y)
    ) {
        return source_params.mask_fill;
    }
    return textureLoad(mask_texture, mask_texel, 0).r;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(position.xy);
    var src = load_source(texel);
    if (source_params.source_kind == 1u) {
        src = source_params.color;
    }
    let dst = textureLoad(dest_texture, texel, 0);

    var src_a = to_u8(src.a);
    if (source_params.has_mask == 1u) {
        src_a = (src_a * to_u8(load_mask(texel + source_params.target_origin))) / 255;
    }
    let opacity_u8 = i32(clamp(floor(source_params.opacity * 256.0 + 0.5), 0.0, 256.0));
    src_a = (src_a * opacity_u8) / 256;
    if (src_a == 0) {
        return dst;
    }

    let dst_a = to_u8(dst.a);
    let alpha_num = src_a * 255 + dst_a * (255 - src_a);
    let out_a = min(div_round(alpha_num, 255), 255);

    let src_r = to_u8(src.r);
    let src_g = to_u8(src.g);
    let src_b = to_u8(src.b);
    let dst_r = to_u8(dst.r);
    let dst_g = to_u8(dst.g);
    let dst_b = to_u8(dst.b);
    let out_r = composite_channel(src_r, dst_r, color_burn_channel(dst_r, src_r), src_a, dst_a, alpha_num);
    let out_g = composite_channel(src_g, dst_g, color_burn_channel(dst_g, src_g), src_a, dst_a, alpha_num);
    let out_b = composite_channel(src_b, dst_b, color_burn_channel(dst_b, src_b), src_a, dst_a, alpha_num);

    return vec4<f32>(
        f32(out_r) / 255.0,
        f32(out_g) / 255.0,
        f32(out_b) / 255.0,
        f32(out_a) / 255.0,
    );
}
"#;

pub(crate) const GLOW_DODGE_SHADER: &str = r#"
@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var dest_texture: texture_2d<f32>;

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
    mask_origin: vec2<i32>,
    mask_fill: f32,
    _pad1: u32,
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

fn to_u8(value: f32) -> i32 {
    return i32(clamp(floor(value * 255.0 + 0.5), 0.0, 255.0));
}

fn div_round_255(value: i32) -> i32 {
    return (value + 127) / 255;
}

fn quantize_u8(value: vec4<f32>) -> vec4<f32> {
    return floor(clamp(value, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + vec4<f32>(0.5)) / 255.0;
}

fn glow_dodge_channel(dst: i32, strength: i32) -> i32 {
    if (strength >= 255) {
        return 255;
    }
    return min(255, (dst * 255) / max(255 - strength, 1));
}

fn load_source(texel: vec2<i32>) -> vec4<f32> {
    if (source_params.source_kind == 1u) {
        return source_params.color;
    }
    let source_texel = texel + source_params.target_origin - source_params.source_origin;
    let source_size = textureDimensions(source_texture);
    if (
        source_texel.x < 0 ||
        source_texel.y < 0 ||
        source_texel.x >= i32(source_size.x) ||
        source_texel.y >= i32(source_size.y)
    ) {
        return vec4<f32>(1.0, 1.0, 1.0, 0.0);
    }
    return textureLoad(source_texture, source_texel, 0);
}

fn load_mask(global_texel: vec2<i32>) -> f32 {
    let mask_texel = global_texel - source_params.mask_origin;
    let mask_size = textureDimensions(mask_texture);
    if (
        mask_texel.x < 0 ||
        mask_texel.y < 0 ||
        mask_texel.x >= i32(mask_size.x) ||
        mask_texel.y >= i32(mask_size.y)
    ) {
        return source_params.mask_fill;
    }
    return textureLoad(mask_texture, mask_texel, 0).r;
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(position.xy);
    var src = load_source(texel);
    if (source_params.source_kind == 1u) {
        src = source_params.color;
    }
    let dst = textureLoad(dest_texture, texel, 0);

    var src_a = to_u8(src.a);
    if (source_params.has_mask == 1u) {
        src_a = (src_a * to_u8(load_mask(texel + source_params.target_origin))) / 255;
    }
    let opacity_u8 = i32(clamp(floor(source_params.opacity * 256.0 + 0.5), 0.0, 256.0));
    src_a = (src_a * opacity_u8) / 256;
    if (src_a == 0) {
        return dst;
    }

    let src_r = to_u8(src.r);
    let src_g = to_u8(src.g);
    let src_b = to_u8(src.b);
    let dst_r = to_u8(dst.r);
    let dst_g = to_u8(dst.g);
    let dst_b = to_u8(dst.b);
    let dst_a = to_u8(dst.a);

    let strength_r = div_round_255(src_r * src_a);
    let strength_g = div_round_255(src_g * src_a);
    let strength_b = div_round_255(src_b * src_a);
    let dodge = vec3<f32>(
        f32(glow_dodge_channel(dst_r, strength_r)) / 255.0,
        f32(glow_dodge_channel(dst_g, strength_g)) / 255.0,
        f32(glow_dodge_channel(dst_b, strength_b)) / 255.0,
    );

    let src_a_f = f32(src_a) / 255.0;
    let dst_a_f = f32(dst_a) / 255.0;
    let out_a = src_a_f + dst_a_f * (1.0 - src_a_f);
    if (out_a <= 0.0) {
        return vec4<f32>(1.0, 1.0, 1.0, 0.0);
    }
    let dst_blend = min(dst_a_f / out_a, 1.0);
    let src_rgb = vec3<f32>(f32(src_r), f32(src_g), f32(src_b)) / vec3<f32>(255.0);
    let out_pm = dodge * out_a * dst_blend + src_rgb * src_a_f * (1.0 - dst_blend);
    let out_rgb = out_pm / out_a;
    return quantize_u8(vec4<f32>(out_rgb, out_a));
}
"#;
