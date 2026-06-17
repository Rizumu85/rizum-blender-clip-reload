#[derive(Clone, Copy, Debug)]
pub(crate) enum PlannedLutFilterMode {
    ToneCurveRgb,
    GradientMapLum,
    ThresholdLum,
    Hsl {
        hue_turns: f32,
        saturation_delta: f32,
        luminosity_delta: f32,
    },
}

const TONE_CURVE_COMPACT_STRIDE: usize = 0x82;
const TONE_CURVE_TABLE_SIZE: usize = 0x10000;
const TONE_CURVE_BYTE_SAMPLE_STEP: usize = 257;
const FILTER_TYPE_BRIGHTNESS_CONTRAST: u32 = 1;
const FILTER_TYPE_LEVEL_CORRECTION: u32 = 2;
const FILTER_TYPE_TONE_CURVE: u32 = 3;
const FILTER_TYPE_HSL: u32 = 4;
const FILTER_TYPE_COLOR_BALANCE: u32 = 5;
const FILTER_TYPE_INVERT: u32 = 6;
const FILTER_TYPE_POSTERIZATION: u32 = 7;
const FILTER_TYPE_THRESHOLD: u32 = 8;
const FILTER_TYPE_GRADIENT_MAP: u32 = 9;

pub(crate) fn lut_filter_rgba(
    filter_type: u32,
    payload: &[u8],
) -> Option<(&'static str, PlannedLutFilterMode, Vec<u8>)> {
    match filter_type {
        FILTER_TYPE_BRIGHTNESS_CONTRAST => Some((
            "BrightnessContrast",
            PlannedLutFilterMode::ToneCurveRgb,
            brightness_contrast_lut_rgba(payload)?,
        )),
        FILTER_TYPE_LEVEL_CORRECTION => Some((
            "LevelCorrection",
            PlannedLutFilterMode::ToneCurveRgb,
            level_correction_lut_rgba(payload)?,
        )),
        FILTER_TYPE_TONE_CURVE => Some((
            "ToneCurve",
            PlannedLutFilterMode::ToneCurveRgb,
            tone_curve_lut_rgba(payload)?,
        )),
        FILTER_TYPE_HSL => {
            let (hue_turns, saturation_delta, luminosity_delta) = hsl_params(payload)?;
            Some((
                "HueSaturationLuminosity",
                PlannedLutFilterMode::Hsl {
                    hue_turns,
                    saturation_delta,
                    luminosity_delta,
                },
                identity_lut_rgba(),
            ))
        }
        FILTER_TYPE_COLOR_BALANCE => Some((
            "ColorBalance",
            PlannedLutFilterMode::ToneCurveRgb,
            color_balance::color_balance_lut_rgba(payload)?,
        )),
        FILTER_TYPE_INVERT => Some((
            "Invert",
            PlannedLutFilterMode::ToneCurveRgb,
            invert_lut_rgba(),
        )),
        FILTER_TYPE_POSTERIZATION => Some((
            "Posterization",
            PlannedLutFilterMode::ToneCurveRgb,
            posterization_lut_rgba(payload)?,
        )),
        FILTER_TYPE_THRESHOLD => Some((
            "Threshold",
            PlannedLutFilterMode::ThresholdLum,
            threshold_lut_rgba(payload)?,
        )),
        FILTER_TYPE_GRADIENT_MAP => Some((
            "GradientMap",
            PlannedLutFilterMode::GradientMapLum,
            gradient_map::gradient_map_lut_rgba(payload)?,
        )),
        _ => None,
    }
}

fn brightness_contrast_lut_rgba(payload: &[u8]) -> Option<Vec<u8>> {
    let brightness = read_be_i32(payload, 0)?;
    let contrast = read_be_i32(payload, 4)?;
    let brightness_lut = brightness_lut(brightness);
    let contrast_lut = contrast_lut(contrast);
    let mut combined = [0u8; 256];
    for input in 0..256usize {
        combined[input] = contrast_lut[usize::from(brightness_lut[input])];
    }
    Some(uniform_rgb_lut_rgba(&combined))
}

