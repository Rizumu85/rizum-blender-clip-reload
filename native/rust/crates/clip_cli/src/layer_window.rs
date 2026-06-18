use std::path::Path;

use clip_model::LayerId;

pub(crate) fn dump_layer_window(
    path: &Path,
    layer_id: LayerId,
    x: u32,
    y: u32,
    radius: u32,
) -> Result<(), String> {
    let image = read_layer_rgba(path, layer_id)?;
    print_rgba_layer_window(&image, layer_id, x, y, radius);

    match clip_file::read_layer_mask_alpha(path, layer_id) {
        Ok(mask) => print_alpha_layer_window(&mask, layer_id, x, y, radius),
        Err(err) => println!("mask window layer={} unavailable={err}", layer_id.0),
    }
    Ok(())
}

pub(crate) fn dump_layer_rgba(
    path: &Path,
    layer_id: LayerId,
    out_path: &Path,
) -> Result<(), String> {
    let image = read_layer_rgba(path, layer_id)?;
    let expected_len = usize::try_from(u64::from(image.width) * u64::from(image.height) * 4)
        .map_err(|_| format!("layer {} byte length does not fit in usize", layer_id.0))?;
    if image.pixels.len() != expected_len {
        return Err(format!(
            "layer {} decoded byte length mismatch: expected {}, got {}",
            layer_id.0,
            expected_len,
            image.pixels.len()
        ));
    }
    std::fs::write(out_path, &image.pixels).map_err(|err| {
        format!(
            "failed to write layer {} RGBA to {out_path:?}: {err}",
            layer_id.0
        )
    })?;
    println!(
        "wrote layer rgba layer={} size={}x{} path={:?}",
        layer_id.0, image.width, image.height, out_path
    );
    Ok(())
}

fn read_layer_rgba(
    path: &Path,
    layer_id: LayerId,
) -> Result<clip_file::tiles::RgbaTileImage, String> {
    clip_file::read_layer_render_rgba(path, layer_id).map_err(|err| {
        format!(
            "failed to read layer render {} from {path:?}: {err}",
            layer_id.0
        )
    })
}

fn print_rgba_layer_window(
    image: &clip_file::tiles::RgbaTileImage,
    layer_id: LayerId,
    x: u32,
    y: u32,
    radius: u32,
) {
    println!(
        "layer window layer={} size={}x{} center=({}, {}) radius={}",
        layer_id.0, image.width, image.height, x, y, radius,
    );
    if image.width == 0 || image.height == 0 {
        return;
    }
    for (sample_x, sample_y) in sample_window(image.width, image.height, x, y, radius) {
        if sample_x == x.saturating_sub(radius) {
            print!("  y={sample_y}:");
        }
        let index = usize::try_from(
            (u64::from(sample_y) * u64::from(image.width) + u64::from(sample_x)) * 4,
        )
        .expect("layer pixel index fits in usize");
        let pixel = &image.pixels[index..index + 4];
        print!(
            " x={sample_x}[{},{},{},{}]",
            pixel[0], pixel[1], pixel[2], pixel[3]
        );
        if sample_x == x.saturating_add(radius).min(image.width - 1) {
            println!();
        }
    }
}

fn print_alpha_layer_window(
    image: &clip_file::tiles::AlphaTileImage,
    layer_id: LayerId,
    x: u32,
    y: u32,
    radius: u32,
) {
    println!(
        "mask window layer={} size={}x{} center=({}, {}) radius={}",
        layer_id.0, image.width, image.height, x, y, radius,
    );
    if image.width == 0 || image.height == 0 {
        return;
    }
    for (sample_x, sample_y) in sample_window(image.width, image.height, x, y, radius) {
        if sample_x == x.saturating_sub(radius) {
            print!("  y={sample_y}:");
        }
        let index =
            usize::try_from(u64::from(sample_y) * u64::from(image.width) + u64::from(sample_x))
                .expect("mask pixel index fits in usize");
        print!(" x={sample_x}[{}]", image.pixels[index]);
        if sample_x == x.saturating_add(radius).min(image.width - 1) {
            println!();
        }
    }
}

fn sample_window(
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    radius: u32,
) -> impl Iterator<Item = (u32, u32)> {
    let min_x = x.saturating_sub(radius);
    let min_y = y.saturating_sub(radius);
    let max_x = x.saturating_add(radius).min(width - 1);
    let max_y = y.saturating_add(radius).min(height - 1);
    (min_y..=max_y)
        .flat_map(move |sample_y| (min_x..=max_x).map(move |sample_x| (sample_x, sample_y)))
}
