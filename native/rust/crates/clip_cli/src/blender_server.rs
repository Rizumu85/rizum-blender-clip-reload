use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use clip_runtime::{ClipSession, RuntimeGpuRenderer};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct RenderServerRequest {
    clip_path: Option<PathBuf>,
    rgba_path: Option<PathBuf>,
    json_path: Option<PathBuf>,
    previous_manifest_path: Option<PathBuf>,
    shutdown: Option<bool>,
}

pub(crate) fn run_blender_render_server() -> i32 {
    let renderer = match RuntimeGpuRenderer::new_with_texture_cache() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("failed to initialize persistent native renderer: {err}");
            return 1;
        }
    };

    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                let _ = write_response(&mut stdout, false, &err.to_string());
                return 1;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let request: RenderServerRequest = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                let _ = write_response(&mut stdout, false, &err.to_string());
                continue;
            }
        };
        if request.shutdown.unwrap_or(false) {
            let _ = write_response(&mut stdout, true, "");
            return 0;
        }
        match handle_render_request(request, &renderer) {
            Ok(()) => {
                let _ = write_response(&mut stdout, true, "");
            }
            Err(err) => {
                let _ = write_response(&mut stdout, false, &err);
            }
        }
    }
    0
}

fn handle_render_request(
    request: RenderServerRequest,
    renderer: &RuntimeGpuRenderer,
) -> Result<(), String> {
    let clip_path = request
        .clip_path
        .ok_or_else(|| "server request is missing clip_path".to_string())?;
    let rgba_path = request
        .rgba_path
        .ok_or_else(|| "server request is missing rgba_path".to_string())?;
    let json_path = request
        .json_path
        .ok_or_else(|| "server request is missing json_path".to_string())?;
    let previous_manifest = match request.previous_manifest_path {
        Some(path) => Some(super::reload_manifest::read_reload_manifest(&path)?),
        None => None,
    };
    let mut session = ClipSession::open(&clip_path).map_err(|err| err.to_string())?;
    super::blender_worker::write_blender_render_files_with_renderer(
        &mut session,
        &rgba_path,
        &json_path,
        previous_manifest.as_ref(),
        Some(renderer),
    )
}

fn write_response(stdout: &mut impl Write, ok: bool, error: &str) -> Result<(), serde_json::Error> {
    serde_json::to_writer(
        &mut *stdout,
        &json!({
            "ok": ok,
            "error": error,
        }),
    )?;
    let _ = writeln!(stdout);
    let _ = stdout.flush();
    Ok(())
}
