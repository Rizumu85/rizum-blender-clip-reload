use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process;

mod blender_server;
mod blender_worker;
mod blender_worker_render_profile;
mod blender_worker_sparse;
mod blender_worker_task_graph;
mod compare_png;
mod layer_labels;
mod layer_window;
mod options;
mod performance_plan_json;
mod pixel_trace_text;
mod reload_manifest;
mod runner;
mod support_json;
mod support_text;
mod tile_silo_text;

use options::parse_options;

const USAGE: &str = "usage: clip_cli <file.clip> [--plan-only] [--compare-png <ref.png>] [--blender-render-rgba <out.rgba> --blender-render-json <out.json> [--blender-reload-old-json <manifest.json>]] [--dump-layer-window <id> <x> <y> <radius>] [--dump-layer-rgba <id> <out.rgba>] [--gpu-roundtrip-layer <id>] [--gpu-upload-planned-rasters] [--gpu-draw-layer <id>] [--gpu-simple-stack] [--gpu-support-check] [--gpu-support-json] [--gpu-normal-stack] [--gpu-trace-pixel <x> <y>] [--gpu-trace-layer-pixel <layer> <x> <y>] [--tile-silo-estimate] [--performance-plan-json] [--tile-size <px>] | clip_cli --blender-render-server";

fn main() {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(path) = args.next() else {
        eprintln!("{USAGE}");
        process::exit(2);
    };
    if path == OsString::from("--blender-render-server") {
        process::exit(blender_server::run_blender_render_server());
    }
    let options = match parse_options(args.collect()) {
        Ok(options) => options,
        Err(err) => {
            eprintln!("{err}");
            process::exit(2);
        }
    };
    process::exit(runner::run_file_command(PathBuf::from(path), options));
}
