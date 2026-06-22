# Native Direct-Load Rewrite

Last reconciled: 2026-06-20

## Decision

The accepted product path is a native Rust renderer feeding Blender generated
images. The old Python compositor/loader and sidecar PNG workflow are not part
of the installable add-on.

Accepted runtime:

- Rust `.clip` renderer core.
- `wgpu` GPU compositor.
- Blender add-on bridge that creates/updates generated images.
- Pack-on-import and save-time pack for `.blend` persistence.
- Persistent native worker for reloads.
- C ABI surface for future host integration.
- Optional OIIO adapter experiments outside the current Blender product path.

Rejected runtime paths:

- Maintaining Python and native compositors side by side.
- CPU compositor fallback in the product path.
- Post-processing or approximate visual fixes.
- Sidecar PNGs as the normal Blender workflow.

## Blender Add-on Path

Import:

1. User chooses `File > Import > Clip Studio (.clip)`.
2. The add-on starts the native worker asynchronously.
3. No placeholder image is created.
4. When RGBA8 output returns, Blender creates a generated image.
5. Already-open Image/UV editors switch to the new image.
6. The image is packed in a deferred main-thread step.

Reload:

1. Auto-reload uses mtime/size checks with a 0.1s minimum timer interval.
2. Manual reload uses the same native worker path.
3. Reloads do not pack immediately; they mark the image `Needs Pack`.
4. A `save_pre` handler packs dirty native images before saving the `.blend`.
5. If the source is missing, packed pixels remain visible and the image records
   missing-source status.

Diagnostics:

- Normal UI stays artist-facing and compact.
- Developer Mode exposes render timing and deeper diagnostics.
- Copied/opened diagnostics include renderer version, timings, support details,
  reload diff mode, and errors.

## Native Worker

One-shot worker mode is still useful for simple CLI/Blender calls. Persistent
server mode is preferred for reload because it reuses:

- Native process startup.
- `wgpu` device initialization.
- `RuntimeGpuRenderer`.
- Sparse raster/mask resource state.
- Reload manifest context.

Reload manifests compare graph/source order and raster/mask compressed-tile
fingerprints. They may return:

- `no_change`: no image update required.
- `patch`: only dirty rect payloads are returned.
- `full`: replace the whole image.

## Packaging

`tools/build_blender_addon.py` builds `clip_studio_importer.zip` with:

- `blender_manifest.toml`.
- Add-on Python modules at the extension root.
- `LICENSE` and `NOTICE.md`.
- Native `clip_cli` worker and `clip_capi` library under `native/<platform>/`.

Windows x64 is the current maintainer-tested release platform. Linux x64,
macOS x64, and macOS arm64 package support exists in the builder, but those
packages must be built and uploaded as separate platform-specific extension
zips. Treat Linux/macOS zips as test candidates until real-device testing.

```powershell
python tools\build_blender_addon.py --platform all --output-dir dist
```

The package builder stages one platform at a time, so every output zip contains
only the matching `native/<platform>/` binary directory. Blender's
wheel-oriented `--split-platforms` output was checked and does not filter this
repo's `native/<platform>/` directories by itself. The `Build extension package`
GitHub Actions workflow builds Windows, Linux, and macOS native artifacts and
uploads split platform zips for release review.

## Future Integration

True file-backed `.clip` image loading can be revisited later if Blender exposes
a suitable public filetype/ImageInput path. Until then, generated images are the
accepted stable product path.
