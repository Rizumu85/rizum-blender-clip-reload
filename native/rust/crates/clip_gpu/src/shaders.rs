mod lut_filter;
mod through;

pub(crate) use lut_filter::LUT_FILTER_SHADER;
pub(crate) use through::THROUGH_GROUP_RESOLVE_SHADER;

pub(crate) const COPY_RASTER_SHADER: &str = r#"
@group(0) @binding(0)
var source_texture: texture_2d<f32>;

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
    return textureLoad(source_texture, vec2<i32>(position.xy), 0);
}
"#;

pub(crate) const NORMAL_ALPHA_OVER_SHADER: &str = r#"
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
    blend_kind: u32,
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

fn quantize_u8(value: vec4<f32>) -> vec4<f32> {
    return floor(clamp(value, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + vec4<f32>(0.5)) / 255.0;
}

fn quantize_rgb_u8(value: vec3<f32>) -> vec3<f32> {
    return floor(clamp(value, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 + vec3<f32>(0.5)) / 255.0;
}

fn min3(value: vec3<f32>) -> f32 {
    return min(value.r, min(value.g, value.b));
}

fn max3(value: vec3<f32>) -> f32 {
    return max(value.r, max(value.g, value.b));
}

fn lum(value: vec3<f32>) -> f32 {
    return 0.3 * value.r + 0.59 * value.g + 0.11 * value.b;
}

fn color_compare_lum(value: vec3<f32>) -> f32 {
    return 0.2126 * value.r + 0.7152 * value.g + 0.0722 * value.b;
}

fn sat(value: vec3<f32>) -> f32 {
    return max3(value) - min3(value);
}

fn set_lum(value: vec3<f32>, target_lum: f32) -> vec3<f32> {
    var out = value + vec3<f32>(target_lum - lum(value));
    let out_lum = lum(out);
    let out_min = min3(out);
    let out_max = max3(out);
    if (out_min < 0.0) {
        out = vec3<f32>(out_lum) + (out - vec3<f32>(out_lum)) *
            (out_lum / max(out_lum - out_min, 0.000001));
    }
    if (out_max > 1.0) {
        out = vec3<f32>(out_lum) + (out - vec3<f32>(out_lum)) *
            ((1.0 - out_lum) / max(out_max - out_lum, 0.000001));
    }
    return out;
}

fn set_sat(value: vec3<f32>, target_sat: f32) -> vec3<f32> {
    let value_min = min3(value);
    let span = max3(value) - value_min;
    if (span <= 0.0) {
        return vec3<f32>(0.0);
    }
    var rel = (value - vec3<f32>(value_min)) / max(span, 0.000001);
    if (span <= (2.0 / 255.0)) {
        rel = floor(rel + vec3<f32>(0.000001));
    }
    return rel * target_sat;
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

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(position.xy);
    var src = load_source(texel);
    if (source_params.source_kind == 1u) {
        src = source_params.color;
    }
    let dst = textureLoad(dest_texture, texel, 0);
    src.a = clamp(src.a * source_params.opacity, 0.0, 1.0);
    if (source_params.has_mask == 1u) {
        src.a = src.a * textureLoad(mask_texture, texel + source_params.target_origin, 0).r;
    }
    let out_alpha = src.a + dst.a * (1.0 - src.a);
    var out_rgb = dst.rgb;
    if (out_alpha > 0.0) {
        out_rgb = (src.rgb * src.a + dst.rgb * dst.a * (1.0 - src.a)) / out_alpha;
    }
    return quantize_u8(vec4<f32>(out_rgb, out_alpha));
}
"#;

pub(crate) const CLIPPED_NORMAL_PRESERVE_SHADER: &str = r#"
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
    blend_kind: u32,
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

fn quantize_u8(value: vec4<f32>) -> vec4<f32> {
    return floor(clamp(value, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + vec4<f32>(0.5)) / 255.0;
}

fn quantize_rgb_u8(value: vec3<f32>) -> vec3<f32> {
    return floor(clamp(value, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 + vec3<f32>(0.5)) / 255.0;
}

fn min3(value: vec3<f32>) -> f32 {
    return min(value.r, min(value.g, value.b));
}

fn max3(value: vec3<f32>) -> f32 {
    return max(value.r, max(value.g, value.b));
}

fn lum(value: vec3<f32>) -> f32 {
    return 0.3 * value.r + 0.59 * value.g + 0.11 * value.b;
}

fn color_compare_lum(value: vec3<f32>) -> f32 {
    return 0.2126 * value.r + 0.7152 * value.g + 0.0722 * value.b;
}

fn sat(value: vec3<f32>) -> f32 {
    return max3(value) - min3(value);
}

fn set_lum(value: vec3<f32>, target_lum: f32) -> vec3<f32> {
    var out = value + vec3<f32>(target_lum - lum(value));
    let out_lum = lum(out);
    let out_min = min3(out);
    let out_max = max3(out);
    if (out_min < 0.0) {
        out = vec3<f32>(out_lum) + (out - vec3<f32>(out_lum)) *
            (out_lum / max(out_lum - out_min, 0.000001));
    }
    if (out_max > 1.0) {
        out = vec3<f32>(out_lum) + (out - vec3<f32>(out_lum)) *
            ((1.0 - out_lum) / max(out_max - out_lum, 0.000001));
    }
    return out;
}

fn set_sat(value: vec3<f32>, target_sat: f32) -> vec3<f32> {
    let value_min = min3(value);
    let span = max3(value) - value_min;
    if (span <= 0.0) {
        return vec3<f32>(0.0);
    }
    var rel = (value - vec3<f32>(value_min)) / max(span, 0.000001);
    if (span <= (2.0 / 255.0)) {
        rel = floor(rel + vec3<f32>(0.000001));
    }
    return rel * target_sat;
}

fn vivid_light_channel(src: f32, dst: f32) -> f32 {
    if (src < 0.5) {
        if (dst >= 1.0) {
            return 1.0;
        }
        if (2.0 * src <= 0.0) {
            return 0.0;
        }
        return 1.0 - min((1.0 - dst) / max(2.0 * src, 0.000001), 1.0);
    }
    let dodge_src = 2.0 * (src - 0.5);
    if (dst <= 0.0) {
        return 0.0;
    }
    if (dodge_src >= 1.0) {
        return 1.0;
    }
    return min(dst / max(1.0 - dodge_src, 0.000001), 1.0);
}

fn soft_light_channel(src: f32, dst: f32) -> f32 {
    if (src < 0.5) {
        return dst - (1.0 - 2.0 * src) * dst * (1.0 - dst);
    }
    var curve = sqrt(dst);
    if (dst < 0.25) {
        curve = ((16.0 * dst - 12.0) * dst + 4.0) * dst;
    }
    return dst + (2.0 * src - 1.0) * (curve - dst);
}

fn hsl_blend(src: vec3<f32>, dst: vec3<f32>) -> vec3<f32> {
    if (source_params.blend_kind == 23u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum(set_sat(src_q, sat(dst_q)), lum(dst_q));
    }
    if (source_params.blend_kind == 24u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum(set_sat(dst_q, sat(src_q)), lum(dst_q));
    }
    if (source_params.blend_kind == 26u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum(dst_q, lum(src_q));
    }
    return set_lum(src, lum(dst));
}

fn blend_rgb(src: vec3<f32>, dst: vec3<f32>) -> vec3<f32> {
    if (source_params.blend_kind == 0u) {
        return src;
    }
    if (source_params.blend_kind == 1u) {
        return min(src, dst);
    }
    if (source_params.blend_kind == 2u) {
        return src * dst;
    }
    if (source_params.blend_kind == 4u) {
        return max(src + dst - vec3<f32>(1.0), vec3<f32>(0.0));
    }
    if (source_params.blend_kind == 5u) {
        return max(dst - src, vec3<f32>(0.0));
    }
    if (source_params.blend_kind == 6u) {
        if (color_compare_lum(src) < color_compare_lum(dst)) {
            return src;
        }
        return dst;
    }
    if (source_params.blend_kind == 7u) {
        return max(src, dst);
    }
    if (source_params.blend_kind == 8u) {
        return vec3<f32>(1.0) - (vec3<f32>(1.0) - src) * (vec3<f32>(1.0) - dst);
    }
    if (source_params.blend_kind == 11u) {
        return min(src + dst, vec3<f32>(1.0));
    }
    if (source_params.blend_kind == 13u) {
        if (color_compare_lum(src) > color_compare_lum(dst)) {
            return src;
        }
        return dst;
    }
    if (source_params.blend_kind == 14u) {
        return select(
            vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - src) * (vec3<f32>(1.0) - dst),
            2.0 * src * dst,
            dst < vec3<f32>(0.5),
        );
    }
    if (source_params.blend_kind == 15u) {
        return vec3<f32>(
            soft_light_channel(src.r, dst.r),
            soft_light_channel(src.g, dst.g),
            soft_light_channel(src.b, dst.b),
        );
    }
    if (source_params.blend_kind == 16u) {
        return select(
            vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - src) * (vec3<f32>(1.0) - dst),
            2.0 * src * dst,
            src < vec3<f32>(0.5),
        );
    }
    if (source_params.blend_kind == 18u) {
        return clamp(2.0 * src + dst - vec3<f32>(1.0), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if (source_params.blend_kind == 19u) {
        return select(
            max(dst, 2.0 * (src - vec3<f32>(0.5))),
            min(dst, 2.0 * src),
            src < vec3<f32>(0.5),
        );
    }
    if (source_params.blend_kind == 21u) {
        return abs(src - dst);
    }
    if (source_params.blend_kind == 22u) {
        return src + dst - 2.0 * src * dst;
    }
    if (source_params.blend_kind == 23u || source_params.blend_kind == 24u || source_params.blend_kind == 25u || source_params.blend_kind == 26u) {
        return hsl_blend(src, dst);
    }
    if (source_params.blend_kind == 36u) {
        return min(dst / max(src, vec3<f32>(0.000001)), vec3<f32>(1.0));
    }
    let vivid = vec3<f32>(
        vivid_light_channel(src.r, dst.r),
        vivid_light_channel(src.g, dst.g),
        vivid_light_channel(src.b, dst.b),
    );
    if (source_params.blend_kind == 20u) {
        return select(vec3<f32>(0.0), vec3<f32>(1.0), vivid >= vec3<f32>(127.0 / 255.0));
    }
    return vivid;
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

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(position.xy);
    var src = load_source(texel);
    if (source_params.source_kind == 1u) {
        src = source_params.color;
    }
    let dst = textureLoad(dest_texture, texel, 0);
    src.a = clamp(src.a * source_params.opacity, 0.0, 1.0);
    if (source_params.has_mask == 1u) {
        src.a = src.a * textureLoad(mask_texture, texel + source_params.target_origin, 0).r;
    }
    var strength = src.a;
    if (dst.a <= 0.0) {
        strength = 0.0;
    }
    let blended = quantize_rgb_u8(blend_rgb(src.rgb, dst.rgb));
    let out_rgb = blended * strength + dst.rgb * (1.0 - strength);
    return quantize_u8(vec4<f32>(out_rgb, dst.a));
}
"#;

pub(crate) const ADD_GLOW_SHADER: &str = r#"
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

fn div255(value: i32) -> i32 {
    return value / 255;
}

fn add_glow_channel(dst: i32, src: i32, src_a: i32, dst_a: i32) -> i32 {
    var rgb = dst + src;
    if (src_a < 255) {
        let b = div255(dst_a * (255 - src_a));
        let denom = max(b + src_a, 1);
        rgb = (b * dst + rgb * src_a) / denom;
    }
    rgb = min(rgb, 255);

    if (dst_a <= 254) {
        let inv_dst_a = 255 - dst_a;
        if (src_a == 255) {
            rgb = div255(inv_dst_a * src + rgb * dst_a);
        } else {
            let b = div255(inv_dst_a * src_a);
            let denom = max(dst_a + b, 1);
            rgb = (b * src + rgb * dst_a) / denom;
        }
    }

    return clamp(rgb, 0, 255);
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
        src_a = div255(src_a * to_u8(textureLoad(mask_texture, texel + source_params.target_origin, 0).r));
    }
    let opacity_u8 = i32(clamp(floor(source_params.opacity * 256.0 + 0.5), 0.0, 256.0));
    src_a = (src_a * opacity_u8) / 256;
    if (src_a == 0) {
        return dst;
    }

    let dst_a = to_u8(dst.a);
    let out_a = min(div255((255 - src_a) * dst_a) + src_a, 255);
    let out_r = add_glow_channel(to_u8(dst.r), to_u8(src.r), src_a, dst_a);
    let out_g = add_glow_channel(to_u8(dst.g), to_u8(src.g), src_a, dst_a);
    let out_b = add_glow_channel(to_u8(dst.b), to_u8(src.b), src_a, dst_a);
    return vec4<f32>(
        f32(out_r) / 255.0,
        f32(out_g) / 255.0,
        f32(out_b) / 255.0,
        f32(out_a) / 255.0,
    );
}
"#;

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
        src_a = (src_a * to_u8(textureLoad(mask_texture, texel + source_params.target_origin, 0).r)) / 255;
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
        src_a = (src_a * to_u8(textureLoad(mask_texture, texel + source_params.target_origin, 0).r)) / 255;
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
        src_a = (src_a * to_u8(textureLoad(mask_texture, texel + source_params.target_origin, 0).r)) / 255;
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

pub(crate) const CLIPPED_BYTE_PRESERVE_SHADER: &str = r#"
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
    blend_kind: u32,
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

fn to_u8(value: f32) -> i32 {
    return i32(clamp(floor(value * 255.0 + 0.5), 0.0, 255.0));
}

fn div255(value: i32) -> i32 {
    return value / 255;
}

fn div_round_255(value: i32) -> i32 {
    return (value + 127) / 255;
}

fn add_glow_channel(dst: i32, src: i32, src_a: i32, dst_a: i32) -> i32 {
    var rgb = dst + src;
    if (src_a < 255) {
        let b = div255(dst_a * (255 - src_a));
        let denom = max(b + src_a, 1);
        rgb = (b * dst + rgb * src_a) / denom;
    }
    rgb = min(rgb, 255);

    if (dst_a <= 254) {
        let inv_dst_a = 255 - dst_a;
        if (src_a == 255) {
            rgb = div255(inv_dst_a * src + rgb * dst_a);
        } else {
            let b = div255(inv_dst_a * src_a);
            let denom = max(dst_a + b, 1);
            rgb = (b * src + rgb * dst_a) / denom;
        }
    }

    return clamp(rgb, 0, 255);
}

fn color_dodge_channel(dst: i32, src: i32) -> i32 {
    if (src >= 255) {
        return 255;
    }
    return min(255, (dst * 255) / max(255 - src, 1));
}

fn color_burn_channel(dst: i32, src: i32) -> i32 {
    if (src <= 0) {
        return 0;
    }
    return 255 - min(255, ((255 - dst) * 255) / max(src, 1));
}

fn glow_dodge_channel(dst: i32, strength: i32) -> i32 {
    if (strength >= 255) {
        return 255;
    }
    return min(255, (dst * 255) / max(255 - strength, 1));
}

fn preserve_channel(dst: i32, blended: i32, src_a: i32) -> i32 {
    return clamp(div_round_255(dst * (255 - src_a) + blended * src_a), 0, 255);
}

fn clipped_byte_channel(src: i32, dst: i32, src_a: i32) -> i32 {
    if (source_params.blend_kind == 12u) {
        return add_glow_channel(dst, src, src_a, 255);
    }
    if (source_params.blend_kind == 9u) {
        return preserve_channel(dst, color_dodge_channel(dst, src), src_a);
    }
    if (source_params.blend_kind == 3u) {
        return preserve_channel(dst, color_burn_channel(dst, src), src_a);
    }
    if (source_params.blend_kind == 10u) {
        return glow_dodge_channel(dst, div_round_255(src * src_a));
    }
    return dst;
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
        src_a = (src_a * to_u8(textureLoad(mask_texture, texel + source_params.target_origin, 0).r)) / 255;
    }
    let opacity_u8 = i32(clamp(floor(source_params.opacity * 256.0 + 0.5), 0.0, 256.0));
    src_a = (src_a * opacity_u8) / 256;

    let dst_a = to_u8(dst.a);
    if (src_a == 0 || dst_a == 0) {
        return dst;
    }

    let src_r = to_u8(src.r);
    let src_g = to_u8(src.g);
    let src_b = to_u8(src.b);
    let dst_r = to_u8(dst.r);
    let dst_g = to_u8(dst.g);
    let dst_b = to_u8(dst.b);

    return vec4<f32>(
        f32(clipped_byte_channel(src_r, dst_r, src_a)) / 255.0,
        f32(clipped_byte_channel(src_g, dst_g, src_a)) / 255.0,
        f32(clipped_byte_channel(src_b, dst_b, src_a)) / 255.0,
        f32(dst_a) / 255.0,
    );
}
"#;

pub(crate) const STANDARD_BLEND_SHADER: &str = r#"
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
    blend_kind: u32,
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

fn quantize_u8(value: vec4<f32>) -> vec4<f32> {
    return floor(clamp(value, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + vec4<f32>(0.5)) / 255.0;
}

fn quantize_rgb_u8(value: vec3<f32>) -> vec3<f32> {
    return floor(clamp(value, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 + vec3<f32>(0.5)) / 255.0;
}

fn to_u8(value: f32) -> i32 {
    return i32(clamp(floor(value * 255.0 + 0.5), 0.0, 255.0));
}

fn min3(value: vec3<f32>) -> f32 {
    return min(value.r, min(value.g, value.b));
}

fn max3(value: vec3<f32>) -> f32 {
    return max(value.r, max(value.g, value.b));
}

fn lum(value: vec3<f32>) -> f32 {
    return 0.3 * value.r + 0.59 * value.g + 0.11 * value.b;
}

fn color_compare_lum(value: vec3<f32>) -> f32 {
    return 0.2126 * value.r + 0.7152 * value.g + 0.0722 * value.b;
}

fn sat(value: vec3<f32>) -> f32 {
    return max3(value) - min3(value);
}

fn set_lum(value: vec3<f32>, target_lum: f32) -> vec3<f32> {
    var out = value + vec3<f32>(target_lum - lum(value));
    let out_lum = lum(out);
    let out_min = min3(out);
    let out_max = max3(out);
    if (out_min < 0.0) {
        out = vec3<f32>(out_lum) + (out - vec3<f32>(out_lum)) *
            (out_lum / max(out_lum - out_min, 0.000001));
    }
    if (out_max > 1.0) {
        out = vec3<f32>(out_lum) + (out - vec3<f32>(out_lum)) *
            ((1.0 - out_lum) / max(out_max - out_lum, 0.000001));
    }
    return out;
}

fn set_sat(value: vec3<f32>, target_sat: f32) -> vec3<f32> {
    let value_min = min3(value);
    let span = max3(value) - value_min;
    if (span <= 0.0) {
        return vec3<f32>(0.0);
    }
    var rel = (value - vec3<f32>(value_min)) / max(span, 0.000001);
    if (span <= (2.0 / 255.0)) {
        rel = floor(rel + vec3<f32>(0.000001));
    }
    return rel * target_sat;
}

fn vivid_light_channel(src: f32, dst: f32) -> f32 {
    if (src < 0.5) {
        if (dst >= 1.0) {
            return 1.0;
        }
        if (2.0 * src <= 0.0) {
            return 0.0;
        }
        return 1.0 - min((1.0 - dst) / max(2.0 * src, 0.000001), 1.0);
    }
    let dodge_src = 2.0 * (src - 0.5);
    if (dst <= 0.0) {
        return 0.0;
    }
    if (dodge_src >= 1.0) {
        return 1.0;
    }
    return min(dst / max(1.0 - dodge_src, 0.000001), 1.0);
}

fn soft_light_channel(src: f32, dst: f32) -> f32 {
    if (src < 0.5) {
        return dst - (1.0 - 2.0 * src) * dst * (1.0 - dst);
    }
    var curve = sqrt(dst);
    if (dst < 0.25) {
        curve = ((16.0 * dst - 12.0) * dst + 4.0) * dst;
    }
    return dst + (2.0 * src - 1.0) * (curve - dst);
}

fn hsl_blend(src: vec3<f32>, dst: vec3<f32>) -> vec3<f32> {
    if (source_params.blend_kind == 23u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum(set_sat(src_q, sat(dst_q)), lum(dst_q));
    }
    if (source_params.blend_kind == 24u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum(set_sat(dst_q, sat(src_q)), lum(dst_q));
    }
    if (source_params.blend_kind == 26u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum(dst_q, lum(src_q));
    }
    return set_lum(src, lum(dst));
}

fn blend_rgb(src: vec3<f32>, dst: vec3<f32>) -> vec3<f32> {
    if (source_params.blend_kind == 1u) {
        return min(src, dst);
    }
    if (source_params.blend_kind == 2u) {
        return src * dst;
    }
    if (source_params.blend_kind == 4u) {
        return max(src + dst - vec3<f32>(1.0), vec3<f32>(0.0));
    }
    if (source_params.blend_kind == 5u) {
        return max(dst - src, vec3<f32>(0.0));
    }
    if (source_params.blend_kind == 6u) {
        if (color_compare_lum(src) < color_compare_lum(dst)) {
            return src;
        }
        return dst;
    }
    if (source_params.blend_kind == 7u) {
        return max(src, dst);
    }
    if (source_params.blend_kind == 8u) {
        return vec3<f32>(1.0) - (vec3<f32>(1.0) - src) * (vec3<f32>(1.0) - dst);
    }
    if (source_params.blend_kind == 11u) {
        return min(src + dst, vec3<f32>(1.0));
    }
    if (source_params.blend_kind == 13u) {
        if (color_compare_lum(src) > color_compare_lum(dst)) {
            return src;
        }
        return dst;
    }
    if (source_params.blend_kind == 14u) {
        return select(
            vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - src) * (vec3<f32>(1.0) - dst),
            2.0 * src * dst,
            dst < vec3<f32>(0.5),
        );
    }
    if (source_params.blend_kind == 15u) {
        return vec3<f32>(
            soft_light_channel(src.r, dst.r),
            soft_light_channel(src.g, dst.g),
            soft_light_channel(src.b, dst.b),
        );
    }
    if (source_params.blend_kind == 16u) {
        return select(
            vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - src) * (vec3<f32>(1.0) - dst),
            2.0 * src * dst,
            src < vec3<f32>(0.5),
        );
    }
    if (source_params.blend_kind == 18u) {
        return clamp(2.0 * src + dst - vec3<f32>(1.0), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if (source_params.blend_kind == 19u) {
        return select(
            max(dst, 2.0 * (src - vec3<f32>(0.5))),
            min(dst, 2.0 * src),
            src < vec3<f32>(0.5),
        );
    }
    if (source_params.blend_kind == 21u) {
        return abs(src - dst);
    }
    if (source_params.blend_kind == 22u) {
        return src + dst - 2.0 * src * dst;
    }
    if (source_params.blend_kind == 23u || source_params.blend_kind == 24u || source_params.blend_kind == 25u || source_params.blend_kind == 26u) {
        return hsl_blend(src, dst);
    }
    if (source_params.blend_kind == 36u) {
        return min(dst / max(src, vec3<f32>(0.000001)), vec3<f32>(1.0));
    }
    let vivid = vec3<f32>(
        vivid_light_channel(src.r, dst.r),
        vivid_light_channel(src.g, dst.g),
        vivid_light_channel(src.b, dst.b),
    );
    if (source_params.blend_kind == 20u) {
        return select(vec3<f32>(0.0), vec3<f32>(1.0), vivid >= vec3<f32>(127.0 / 255.0));
    }
    return vivid;
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

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(position.xy);
    var src = load_source(texel);
    if (source_params.source_kind == 1u) {
        src = source_params.color;
    }
    let dst = textureLoad(dest_texture, texel, 0);

    src.a = clamp(src.a * source_params.opacity, 0.0, 1.0);
    if (source_params.has_mask == 1u) {
        src.a = src.a * textureLoad(mask_texture, texel + source_params.target_origin, 0).r;
    }
    if (src.a <= 0.0) {
        return dst;
    }

    let blended = quantize_rgb_u8(blend_rgb(src.rgb, dst.rgb));
    let out_alpha = src.a + dst.a * (1.0 - src.a);
    if (out_alpha <= 0.0) {
        return vec4<f32>(1.0, 1.0, 1.0, 0.0);
    }
    let out_pm =
        (1.0 - dst.a) * src.rgb * src.a +
        (1.0 - src.a) * dst.rgb * dst.a +
        src.a * dst.a * blended;
    return quantize_u8(vec4<f32>(out_pm / out_alpha, out_alpha));
}
"#;
