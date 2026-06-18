
@group(0) @binding(0)
var atlas_texture: texture_2d<f32>;

@group(0) @binding(1)
var dest_texture: texture_2d<f32>;

@group(0) @binding(6)
var mask_atlas_texture: texture_2d<f32>;

@group(0) @binding(2)
var<storage, read> event_headers: array<u32>;

@group(0) @binding(3)
var<storage, read> work_indices: array<u32>;

@group(0) @binding(4)
var<storage, read> tile_spans: array<u32>;

struct TileSiloParams {
    target_origin: vec2<i32>,
    tile_size: u32,
    tile_cols: u32,
    mode: u32,
    resolve_blend_kind: u32,
    base_event_count: u32,
    _padding0: u32,
    _padding: vec3<u32>,
};

@group(0) @binding(5)
var<uniform> params: TileSiloParams;

@group(0) @binding(7)
var<storage, read> raster_payloads: array<u32>;

@group(0) @binding(8)
var<storage, read> filter_payloads: array<u32>;

@group(0) @binding(9)
var lut_texture: texture_2d<f32>;

@group(0) @binding(10)
var<storage, read> scope_payloads: array<u32>;

const EVENT_HEADER_WORDS: u32 = 4u;
const RASTER_PAYLOAD_WORDS: u32 = 10u;
const POINT_FILTER_PAYLOAD_WORDS: u32 = 10u;
const SCOPE_PAYLOAD_WORDS: u32 = 8u;
const NO_MASK_ATLAS_COORD: u32 = 0xffffffffu;
const TILE_EVENT_KIND_RASTER: u32 = 1u;
const TILE_EVENT_KIND_BEGIN_CONTAINER: u32 = 5u;
const TILE_EVENT_KIND_END_CONTAINER: u32 = 6u;
const TILE_EVENT_KIND_POINT_FILTER: u32 = 7u;
const TILE_EVENT_KIND_SPECIAL_BLEND_RASTER: u32 = 8u;
const TILE_EVENT_KIND_BEGIN_THROUGH: u32 = 9u;
const TILE_EVENT_KIND_END_THROUGH: u32 = 10u;
const MODE_NORMAL: u32 = 0u;
const MODE_PRESERVE_ALPHA: u32 = 1u;
const MODE_CLIPPING_RUN: u32 = 2u;

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

fn event_word(event_index: u32, word_index: u32) -> u32 {
    let payload_offset = event_headers[event_index * EVENT_HEADER_WORDS + 2u];
    return raster_payloads[payload_offset * RASTER_PAYLOAD_WORDS + word_index];
}

fn event_kind(event_index: u32) -> u32 {
    return event_headers[event_index * EVENT_HEADER_WORDS];
}

fn filter_word(event_index: u32, word_index: u32) -> u32 {
    let payload_offset = event_headers[event_index * EVENT_HEADER_WORDS + 2u];
    return filter_payloads[payload_offset * POINT_FILTER_PAYLOAD_WORDS + word_index];
}

fn scope_word(event_index: u32, word_index: u32) -> u32 {
    let payload_offset = event_headers[event_index * EVENT_HEADER_WORDS + 2u];
    return scope_payloads[payload_offset * SCOPE_PAYLOAD_WORDS + word_index];
}

