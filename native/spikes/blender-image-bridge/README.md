# Blender Image Datablock Bridge Spike

This spike checks the Blender boundary after the OIIO milestone.

The OIIO `.clip` plugin can open through Blender's Python `OpenImageIO` module,
but Blender's stock `bpy.data.images.load(".clip")` path does not call external
OIIO plugins for unknown extensions. This probe therefore tests the next
no-sidecar route:

1. Read RGBA bytes from a native pixel source.
2. Create a generated `bpy.types.Image`.
3. Upload the bytes into the image datablock with `foreach_set`.

This is not a Python compositor and not a fallback renderer. It is only a
Blender API boundary probe for getting native renderer output into an image
datablock without writing a sidecar PNG.

Accepted production persistence:

- Pack the latest rendered pixels into the `.blend` by default.
- Store the source `.clip` path and freshness metadata as image custom
  properties.
- On `.blend` reopen, show the packed last render immediately, then let the
  add-on re-render from `.clip` if the source is present and changed.
- If the `.clip` source is missing, keep the packed pixels visible and report
  the missing source.

Example:

```powershell
& "E:\Program Files\Blender Foundation\Blender 5.0\blender.exe" `
  --background --factory-startup `
  --python native\spikes\blender-image-bridge\probe_image_bridge.py `
  -- --plugin-dir native\oiio\build-blender50 --clip img\Test_Clipping.clip
```

## Result

Verified with Blender 5.0.1 and the Rust-backed OIIO adapter.

- OIIO opens `img/Test_Clipping.clip` through the external `clip.imageio.dll`.
- The probe creates a generated 512x512 Blender `Image` from Rust-backed OIIO
  bytes.
- `image.pixels.foreach_set(...)` uploads the RGBA data without writing any PNG.

Synthetic upload timings from this machine:

| Size | `uint8 -> float32` conversion | Blender `foreach_set + update` |
| --- | ---: | ---: |
| 512x512 | 2.037 ms | 0.805 ms |
| 1024x1024 | 8.325 ms | 3.971 ms |
| 2048x2048 | 32.993 ms | 14.522 ms |
| 4096x4096 | 125.927 ms | 58.130 ms |

The stock Blender datablock bridge is viable as a no-sidecar path. The main
remaining cost at large sizes is Python-side float conversion, not the Blender
bulk pixel upload itself.
