pub(crate) const LUT_FILTER_SHADER: &str = r#"
@group(0) @binding(0)
var source_texture: texture_2d<f32>;

@group(0) @binding(1)
var mask_texture: texture_2d<f32>;

@group(0) @binding(2)
var lut_texture: texture_2d<f32>;

struct FilterParams {
    opacity: f32,
    has_mask: u32,
    mode: u32,
    hsl_hue_turns: f32,
    target_origin_x: i32,
    target_origin_y: i32,
    hsl_saturation_delta: f32,
    hsl_luminosity_delta: f32,
    mask_origin_x: i32,
    mask_origin_y: i32,
    mask_fill: f32,
    _pad4: u32,
};

@group(0) @binding(3)
var<uniform> filter_params: FilterParams;

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

fn gradient_lum_u8(value: vec3<f32>) -> i32 {
    let lum = value.r * 255.0 * 0.3 + value.g * 255.0 * 0.59 + value.b * 255.0 * 0.11;
    return i32(clamp(floor(lum), 0.0, 255.0));
}

fn threshold_lum_u8(value: vec3<f32>) -> i32 {
    let lum = value.r * 255.0 * 0.299 + value.g * 255.0 * 0.587 + value.b * 255.0 * 0.114;
    return i32(clamp(floor(lum), 0.0, 255.0));
}

fn rgb_to_hsv_u8(value: vec3<f32>) -> vec3<f32> {
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

fn hsv_to_rgb_u8(hue: f32, saturation: f32, value: f32) -> vec3<f32> {
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
    return quantize_rgb_u8(rgb);
}

fn apply_hsl_adjust(value: vec3<f32>) -> vec3<f32> {
    let hsv = rgb_to_hsv_u8(value);
    var hue = hsv.x + filter_params.hsl_hue_turns;
    var saturation = hsv.y;
    var luminosity = hsv.z;

    if (filter_params.hsl_luminosity_delta > 0.0) {
        luminosity = luminosity + filter_params.hsl_luminosity_delta * (1.0 - luminosity);
        saturation = saturation - filter_params.hsl_luminosity_delta * saturation;
    } else if (filter_params.hsl_luminosity_delta < 0.0) {
        luminosity = luminosity + filter_params.hsl_luminosity_delta * luminosity;
    }

    if (filter_params.hsl_saturation_delta > 0.0) {
        let inc = filter_params.hsl_saturation_delta * (1.0 - saturation);
        saturation = saturation + inc;
        luminosity = luminosity + luminosity * inc;
    } else if (filter_params.hsl_saturation_delta < 0.0) {
        let dec = -filter_params.hsl_saturation_delta * saturation;
        saturation = saturation - dec;
        luminosity = luminosity - luminosity * dec * 0.5;
    }

    return hsv_to_rgb_u8(hue, clamp(saturation, 0.0, 1.0), clamp(luminosity, 0.0, 1.0));
}

fn load_mask(global_texel: vec2<i32>) -> f32 {
    let mask_texel = global_texel - vec2<i32>(
        filter_params.mask_origin_x,
        filter_params.mask_origin_y,
    );
    let mask_size = textureDimensions(mask_texture);
    if (
        mask_texel.x < 0 ||
        mask_texel.y < 0 ||
        mask_texel.x >= i32(mask_size.x) ||
        mask_texel.y >= i32(mask_size.y)
    ) {
        return filter_params.mask_fill;
    }
    return textureLoad(mask_texture, mask_texel, 0).r;
}
@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(position.xy);
    let before = textureLoad(source_texture, texel, 0);
    var mapped = vec3<f32>(
        textureLoad(lut_texture, vec2<i32>(to_u8(before.r), 0), 0).r,
        textureLoad(lut_texture, vec2<i32>(to_u8(before.g), 0), 0).g,
        textureLoad(lut_texture, vec2<i32>(to_u8(before.b), 0), 0).b,
    );
    if (filter_params.mode == 1u) {
        mapped = textureLoad(lut_texture, vec2<i32>(gradient_lum_u8(before.rgb), 0), 0).rgb;
    } else if (filter_params.mode == 2u) {
        mapped = textureLoad(lut_texture, vec2<i32>(threshold_lum_u8(before.rgb), 0), 0).rgb;
    } else if (filter_params.mode == 3u) {
        mapped = apply_hsl_adjust(before.rgb);
    }
    var strength = clamp(filter_params.opacity, 0.0, 1.0);
    if (filter_params.has_mask == 1u) {
        let mask_texel = texel + vec2<i32>(
            filter_params.target_origin_x,
            filter_params.target_origin_y,
        );
        strength = strength * load_mask(mask_texel);
    }
    let rgb = before.rgb * (1.0 - strength) + mapped * strength;
    return quantize_u8(vec4<f32>(rgb, before.a));
}
"#;
