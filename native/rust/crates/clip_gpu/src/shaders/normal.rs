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

fn to_u8(value: f32) -> i32 {
    return i32(clamp(floor(value * 255.0 + 0.5), 0.0, 255.0));
}

fn opacity_to_u8(value: f32) -> i32 {
    return i32(clamp(floor(value * 256.0 + 0.5), 0.0, 256.0));
}

fn normal_alpha_over_channel(dst: i32, src: i32, src_a: i32, carry: i32, out_a: i32) -> i32 {
    return clamp((src * src_a + dst * carry + (out_a - 1) / 2) / out_a, 0, 255);
}

fn min3(value: vec3<f32>) -> f32 {
    return min(value.r, min(value.g, value.b));
}

fn max3(value: vec3<f32>) -> f32 {
    return max(value.r, max(value.g, value.b));
}

fn lum(value: vec3<f32>) -> f32 {
    return 0.3 * value.r + 0.6 * value.g + 0.1 * value.b;
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
        if (source_params.source_kind == 2u) {
            return vec4<f32>(0.0, 0.0, 0.0, 0.0);
        }
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
    src_a = (src_a * opacity_to_u8(source_params.opacity)) / 256;
    if (src_a <= 0) {
        return dst;
    }

    let dst_a = to_u8(dst.a);
    let carry = (dst_a * (255 - src_a)) / 255;
    let out_a = min(carry + src_a, 255);
    let out_r = normal_alpha_over_channel(to_u8(dst.r), to_u8(src.r), src_a, carry, out_a);
    let out_g = normal_alpha_over_channel(to_u8(dst.g), to_u8(src.g), src_a, carry, out_a);
    let out_b = normal_alpha_over_channel(to_u8(dst.b), to_u8(src.b), src_a, carry, out_a);
    return vec4<f32>(
        f32(out_r) / 255.0,
        f32(out_g) / 255.0,
        f32(out_b) / 255.0,
        f32(out_a) / 255.0,
    );
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

fn floor_quantize_u8(value: vec4<f32>) -> vec4<f32> {
    return floor(clamp(value, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0) / 255.0;
}

fn quantize_rgb_u8(value: vec3<f32>) -> vec3<f32> {
    return floor(clamp(value, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 + vec3<f32>(0.5)) / 255.0;
}

fn ceil_rgb_u8(value: vec3<f32>) -> vec3<f32> {
    return ceil(clamp(value, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 - vec3<f32>(0.000001)) / 255.0;
}

fn round_scalar_u8(value: f32) -> f32 {
    return floor(clamp(value, 0.0, 1.0) * 255.0 + 0.5) / 255.0;
}

fn min3(value: vec3<f32>) -> f32 {
    return min(value.r, min(value.g, value.b));
}

fn max3(value: vec3<f32>) -> f32 {
    return max(value.r, max(value.g, value.b));
}

fn lum(value: vec3<f32>) -> f32 {
    return 0.3 * value.r + 0.6 * value.g + 0.1 * value.b;
}

fn lum_rec601(value: vec3<f32>) -> f32 {
    return 0.299 * value.r + 0.587 * value.g + 0.114 * value.b;
}

fn lum_color_low(value: vec3<f32>) -> f32 {
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

fn set_lum_rec601(value: vec3<f32>, target_lum: f32) -> vec3<f32> {
    var out = value + vec3<f32>(target_lum - lum_rec601(value));
    let out_lum = lum_rec601(out);
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

fn set_lum_saturation(value: vec3<f32>, target_lum: f32, base_sat: f32) -> vec3<f32> {
    let needs_ceil = base_sat > (4.0 / 255.0) && max3(value + vec3<f32>(target_lum - lum(value))) > 1.0;
    var out = set_lum(value, target_lum);
    if (needs_ceil) {
        let ceiled = ceil_rgb_u8(out);
        let clipped_min = min3(out);
        if (out.r <= clipped_min + 0.000001) { out.r = ceiled.r; }
        if (out.g <= clipped_min + 0.000001) { out.g = ceiled.g; }
        if (out.b <= clipped_min + 0.000001) { out.b = ceiled.b; }
    }
    return out;
}

fn set_lum_hue(value: vec3<f32>, target_lum: f32, base_sat: f32) -> vec3<f32> {
    // CSP's fixed-point Hue blend ceils the min channel after set_lum repositioning
    // when the base saturation is non-trivial. Unlike Saturation, the ceil applies
    // without a high-clamp requirement because CSP's fixed-point Hue always floors
    // the division and the rounding bias goes the wrong way on the minimum channel.
    // Use a lower threshold than Saturation (2/255 vs 4/255) to catch more cases.
    var out = set_lum(value, target_lum);
    if (base_sat > (2.0 / 255.0)) {
        let ceiled = ceil_rgb_u8(out);
        let clipped_min = min3(out);
        if (out.r <= clipped_min + 0.000001) { out.r = ceiled.r; }
        if (out.g <= clipped_min + 0.000001) { out.g = ceiled.g; }
        if (out.b <= clipped_min + 0.000001) { out.b = ceiled.b; }
    }
    return out;
}

fn set_lum_color_low(value: vec3<f32>, target_rgb: vec3<f32>) -> vec3<f32> {
    var out = value + vec3<f32>(round_scalar_u8(lum_color_low(target_rgb)) - round_scalar_u8(lum_color_low(value)));
    let out_lum = lum_color_low(out);
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

fn set_lum_color(value: vec3<f32>, target_rgb: vec3<f32>) -> vec3<f32> {
    let shifted = value + vec3<f32>(lum(target_rgb) - lum(value));
    if (min3(shifted) < 0.0) {
        return set_lum_color_low(value, target_rgb);
    }
    return set_lum(value, lum(target_rgb));
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
        return set_lum_hue(set_sat(src_q, sat(dst_q)), lum(dst_q), sat(dst_q));
    }
    if (source_params.blend_kind == 24u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum_saturation(set_sat(dst_q, sat(src_q)), lum(dst_q), sat(dst_q));
    }
    if (source_params.blend_kind == 26u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum_rec601(dst_q, lum_rec601(src_q));
    }
    if (source_params.blend_kind == 25u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum_color(src_q, dst_q);
    }
    return set_lum(src, lum(dst));  // unreachable -- fallback safety
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
        if (source_params.source_kind == 2u) {
            return vec4<f32>(0.0, 0.0, 0.0, 0.0);
        }
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
    let blended_raw = blend_rgb(src.rgb, dst.rgb);
    var blended = select(
        quantize_rgb_u8(blended_raw),
        clamp(blended_raw, vec3<f32>(0.0), vec3<f32>(1.0)),
        source_params.blend_kind == 2u,
    );
    if (source_params.blend_kind == 5u && src.a > 0.0 && src.a < 1.0) {
        let src_q = quantize_rgb_u8(src.rgb);
        let dst_q = quantize_rgb_u8(dst.rgb);
        blended = select(blended, vec3<f32>(1.0 / 255.0), src_q == dst_q);
    }
    let out_rgb = blended * strength + dst.rgb * (1.0 - strength);
    let out = vec4<f32>(out_rgb, dst.a);
    if (source_params.blend_kind == 23u && strength > 0.0 && strength < 1.0) {
        return floor_quantize_u8(out);
    }
    return quantize_u8(out);
}
"#;
