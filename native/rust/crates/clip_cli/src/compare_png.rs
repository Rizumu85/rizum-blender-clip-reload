use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use clip_model::Rect;
use clip_runtime::ClipSession;

#[derive(Debug)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn render_full_image(session: &mut ClipSession) -> Result<RgbaImage, String> {
    let canvas = session.summary().canvas;
    let byte_len = usize::try_from(u64::from(canvas.width) * u64::from(canvas.height) * 4)
        .map_err(|_| "canvas byte length does not fit in usize".to_string())?;
    let mut pixels = vec![0u8; byte_len];
    session
        .read_rgba8_region(Rect::new(0, 0, canvas.width, canvas.height), &mut pixels)
        .map_err(|err| err.to_string())?;
    Ok(RgbaImage {
        width: canvas.width,
        height: canvas.height,
        pixels,
    })
}

pub fn read_png_rgba8(path: &Path) -> Result<RgbaImage, String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let decoder = png::Decoder::new(BufReader::new(file));
    let mut reader = decoder.read_info().map_err(|err| err.to_string())?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|err| err.to_string())?;
    let bytes = &buffer[..info.buffer_size()];
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!("unsupported PNG bit depth {:?}", info.bit_depth));
    }

    let pixels = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity((bytes.len() / 3) * 4);
            for pixel in bytes.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
            rgba
        }
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity((bytes.len() / 2) * 4);
            for pixel in bytes.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
            rgba
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity(bytes.len() * 4);
            for gray in bytes {
                rgba.extend_from_slice(&[*gray, *gray, *gray, 255]);
            }
            rgba
        }
        png::ColorType::Indexed => {
            return Err("indexed PNG references are not supported".to_string());
        }
    };

    let expected_len = usize::try_from(u64::from(info.width) * u64::from(info.height) * 4)
        .map_err(|_| "PNG byte length does not fit in usize".to_string())?;
    if pixels.len() != expected_len {
        return Err(format!(
            "decoded PNG byte length mismatch: got {} expected {}",
            pixels.len(),
            expected_len,
        ));
    }

    Ok(RgbaImage {
        width: info.width,
        height: info.height,
        pixels,
    })
}

pub fn compare_png_report(
    reference_path: &Path,
    rendered: &RgbaImage,
    reference: &RgbaImage,
) -> Result<String, String> {
    if rendered.width != reference.width || rendered.height != reference.height {
        return Err(format!(
            "image size mismatch: rendered={}x{} reference={}x{}",
            rendered.width, rendered.height, reference.width, reference.height,
        ));
    }
    let stats = compare_images(&rendered.pixels, &reference.pixels, rendered.width);
    Ok(format!(
        "compare png ref={:?} size={}x{} raw_max={} raw_max_at=({},{},{}) raw_mean={:.6} raw_diff_px={} raw_visible_px={} premul_max={} premul_max_at=({},{},{}) premul_mean={:.6} premul_diff_px={} premul_visible_px={}",
        reference_path,
        rendered.width,
        rendered.height,
        stats.raw_max,
        stats.raw_max_at.x,
        stats.raw_max_at.y,
        channel_name(stats.raw_max_at.channel),
        stats.raw_mean,
        stats.raw_diff_px,
        stats.raw_visible_px,
        stats.premul_max,
        stats.premul_max_at.x,
        stats.premul_max_at.y,
        channel_name(stats.premul_max_at.channel),
        stats.premul_mean,
        stats.premul_diff_px,
        stats.premul_visible_px,
    ))
}

