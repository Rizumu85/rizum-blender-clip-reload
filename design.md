# design.md - Blender `.clip` Native Loader UX

## Project Goal

Let an artist use Clip Studio Paint `.clip` files directly in Blender as image textures, with a flattened full-resolution result that can be refreshed without manual PNG or PSD export.

## User Workflow

1. Install the Blender add-on from `clip_studio_importer.zip`.
2. Use `File > Import > Clip Studio (.clip)` to choose a `.clip` file.
3. The add-on decodes the `.clip`, writes a sidecar PNG cache at `<file>.clip.png`, and loads that PNG as a normal Blender image.
4. When the source `.clip` is saved again, auto-reload watches the file timestamp and refreshes the Blender image after the background decode finishes.
5. If auto-reload is disabled or the user wants an immediate refresh, the Image Editor N-panel exposes `Reload from .clip`.

## Blender UI Surface

- Import menu entry: `File > Import > Clip Studio (.clip)`.
- Image Editor N-panel: `Image > Clip Studio`.
- Add-on preferences:
  - `Auto-reload on .clip change`
  - `Poll interval (seconds)`
  - `Debug log`

## Interaction Principles

- Keep the Blender UI responsive while reloading large `.clip` files.
- Treat the PNG sidecar as a cache, not as the user-facing source of truth.
- Make failures visible through Blender reports for direct actions, and through console/debug logs for background work.
- Avoid adding CSP-editing concepts to Blender. The add-on is read-only and only presents the flattened canvas.

## Current UX Gaps

- Background decode progress is only shown as a small `Decoding in background` label when the image panel is visible.
- Unknown or unsupported layer features are currently console warnings, not surfaced in Blender's UI.
- Import/reload always writes a sidecar PNG next to the `.clip`; there is no cache-location preference.
- Fidelity failures are only visible through rendered image differences; Blender does not yet summarize unsupported layer kinds or skipped semantics in the UI.
