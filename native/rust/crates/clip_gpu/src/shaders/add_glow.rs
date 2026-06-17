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
        rgb = (b * dst + rgb * src_a + denom / 2) / denom;
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
        src_a = div255(src_a * to_u8(load_mask(texel + source_params.target_origin)));
    }
    let opacity_u8 = i32(clamp(floor(source_params.opacity * 256.0 + 0.5), 0.0, 256.0));
    src_a = (src_a * opacity_u8) / 256;
    if (src_a == 0) {
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
"#;