fn quantize_u8(value: vec4<f32>) -> vec4<f32> {
    return floor(clamp(value, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + vec4<f32>(0.5)) / 255.0;
}

fn quantize_rgb_u8(value: vec3<f32>) -> vec3<f32> {
    return floor(clamp(value, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0 + vec3<f32>(0.5)) / 255.0;
}

fn truncate_rgb_u8(value: vec3<f32>) -> vec3<f32> {
    return floor(clamp(value, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0) / 255.0;
}

fn floor_quantize_u8(value: vec4<f32>) -> vec4<f32> {
    return floor(clamp(value, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0) / 255.0;
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

fn div_round(numerator: i32, denominator: i32) -> i32 {
    return (numerator + denominator / 2) / denominator;
}

fn opacity_to_u8(value: f32) -> i32 {
    return i32(clamp(floor(value * 256.0 + 0.5), 0.0, 256.0));
}

fn filter_gradient_lum_u8(value: vec3<f32>) -> i32 {
    let lum_value = value.r * 255.0 * 0.3 + value.g * 255.0 * 0.59 + value.b * 255.0 * 0.11;
    return i32(clamp(floor(lum_value), 0.0, 255.0));
}

fn filter_threshold_lum_u8(value: vec3<f32>) -> i32 {
    let lum_value = value.r * 255.0 * 0.299 + value.g * 255.0 * 0.587 + value.b * 255.0 * 0.114;
    return i32(clamp(floor(lum_value), 0.0, 255.0));
}

fn filter_rgb_to_hsv_u8(value: vec3<f32>) -> vec3<f32> {
    let rgb = quantize_rgb_u8(value);
    let mx = max(max(rgb.r, rgb.g), rgb.b);
    let mn = min(min(rgb.r, rgb.g), rgb.b);
    let delta = mx - mn;
    var hue = 0.0;
    if (delta > 0.000001) {
        if (mx == rgb.b) {
            hue = ((rgb.r - rgb.g) / max(delta, 0.000001)) + 4.0;
        } else if (mx == rgb.g) {
            hue = ((rgb.b - rgb.r) / max(delta, 0.000001)) + 2.0;
        } else {
            hue = (rgb.g - rgb.b) / max(delta, 0.000001);
            hue = hue - floor(hue / 6.0) * 6.0;
        }
        hue = hue / 6.0;
    }
    var saturation = 0.0;
    if (mx > 0.000001) {
        saturation = delta / max(mx, 0.000001);
    }
    return vec3<f32>(hue, saturation, mx);
}

fn filter_hsv_to_rgb_u8(hue: f32, saturation: f32, value: f32) -> vec3<f32> {
    let wrapped_hue = hue - floor(hue);
    let h = wrapped_hue * 6.0;
    let sector = i32(floor(h)) % 6;
    let f = h - floor(h);
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * f);
    let t = value * (1.0 - saturation * (1.0 - f));
    var rgb = vec3<f32>(value, p, q);
    if (sector == 0) {
        rgb = vec3<f32>(value, t, p);
    } else if (sector == 1) {
        rgb = vec3<f32>(q, value, p);
    } else if (sector == 2) {
        rgb = vec3<f32>(p, value, t);
    } else if (sector == 3) {
        rgb = vec3<f32>(p, q, value);
    } else if (sector == 4) {
        rgb = vec3<f32>(t, p, value);
    }
    return truncate_rgb_u8(rgb);
}

fn apply_hsl_adjust_filter(value: vec3<f32>, event_index: u32) -> vec3<f32> {
    let hsv = filter_rgb_to_hsv_u8(value);
    var hue = hsv.x + bitcast<f32>(filter_word(event_index, 3u));
    var saturation = hsv.y;
    var luminosity = hsv.z;
    let saturation_delta = bitcast<f32>(filter_word(event_index, 4u));
    let luminosity_delta = bitcast<f32>(filter_word(event_index, 5u));

    if (luminosity_delta > 0.0) {
        luminosity = luminosity + luminosity_delta * (1.0 - luminosity);
        saturation = saturation - luminosity_delta * saturation;
    } else if (luminosity_delta < 0.0) {
        luminosity = luminosity + luminosity_delta * luminosity;
    }

    if (saturation_delta > 0.0 && saturation > 0.0) {
        var inc = saturation_delta * (1.0 - saturation);
        let value_delta = luminosity * inc;
        if (luminosity + value_delta > 1.0 && value_delta > 0.0) {
            inc = inc * (1.0 - luminosity) / value_delta;
            luminosity = 1.0;
        } else {
            luminosity = luminosity + value_delta;
        }
        saturation = saturation + inc;
    } else if (saturation_delta < 0.0) {
        let dec = -saturation_delta * saturation;
        saturation = saturation - dec;
        luminosity = luminosity - luminosity * dec * 0.5;
    }

    return filter_hsv_to_rgb_u8(hue, clamp(saturation, 0.0, 1.0), clamp(luminosity, 0.0, 1.0));
}

fn filter_contains(event_index: u32, local_texel: vec2<i32>) -> bool {
    let origin = vec2<i32>(
        i32(filter_word(event_index, 6u)),
        i32(filter_word(event_index, 7u)),
    );
    let size = vec2<i32>(
        i32(filter_word(event_index, 8u)),
        i32(filter_word(event_index, 9u)),
    );
    return !(
        local_texel.x < origin.x ||
        local_texel.y < origin.y ||
        local_texel.x >= origin.x + size.x ||
        local_texel.y >= origin.y + size.y
    );
}

fn scope_contains(event_index: u32, local_texel: vec2<i32>) -> bool {
    let origin = vec2<i32>(
        i32(scope_word(event_index, 2u)),
        i32(scope_word(event_index, 3u)),
    );
    let size = vec2<i32>(
        i32(scope_word(event_index, 4u)),
        i32(scope_word(event_index, 5u)),
    );
    return !(
        local_texel.x < origin.x ||
        local_texel.y < origin.y ||
        local_texel.x >= origin.x + size.x ||
        local_texel.y >= origin.y + size.y
    );
}

fn apply_point_filter_event(event_index: u32, before: vec4<f32>) -> vec4<f32> {
    let lut_row = i32(filter_word(event_index, 0u));
    let mode = filter_word(event_index, 2u);
    var mapped = vec3<f32>(
        textureLoad(lut_texture, vec2<i32>(to_u8(before.r), lut_row), 0).r,
        textureLoad(lut_texture, vec2<i32>(to_u8(before.g), lut_row), 0).g,
        textureLoad(lut_texture, vec2<i32>(to_u8(before.b), lut_row), 0).b,
    );
    if (mode == 1u) {
        mapped = textureLoad(lut_texture, vec2<i32>(filter_gradient_lum_u8(before.rgb), lut_row), 0).rgb;
    } else if (mode == 2u) {
        mapped = textureLoad(lut_texture, vec2<i32>(filter_threshold_lum_u8(before.rgb), lut_row), 0).rgb;
    } else if (mode == 3u) {
        mapped = apply_hsl_adjust_filter(before.rgb, event_index);
    }
    let strength = clamp(bitcast<f32>(filter_word(event_index, 1u)), 0.0, 1.0);
    let rgb = before.rgb * (1.0 - strength) + mapped * strength;
    return quantize_u8(vec4<f32>(rgb, before.a));
}

fn resolve_container_scope(event_index: u32, scope_dst: vec4<f32>, dst: vec4<f32>) -> vec4<f32> {
    let blend_kind = scope_word(event_index, 1u);
    let opacity = clamp(bitcast<f32>(scope_word(event_index, 0u)), 0.0, 1.0);
    if (blend_kind == 0u) {
        let src_a = (to_u8(scope_dst.a) * opacity_to_u8(opacity)) / 256;
        if (src_a <= 0) {
            return dst;
        }
        return apply_normal_alpha(scope_dst, dst, src_a);
    }
    if (is_byte_domain_special_blend(blend_kind)) {
        let src_a = (to_u8(scope_dst.a) * opacity_to_u8(opacity)) / 256;
        if (src_a <= 0) {
            return dst;
        }
        return apply_byte_standard_alpha(scope_dst, dst, src_a, blend_kind);
    }
    var src = scope_dst;
    src.a = clamp(src.a * opacity, 0.0, 1.0);
    if (src.a <= 0.0) {
        return dst;
    }
    return apply_standard(src, dst, blend_kind);
}

fn resolve_through_scope(event_index: u32, before: vec4<f32>, after: vec4<f32>) -> vec4<f32> {
    let strength = clamp(bitcast<f32>(scope_word(event_index, 0u)), 0.0, 1.0);
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

fn normal_alpha_over_channel(dst: i32, src: i32, src_a: i32, carry: i32, out_a: i32) -> i32 {
    return clamp((src * src_a + dst * carry + (out_a - 1) / 2) / out_a, 0, 255);
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

fn lum_hue(value: vec3<f32>) -> f32 {
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
    var out = value + vec3<f32>(target_lum - lum_hue(value));
    let out_lum = lum_hue(out);
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
    if (base_sat > (2.0 / 255.0)) {
        let ceiled = ceil_rgb_u8(out);
        let clipped_min = min3(out);
        if (out.r <= clipped_min + 0.000001) { out.r = ceiled.r; }
        if (out.g <= clipped_min + 0.000001) { out.g = ceiled.g; }
        if (out.b <= clipped_min + 0.000001) { out.b = ceiled.b; }
    }
    return out;
}

fn set_lum_hue_partial(value: vec3<f32>, target_lum: f32, base_sat: f32) -> vec3<f32> {
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

fn is_byte_domain_special_blend(blend_kind: u32) -> bool {
    return blend_kind == 3u || blend_kind == 9u || blend_kind == 10u || blend_kind == 12u;
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

fn add_glow_channel(dst: i32, src: i32, src_a: i32, dst_a: i32) -> i32 {
    var rgb = dst + src;
    if (src_a < 255) {
        let b = div_round_255(dst_a * (255 - src_a));
        let denom = max(b + src_a, 1);
        let numer = b * dst + rgb * src_a;
        if (dst_a <= 254) {
            rgb = numer / denom;
        } else {
            rgb = (numer + denom / 2) / denom;
        }
    }
    rgb = min(rgb, 255);

    if (dst_a <= 254) {
        let inv_dst_a = 255 - dst_a;
        if (src_a == 255) {
            rgb = div_round_255(inv_dst_a * src + rgb * dst_a);
        } else {
            let b = div_round_255(inv_dst_a * src_a);
            let denom = max(dst_a + b, 1);
            rgb = (b * src + rgb * dst_a + denom / 2) / denom;
        }
    }

    return clamp(rgb, 0, 255);
}

fn byte_composite_channel(src: i32, dst: i32, blended: i32, src_a: i32, dst_a: i32, alpha_num: i32) -> i32 {
    let numerator =
        src * src_a * (255 - dst_a) +
        dst * dst_a * (255 - src_a) +
        blended * src_a * dst_a;
    return clamp(div_round(numerator, alpha_num), 0, 255);
}

fn preserve_channel(dst: i32, blended: i32, src_a: i32) -> i32 {
    return clamp(div_round_255(dst * (255 - src_a) + blended * src_a), 0, 255);
}

fn hsl_blend(src: vec3<f32>, dst: vec3<f32>, src_alpha: f32, blend_kind: u32) -> vec3<f32> {
    if (blend_kind == 23u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        let saturated = set_sat(src_q, sat(dst_q));
        if (src_alpha >= 1.0) {
            return set_lum_hue(saturated, lum_hue(dst_q), sat(dst_q));
        }
        return set_lum_hue_partial(saturated, lum(dst_q), sat(dst_q));
    }
    if (blend_kind == 24u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum_saturation(set_sat(dst_q, sat(src_q)), lum(dst_q), sat(dst_q));
    }
    if (blend_kind == 26u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum_rec601(dst_q, lum_rec601(src_q));
    }
    if (blend_kind == 25u) {
        let src_q = quantize_rgb_u8(src);
        let dst_q = quantize_rgb_u8(dst);
        return set_lum_color(src_q, dst_q);
    }
    return set_lum(src, lum(dst));  // unreachable -- fallback safety
}

fn blend_rgb(src: vec3<f32>, dst: vec3<f32>, src_alpha: f32, blend_kind: u32) -> vec3<f32> {
    if (blend_kind == 0u) {
        return src;
    }
    if (blend_kind == 1u) {
        return min(src, dst);
    }
    if (blend_kind == 2u) {
        return src * dst;
    }
    if (blend_kind == 4u) {
        return max(src + dst - vec3<f32>(1.0), vec3<f32>(0.0));
    }
    if (blend_kind == 5u) {
        return max(dst - src, vec3<f32>(0.0));
    }
    if (blend_kind == 6u) {
        if (color_compare_lum(src) < color_compare_lum(dst)) {
            return src;
        }
        return dst;
    }
    if (blend_kind == 7u) {
        return max(src, dst);
    }
    if (blend_kind == 8u) {
        return vec3<f32>(1.0) - (vec3<f32>(1.0) - src) * (vec3<f32>(1.0) - dst);
    }
    if (blend_kind == 11u) {
        return min(src + dst, vec3<f32>(1.0));
    }
    if (blend_kind == 13u) {
        if (color_compare_lum(src) > color_compare_lum(dst)) {
            return src;
        }
        return dst;
    }
    if (blend_kind == 14u) {
        return select(
            vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - src) * (vec3<f32>(1.0) - dst),
            2.0 * src * dst,
            dst < vec3<f32>(0.5),
        );
    }
    if (blend_kind == 15u) {
        return vec3<f32>(
            soft_light_channel(src.r, dst.r),
            soft_light_channel(src.g, dst.g),
            soft_light_channel(src.b, dst.b),
        );
    }
    if (blend_kind == 16u) {
        return select(
            vec3<f32>(1.0) - 2.0 * (vec3<f32>(1.0) - src) * (vec3<f32>(1.0) - dst),
            2.0 * src * dst,
            src < vec3<f32>(0.5),
        );
    }
    if (blend_kind == 18u) {
        return clamp(2.0 * src + dst - vec3<f32>(1.0), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if (blend_kind == 19u) {
        return select(
            max(dst, 2.0 * (src - vec3<f32>(0.5))),
            min(dst, 2.0 * src),
            src < vec3<f32>(0.5),
        );
    }
    if (blend_kind == 21u) {
        return abs(src - dst);
    }
    if (blend_kind == 22u) {
        return src + dst - 2.0 * src * dst;
    }
    if (blend_kind == 23u || blend_kind == 24u || blend_kind == 25u || blend_kind == 26u) {
        return hsl_blend(src, dst, src_alpha, blend_kind);
    }
    if (blend_kind == 36u) {
        return min(dst / max(src, vec3<f32>(0.000001)), vec3<f32>(1.0));
    }
    let vivid = vec3<f32>(
        vivid_light_channel(src.r, dst.r),
        vivid_light_channel(src.g, dst.g),
        vivid_light_channel(src.b, dst.b),
    );
    if (blend_kind == 20u) {
        return select(vec3<f32>(0.0), vec3<f32>(1.0), vivid >= vec3<f32>(127.0 / 255.0));
    }
    return vivid;
}

fn source_origin(event_index: u32) -> vec2<i32> {
    return vec2<i32>(
        bitcast<i32>(event_word(event_index, 4u)),
        bitcast<i32>(event_word(event_index, 5u)),
    );
}

fn source_size(event_index: u32) -> vec2<i32> {
    return vec2<i32>(
        i32(event_word(event_index, 2u)),
        i32(event_word(event_index, 3u)),
    );
}

fn source_texel_for_event(event_index: u32, local_texel: vec2<i32>) -> vec2<i32> {
    return local_texel + params.target_origin - source_origin(event_index);
}

fn source_contains(event_index: u32, source_texel: vec2<i32>) -> bool {
    let size = source_size(event_index);
    return !(
        source_texel.x < 0 ||
        source_texel.y < 0 ||
        source_texel.x >= size.x ||
        source_texel.y >= size.y
    );
}

fn load_source_at(event_index: u32, source_texel: vec2<i32>) -> vec4<f32> {
    let atlas_origin = vec2<i32>(
        i32(event_word(event_index, 0u)),
        i32(event_word(event_index, 1u)),
    );
    return textureLoad(atlas_texture, atlas_origin + source_texel, 0);
}

fn load_event_mask(event_index: u32, source_texel: vec2<i32>) -> f32 {
    let mask_atlas_x = event_word(event_index, 8u);
    if (mask_atlas_x == NO_MASK_ATLAS_COORD) {
        return 1.0;
    }
    let mask_atlas_origin = vec2<i32>(
        i32(mask_atlas_x),
        i32(event_word(event_index, 9u)),
    );
    return textureLoad(mask_atlas_texture, mask_atlas_origin + source_texel, 0).r;
}

fn apply_normal_alpha(src: vec4<f32>, dst: vec4<f32>, src_alpha: i32) -> vec4<f32> {
    let src_a = src_alpha;
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

fn event_source_alpha(src: vec4<f32>, event_index: u32, mask_value: f32) -> i32 {
    var src_a = to_u8(src.a);
    if (event_word(event_index, 8u) != NO_MASK_ATLAS_COORD) {
        src_a = (src_a * to_u8(mask_value)) / 255;
    }
    return to_u8(f32(src_a) / 255.0 * bitcast<f32>(event_word(event_index, 6u)));
}

fn event_source_alpha_byte(src: vec4<f32>, event_index: u32, mask_value: f32) -> i32 {
    var src_a = to_u8(src.a);
    if (event_word(event_index, 8u) != NO_MASK_ATLAS_COORD) {
        src_a = div255(src_a * to_u8(mask_value));
    }
    let opacity_u8 = i32(clamp(floor(bitcast<f32>(event_word(event_index, 6u)) * 256.0 + 0.5), 0.0, 256.0));
    return (src_a * opacity_u8) / 256;
}

fn apply_normal(src: vec4<f32>, dst: vec4<f32>, event_index: u32, mask_value: f32) -> vec4<f32> {
    return apply_normal_alpha(src, dst, event_source_alpha(src, event_index, mask_value));
}

fn apply_add_glow_standard_alpha(src: vec4<f32>, dst: vec4<f32>, src_a: i32) -> vec4<f32> {
    if (src_a <= 0) {
        return dst;
    }
    let dst_a = to_u8(dst.a);
    let out_a = min(div_round_255((255 - src_a) * dst_a) + src_a, 255);
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

fn apply_glow_dodge_standard_alpha(src: vec4<f32>, dst: vec4<f32>, src_a: i32) -> vec4<f32> {
    if (src_a <= 0) {
        return dst;
    }
    let src_r = to_u8(src.r);
    let src_g = to_u8(src.g);
    let src_b = to_u8(src.b);
    let dst_r = to_u8(dst.r);
    let dst_g = to_u8(dst.g);
    let dst_b = to_u8(dst.b);
    let dst_a = to_u8(dst.a);

    let alpha_num = src_a * 255 + dst_a * (255 - src_a);
    if (dst_a > 0 && dst_r == 0 && dst_g == 0 && dst_b == 0) {
        let out_a = min(src_a + dst_a, 255);
        let out_r = clamp(div_round(src_r * src_a * (255 - dst_a), alpha_num), 0, 255);
        let out_g = clamp(div_round(src_g * src_a * (255 - dst_a), alpha_num), 0, 255);
        let out_b = clamp(div_round(src_b * src_a * (255 - dst_a), alpha_num), 0, 255);
        return vec4<f32>(
            f32(out_r) / 255.0,
            f32(out_g) / 255.0,
            f32(out_b) / 255.0,
            f32(out_a) / 255.0,
        );
    }

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

fn apply_dodge_burn_standard_alpha(src: vec4<f32>, dst: vec4<f32>, src_a: i32, blend_kind: u32) -> vec4<f32> {
    if (src_a <= 0) {
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

    var blend_r = color_dodge_channel(dst_r, src_r);
    var blend_g = color_dodge_channel(dst_g, src_g);
    var blend_b = color_dodge_channel(dst_b, src_b);
    if (blend_kind == 3u) {
        blend_r = color_burn_channel(dst_r, src_r);
        blend_g = color_burn_channel(dst_g, src_g);
        blend_b = color_burn_channel(dst_b, src_b);
    }

    let out_r = byte_composite_channel(src_r, dst_r, blend_r, src_a, dst_a, alpha_num);
    let out_g = byte_composite_channel(src_g, dst_g, blend_g, src_a, dst_a, alpha_num);
    let out_b = byte_composite_channel(src_b, dst_b, blend_b, src_a, dst_a, alpha_num);
    return vec4<f32>(
        f32(out_r) / 255.0,
        f32(out_g) / 255.0,
        f32(out_b) / 255.0,
        f32(out_a) / 255.0,
    );
}

fn apply_byte_standard_alpha(src: vec4<f32>, dst: vec4<f32>, src_a: i32, blend_kind: u32) -> vec4<f32> {
    if (blend_kind == 12u) {
        return apply_add_glow_standard_alpha(src, dst, src_a);
    }
    if (blend_kind == 10u) {
        return apply_glow_dodge_standard_alpha(src, dst, src_a);
    }
    return apply_dodge_burn_standard_alpha(src, dst, src_a, blend_kind);
}

fn apply_byte_standard(src: vec4<f32>, dst: vec4<f32>, event_index: u32, mask_value: f32, blend_kind: u32) -> vec4<f32> {
    return apply_byte_standard_alpha(src, dst, event_source_alpha_byte(src, event_index, mask_value), blend_kind);
}

fn apply_byte_preserve_alpha(src: vec4<f32>, dst: vec4<f32>, src_a: i32, blend_kind: u32) -> vec4<f32> {
    let dst_a = to_u8(dst.a);
    if (src_a <= 0 || dst_a == 0) {
        return dst;
    }

    let src_r = to_u8(src.r);
    let src_g = to_u8(src.g);
    let src_b = to_u8(src.b);
    let dst_r = to_u8(dst.r);
    let dst_g = to_u8(dst.g);
    let dst_b = to_u8(dst.b);

    var out_r = dst_r;
    var out_g = dst_g;
    var out_b = dst_b;
    if (blend_kind == 12u) {
        out_r = add_glow_channel(dst_r, src_r, src_a, 255);
        out_g = add_glow_channel(dst_g, src_g, src_a, 255);
        out_b = add_glow_channel(dst_b, src_b, src_a, 255);
    } else if (blend_kind == 9u) {
        out_r = preserve_channel(dst_r, color_dodge_channel(dst_r, src_r), src_a);
        out_g = preserve_channel(dst_g, color_dodge_channel(dst_g, src_g), src_a);
        out_b = preserve_channel(dst_b, color_dodge_channel(dst_b, src_b), src_a);
    } else if (blend_kind == 3u) {
        out_r = preserve_channel(dst_r, color_burn_channel(dst_r, src_r), src_a);
        out_g = preserve_channel(dst_g, color_burn_channel(dst_g, src_g), src_a);
        out_b = preserve_channel(dst_b, color_burn_channel(dst_b, src_b), src_a);
    } else if (blend_kind == 10u) {
        out_r = glow_dodge_channel(dst_r, div_round_255(src_r * src_a));
        out_g = glow_dodge_channel(dst_g, div_round_255(src_g * src_a));
        out_b = glow_dodge_channel(dst_b, div_round_255(src_b * src_a));
    }

    return vec4<f32>(
        f32(out_r) / 255.0,
        f32(out_g) / 255.0,
        f32(out_b) / 255.0,
        f32(dst_a) / 255.0,
    );
}

fn apply_byte_preserve(src: vec4<f32>, dst: vec4<f32>, event_index: u32, mask_value: f32, blend_kind: u32) -> vec4<f32> {
    return apply_byte_preserve_alpha(src, dst, event_source_alpha_byte(src, event_index, mask_value), blend_kind);
}

fn apply_standard(src: vec4<f32>, dst: vec4<f32>, blend_kind: u32) -> vec4<f32> {
    let blended_raw = blend_rgb(src.rgb, dst.rgb, src.a, blend_kind);
    var blended = select(
        quantize_rgb_u8(blended_raw),
        clamp(blended_raw, vec3<f32>(0.0), vec3<f32>(1.0)),
        blend_kind == 2u,
    );
    if (blend_kind == 5u && src.a > 0.0 && src.a < 1.0) {
        let src_q = quantize_rgb_u8(src.rgb);
        let dst_q = quantize_rgb_u8(dst.rgb);
        blended = select(blended, vec3<f32>(1.0 / 255.0), src_q == dst_q);
    }
    let out_alpha = src.a + dst.a * (1.0 - src.a);
    if (out_alpha <= 0.0) {
        return vec4<f32>(1.0, 1.0, 1.0, 0.0);
    }
    let out_pm =
        (1.0 - dst.a) * src.rgb * src.a +
        (1.0 - src.a) * dst.rgb * dst.a +
        src.a * dst.a * blended;
    let out = vec4<f32>(out_pm / out_alpha, out_alpha);
    if (blend_kind == 23u && src.a > 0.0 && src.a < 1.0) {
        return floor_quantize_u8(out);
    }
    return quantize_u8(out);
}

fn apply_preserve(src: vec4<f32>, dst: vec4<f32>, blend_kind: u32) -> vec4<f32> {
    var strength = src.a;
    if (dst.a <= 0.0) {
        strength = 0.0;
    }
    let blended_raw = blend_rgb(src.rgb, dst.rgb, strength, blend_kind);
    var blended = select(
        quantize_rgb_u8(blended_raw),
        clamp(blended_raw, vec3<f32>(0.0), vec3<f32>(1.0)),
        blend_kind == 2u,
    );
    if (blend_kind == 5u && src.a > 0.0 && src.a < 1.0) {
        let src_q = quantize_rgb_u8(src.rgb);
        let dst_q = quantize_rgb_u8(dst.rgb);
        blended = select(blended, vec3<f32>(1.0 / 255.0), src_q == dst_q);
    }
    let out_rgb = blended * strength + dst.rgb * (1.0 - strength);
    let out = vec4<f32>(out_rgb, dst.a);
    if (blend_kind == 23u && strength > 0.0 && strength < 1.0) {
        return floor_quantize_u8(out);
    }
    return quantize_u8(out);
}

fn apply_clipping_run_resolve(src: vec4<f32>, dst: vec4<f32>) -> vec4<f32> {
    if (src.a <= 0.0) {
        return dst;
    }
    if (params.resolve_blend_kind == 0u) {
        return apply_normal_alpha(src, dst, to_u8(src.a));
    }
    if (is_byte_domain_special_blend(params.resolve_blend_kind)) {
        return apply_byte_standard_alpha(src, dst, to_u8(src.a), params.resolve_blend_kind);
    }
    return apply_standard(src, dst, params.resolve_blend_kind);
}

fn apply_raster_event_to_accumulator(src_input: vec4<f32>, accumulator: vec4<f32>, event_index: u32, mask_value: f32, blend_kind: u32) -> vec4<f32> {
    var src = src_input;
    if (blend_kind == 0u) {
        return apply_normal(src, accumulator, event_index, mask_value);
    }
    if (is_byte_domain_special_blend(blend_kind)) {
        return apply_byte_standard(src, accumulator, event_index, mask_value, blend_kind);
    }
    src.a = clamp(src.a * bitcast<f32>(event_word(event_index, 6u)), 0.0, 1.0);
    if (event_word(event_index, 8u) != NO_MASK_ATLAS_COORD) {
        src.a = src.a * mask_value;
    }
    if (src.a <= 0.0) {
        return accumulator;
    }
    return apply_standard(src, accumulator, blend_kind);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let local_texel = vec2<i32>(position.xy);
    let local_tile = vec2<u32>(position.xy) / vec2<u32>(params.tile_size);
    let tile_index = local_tile.y * params.tile_cols + local_tile.x;
    let span_base = tile_index * 2u;
    let span_start = tile_spans[span_base];
    let span_count = tile_spans[span_base + 1u];
    var dst = textureLoad(dest_texture, local_texel, 0);
    var clip_dst = vec4<f32>(1.0, 1.0, 1.0, 0.0);
    var scope0_dst = vec4<f32>(1.0, 1.0, 1.0, 0.0);
    var scope1_dst = vec4<f32>(1.0, 1.0, 1.0, 0.0);
    var scope2_dst = vec4<f32>(1.0, 1.0, 1.0, 0.0);
    var scope_depth = 0u;
    var through_before = vec4<f32>(1.0, 1.0, 1.0, 0.0);
    var through_after = vec4<f32>(1.0, 1.0, 1.0, 0.0);
    var through_active = false;

    for (var index = 0u; index < span_count; index = index + 1u) {
        let event_index = work_indices[span_start + index];
        let kind = event_kind(event_index);
        if (kind == TILE_EVENT_KIND_BEGIN_THROUGH) {
            if (scope_contains(event_index, local_texel)) {
                through_before = dst;
                through_after = dst;
                through_active = true;
            }
            continue;
        }
        if (kind == TILE_EVENT_KIND_END_THROUGH) {
            if (through_active && scope_contains(event_index, local_texel)) {
                dst = resolve_through_scope(event_index, through_before, through_after);
                through_active = false;
            }
            continue;
        }
        if (kind == TILE_EVENT_KIND_BEGIN_CONTAINER) {
            if (scope_contains(event_index, local_texel)) {
                if (scope_depth == 0u) {
                    scope0_dst = vec4<f32>(1.0, 1.0, 1.0, 0.0);
                    scope_depth = 1u;
                } else if (scope_depth == 1u) {
                    scope1_dst = vec4<f32>(1.0, 1.0, 1.0, 0.0);
                    scope_depth = 2u;
                } else if (scope_depth == 2u) {
                    scope2_dst = vec4<f32>(1.0, 1.0, 1.0, 0.0);
                    scope_depth = 3u;
                }
            }
            continue;
        }
        if (kind == TILE_EVENT_KIND_END_CONTAINER) {
            if (scope_contains(event_index, local_texel)) {
                if (scope_depth == 3u) {
                    scope1_dst = resolve_container_scope(event_index, scope2_dst, scope1_dst);
                    scope_depth = 2u;
                } else if (scope_depth == 2u) {
                    scope0_dst = resolve_container_scope(event_index, scope1_dst, scope0_dst);
                    scope_depth = 1u;
                } else if (scope_depth == 1u) {
                    if (through_active) {
                        through_after = resolve_container_scope(event_index, scope0_dst, through_after);
                    } else {
                        dst = resolve_container_scope(event_index, scope0_dst, dst);
                    }
                    scope_depth = 0u;
                }
            }
            continue;
        }
        if (kind == TILE_EVENT_KIND_POINT_FILTER) {
            if (filter_contains(event_index, local_texel)) {
                if (params.mode == MODE_CLIPPING_RUN) {
                    clip_dst = apply_point_filter_event(event_index, clip_dst);
                } else if (scope_depth == 3u) {
                    scope2_dst = apply_point_filter_event(event_index, scope2_dst);
                } else if (scope_depth == 2u) {
                    scope1_dst = apply_point_filter_event(event_index, scope1_dst);
                } else if (scope_depth == 1u) {
                    scope0_dst = apply_point_filter_event(event_index, scope0_dst);
                } else if (through_active) {
                    through_after = apply_point_filter_event(event_index, through_after);
                } else {
                    dst = apply_point_filter_event(event_index, dst);
                }
            }
            continue;
        }
        let source_texel = source_texel_for_event(event_index, local_texel);
        if (!source_contains(event_index, source_texel)) {
            continue;
        }
        var src = load_source_at(event_index, source_texel);
        let blend_kind = event_word(event_index, 7u);
        let mask_value = load_event_mask(event_index, source_texel);
        if (params.mode == MODE_CLIPPING_RUN) {
            if (event_index < params.base_event_count) {
                clip_dst = apply_normal(src, clip_dst, event_index, mask_value);
                continue;
            }
            if (is_byte_domain_special_blend(blend_kind)) {
                clip_dst = apply_byte_preserve(src, clip_dst, event_index, mask_value, blend_kind);
                continue;
            }
            src.a = clamp(src.a * bitcast<f32>(event_word(event_index, 6u)), 0.0, 1.0);
            if (event_word(event_index, 8u) != NO_MASK_ATLAS_COORD) {
                src.a = src.a * mask_value;
            }
            if (src.a <= 0.0 || clip_dst.a <= 0.0) { continue; }
            clip_dst = apply_preserve(src, clip_dst, blend_kind);
        } else if (params.mode == MODE_PRESERVE_ALPHA) {
            if (is_byte_domain_special_blend(blend_kind)) {
                dst = apply_byte_preserve(src, dst, event_index, mask_value, blend_kind);
                continue;
            }
            src.a = clamp(src.a * bitcast<f32>(event_word(event_index, 6u)), 0.0, 1.0);
            if (event_word(event_index, 8u) != NO_MASK_ATLAS_COORD) {
                src.a = src.a * mask_value;
            }
            if (src.a <= 0.0 || dst.a <= 0.0) { continue; }
            dst = apply_preserve(src, dst, blend_kind);
        } else if (scope_depth == 3u) {
            scope2_dst = apply_raster_event_to_accumulator(src, scope2_dst, event_index, mask_value, blend_kind);
        } else if (scope_depth == 2u) {
            scope1_dst = apply_raster_event_to_accumulator(src, scope1_dst, event_index, mask_value, blend_kind);
        } else if (scope_depth == 1u) {
            scope0_dst = apply_raster_event_to_accumulator(src, scope0_dst, event_index, mask_value, blend_kind);
        } else if (through_active) {
            through_after = apply_raster_event_to_accumulator(src, through_after, event_index, mask_value, blend_kind);
        } else {
            dst = apply_raster_event_to_accumulator(src, dst, event_index, mask_value, blend_kind);
        }
    }

    if (params.mode == MODE_CLIPPING_RUN) {
        return apply_clipping_run_resolve(clip_dst, dst);
    }
    return dst;
}
