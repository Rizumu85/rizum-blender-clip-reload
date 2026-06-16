pub(super) fn color_balance_lut_rgba(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() < 40 {
        return None;
    }
    let mut vals = [0i32; 10];
    for (index, value) in vals.iter_mut().enumerate() {
        *value = super::read_be_i32(payload, index * 4)?;
    }

    let preserve_luminosity = vals[0] != 0;
    let r_shadow = vals[1];
    let g_shadow = vals[2];
    let b_shadow = vals[3];
    let r_mid = vals[4];
    let g_mid = vals[5];
    let b_mid = vals[6];
    let r_high = vals[7];
    let g_high = vals[8];
    let b_high = vals[9];

    let levels = if preserve_luminosity {
        [
            (
                make_mid(r_mid, g_mid, b_mid),
                make_low(r_shadow, g_shadow, b_shadow),
                make_high(r_high, g_high, b_high),
            ),
            (
                make_mid(g_mid, b_mid, r_mid),
                make_low(g_shadow, b_shadow, r_shadow),
                make_high(g_high, b_high, r_high),
            ),
            (
                make_mid(b_mid, r_mid, g_mid),
                make_low(b_shadow, r_shadow, g_shadow),
                make_high(b_high, r_high, g_high),
            ),
        ]
    } else {
        [
            make_normal_level(r_shadow, r_mid, r_high),
            make_normal_level(g_shadow, g_mid, g_high),
            make_normal_level(b_shadow, b_mid, b_high),
        ]
    };

    let red = level_lut(levels[0]);
    let green = level_lut(levels[1]);
    let blue = level_lut(levels[2]);
    Some(super::rgb_luts_rgba(&red, &green, &blue))
}

fn make_low(a: i32, b: i32, mut c: i32) -> f64 {
    if c <= b {
        c = b;
    }
    let mut value = c - a;
    if c <= a {
        value = 0;
    }
    f64::from(value)
}

fn make_mid(a: i32, b: i32, c: i32) -> f64 {
    0.5 - f64::from(((a * 2) - b) - c) * 0.3 / 400.0
}

fn make_high(a: i32, b: i32, mut c: i32) -> f64 {
    if b <= c {
        c = b;
    }
    if c < a {
        return f64::from((c - a) + 255);
    }
    255.0
}

fn make_normal_level(a: i32, b: i32, c: i32) -> (f64, f64, f64) {
    let mut mid = 0.5 - (f64::from(b) / 100.0) * 0.2;
    let mut low = 0.0;
    let mut high = 255.0;
    if c < 0 {
        mid -= (f64::from(c) / 100.0) * 0.08;
    } else if c > 0 {
        mid -= (f64::from(c) / 100.0) * 0.12;
        high = 255.0 - (f64::from(c) / 100.0) * 96.0;
    }

    if a > 0 {
        mid -= (f64::from(a) / 100.0) * 0.08;
    } else if a < 0 {
        low = -(f64::from(a) / 100.0) * 96.0;
        mid -= (f64::from(a) / 100.0) * 0.12;
    }
    (mid, low, high)
}

fn level_lut(level: (f64, f64, f64)) -> [u8; 256] {
    let (mid, low, high) = level;
    let start = clamp_to_lut_index(low as i32);
    let end = clamp_to_lut_index(high as i32);
    if start >= end {
        return super::identity_lut();
    }

    let mid_index = ((high - low) * mid + low) as i32;
    let mid_t = (f64::from(mid_index - start as i32) / (end - start) as f64).clamp(1e-6, 0.999999);
    let gamma = 0.5f64.ln() / mid_t.ln();

    let mut lut = [0u8; 256];
    lut[..=start].fill(0);
    lut[end..].fill(255);
    for (input, value) in lut.iter_mut().enumerate().take(end).skip(start + 1) {
        let t = ((input - start) as f64 / (end - start) as f64).clamp(0.0, 1.0);
        *value = (t.powf(gamma) * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8;
    }
    lut
}

fn clamp_to_lut_index(value: i32) -> usize {
    value.clamp(0, 255) as usize
}
