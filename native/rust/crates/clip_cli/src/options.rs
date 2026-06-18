use std::ffi::OsString;
use std::path::PathBuf;

use clip_model::LayerId;

#[derive(Debug, Default, Eq, PartialEq)]
pub struct CliOptions {
    pub plan_only: bool,
    pub gpu_roundtrip_layer_id: Option<LayerId>,
    pub gpu_upload_planned_rasters: bool,
    pub gpu_draw_layer_id: Option<LayerId>,
    pub gpu_simple_stack: bool,
    pub gpu_support_check: bool,
    pub gpu_support_json: bool,
    pub gpu_normal_stack: bool,
    pub gpu_trace_pixel: Option<(u32, u32)>,
    pub gpu_trace_layer_pixel: Option<(LayerId, u32, u32)>,
    pub tile_silo_estimate: bool,
    pub tile_size: u32,
    pub dump_layer_window: Option<(LayerId, u32, u32, u32)>,
    pub dump_layer_rgba: Option<(LayerId, PathBuf)>,
    pub compare_png_path: Option<PathBuf>,
    pub blender_render_rgba_path: Option<PathBuf>,
    pub blender_render_json_path: Option<PathBuf>,
    pub blender_reload_old_json_path: Option<PathBuf>,
}

pub fn parse_options(args: Vec<OsString>) -> Result<CliOptions, String> {
    let mut options = CliOptions {
        tile_size: 256,
        ..CliOptions::default()
    };
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "--gpu-roundtrip-layer" {
            let layer_id = parse_next_u32(&mut iter, "--gpu-roundtrip-layer")?;
            options.gpu_roundtrip_layer_id = Some(LayerId(layer_id));
        } else if arg == "--gpu-upload-planned-rasters" {
            options.gpu_upload_planned_rasters = true;
        } else if arg == "--gpu-draw-layer" {
            let layer_id = parse_next_u32(&mut iter, "--gpu-draw-layer")?;
            options.gpu_draw_layer_id = Some(LayerId(layer_id));
        } else if arg == "--gpu-simple-stack" {
            options.gpu_simple_stack = true;
        } else if arg == "--gpu-support-check" {
            options.gpu_support_check = true;
        } else if arg == "--gpu-support-json" {
            options.gpu_support_json = true;
        } else if arg == "--gpu-normal-stack" {
            options.gpu_normal_stack = true;
        } else if arg == "--gpu-trace-pixel" {
            let x = parse_next_u32(&mut iter, "--gpu-trace-pixel x")?;
            let y = parse_next_u32(&mut iter, "--gpu-trace-pixel y")?;
            options.gpu_trace_pixel = Some((x, y));
        } else if arg == "--gpu-trace-layer-pixel" {
            let layer_id = parse_next_u32(&mut iter, "--gpu-trace-layer-pixel layer id")?;
            let x = parse_next_u32(&mut iter, "--gpu-trace-layer-pixel x")?;
            let y = parse_next_u32(&mut iter, "--gpu-trace-layer-pixel y")?;
            options.gpu_trace_layer_pixel = Some((LayerId(layer_id), x, y));
        } else if arg == "--tile-silo-estimate" {
            options.tile_silo_estimate = true;
        } else if arg == "--tile-size" {
            let tile_size = parse_next_u32(&mut iter, "--tile-size")?;
            if tile_size == 0 {
                return Err("--tile-size must be greater than zero".to_string());
            }
            options.tile_size = tile_size;
        } else if arg == "--dump-layer-window" {
            let layer_id = parse_next_u32(&mut iter, "--dump-layer-window layer id")?;
            let x = parse_next_u32(&mut iter, "--dump-layer-window x")?;
            let y = parse_next_u32(&mut iter, "--dump-layer-window y")?;
            let radius = parse_next_u32(&mut iter, "--dump-layer-window radius")?;
            options.dump_layer_window = Some((LayerId(layer_id), x, y, radius));
        } else if arg == "--dump-layer-rgba" {
            let layer_id = parse_next_u32(&mut iter, "--dump-layer-rgba layer id")?;
            let path = next_os_string(&mut iter, "--dump-layer-rgba output path")?;
            options.dump_layer_rgba = Some((LayerId(layer_id), PathBuf::from(path)));
        } else if arg == "--compare-png" {
            let path = next_os_string(&mut iter, "--compare-png")?;
            options.compare_png_path = Some(PathBuf::from(path));
        } else if arg == "--blender-render-rgba" {
            let path = next_os_string(&mut iter, "--blender-render-rgba")?;
            options.blender_render_rgba_path = Some(PathBuf::from(path));
        } else if arg == "--blender-render-json" {
            let path = next_os_string(&mut iter, "--blender-render-json")?;
            options.blender_render_json_path = Some(PathBuf::from(path));
        } else if arg == "--blender-reload-old-json" {
            let path = next_os_string(&mut iter, "--blender-reload-old-json")?;
            options.blender_reload_old_json_path = Some(PathBuf::from(path));
        } else if arg == "--plan-only" {
            options.plan_only = true;
        } else {
            return Err(format!("unknown argument {:?}", arg));
        }
    }
    Ok(options)
}

fn parse_next_u32(iter: &mut impl Iterator<Item = OsString>, label: &str) -> Result<u32, String> {
    let value = next_os_string(iter, label)?;
    value
        .to_str()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| format!("invalid integer for {label}"))
}

fn next_os_string(
    iter: &mut impl Iterator<Item = OsString>,
    label: &str,
) -> Result<OsString, String> {
    iter.next()
        .ok_or_else(|| format!("missing value for {label}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_trace_and_tile_options() {
        let options = parse_options(args(&[
            "--gpu-trace-layer-pixel",
            "42",
            "10",
            "20",
            "--tile-silo-estimate",
            "--tile-size",
            "128",
        ]))
        .unwrap();

        assert_eq!(options.gpu_trace_layer_pixel, Some((LayerId(42), 10, 20)));
        assert!(options.tile_silo_estimate);
        assert_eq!(options.tile_size, 128);
    }

    #[test]
    fn rejects_missing_values_without_exiting_process() {
        let err = parse_options(args(&["--gpu-trace-pixel", "10"])).unwrap_err();

        assert_eq!(err, "missing value for --gpu-trace-pixel y");
    }

    #[test]
    fn rejects_zero_tile_size() {
        let err = parse_options(args(&["--tile-size", "0"])).unwrap_err();

        assert_eq!(err, "--tile-size must be greater than zero");
    }
}
