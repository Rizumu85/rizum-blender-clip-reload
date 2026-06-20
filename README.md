# Rizum Clip Reload

Import Clip Studio Paint `.clip` files into Blender as packed, reloadable image
textures.

- [English](#english)
- [简体中文](#简体中文)

---

## English

## What It Does

Rizum Clip Reload lets Blender open Clip Studio Paint artwork from `.clip` files
as a flattened image texture. It is made for artists who draw in Clip Studio
Paint and use Blender for materials, layout, animation, or reference work.

You can import a `.clip` file, keep the rendered image packed inside the
`.blend`, and reload it when the source file changes. The extension focuses on
the final visual result. It does not turn Clip Studio layers into editable
Blender layers.

## Install

1. Use Blender 4.2 or newer.
2. Open `Edit > Preferences > Get Extensions`.
3. Choose `Install from Disk...`.
4. Select `clip_studio_importer.zip`.
5. Enable `Rizum Clip Reload`.

## Use

1. In Blender, choose `File > Import > Clip Studio (.clip)`.
2. Select your `.clip` file.
3. Blender creates a generated image from the file.
4. The image is packed into the `.blend`.
5. After editing the `.clip` in Clip Studio Paint, save it again.
6. Blender can reload the image automatically, or you can press `Manual Reload`
   in the Image Editor sidebar.

If the original `.clip` file is missing later, Blender keeps showing the last
packed render.

## Current Status

Package version: `0.8.67`.

Works today:

- Raster layers and full-color artwork.
- Paper/background layers.
- Folders and clipped layers.
- Layer masks and opacity.
- Common blend modes.
- Adjustment and filter layers covered by current verification samples.
- Manual reload and non-blocking auto-reload.
- Packed image persistence inside `.blend` files.
- Clear status messages for missing sources, render errors, and pack state.
- UI translations for Simplified Chinese, Japanese, and Spanish.

Not in scope right now:

- Editable layer import.
- Vector strokes or fills.
- Text layers.
- Bubble/frame renderers.
- 3D layers or animation timelines.
- Writing changes back to `.clip` files.

## Build Package

Build the native renderer first:

```powershell
cd native\rust
cargo build --release -q -p clip_capi
cargo build --release -q -p clip_cli
cd ..\..
```

Then build the installable extension zip:

```powershell
python tools\build_blender_addon.py
```

The script writes `clip_studio_importer.zip`. The zip contains the Blender
extension files, `LICENSE`, `NOTICE.md`, and the native renderer files under
`native/<platform>/`.

By default the script packages the current host platform. Windows x64 is tested
on the maintainer's machine. Linux x64, macOS x64, and macOS arm64 package
support is present but maintainer-untested because the maintainer does not have
those devices. Build those packages on matching machines, or pass explicit
artifact directories:

```powershell
python tools\build_blender_addon.py --platform linux-x64
python tools\build_blender_addon.py --platform macos-x64
python tools\build_blender_addon.py --platform macos-arm64
python tools\build_blender_addon.py --platform linux-x64 --native-artifact-dir linux-x64=path\to\linux\artifacts
```

To build one zip containing all supported native packages, collect native
artifacts under `native/artifacts/<platform>/` and run:

```powershell
python tools\build_blender_addon.py --platform all --output clip_studio_importer-universal.zip
```

The `Build extension package` GitHub Actions workflow builds the platform
artifacts and uploads `clip_studio_importer-universal.zip` as a workflow
artifact.

To let Blender perform the final extension build, pass a Blender executable:

```powershell
python tools\build_blender_addon.py --blender "C:\Program Files\Blender Foundation\Blender 4.2\blender.exe"
```

## Verification

Use the native CLI against CSP-exported PNG references:

```powershell
cd native\rust
cargo run -q -p clip_cli -- ..\..\img\Test_ToneCurve.clip --compare-png ..\..\img\Test_ToneCurve.png
cargo run -q -p clip_cli -- ..\..\img\Test_AddGlowMultiply.clip --compare-png ..\..\img\Test_AddGlowMultiply.png
```

For the convergence gate:

```powershell
scripts\verify_native_convergence.ps1 -SkipClipCompare
```

## Project Layout

- `clip_studio_importer/` - Blender extension source.
- `clip_studio_importer.zip` - installable extension package.
- `native/rust/` - Rust native renderer workspace.
- `scripts/` - verification and maintenance scripts.
- `tests/` - Python-side bridge/package tests.
- `docs/AI_MEMORY.md` - compact current project state.
- `docs/plan.md` - durable direction and open work.
- `docs/native-code-architecture.md` - native crate/module boundaries.
- `docs/native-tile-event-renderer.md` - current tile-event renderer model.
- `docs/native-performance-investigation.md` - compressed performance evidence.
- `docs/analysis.md` - append-only historical evidence and rejected hypotheses.

## Roadmap

- Keep the installable extension on the native renderer path.
- Improve visual fidelity only when a sample-backed semantic issue remains
  visible enough to justify the risk.
- Improve reload and rendering performance through measured renderer/cache work.
- Keep verification on CSP PNG exports and native `clip_cli --compare-png`.
- Explore true file-backed `.clip` integration later, without sidecar PNGs.

---

## 简体中文

Rizum Clip Reload 可以把 Clip Studio Paint 的 `.clip` 文件导入 Blender，
作为打包在 `.blend` 里的可重载的图片纹理使用。

## 它能做什么

这个插件适合这样的流程：你在 Clip Studio Paint 里画图，在 Blender 里做
材质、排版、动画或参考。

导入后，插件会用内置 native renderer 渲染 `.clip`，然后在 Blender 里创建
generated image。图片会被打包进 `.blend`，之后 `.clip` 源文件变化时可以自动
或手动重新加载。

插件关注最终画面是否正确。它不会把 Clip Studio 的图层变成 Blender 里可编辑
的图层。

## 安装

1. 使用 Blender 4.2 或更新版本。
2. 打开 `Edit > Preferences > Get Extensions`。
3. 选择 `Install from Disk...`。
4. 选择 `clip_studio_importer.zip` 或 universal 包。
5. 启用 `Rizum Clip Reload`。

## 使用

1. 在 Blender 中选择 `File > Import > Clip Studio (.clip)`。
2. 选择你的 `.clip` 文件。
3. Blender 会创建一张 generated image。
4. 图片会被打包进 `.blend`。
5. 在 Clip Studio Paint 里修改并保存 `.clip` 后，Blender 可以自动重新加载。
6. 也可以在 Image Editor 侧边栏点击 `Manual Reload` 手动刷新。

如果之后找不到原始 `.clip` 文件，Blender 仍然会显示上一次已打包的画面。

## 当前状态

包版本：`0.8.67`。

目前支持：

- 栅格图层和全彩图像。
- 纸张/背景图层。
- 文件夹和裁剪图层。
- 图层蒙版和透明度。
- 常见混合模式。
- 当前验证样本覆盖的调整图层和滤镜图层。
- 手动重载和非阻塞自动重载。
- 图片结果打包保存进 `.blend`。
- 缺失源文件、渲染错误、pack 状态等清晰提示。
- 简体中文、日语、西班牙语 UI 翻译。

目前不做：

- 导入可编辑图层。
- 矢量线条或填充。
- 文字图层。
- 对话框/分镜框渲染器。
- 3D 图层或动画时间轴。
- 把修改写回 `.clip` 文件。

## 构建插件包

先构建 native renderer：

```powershell
cd native\rust
cargo build --release -q -p clip_capi
cargo build --release -q -p clip_cli
cd ..\..
```

然后构建可安装扩展包：

```powershell
python tools\build_blender_addon.py
```

构建所有平台 native artifact 后，可以生成一个 universal zip：

```powershell
python tools\build_blender_addon.py --platform all --output clip_studio_importer-universal.zip
```

Windows x64 已在维护者机器上测试。Linux x64、macOS x64、macOS arm64 包支持
已经存在，但因为没有对应设备，仍标记为维护者未测试。