#[derive(Debug)]
struct CompareStats {
    raw_max: u8,
    raw_max_at: MaxLocation,
    raw_mean: f64,
    raw_diff_px: usize,
    raw_visible_px: usize,
    premul_max: u8,
    premul_max_at: MaxLocation,
    premul_mean: f64,
    premul_diff_px: usize,
    premul_visible_px: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct MaxLocation {
    x: u32,
    y: u32,
    channel: usize,
}

fn compare_images(actual: &[u8], reference: &[u8], width: u32) -> CompareStats {
    assert_eq!(actual.len(), reference.len());
    assert_eq!(actual.len() % 4, 0);

    let mut raw_max = 0u8;
    let mut raw_max_at = MaxLocation {
        x: 0,
        y: 0,
        channel: 0,
    };
    let mut raw_abs_sum = 0u64;
    let mut raw_diff_px = 0usize;
    let mut raw_visible_px = 0usize;
    let mut premul_max = 0u8;
    let mut premul_max_at = MaxLocation {
        x: 0,
        y: 0,
        channel: 0,
    };
    let mut premul_abs_sum = 0u64;
    let mut premul_diff_px = 0usize;
    let mut premul_visible_px = 0usize;

    for (pixel_index, (actual_px, reference_px)) in actual
        .chunks_exact(4)
        .zip(reference.chunks_exact(4))
        .enumerate()
    {
        let mut raw_pixel_max = 0u8;
        let mut premul_pixel_max = 0u8;
        for channel in 0..4 {
            let diff = u8_abs_diff(actual_px[channel], reference_px[channel]);
            raw_pixel_max = raw_pixel_max.max(diff);
            raw_abs_sum += u64::from(diff);
            if diff > raw_max {
                raw_max = diff;
                raw_max_at = max_location(pixel_index, width, channel);
            }

            let actual_value = premul_channel(actual_px, channel);
            let reference_value = premul_channel(reference_px, channel);
            let premul_diff = u8_abs_diff(actual_value, reference_value);
            premul_pixel_max = premul_pixel_max.max(premul_diff);
            premul_abs_sum += u64::from(premul_diff);
            if premul_diff > premul_max {
                premul_max = premul_diff;
                premul_max_at = max_location(pixel_index, width, channel);
            }
        }
        if raw_pixel_max > 0 {
            raw_diff_px += 1;
        }
        if raw_pixel_max > 1 {
            raw_visible_px += 1;
        }
        if premul_pixel_max > 0 {
            premul_diff_px += 1;
        }
        if premul_pixel_max > 1 {
            premul_visible_px += 1;
        }
    }

    let channel_count = actual.len() as f64;
    CompareStats {
        raw_max,
        raw_max_at,
        raw_mean: raw_abs_sum as f64 / channel_count,
        raw_diff_px,
        raw_visible_px,
        premul_max,
        premul_max_at,
        premul_mean: premul_abs_sum as f64 / channel_count,
        premul_diff_px,
        premul_visible_px,
    }
}

fn max_location(pixel_index: usize, width: u32, channel: usize) -> MaxLocation {
    let pixel_index = u32::try_from(pixel_index).expect("image pixel index fits in u32");
    MaxLocation {
        x: pixel_index % width,
        y: pixel_index / width,
        channel,
    }
}

fn channel_name(channel: usize) -> &'static str {
    match channel {
        0 => "r",
        1 => "g",
        2 => "b",
        3 => "a",
        _ => "?",
    }
}

fn premul_channel(pixel: &[u8], channel: usize) -> u8 {
    if channel == 3 {
        pixel[3]
    } else {
        (((u16::from(pixel[channel]) * u16::from(pixel[3])) + 127) / 255) as u8
    }
}

fn u8_abs_diff(lhs: u8, rhs: u8) -> u8 {
    lhs.max(rhs) - lhs.min(rhs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_compare_report_with_raw_and_premul_stats() {
        let rendered = RgbaImage {
            width: 1,
            height: 1,
            pixels: vec![10, 20, 30, 128],
        };
        let reference = RgbaImage {
            width: 1,
            height: 1,
            pixels: vec![12, 20, 26, 128],
        };

        let report = compare_png_report(Path::new("ref.png"), &rendered, &reference).unwrap();

        assert!(report.contains("compare png ref=\"ref.png\" size=1x1"));
        assert!(report.contains("raw_max=4"));
        assert!(report.contains("premul_max=2"));
    }

    #[test]
    fn reports_size_mismatch() {
        let rendered = RgbaImage {
            width: 1,
            height: 1,
            pixels: vec![0, 0, 0, 0],
        };
        let reference = RgbaImage {
            width: 2,
            height: 1,
            pixels: vec![0; 8],
        };

        let err = compare_png_report(Path::new("ref.png"), &rendered, &reference).unwrap_err();

        assert_eq!(err, "image size mismatch: rendered=1x1 reference=2x1");
    }
}