fn level_correction_lut_rgba(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.len() < 0x40 {
        return None;
    }
    let group = [
        read_be_u16(payload, 0)?,
        read_be_u16(payload, 2)?,
        read_be_u16(payload, 4)?,
        read_be_u16(payload, 6)?,
        read_be_u16(payload, 8)?,
    ];
    Some(uniform_rgb_lut_rgba(&level_correction_lut(group)))
}

fn tone_curve_lut_rgba(payload: &[u8]) -> Option<Vec<u8>> {
    let curves = tone_curve_compact_curves(payload)?;
    if curves.is_empty() {
        return None;
    }
    let mut luts = Vec::with_capacity(curves.len().min(4));
    for curve in curves.iter().take(4) {
        luts.push(tone_curve_compact_lut16(curve)?);
    }
    let master = &luts[0];
    let mut lut_rgba = vec![0u8; 256 * 4];
    for input in 0..256usize {
        let offset = input * 4;
        let sample_index = input * TONE_CURVE_BYTE_SAMPLE_STEP;
        let red_index = luts
            .get(1)
            .map(|lut| usize::from(lut[sample_index]))
            .unwrap_or(sample_index);
        let green_index = luts
            .get(2)
            .map(|lut| usize::from(lut[sample_index]))
            .unwrap_or(sample_index);
        let blue_index = luts
            .get(3)
            .map(|lut| usize::from(lut[sample_index]))
            .unwrap_or(sample_index);
        lut_rgba[offset] = (master[red_index] >> 8) as u8;
        lut_rgba[offset + 1] = (master[green_index] >> 8) as u8;
        lut_rgba[offset + 2] = (master[blue_index] >> 8) as u8;
        lut_rgba[offset + 3] = 255;
    }
    Some(lut_rgba)
}

fn invert_lut_rgba() -> Vec<u8> {
    let mut lut = [0u8; 256];
    for (input, value) in lut.iter_mut().enumerate() {
        *value = 255 - input as u8;
    }
    uniform_rgb_lut_rgba(&lut)
}

fn hsl_params(payload: &[u8]) -> Option<(f32, f32, f32)> {
    // The native per-pixel routine consumes fixed-point arguments, but the
    // SQLite filter payload is only partly pre-scaled by the caller.
    Some((
        read_be_i32(payload, 0)? as f32 / 360.0,
        read_be_i32(payload, 4)? as f32 / 100.0,
        read_be_i32(payload, 8)? as f32 / 100.0,
    ))
}

fn posterization_lut_rgba(payload: &[u8]) -> Option<Vec<u8>> {
    let levels = read_be_i32(payload, 0)?.max(2) as i64;
    let mut lut = [0u8; 256];
    for (input, value) in lut.iter_mut().enumerate() {
        let bin = ((input as i64 * levels) / 256).min(levels - 1);
        *value = ((bin as f32 * 255.0 / (levels - 1) as f32) + 0.5)
            .floor()
            .clamp(0.0, 255.0) as u8;
    }
    Some(uniform_rgb_lut_rgba(&lut))
}

fn threshold_lut_rgba(payload: &[u8]) -> Option<Vec<u8>> {
    let threshold = read_be_i32(payload, 0)?;
    let mut lut = [0u8; 256];
    for (luminosity, value) in lut.iter_mut().enumerate() {
        *value = if luminosity as i32 >= threshold {
            255
        } else {
            0
        };
    }
    Some(uniform_rgb_lut_rgba(&lut))
}

fn linear_lut(start_x: i32, start_y: i32, end_x: i32, end_y: i32) -> [u8; 256] {
    let mut lut = [0u8; 256];
    if start_x > 0 {
        let end = start_x.min(256) as usize;
        lut[..end].fill(clamp_i32_to_byte(start_y));
    }

    let span = end_x - start_x;
    let slope = if span != 0 {
        (end_y - start_y) as f32 / span as f32
    } else {
        0.0
    };
    let lo = start_x.max(0);
    let hi = end_x.min(256);
    if lo < hi {
        for x in lo..hi {
            lut[x as usize] = (x as f32 * slope + (start_y as f32 - start_x as f32 * slope) + 0.5)
                .floor()
                .clamp(0.0, 255.0) as u8;
        }
    }

    if end_x < 256 {
        let start = end_x.max(0) as usize;
        lut[start..].fill(clamp_i32_to_byte(end_y));
    }
    lut
}

