use std::path::Path;

pub fn read_reload_manifest(path: &Path) -> Result<clip_runtime::ReloadDiffManifest, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    serde_json::from_slice(&bytes).map_err(|err| err.to_string())
}
