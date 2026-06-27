# Rizum Clip Reload Notices

Rizum Clip Reload is distributed under the GNU General Public License version 3
or any later version.

## Reference Projects

The following projects were reviewed as public prior art or implementation
references. Their source code is not bundled in the extension package unless a
future change explicitly says otherwise.

- Kazuhito00/clip_studio_paint_tool, MIT License, copyright 2024
  KazuhitoTakahashi. Credited for early reference and implementation work.
- Avarel/silicate, MIT License, copyright 2021-2026 An Tran. This project was
  used as an algorithm and architecture reference for tile-local GPU rendering;
  its source code is not bundled or copied into this extension.
- dobrokot/clip_to_psd, MIT License. This project was reviewed as public prior
  art for `.clip` structure research and text-layer metadata behavior,
  including its editable PSD text export path; its source code is not bundled or
  copied into this extension.

If a future change copies a substantial portion of any referenced project, keep the
original MIT copyright and permission notice with that copied material and
update the extension manifest copyright list if needed.

## Text Rendering Dependencies

Native flattened text rendering uses the following third-party libraries as
Rust dependencies. Their source code is not copied into this repository, but the
packaged native worker may link them through the Rust build:

- rust-skia `skia-safe` and `skia-bindings`, MIT License. Used for Skia CPU
  surfaces, text rasterization, `TextBlobBuilder`, and text shaping probes.
- Google Skia, BSD-style license. Used through rust-skia as the underlying 2D
  graphics and text rasterization engine.
- `fontdb`, MIT License. Used to resolve `.clip` font names against installed
  system fonts.
- `ttf-parser`, MIT OR Apache-2.0. Used to read OpenType underline and
  strikethrough metrics.
- `unicode-width`, MIT OR Apache-2.0. Used by the native runtime for Unicode
  text width helpers.

The extension does not bundle font files. Text rendering resolves font names
against fonts installed on the user's operating system.

## MIT License Notice: Kazuhito00/clip_studio_paint_tool

MIT License

Copyright (c) 2024 KazuhitoTakahashi

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
