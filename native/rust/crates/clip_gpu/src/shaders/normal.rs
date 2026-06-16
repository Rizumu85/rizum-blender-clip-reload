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
    src.a = clamp(src.a * source_params.opacity, 0.0, 1.0);
    if (source_params.has_mask == 1u) {
        src.a = src.a * load_mask(texel + source_params.target_origin);
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
    src.a = clamp(src.a * source_params.opacity, 0.0, 1.0);
    if (source_params.has_mask == 1u) {
        src.a = src.a * load_mask(texel + source_params.target_origin);
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