fn brightness_lut(amount: i32) -> [u8; 256] {
    let amount = amount.clamp(-127, 127);
    if amount > 0 {
        return linear_lut(amount, 0, 255, 255 - amount);
    }
    if amount < 0 {
        return linear_lut(0, -amount, 255, 255);
    }
    identity_lut()
}

fn contrast_lut(amount: i32) -> [u8; 256] {
    if amount == 0 || !(-127..128).contains(&amount) {
        return identity_lut();
    }
    if amount > 0 {
        return linear_lut(amount, 0, 255 - amount, 255);
    }
    linear_lut(0, -amount, 255, 255 + amount)
}

fn level_correction_lut(group: [u16; 5]) -> [u8; 256] {
    let [in_low_raw, mid_raw, in_high_raw, out_low_raw, out_high_raw] = group;
    let in_low = f32::from(in_low_raw) * 255.0 / 65535.0;
    let mid = f32::from(mid_raw) * 255.0 / 65535.0;
    let in_high = f32::from(in_high_raw) * 255.0 / 65535.0;
    let out_low = f32::from(out_low_raw) * 255.0 / 65535.0;
    let out_high = f32::from(out_high_raw) * 255.0 / 65535.0;
    if in_high <= in_low {
        return identity_lut();
    }

    let mid_t = ((mid - in_low) / (in_high - in_low)).clamp(1e-4, 0.9999);
    let gamma = 0.5f32.ln() / mid_t.ln();
    let exponent = 1.0 / gamma.max(1e-4);
    let mut lut = [0u8; 256];
    for (input, value) in lut.iter_mut().enumerate() {
        let t = ((input as f32 - in_low) / (in_high - in_low)).clamp(0.0, 1.0);
        let y = out_low + t.powf(exponent) * (out_high - out_low);
        *value = (y + 0.5).floor().clamp(0.0, 255.0) as u8;
    }
    lut
}

fn tone_curve_compact_curves(payload: &[u8]) -> Option<Vec<Vec<(u16, u16)>>> {
    if !payload.len().is_multiple_of(TONE_CURVE_COMPACT_STRIDE) {
        return None;
    }
    let mut curves = Vec::with_capacity(payload.len() / TONE_CURVE_COMPACT_STRIDE);
    for chunk in payload.chunks_exact(TONE_CURVE_COMPACT_STRIDE) {
        let count = read_be_u16(chunk, 0)? as usize;
        if count > 32 {
            return None;
        }
        let mut points = Vec::with_capacity(count);
        for point_index in 0..count {
            let point_offset = 2 + point_index * 4;
            let x = read_be_u16(chunk, point_offset)?;
            let y = read_be_u16(chunk, point_offset + 2)?;
            points.push((x, y));
        }
        curves.push(points);
    }
    Some(curves)
}

