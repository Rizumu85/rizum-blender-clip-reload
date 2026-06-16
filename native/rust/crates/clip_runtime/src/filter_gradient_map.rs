const GRADIENT_STOP_DENOMINATOR: f32 = 32768.0 * 256.0 / 255.0;

pub(super) fn gradient_map_lut_rgba(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() < 28 {
        return None;
    }
    let count = super::read_be_i32(payload, 12)?;
    if count <= 0 {
        return None;
    }
    let mut nodes = Vec::new();
    let mut offset = 28usize;
    for _ in 0..count {
        if offset.checked_add(28)? > payload.len() {
            break;
        }
        let r_raw = super::read_be_u32(payload, offset)?;
        let g_raw = super::read_be_u32(payload, offset + 4)?;
        let b_raw = super::read_be_u32(payload, offset + 8)?;
        let stop_raw = super::read_be_u32(payload, offset + 20)?;
        nodes.push((
            stop_raw as f32 / GRADIENT_STOP_DENOMINATOR,
            [
                gradient_color_byte(r_raw),
                gradient_color_byte(g_raw),
                gradient_color_byte(b_raw),
            ],
        ));
        offset += 28;
    }
    if nodes.is_empty() {
        return None;
    }
    nodes.sort_by(|left, right| left.0.total_cmp(&right.0));

    let mut lut_rgba = vec![0u8; 256 * 4];
    for input in 0..256usize {
        let lum = input as f32 / 255.0;
        let color = gradient_map_color_at_lum(lum, &nodes);
        let out = input * 4;
        lut_rgba[out] = color[0];
        lut_rgba[out + 1] = color[1];
        lut_rgba[out + 2] = color[2];
        lut_rgba[out + 3] = 255;
    }
    Some(lut_rgba)
}

fn gradient_color_byte(raw_channel: u32) -> u8 {
    let compact = ((raw_channel >> 16) & 0xffff) as f32;
    (compact / 256.0 + 0.5).floor().clamp(0.0, 255.0) as u8
}

fn gradient_map_color_at_lum(lum: f32, nodes: &[(f32, [u8; 3])]) -> [u8; 3] {
    let (first_pos, first_color) = nodes[0];
    if lum <= first_pos {
        return first_color;
    }
    let (last_pos, last_color) = nodes[nodes.len() - 1];
    if lum >= last_pos {
        return last_color;
    }
    for pair in nodes.windows(2) {
        let (p0, c0) = pair[0];
        let (p1, c1) = pair[1];
        if lum >= p0 && lum <= p1 {
            let t = ((lum - p0) / (p1 - p0).max(1e-6)).clamp(0.0, 1.0);
            return [
                lerp_gradient_byte(c0[0], c1[0], t),
                lerp_gradient_byte(c0[1], c1[1], t),
                lerp_gradient_byte(c0[2], c1[2], t),
            ];
        }
    }
    last_color
}

fn lerp_gradient_byte(start: u8, end: u8, t: f32) -> u8 {
    (f32::from(start) * (1.0 - t) + f32::from(end) * t + 0.5)
        .floor()
        .clamp(0.0, 255.0) as u8
}
