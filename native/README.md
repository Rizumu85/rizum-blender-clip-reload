# Native Rewrite

This directory contains the native direct-load rewrite.

Accepted direction:

- Rust renderer core.
- `wgpu` GPU compositor.
- Thin C++ OpenImageIO `ImageInput` adapter for OIIO-level hosts and possible
  Blender ImBuf/source integration.
- Stock Blender image-datablock bridge if no public image filetype
  registration API is available.
- No runtime CPU compositor fallback.
- No Python loader/compositor compatibility layer after native direct-load is accepted.
- No sidecar PNG workflow after native direct-load is accepted.

Architecture details: `../docs/native-code-architecture.md`.