fn tone_curve_compact_lut16(points: &[(u16, u16)]) -> Option<Vec<u16>> {
    if points.len() < 2 {
        return Some(
            (0..TONE_CURVE_TABLE_SIZE)
                .map(|value| value as u16)
                .collect(),
        );
    }

    let pts: Vec<(f64, f64)> = points
        .iter()
        .map(|(x, y)| (f64::from(*x), f64::from(*y)))
        .collect();
    let mut table = vec![0.0f64; TONE_CURVE_TABLE_SIZE];
    let step_x = (i32::from(points.last()?.0) - i32::from(points.first()?.0)).abs() as f64
        / TONE_CURVE_TABLE_SIZE as f64;
    if step_x <= 0.0 {
        return None;
    }

    if pts.len() == 2 {
        let (x0, y0) = pts[0];
        let (x1, y1) = pts[1];
        let mut sample_x = x0;
        for value in &mut table {
            *value = if x1 == x0 {
                y0
            } else {
                ((y1 - y0) / (x1 - x0)) * (sample_x - x0) + y0
            };
            sample_x += step_x;
        }
    } else {
        let mut have_previous = false;
        let mut previous_x = 0.0;
        let mut previous_y = 0.0;
        let mut base_x = 0.0;
        let mut previous_out_idx = 0usize;
        for curve_idx in 1..pts.len() - 1 {
            let (mut x_prev, mut y_prev) = pts[curve_idx - 1];
            let (x_mid, y_mid) = pts[curve_idx];
            let (mut x_next, mut y_next) = pts[curve_idx + 1];
            if curve_idx == 1 {
                x_prev -= x_mid - x_prev;
                y_prev -= y_mid - y_prev;
            }
            if curve_idx == pts.len() - 2 {
                x_next -= x_mid - x_next;
                y_next -= y_mid - y_next;
            }

            let mut segment_previous_x = previous_x;
            for sample_idx in 0..33 {
                let t = f64::from(sample_idx) / 32.0;
                let w_prev = (1.0 - t) * (1.0 - t) * 0.5;
                let w_next = t * t * 0.5;
                let w_mid = (t - t * t) + 0.5;
                let x = x_prev * w_prev + x_mid * w_mid + x_next * w_next;
                let y = y_prev * w_prev + y_mid * w_mid + y_next * w_next;

                if have_previous {
                    let lo = x.min(segment_previous_x);
                    let hi = x.max(segment_previous_x);
                    let mut sample_offset = 0.0;
                    while sample_offset <= hi - lo + 1e-9 {
                        let sample_x = sample_offset + lo;
                        let out_idx = ((sample_x - base_x) / step_x + 0.5) as i32;
                        if (0..TONE_CURVE_TABLE_SIZE as i32).contains(&out_idx) {
                            let sample_y = if x == segment_previous_x {
                                previous_y
                            } else {
                                ((y - previous_y) / (x - segment_previous_x))
                                    * (sample_x - segment_previous_x)
                                    + previous_y
                            };
                            let out_idx = out_idx as usize;
                            table[out_idx] = sample_y;
                            let fill_start = previous_out_idx + 1;
                            if fill_start < out_idx {
                                table[fill_start..out_idx].fill(sample_y);
                            }
                            previous_out_idx = out_idx;
                        }
                        sample_offset += step_x;
                    }
                } else {
                    base_x = x;
                }
                have_previous = true;
                segment_previous_x = x;
                previous_y = y;
            }
            previous_x = segment_previous_x;
        }
    }

    let mut lut: Vec<u16> = table
        .iter()
        .map(|value| (value + 0.5).floor().clamp(0.0, 65535.0) as u16)
        .collect();
    if points.first()?.1 == 0 {
        lut[0] = 0;
    }
    if points.last()?.1 == u16::MAX {
        lut[TONE_CURVE_TABLE_SIZE - 1] = u16::MAX;
    }
    Some(lut)
}

fn uniform_rgb_lut_rgba(lut: &[u8; 256]) -> Vec<u8> {
    rgb_luts_rgba(lut, lut, lut)
}

fn identity_lut_rgba() -> Vec<u8> {
    uniform_rgb_lut_rgba(&identity_lut())
}

fn rgb_luts_rgba(red: &[u8; 256], green: &[u8; 256], blue: &[u8; 256]) -> Vec<u8> {
    let mut lut_rgba = vec![0u8; 256 * 4];
    for input in 0..256usize {
        let offset = input * 4;
        lut_rgba[offset] = red[input];
        lut_rgba[offset + 1] = green[input];
        lut_rgba[offset + 2] = blue[input];
        lut_rgba[offset + 3] = 255;
    }
    lut_rgba
}

fn identity_lut() -> [u8; 256] {
    let mut lut = [0u8; 256];
    for (index, value) in lut.iter_mut().enumerate() {
        *value = index as u8;
    }
    lut
}

fn clamp_i32_to_byte(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

fn read_be_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

#[cfg(test)]
#[path = "filter_lut_tests.rs"]
mod tests;

#[path = "filter_color_balance.rs"]
mod color_balance;

#[path = "filter_gradient_map.rs"]
mod gradient_map;
