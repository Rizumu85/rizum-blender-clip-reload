# OpenImageIO Adapter

This directory contains the C++ `ImageInput` plugin for `.clip` files.

The current adapter opens `.clip` files through the Rust C ABI. OpenImageIO owns
plugin discovery and scanline requests; Rust owns `.clip` parsing and pixel
production.

Responsibilities:

- Register the `.clip` extension with OpenImageIO.
- Populate `ImageSpec`.
- Open a Rust `clip_runtime` session through `clip_capi`.
- Forward OIIO pixel requests to Rust.

Non-responsibilities:

- Parsing `.clip` chunks.
- Decoding raster tiles.
- Building render graphs.
- Owning GPU resources.
- Writing sidecar PNG files.
- Falling back to the Python compositor.

The adapter must stay thin. If behavior starts accumulating here, move it into a
Rust module and expose only the required C ABI call.

## Build

Requirements:

- CMake.
- A C++20 compiler.
- An OpenImageIO SDK that matches the OpenImageIO ABI used by the host process
  being tested.
- A built `clip_capi` library from `native/rust`.

Build the Rust ABI library first:

```powershell
cd native\rust
cargo build -p clip_capi --release
```

Example:

```powershell
cmake -S native/oiio -B native/oiio/build `
  -DOpenImageIO_DIR=C:\path\to\OpenImageIO\lib\cmake\OpenImageIO `
  -DCLIP_CAPI_LIBRARY=E:\Documents\Claude\Projects\rizum-blender-clip-reload\native\rust\target\release\clip_capi.dll.lib
cmake --build native/oiio/build --config Release
```

The plugin output is named `clip.imageio.dll` on Windows and `clip.imageio.so`
on Unix-like systems. On Windows, place `clip_capi.dll` beside
`clip.imageio.dll` or otherwise ensure it is on the host process DLL search
path.

## Probe

The CMake project also builds `clip_oiio_probe`, a tiny executable that asks
OpenImageIO to load the plugin and read the current Rust-backed placeholder
image.

```powershell
native\oiio\build\Release\clip_oiio_probe.exe native\oiio\build\Release img\Test_Clipping.clip
```

Expected output shape:

```text
opened clip image: 512x512 channels=4 first_pixel=[0,0,224,255]
```

## Blender/OIIO Discovery

For standalone OpenImageIO tools or a process launched from a shell, point OIIO
at the plugin directory:

```powershell
$env:OPENIMAGEIO_PLUGIN_PATH = "E:\Documents\Claude\Projects\rizum-blender-clip-reload\native\oiio\build\Release"
```

If Blender's bundled OpenImageIO can load external plugins with a compatible
ABI, the same plugin directory should allow `.clip` files to be discovered as an
image format. If discovery fails, the next investigation is plugin search path,
binary naming, or ABI mismatch, not renderer logic.

## Milestone 1 Result

Verified on this machine with Blender 5.0.1 and its bundled OpenImageIO 3.0.9.1:

- `clip.imageio.dll` builds and exports the expected OIIO plugin symbols.
- Blender's bundled Python `OpenImageIO` module can load the plugin, open
  `img/Test_Clipping.clip`, and read the deterministic 64x64 RGBA placeholder.
- Blender's ordinary image loader does not load `.clip` through that external
  OIIO plugin. `bpy.data.images.load()` and `bpy.ops.image.open()` create an
  empty image and log `IMB_load_image_from_memory: unknown file-format`.

The source-level reason is Blender's ImBuf layer. `IMB_load_image_from_memory`
iterates Blender's static `IMB_FILE_TYPES` table. OIIO-backed formats such as
PSD have explicit ImBuf entries that call `imb_oiio_check(..., "psd")` and
`imb_oiio_read(...)`; Blender does not hand arbitrary unknown extensions to
OpenImageIO.

Implication: an external OIIO `ImageInput` plugin is enough for OIIO-level
loading, but not enough by itself for Blender's stock image datablock loader to
accept `.clip` as a true file-backed image format. The stock Blender path is the
image-datablock bridge documented in `native/spikes/blender-image-bridge/` and
`docs/native-code-architecture.md`. If true `bpy.data.images.load(".clip")`
behavior is required later, it needs a Blender ImBuf/source bridge or upstream
change rather than an external OIIO plugin alone.

## Rust ABI Milestone Result

Completed: the adapter now calls the Rust C ABI.

- `clip_renderer_session_open` owns `.clip` parsing.
- `clip_renderer_session_info` provides real canvas dimensions for `ImageSpec`.
- `clip_renderer_session_read_rgba8` provides deterministic placeholder pixels
  until the GPU renderer exists.

The C++ adapter must remain glue only.

Verified on this machine with Blender 5.0.1 and its bundled OpenImageIO 3.0.9.1:

- `img/Test_Clipping.clip` opens as `512x512`, `4` channels, root layer `2`,
  layer count `4`, external data count `7`, first pixel `[0,0,224,255]`.
- `img/Ref_Terra404_Live2D.clip` opens as `4800x6100`, root layer `2`, layer
  count `1212`, external data count `2243`.
- The Blender image-datablock bridge spike consumes the Rust-backed OIIO bytes
  and creates a generated 512x512 image without writing a sidecar PNG.

Current limitation: `IOProxy` support is disabled because the Rust ABI opens
filesystem paths. A future Blender ImBuf/source bridge should add a dedicated
open-from-memory ABI instead of routing memory input through a path-only API.
