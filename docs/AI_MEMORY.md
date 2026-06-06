# AI Memory

Last reconciled: 2026-06-05

## Project Role

This repository is the Blender-facing `.clip` importer and verification harness.

- Runtime code: `clip_loader.py` and `clip_studio_importer/clip_loader.py`
- Blender package: `clip_studio_importer.zip`
- Test corpus: `img/*.clip`, `img/*.png`, optional PSD layer exports
- Adjacent native reverse workspace: `E:\Documents\Claude\Projects\rizum-clip-studio-paint`

Use this file as the first stop for an agent. Use `docs/analysis.md` as the append-only evidence log and `docs/plan.md` as the short direction plan.

## Current Native Vector Status

The native no-pattern dab/spacing path is default-enabled for saved `0x2081` vector strokes where the importer has native backing.

Current non-vector filter note:

- `Test_ToneCurve.clip`: accepted on 2026-06-04 as a byte-domain Tone Curve
  LUT rule for compact payloads with non-identity RGB curves. The SQLite
  payload stores compact 16-bit point coordinates, but PSD `curv` export
  exposes byte UI points, and the native runtime path expands curves to four
  `0x104` blocks before merging R/G/B LUTs with the master curve. The importer
  converts each compact coordinate with `ceil(value / 257)`, builds the
  quadratic B-spline table in byte-domain span `/255` with
  `t = sample_idx / 257`, then rounds the byte LUT. Verification improves
  `Test_ToneCurve` from `max=25`, `mean=0.488793`, `visible=8810` to `max=17`,
  `mean=0.018200`, `visible=6107`; the strict single-filter matrix still
  passes, and vector/texture/SizePressure guards are unchanged.

## Long-Context Helper

Claude Code CLI is available as `claude.exe` and is configured to use the DeepSeek Anthropic-compatible endpoint with the `opus` alias mapped to the 1M-context model. User-level default `effortLevel` is set to `max`.

Use it as a slow, long-context helper for bulk reading or parallel analysis, then audit its output before changing code or native conclusions. The adjacent reverse workspace now uses a hybrid native workflow: IDA/Hex-Rays for decompilation, xrefs, structure recovery, and database annotations when useful; Windows r2 for stable disassembly/xrefs/strings/runtime-adjacent evidence; WSL Ubuntu r2 for static `pdg`/`pdd` decompiler assistance. Good fits:

- summarising huge `docs/analysis.md` ranges;
- comparing historical plan/analysis entries;
- drafting r2 command batches or reviewing saved r2 output files from the adjacent reverse workspace.

Prefer explicit effort on dispatched jobs anyway:

```powershell
claude -p --effort max --model opus --add-dir E:\Documents\Claude\Projects\rizum-clip-studio-paint "..."
```

When the prompt contains leading hyphen-like text or many bullets, pipe it through stdin instead of passing it as a positional argument, otherwise Claude CLI may parse prompt fragments as options.

Exact or effectively solved guard targets:

- `Vector_Baseline.clip`: pixel-exact.
- `Vector_FlowPressure_50.clip`: pixel-exact.
- `Vector_Flow_50.clip`: pixel-exact.
- `Vector_OpacityRandom_50.clip`: pixel-exact after `PWVectorSplineCurve` point-flag control collapse.
- `Vector_OpacityPressure.clip`: pixel-exact after per-emitted-sample `0x31` range-lane evaluation.
- `Vector_SizeRandom_50.clip`: pixel-exact after evaluating `SizeEffector=0x81` from the live per-dab native random state instead of endpoint random interpolation.
- `Vector_SizeTilt_50.clip`: pixel-exact after evaluating `SizeEffector=0x21`
  per emitted sample from interpolated compact `+40`, rather than linearly
  interpolating endpoint effector outputs.
- `Vector_OpacityTilt_50.clip`: pixel-exact after the same per emitted sample
  `0x21` rule on `OpacityEffector`.
- `Vector_SizeVelocity_50.clip` and `Vector_OpacityVelocity_50.clip`:
  pixel-exact with the per emitted sample `0x41` auxiliary-lane rule using
  interpolated compact `+44`.
- `Vector_Opacity_50.clip`: one non-visible byte difference only.

Open blocker:

- `Vector_SizePressure.clip`: `max=226`, `mean=0.024732`, `visible=153`.
  Before the accepted 1px feedback-step floor this was `mean=0.024409`,
  `visible=151`; old residual attribution files still describe the same broad
  hard-edge superset shape.
- Current no-edit replay files:
  `tmp_vector_probe/sizepressure_current_feedback_trace_codex_v2.json` and
  `tmp_vector_probe/sizepressure_current_boundary_attribution_codex_v2.json`.
  The accepted 1px feedback floor makes the current trace `213` hard dabs.
  The residual is `152` importer-extra black hard-edge pixels plus one
  native-only black pixel at `(530,462)`, so global shrink remains diagnostic
  only.
- Current attribution still clusters by hard-boundary geometry: segment `26`
  contributes `53` extra pixels and segment `9` contributes `23`. The
  last-covering dab shrink needed to remove extra pixels is small but not
  uniform (`median=0.0713px`, `122/152 <= 0.25px`, `max=1.261px`).
- The residue is concentrated in several large tail dabs, especially around segment `26`.
- Current pressure-field comparison:
  `tmp_vector_probe/current_pressure_field_compare_codex_v1.json` compares
  SizePressure, exact OpacityPressure, and global-pressure fixtures. For
  `Vector_SizePressure`, compact `f32+52/+56/+60` are all `1.0` and
  `f32+64..+76` are all `0.0`; only `+36/+40/+44` vary. There is no hidden
  neutral tail multiplier or runtime global/device pressure curve in the saved
  data.
- Fresh no-edit diagnostics reject more shortcuts. Direct trace replay matches
  current `visible=153`; feedback-only variants are too weak (`primary-only`
  stays `153`, `+4%` reaches `148`, raw-secondary feedback reaches `139` but
  is non-native). Center/radius `float32` casts are neutral. Radius
  fixed-point truncation is diagnostic only (`floor(radius*8)/8` reaches
  `visible=111` but creates `16` missing pixels) and conflicts with the native
  double-precision hard row. Local segment `26`/`9` center nudges bottom out
  around `visible=134` and are not native-backed.
- A 2026-06-04 GUI rasterize/nudge dynamic attempt still did not capture the
  saved-vector Planeswalker render path. Frida hooks on the known route emitted
  only `ready`, and Stalker hot targets identify as stack-cookie / CRT malloc /
  HeapAlloc / event-sync / container helpers, with no hits near `0x1422CC1E0`,
  `0x1422D8550`, `0x14260F550`, or `0x142640150`. Treat this GUI operation as
  cache/UI machinery, not native dab evidence.
- Re-reading `0x1425A4100 -> 0x1422CC1E0` maps the active saved-vector stack
  arguments as `a5=StyleFlag&2`, `a6=StyleFlag&0x20`, `a7=feedback_state`,
  `a8=1`, `a9=0`, `a10=0`. Current `StyleFlag=0x1c240` clears both `a5` and
  `a6`. The overshoot endpoint branch at `0x1422CC595..0x1422CC5B9` requires
  the `a6` slot, and the final `rdi==0` endpoint fallback requires carry to be
  inside a tiny endpoint epsilon. Current SizePressure therefore does not have
  an active hidden endpoint dab/fallback.
- A 2026-06-04 r2/SQLite continuation reclosed the alternate writer route.
  The vector object tail maps to `BrushStyle.MainId`, not `_PW_ID`. Current
  `Vector_SizePressure` uses tail `MainId=10`, `StyleFlag=0x1c240`,
  `AntiAlias=0`, `PatternStyle=0`, `TexturePattern=0`, and `StyleFlag&0x20`
  is clear. Exact `Vector_OpacityPressure` uses the same style flags with
  `MainId=11`; AA compact `0x41` samples use `MainId=10` with `StyleFlag&0x20`
  clear too. Therefore `0x14255C980` dispatches the ordinary
  `0x14255DFE0` branch for these fixtures, not the alternate `0x142558A90`
  branch.
- The same r2 pass rechecked the downstream hard path:
  `0x14255DFE0 -> 0x14260F550 -> 0x14260DB90 -> 0x14263F410` reaches the hard
  circle helper `0x142640150` when AA/softness is off and thickness ratio is
  about `1.0`. `0x142640150` matches the importer hard span formula
  `sqrt(radius^2 - (row - (cy - 0.5))^2) - 0.4`; `0x14263F410` is the
  dispatcher that installs center, clip rect, radius, AA/softness, and
  thickness state before selecting hard/soft/stretched helpers.
- A no-edit runtime monkeypatch of `_vector_sample_point()` variants
  (`float`, `floor`, `round`, `int+0.5`) is neutral for current
  `Vector_SizePressure` (`visible=153`) and preserves exact
  `Vector_OpacityPressure`, `Vector_Hardness_50`, and `Vector_SizeVelocity_50`.
  It only gives a small AA compact diagnostic (`Vector_AA_Medium` mean improves
  from `0.016958` to `0.015083` with float centers). Do not reopen generic
  sampled-point integerization as the SizePressure cause.
- Recomputing current tail dabs through the importer graph evaluator shows
  segment `26`'s dominant samples have graph6 output only about
  `0.022..0.023`, so `amount1=1.5` contributes roughly a 1.1% radius boost in
  the largest residual cluster. Graph5 is identity for this file. This keeps
  the old `amount1=1.5 -> 1.4` improvement diagnostic only: the open mismatch
  is mainly primary/t/sample distribution or a local footprint nuance, not a
  large secondary pressure curve contribution.
- A follow-up r2 pass closed the ordinary spline-control and scalar-sampling
  suspects for this fixture. `0x143204EA0` is exactly the importer's
  `_native_spline_limit_control`: if control distance squared is below `100.0`
  it keeps the point; otherwise it limits by
  `max(10.0, sqrt(neighbor_distance_squared * 6.25))`. The wrapper
  `0x142626EE0` calls it as `limit(cur,next,prev)`,
  `limit(prev,cur,prevprev)`, then symmetrically for next controls, gated by
  node `+0x40 & 1`; current SizePressure point flags are all zero. Re-reading
  `0x1422CC1E0` also confirms the same refined spline `t` returned by vtable
  `+0x68` is used to linearly interpolate compact `f32+36/+40/+44/+52` into
  sample `+0x10/+0x18/+0x20/+0x30`. Compact `+56/+60` are the
  `0x1000/0x2000` flag-gated sample `+0x38/+0x40` fields, but they are all
  `1.0` here. `0x142568040`'s optional `0x100` final-factor branch multiplies
  by sample `+0x30` from compact `+52`; current `+52` is also all `1.0`, so
  that native branch is value-neutral for the remaining `153`.
- No-edit sensitivity probes after that audit were weak. Globally perturbing
  `_native_spline_t_at_distance()` by `+/-1e-6` or `+/-1e-5` is exactly
  neutral; `+1e-4/+5e-4` only moves `153 -> 152`, and negative perturbations
  move to `154`. Quantising the cross-segment residual carry is likewise weak:
  floor/round at `1e-3` or `1e-2` stays in `152..154`, and even clearing
  residual at every segment only reaches `148`. Treat helper suggestions about
  tiny spline phase or residual precision as diagnostic-low unless a real
  native trace contradicts this.
- A Claude Code helper proposed several remaining SizePressure-only
  hypotheses. Codex audit rejects the old `VECTOR_PRESSURE_SIZE_RADIUS_SCALE`
  route for current no-pattern dabs because the active branch uses
  `native_radius_base = width` before `_draw_native_dab_rgba`; changing that
  legacy constant was already metric-neutral. The `0x31` composition-order
  idea is also low value for this file because `0x142568040` is already
  multiplicative and `amount0=1.0`. Keep only two broad next directions:
  capture a real native queue/eval trace that hits `0x1422D8550` /
  `0x14260F550`, or use additional non-exact size-dynamic fixtures to find a
  cross-sample pattern before changing semantics.

Other current native-vector notes:

- `Vector_Hardness_50.clip`: solved on 2026-06-04 and now pixel-exact
  (`max=0`, `mean=0.0`, `visible=0`). The fix combines the native
  fixed-point `0x400` hardness profile table (`0x142664050` scale/offset,
  `0x142663B40` threshold curve, `0x14263AC30` row lookup) with a narrower
  terminal-endpoint rule: do not apply the NoTexture terminal endpoint sample
  to `Hardness<1` or `AntiAlias>0` dabs.
- `Vector_SizeTilt_50.clip` is no longer open. The previous `max=2`,
  `visible=14` tail-edge residual was not circular-AA row coverage: a no-edit
  `0x14263FC50` row-span probe was exactly neutral. Native `0x142568040`
  instead shows `0x21` and `0x41` effectors are evaluated at each emitted
  sample from sample lanes (`+0x18` for secondary/tilt, `+0x20` for auxiliary/
  velocity), so endpoint effector outputs must not be linearly interpolated.

## SizePressure Facts To Preserve

Native-backed facts:

- `SizeEffector=0x31` uses compact primary `f32+36` and secondary `f32+40`.
- Compact point flags are compact `u32+32` / native node `+64`; compact tail `u32+80/+84` maps to node `+112/+116` and is not a hidden point-flag field. Current SizePressure compact `f32+64..+76` values are zero.
- The range-lane formula remains `primary * (1.0 + (amount1 - 1.0) * secondary)`.
- For this file, `amount1=1.5`; the tempting `amount1=1.4` result is only a diagnostic shrink signal.
- `sub_142CB45F0` composes enabled lanes with an incoming curve, but the saved-file route passes an identity/pass-through graph from the default `PWBrushStyleManager`, not a file-local extra curve and not a global/device pressure curve.
- `sub_1422D8550` writes the same effective size to plot radius and to sampler feedback.
- `AutoIntervalType=2`, `Hardness=1.0` gives interval scalar `0.08`.
- The hard no-pattern span uses `sqrt(radius^2 - dy^2) - 0.4` in `sub_142640150`.
- Hard row/alpha/flush semantics are closed for this fixture: `sub_142640150`, `sub_14263F410`, `sub_14263AC30`, and final 16-to-8 alpha flush `(*u16 - 1) >> 7` match the importer. The current remaining `153` pixels are not a row formula, coverage, alpha conversion, or final composite issue.
- `sub_1422CC1E0` advances feedback by the current emitted sample's size-derived interval, then writes residual `state+8 = walk - segment_length`.
- `PWBrushDraw+264` is the no-pattern local draw scale, but this saved full-size route keeps it at `1.0`: it comes from OffscreenSet `+432`, while the observed `0x142614C20` setter writes only OffscreenSet `+440` (`draw+272`) with `1.0 / (1 << v261)`.
- `0x14260DB90` queue `+648` / `0.3*size` is not active SizePressure geometry. It can call `0x142643BF0`, which only writes `context+632/+636/+640` when `context+628` is set; `context+628` is the row-writer mode flag written by `0x142644180` for `UseWaterColor` / `WaterColorType` mode `2`, and current SizePressure has `UseWaterColor=0`. Therefore the optional `0x14266E1E0` rect helper is inactive for this saved route.
- `sub_1422D8550`'s `style+408` / runtime `style+0x198` branch is inactive here because `BrushStyle.Hardness=1.0`. It is not a separate material-opacity radius boost for `Vector_SizePressure`.
- For soft no-pattern brushes, that same `sub_1422D8550` branch is real and
  accepted for `Vector_Hardness_50`: when runtime hardness is below
  `~0.99999999`, native expands plot radius by `1.5 - 0.5 * hardness` and
  enables the profile-table path. `sub_142663B40` builds the radial table with
  threshold `hardness * 1.3 - 0.3`; `sub_14263F410` positions it for the
  current center/radius; `sub_14263AC30` multiplies span coverage by the
  row/x lookup. A profile multiply without the radius expansion is rejected.
- `sub_1422D8550`'s sub-1px feedback-distance clamp is now accepted for the
  no-pattern dab feedback loop: `next_step = max(1.0, 2 * max(size, 0.1) *
  intervalScalar)`. It fixed `Vector_Gap_Narraw` from `visible=556` to exact
  and preserved `Vector_Gap_Normal`, `Vector_Gap_Wide`,
  `Vector_Gap_Fixed_50`, and exact guards. It also improves
  `Vector_SizeVelocity_50` to `visible=113`, but moves paused
  `Vector_SizePressure` to `visible=153`; do not treat this as the
  SizePressure solution.
- `sub_1422D8550`'s sub-1px AA/soft plot promotion is now accepted for the
  no-pattern dab path. When the AA raster flag or softness/profile flag is
  active and evaluated radius is below `1.0`, native scales flow by that
  original radius, promotes plot radius to `1.0`, and then applies any
  soft-hardness expansion. The importer mirrors this with `flow *= radius`,
  `radius = 1.0`, and AA width at least `1.0`. This fixes
  `Vector_SizeVelocity_50` from `visible=113` to exact while preserving exact
  guards. At that checkpoint `Vector_SizeTilt_50` still had `visible=14`, but
  the later per-emitted-sample `0x21` effector fix makes it exact. Paused
  `Vector_SizePressure` remains unchanged at `visible=153` because its AA/soft
  gates are off.
- `0x1422CC1E0`'s terminal short-segment endpoint sample is now accepted for a
  narrow default no-pattern/no-dynamics case. Native `0x1422CC595..0x1422CC5B9`
  handles `walk >= segmentLength` by pinning the sample distance to the segment
  end when the endpoint-force gate and node pointer gate allow it. This is not
  the later `0x1422CC8AC` final-node branch, which still requires residual
  `~0`. The importer only applies it to hard `Hardness~=1.0` / `AntiAlias=0`
  terminal segments when no normal dab emitted and endpoint flag `0x20` is
  clear. This improves
  `Vector_NoTexture` from `max=226`, `mean=0.022792`, `visible=141` to
  `max=226`, `mean=0.010992`, `visible=68`; exact guards remain exact and
  paused `Vector_SizePressure` remains `visible=153`. A broader application
  overdraws the tail of `Vector_Hardness_50`.
- The active sampler route passes `a3=0`, so `0x14255C980 -> 0x14255DFE0` uses `PWBrushDraw+104` as the main size argument. The `PWBrushDraw+112` secondary-resource size path is only selected when `a3 != 0`.
- Fresh r2 checks confirm the active no-pattern submit branches use `PWBrushDraw+264` (`draw+0x108`) for center/radius conversion. `draw+272` (`draw+0x110`) scales a prepared colour/mix scalar and is not the missing radius shrink.
- Fresh r2 checks confirm `0x1422CC1E0` uses the same refined spline `t` from vtable `+0x68` for compact sample-channel interpolation and sampled point generation. `0x142626EE0 -> 0x143200C90` walks chords only to locate the target, recomputes the true spline point with `0x143200940`, and returns that same `t`.
- Current `Vector_SizePressure` SizeEffector outputs stay below `1.0` (`max ~= 0.948196`), so the importer's final size clamp is not active. Bbox clipping removes only part of the residue, and tiny interval-scale changes are diagnostic only; native `AutoIntervalType=2` remains `0.08`.
- Fresh r2 caller checks also keep the sampler setup closed: `0x1425A4100` calls `0x1422CC1E0` with `a4=sub_1422DD7A0(style)`, `a5=StyleFlag&2`, `a6=StyleFlag&0x20`, feedback state as `a7`, then `a8=1`, `a9=0`, `a10=0`. Current `StyleFlag=0x1c240` clears `a5/a6`, and `sub_1422DD7A0`'s style-flag predicate `(StyleFlag&0x30)==0x30` is false.
- Re-reading `0x1422CBAE0` / `0x1422CD520` found no hidden SizePressure field: compact `u32+32` is node flags `+0x40`, compact `f32+36..+76` is node `+0x44..+0x6c`, and packet conversion only fills node `+0x44..+0x5c` plus `0x1000/0x2000` flags. Current `f32+56/+60` are still value-neutral `1.0`, so this path cannot shrink the remaining 151 hard-edge extras.
- A 2026-06-02 isolation pass compared the active SQLite styles: `Vector_SizePressure` and exact `Vector_OpacityPressure` share the same 1024 canvas, `AntiAlias=0`, `AutoIntervalType=2`, `Hardness=1.0`, no spray/pattern/watercolor, and `StyleFlag=0x1c240`; the material difference is whether the dynamic `0x31` effector is installed in Size or Opacity. The remaining cause must therefore be downstream of size changing radius/spacing, not a generic pressure, canvas, AA, auto-interval, transform, or row-writer rule.
- No-edit probes further narrow the shape: using rough quarter-length rather than refined segment length for spline t-search step count is exactly neutral (`Vector_SizePressure` remains `visible=151`, exact guards remain exact). SizePressure-only diagnostics show radius is much more sensitive than feedback: radius scale `0.997` improves `visible=151 -> 98`, feedback scale `1.04` improves only to `143`, and combining them is worse (`106`). These are diagnostic signals only.
- r2 re-read of `0x1422D8550` still shows one evaluated size value: `xmm7` feeds both `state+8 = 2 * max(size, 0.1) * interval` and final plot `+0`. The only visible size split is the sub-1px flow-compensation branch, inactive for current SizePressure. Do not reopen a separate hidden plot-size shrink without new evidence.
- Boundary-shape probe `tmp_vector_probe/sizepressure_extra_boundary_margin_probe_v2.json` reattributes all 151 extras to current hard dabs and shows the integer span shrink needed to drop those pixels is usually tiny (`median=0.059px`; `121/151 <= 0.25px`). This supports a hard-radius boundary mismatch after dynamic size evaluation, but still does not justify a generic radius scale without native evidence.
- Current-source replay on 2026-06-04 rechecked hard-span variants against
  guards: changing the shared hard span to `sqrt(...) - 0.45` or `-0.5`
  improves SizePressure (`visible=107/92`) and NoTexture, but breaks exact
  `Vector_OpacityPressure` (`visible=212/409`). Keep `-0.4` as the generic
  native hard-row constant.
- r2 rechecks on 2026-06-04 keep downstream handoff closed:
  `0x14260F550` copies plot fields as 64-bit/32-bit fields without a hidden
  double-to-float radius truncation, and `0x142568040` still implements the
  multiplicative `0x31` lane model from sample `+0x10/+0x18`. There is no
  native support for changing diagnostic `amount1=1.5 -> 1.4`.
- Effector-call alignment shows the first 29 `SizeEffector` evaluations are point/end setup and the next 233 correspond to emitted dabs. The remaining factor is size-dynamics-only rather than all pressure: `Vector_OpacityPressure` remains exact, `Vector_SizeRandom_50` is exact because native random size is evaluated per emitted dab through the live state pointer, and `Vector_SizeVelocity_50` is now exact after the native sub-1px AA/soft promotion. The older `Vector_SizeTilt_50` residual is now also solved by per-emitted-sample `0x21` evaluation.
- Superseded `Vector_SizeTilt_50` follow-up: the previous 14 visible pixels were final
  paper-composited RGB edge pixels clustered at the last AA dabs. The new PSD
  oracle sharpens this: PSD `Layer 2` has `2911` nonzero-alpha pixels; importer
  has `2912`, with `105` alpha-diff pixels, `25` over visible threshold, all
  importer-high by `+1..+3` in bbox `[167,155,182,173]`. Diagnostic-only probes
  show scaling compact `0x21` output by `0.9999` makes final output invisible
  (`max=1`, `visible=0`) but still leaves PSD-layer alpha diffs and has no
  native support. r2 `0x142568040` confirms the `0x20` lane is ordinary
  `low + (high-low) * graph(sample+0x18)`, but the missing piece was evaluating
  that graph at each emitted sample rather than linearly interpolating endpoint
  effector outputs.
- `Vector_AA_None/Weak/Medium/Strong` compact-120 follow-up: the first ordinary
  `0x2011` layer matches the PSD layer oracle exactly or within alpha 1. The
  remaining visible residuals come from the second compact `0x41` dark
  filled-curve layer. The accepted fallback is still narrow to compact dark
  filled curves: AA level `0` uses hard edge, `radius_scale=0.95`, denser
  sampling, and `+0.25` phase; AA levels `1..3` use raw float sampled points,
  denser sampling, `-0.5` phase, and a PSD-backed per-AA fallback edge map
  (`radius/feather`: AA1 `0.72/0.50`, AA2 `0.62/1.10`, AA3 `0.60/1.25`).
  This improves the family while preserving exact guards: `None visible 1654 ->
  226`, `Weak 1936 -> 1163`, `Medium 2184 -> 1560`, `Strong 2587 -> 2070`.
  Standard cubic `p0,out0,in1,p1` using compact `+88/+96` as the incoming
  handle was tested and rejected because it is much worse; fresh curve probes
  also reject simple line, quadratic-through-tail, and doubled-tail cubic
  interpretations. Keep compact `+104/+112` curve-tail geometry and native scan
  conversion as open questions. The per-AA map is a narrow fallback refinement,
  not proof of fully recovered native curve conversion.
- `Vector_Texture_50.clip` / `Vector_Texture.clip`: the `TextureFlag=0x201`
  average-baseline remap path is now accepted for the narrow texture preview.
  Native evidence maps `0x100` to inversion and `0x200` to the average-baseline
  remap path. Fresh r2 reread of `0x142664260` supersedes the old fixed `1.5`
  amplitude: for `TextureFlag&0x200` or composite `9`, native writes
  `textureCtx+0x4c = trunc(currentTextureScalar * 10.0 * 1024.0)`, and
  `0x142664760` clamps the average-relative remap before `TextureComposite=0`.
  In the current importer preview, the decoded single-channel texture has the
  opposite usable orientation from the native sampled coverage byte, so the
  kept bridge is `remapped = clamp(avg + 10*TextureDensityBase*(tex-avg))` and
  `factor = 1 - avg + remapped` for the `0x200` family. The average uses native
  `0x1426642D0` byte-sum integer division, not floating mean. Verification
  improves `Vector_Texture` to `max=226`, `mean=0.222173`, `visible=8111` and
  `Vector_Texture_50` to `max=182`, `mean=0.359719`, `visible=533`. Exact
  guards, `Vector_NoTexture`, paused `Vector_SizePressure`, AA/thickness
  samples, and material brush-tip samples are unchanged.
- 2026-06-04 texture native-dab rejection: adjacent r2 evidence
  `tmp_r2_csp/texture_helpers_pD_20260604.txt` confirms the native helper
  chain (`0x142664C00`, `0x142664260`, `0x142664760`) but rejects replacing the
  current texture fallback with the no-pattern native dab path. Mode `0` in
  `0x142664760` uses incoming coverage times
  `0x8000 + (baseline or 0x4000) - textureCoverage`; current `0x201` samples
  have zero brightness/contrast, so the `+0x88` threshold/range path is off.
  A default-off importer probe that forced texture samples through native-dab
  geometry regressed both texture fixtures: geometry-only
  `0.734374/1.262050`, native-sign `0.733624/1.083781`, and positive-sign
  `1.056032/1.418662` for `Vector_Texture/Vector_Texture_50`, versus the
  then-current `0.499736/0.709181`; those rejections still stand after the
  accepted density-scalar remap. The next texture improvement needs the true
  row-writer coordinate/context path, not a no-pattern dab swap.
- 2026-06-04 post-Tilt material/texture recheck: do not broaden the Gap fixed-
  UV material path to ordinary `Vector_BrushTip_Material`. Forcing ordinary
  material through the full-lane `42x112` fixed-UV four-sample path regresses
  `mean=0.842944 -> 1.005300`; native spline float centers and step-scale
  variants also regress ordinary or Gap. Texture phase/offset is also rejected:
  integer offsets `[-4..4]` regress both texture fixtures. A tiny improvement
  from rounded texture average contradicts r2 `0x1426642D0`, which computes the
  average by byte accumulation and integer division/truncation. Direct literal
  `0x142664760` mode-0 formulas (`1.5-tex`, `0.5+tex`, average raw/inverted)
  regress strongly. Next progress for material/texture needs the real row
  writer / descriptor setup, not constants, phase, or preview-path grafts.
- Current `Vector_NoTexture` after the terminal endpoint fix has `68` visible
  pixels: `66` importer-extra hard-edge pixels and `2` missing pixels scattered
  across `199` submitted dabs. No-edit probes reject global hard-row/center
  changes: `sqrt(radius^2-dy^2)-0.475` improves NoTexture to `25` visible
  pixels but breaks exact `Vector_OpacityPressure`, tiny center shifts break
  exact guards, and ignoring spline point flags breaks `Vector_OpacityRandom_50`.
  Keep `0x142640150` hard-row semantics unchanged unless new native evidence
  appears.
- r2 recheck confirms the texture row-writer boundary: `0x14260F8B0` calls
  `0x142664C00` to build texture context, `0x142644180` stores the enable flag
  at draw context `+0x1e8`, and `0x14263C060` calls
  `0x142664760(textureCtx, coverage, x, y)` before final accumulation. The
  importer currently applies texture as post-stroke alpha multiplication; exact
  texture needs row-stage coverage adjustment and native sampler coordinates.
- Follow-up texture probes reject two tempting importer shortcuts. First, forcing
  `TexturePattern>0` strokes through the no-pattern native dab feedback path and
  applying `0x142664760`-style per-pixel coverage modulation is worse than the
  current preview even when the texture modifier is confirmed active
  (`texture_calls=198/78`; best combined mean roughly `1.738` versus current
  `0.499736 + 0.709181`). Second, keeping the current polyline fallback geometry
  but moving texture multiplication from whole-stroke post-processing to
  per-segment row/delta writing is also worse overall; it can improve one
  texture sample while badly regressing the other. Preserve the native fact that
  `0x14263C060` passes absolute row `x,y` into `0x142664760`, which then applies
  context `+0x68/+0x6c` offsets and the `+0x58/+0x60` transform before sampling.
  The remaining texture gap needs the true retained row-writer geometry and
  accumulation path, not constants, bbox post-multiply, or a no-pattern dab
  graft.
- Reverse continuation on 2026-06-04 also closes the plain row/flush path:
  Windows r2 confirms `0x14263AC30` has an inlined no-texture coverage-only
  fast path at `0x14263B195..0x14263B24C`. Direct/max writes
  `max(old, (opacityCap * flowCoverage) >> 15)`, build-up writes
  `old + (((opacityCap - old) * flowCoverage) >> 15)`, and `0x142653A40`
  fast flush converts nonzero coverage with `(coverage - 1) >> 7`. Current
  importer native-dab alpha mirrors this for the no-pattern path, so
  `Vector_SizePressure` should keep focusing on radius/sample phase or a
  size-only dynamic split, not row accumulation or final alpha conversion.
  `0x14263D2F0` is now classified as a fast coverage+BGR row writer.
- Reverse continuation on 2026-06-04 names `0x14260F8B0` as the retained brush
  row-context setup helper. Saved-vector setup calls it for paired draw
  contexts, and texture/profile/mask state routes row drawing to retained
  writers such as `0x14263C060`, `0x14263C5C0`, `0x14263C870`,
  `0x14263CE60`, or material tail `0x14263C3A0` instead of the simple coverage
  writer. Current texture fixtures are clean single-style cases:
  `TexturePattern=2`, `TextureFlag=0x201`, `TextureComposite=0`; no importer
  code change is justified until the retained row geometry/accumulation path is
  modeled.
- Extra routing detail: ordinary texture strokes can still enter the circular
  brush-tip path, but `0x14263AC30` dispatches each row span into retained row
  writers when profile/mask/material/texture flags are active. With a secondary
  row buffer this reaches `0x14263C060`; with texture enabled and no secondary
  row it routes toward `0x14263C5C0`. Future texture work should model
  circular dab geometry plus retained row context/final row-plane accumulation,
  not a post-stroke texture multiply.
- 2026-06-04 texture accumulation correction: current texture fixtures use
  `StyleFlag=0x1c240`, which does not set `0x1000`; they are build-up
  accumulation, not direct/max. The earlier shortcut argument that post-stroke
  texture multiplication is roughly equivalent under direct/max is therefore
  not valid for these samples. No-edit probes still reject two simple order
  approximations: per-segment texture-before-alpha-over regresses
  `Vector_Texture/Vector_Texture_50` to `mean=0.845258/1.499181`, and the
  continuous build-up approximation `1 - (1 - alpha)^factor` regresses them to
  `0.900940/1.547325`. Keep the accepted `10x` density remap; the remaining
  target is the true 16-bit retained row writer and sampler coordinates.
- `Vector_Thickness_50*`: accepted native `0x142640420` stretched-AA row
  solver. r2/WSL evidence shows `0x14263F410` dispatches `ThicknessBase=0.5`,
  `AntiAlias=2`, `PatternStyle=0` samples to `0x142640420`; the helper solves
  rotated outer-ellipse row roots, applies half-open spans (`right + 0.4999`),
  subtracts AA width from both major and minor axes for the inner ellipse, and
  builds one-dimensional left/right ramps. The style `0x40` axis flag arrives
  as a native `+90deg` table lookup offset. Implementation in both loader
  entrypoints is gated to no-pattern stretched AA dabs without hardness
  profile. Verification improves current metrics to `Vector_Thickness_50`
  `max=12`, `mean=0.017019`, `visible=318`;
  `Vector_Thickness_50_Angle_45` `max=35`, `mean=0.050444`,
  `visible=651`; and `Vector_Thickness_50_Angle_90` `max=21`,
  `mean=0.052687`, `visible=725`. Exact guards, paused SizePressure,
  SizeTilt, NoTexture, AA compact, texture, and material brush-tip samples are
  unchanged. The earlier row-scan rejection was caused by missing the native
  `+90deg` axis offset and using the wrong row-root denominator, not by the
  row-scan model itself.
- 2026-06-03 rejection pass after texture: do not tune material brush-tip
  colour/height/anchor from metrics. `Vector_BrushTip_Material_Gap.psd` proves
  the native layer is a repeated upright tip (`bbox [17,16,195,40]`, `1727`
  alpha pixels, visible colour near `(192,93,97)`), while the fallback emits
  18 short `12x12` stamps (`bbox [16,22,197,38]`, colour `(177,44,44)`). The
  best no-edit colour/height/anchor variant only moves final PNG mean
  `1.106737 -> 1.099519`; more PSD-like layer shapes worsen final PNG metrics.
  Exact material rendering still needs the native retained/material row writer.
- 2026-06-04 material lane recheck: the pattern mipmap stores two apparent
  `169x449` single-channel lanes in four tiles. Native row-sampler evidence
  says `0x10` material families can select lane/byte `1` for coverage, but
  using the right persisted lane directly worsens both current samples
  (`Vector_BrushTip_Material` mean `0.842944 -> 2.313456`,
  `Vector_BrushTip_Material_Gap` `1.106737 -> 1.180225`). Raising only Gap
  alpha also fails (`0.3 -> 0.4` slightly lowers visible pixels but worsens
  mean; higher values regress). PSD layer color/alpha still proves native is
  doing material-color row writing, not a stroke-color alpha stamp.
- 2026-06-04 native material-color refinement: `0x142637A70` /
  `0x142637C70` confirm the `0x10` two-lane path semantics. Byte/lane `1`
  multiplies incoming coverage; byte/lane `0` becomes a `0..0x8000` mix value
  and blends two context color triples (`ctx+0x24c/+0x250/+0x254` with
  `ctx+0x25c/+0x260/+0x264`). Cached helper `0x14263E970` four-samples the
  material footprint, ignores samples with zero lane `1`, averages lane `0`
  across the remaining samples, and scales coverage by summed lane `1`.
  This explains the PSD material layer's lighter median color, but it is not
  enough to edit the fallback: applying native-ish lane0/lane1 mix with the
  current fallback alpha constants regresses both material samples, while
  using better empirical opacities (`0.8` ordinary, `0.5` gap) is another
  preview constant. Exact material needs native UV/quad sampling plus row
  accumulation, not a direct lane-color patch.
- 2026-06-04 material quad continuation: saved r2 evidence in the adjacent
  reverse workspace confirms `0x142642010` is a true retained material quad
  path. It resolves the source material image, stores material width/height at
  context `+0x1f8/+0x1fc`, derives fixed UV maxima at `+0x200/+0x204`, and
  submits four `(x,y,u,v)` vertices to `0x1426410B0`. Pattern-only scale is
  `2 * effective_size / max(material_width, material_height)`, with caller
  scale and rotation/flip applied before submit. For the current `169x449`
  material, a literal size-10 native aspect is roughly `8x20`, but a no-edit
  fallback probe rejects turning this into constants: current remains
  `Vector_BrushTip_Material mean=0.842944`,
  `Vector_BrushTip_Material_Gap mean=1.106737`; native-aspect `0.75x2.0`
  worsens them to `1.539219` and `1.269750`, and Gap-height-only `1.2x2.0`
  worsens Gap to `1.326781`. The likely current zero-rotation path calls
  `0x142641C60`, which scans the axis-aligned material bbox with fixed UV
  increments `32768 / scaleX` and `32768 / scaleY`; it uses a `+0.5`
  source-pixel bias only when `ctx+0x1d0 == 0`, otherwise bias is `0`. Keep
  this as native evidence for the future material renderer; do not patch the
  simplified preview stamp.
- 2026-06-04 material row continuation: both current material samples have
  `BrushStyle.AntiAlias=2`, so the `0x142641C60` axis path should use the
  nonzero-AA `bias=0` UV start, not the `+0.5` no-AA bias. A no-edit full-lane
  fixed-UV prototype confirms this direction but rejects an importer edit: the
  best axis variant (`current_anchor`, `bias=0`, left-lane coverage, stroke
  color) improves ordinary Material only (`0.842944 -> 0.797631`) while
  worsening Gap (`1.106737 -> 1.122419`). Native-aspect, lane1/right coverage,
  material mix color, PSD-median color, and `+0.5` bias are all worse overall.
  Skipping the duplicate Gap tail point is also rejected (`1.106737 ->
  1.108019`). Fresh `0x14263B7F0` / `0x14263C3A0` disassembly shows why the
  Python stamp prototype is still too shallow: the likely dynamic material
  path calls `0x142637A70` for AA material pixels, then updates native 16-bit
  coverage with `old + ((opacityCap - old) * coverage >> 15)` and delegates
  BGR plane writes to `0x14263DDB0`. The next real boundary is therefore a
  retained material row experiment with 16-bit coverage/BGR planes, not another
  RGBA alpha-over stamp tweak.
- 2026-06-04 retained-row prototype: `0x14263DDB0` was re-read directly. It
  computes `candidate = min(inputCoverage * opacityCap, 0x40000000) >> 15`;
  direct/max mode replaces only when candidate coverage is larger, while
  build-up mode moves 16-bit coverage toward `opacityCap` and blends stored
  BGR bytes with fixed-point weights. A no-edit retained-row prototype that
  kept current material point placement but used 16-bit coverage/BGR planes
  improves ordinary Material when paired with PSD-like color and half flow
  (`0.842944 -> 0.724850`), but still worsens Gap (`1.106737 -> 1.142213`).
  Stroke-color retained rows similarly improve ordinary only and worsen Gap.
  Therefore do not implement a half-native retained material path yet: the
  row-plane direction is real, but Gap still needs native material point
  submission/spacing, final flush, or exact color-context resolution before an
  importer change is safe.
- 2026-06-04 material Gap submission recheck: reverse-side r2 evidence shows
  `0x14255DFE0` routes `draw+0x160 == 2` through `0x14255C680`, but that helper
  is a 256-fixed tile/boundary submission wrapper over `0x14260F550 ->
  0x14260DB90`, not a separate Gap spacing renderer. It either submits one
  queue record or splits a dab across the paired contexts `draw+0x140/+0x150`.
  A no-edit probe that kept Gap height/alpha/anchor constants but forced wide
  material strokes to resample by the native-style interval formula worsened
  `Vector_BrushTip_Material_Gap` (`1.106737 -> 1.148144`) and left ordinary
  Material unchanged. Do not patch Gap by simply resampling wide material
  strokes; the remaining material mismatch is still retained row/color/final
  flush or deeper retained-state phase.
- 2026-06-05 Test_Ballon retained/material resource recheck: the true
  `BrushPatternImage` resources are now accessible through
  `_brush_material_resource_alpha()` without changing rendering. Style 4 /
  `PatternStyle=10` resolves to one image (`ImageIndex=[3]`, `OrderType=3`,
  `Reverse2=34`) with true alpha resource `23x2511`; style 5 /
  `PatternStyle=11` resolves to one image (`ImageIndex=[4]`) with true alpha
  resource `338x50`. Current rendering must still use
  `_brush_material_full_lane_alpha()`'s old cropped preview windows
  (`256x23`, `50x256`), because feeding the full resources directly into the
  simplified stamp renderer regresses `Vector_BrushTip_Material` and
  `Vector_BrushTip_Material_Gap` badly. Verified unchanged after adding the
  helper: opacity-pressure and opacity-velocity exact guards remain `0/0`,
  SizePressure remains `0.024732/153`, and Test_Ballon layer 8 is now
  `0.786705/12602` after the later PatternStyle-10 interval-dab preview.
  Exact retained/material still needs native phase, quad UV,
  row accumulation, and final writer semantics.
- 2026-06-05 retained phase formula probe: r2 reading of
  `0x142558A90 -> 0x1422D8BB0` shows native retained phase uses true material
  dimensions, with a shape like `advance_axis * 4 / (2 * effective_size *
  axis_scale)` after bucketing rotation into 0/90/180/270-like quadrants. For
  `Test_Ballon`, style 4 (`RotationBase=0`, resource `23x2511`) advances about
  `18.4` resource units per size-2.5 sample; style 5 (`RotationBase=90`,
  resource `338x50`) would advance about `40.0`, but its `StyleFlag=0x1c250`
  lacks the retained-state `0x20` bit and should not be treated as the same
  stateful path as style 4 without more evidence. A no-edit probe that
  phase-scrolled the true style-4 resource along the current native
  point-family outline regressed badly (`1.140025/28948` best tested versus
  current `0.808579/12771`). Do not implement direct phase-scrolled alpha
  modulation; exactness requires the real material quad and row writer.
- 2026-06-05 retained angle source continuation: `Test_Ballon` style 4 has
  `StyleFlag=0x1c230`, so `sub_1422DD7A0(style)` returns true through
  `(StyleFlag & 0x30) == 0x30`; `0x142629750` passes that value as
  `0x1422CC1E0` argument `a4/r9d`. With that gate enabled, the walker stores
  `sample+0x48` at stack `rbp-0x18`, fills it by calling the node vtable
  `+0x78` and then `0x141A8DD80` (`atan(y/x) * 57.2957795` with 90/180/270
  quadrant handling), and `0x1422D8BB0` copies the resulting qword into
  `retained_state+0x20`. Slot correction: for flag `0x20` nodes, vtable
  `+0x78` is `0x142623280 -> 0x1431F9D80`, which returns a normalized
  tangent/direction-like vector; `0x142623230 -> 0x1431F9420` is a different
  slot that proves the quadratic position formula only. The later material quad
  builder uses the retained previous/current angle pair for sine/cosine.
  Therefore retained angle is not compact `f32+48` and not a generic
  pressure/secondary lane. This is narrow to retained balloon/material styles;
  ordinary exact guards with `StyleFlag=0x1c240` keep `sub_1422DD7A0` false.
- 2026-06-05 Test_Ballon point-node consumer and PatternStyle-10 interval-dab
  preview: reverse-side r2 evidence shows `0x142629750` walks the compact
  point-node list and `0x1422CC1E0` uses node vtable `+0x70/+0x68/+0x78` for
  segment length, distance-to-`t`, and point evaluation before submitting each
  emitted sample to the writer. For flag `0x20` nodes, the quadratic position
  helper `0x142623230 -> 0x1431F9420` confirms current-node control is used for
  current-to-next segments; the retained-angle slot is separate (`+0x78 =
  0x142623280 -> 0x1431F9D80`). Test_Ballon style 4 has
  `PatternStyle=10`, `IntervalBase=1.0`, `AutoIntervalType=2`, and
  `width=2.5`; the importer now draws only this balloon native point-family
  outline as closed-path interval dabs at `record.width * interval_base`
  instead of one connected capsule. Verification improves Test_Ballon full to
  `1.181103/14326` and layer 8 to `0.786705/12602`; layers 9/10 remain
  `0.325141/1342` and `0.069257/382`; exact opacity guards remain `0/0` and
  SizePressure remains `0.024732/153`. Rejected diagnostics: fill
  supersampling worsens, direct phase-scrolled resource alpha worsens, and a
  shallow retained row/segment-quad overlay regresses to roughly
  `2.13..2.63` mean with `~26k` visible pixels.
- 2026-06-04 material spacing acceptance: dynamic trace
  `tmp_csp_material_gap_frida_trace_20260604.jsonl` captured 87 ordinary
  material queue records. Their centers match the recovered native
  `PWVectorSplineCurve` distance sampler to floating-point noise (`~1e-8`),
  with step `2 * width * IntervalBase = 2.0`; linear resampling differs by up
  to `0.154px`. The same native spline sampler yields the Gap native-style
  10px centers `(21.151,25.813)`, `(30.981,26.971)`,
  `(40.957,28.711)`, ... for `IntervalBase=0.5`. The importer now uses native
  spline/float centers only for wide material-tip strokes, leaving ordinary
  material unchanged because applying it globally regresses current preview
  metrics. Verification: `Vector_BrushTip_Material_Gap` improves
  `mean=1.106737 -> 1.030538`, `visible=1862 -> 1844`;
  `Vector_BrushTip_Material` stays `0.842944`; exact guards
  (`Vector_Baseline`, `Vector_OpacityPressure`, `Vector_SizeRandom_50`) stay
  exact; `Vector_Texture_50`, `Vector_SizePressure`, and
  `test_Filters_Vector_Text` stay unchanged. Remaining Gap mismatch is now
  material row/color/UV/final flush, not primary point spacing.
- 2026-06-04 material Gap aspect/color acceptance: the wide material preview
  now also uses the native material aspect and dynamic material mix color. For
  this fixture, native resolves material dimensions `w=42`, `h=112`, so
  `2 * size / max(w,h)` gives about `7.5x20`; the importer mirrors this for
  wide material tips with width scale `0.75`, height scale `2.0`, centered
  anchor, and full material coverage. The vector stroke header stores main
  color at offsets `+40/+44/+48` and sub color at `+52/+56/+60`; the material
  left-lane crop mean is `69.59/255 = 0.2729`, which mixes `(177,44,44)` with
  sub `(228,224,237)` into `(191,93,97)`, matching the PSD material layer
  median near `(192,93,97)`. Verification: `Vector_BrushTip_Material_Gap`
  improves further to `max=100`, `mean=0.289350`, `visible=1374`;
  ordinary `Vector_BrushTip_Material` remains `0.842944`; exact guards remain
  exact; `Vector_Texture_50`, paused `Vector_SizePressure`, and
  `test_Filters_Vector_Text` remain unchanged. The Gap fallback layer now has
  bbox `[17,16,195,40]`, alpha sum `164592`, alpha max `253`, and median RGB
  `[191,93,97]` versus PSD oracle bbox `[17,16,195,40]`, alpha sum `172314`,
  median `[192,93,97]`. Remaining material work is exact per-pixel lane1
  coverage sampling, retained 16-bit row accumulation, and final flush.
- 2026-06-04 material Gap native-mip / fixed-UV coverage acceptance: wide
  material tips now build alpha from the full left material lane,
  area-downsample it by the native resolved material scale
  (`169x449 -> 42x112`, i.e. `4x`), then render the final stamp through the
  axis-AA fixed-UV footprint implied by `0x142641C60` and `0x14263E970`: floor
  lookup with four samples `(u,v)`, `(u+xstep,v)`, `(u,v+ystep/2)`,
  `(u+xstep/2,v+ystep/2)`. Verification: `Vector_BrushTip_Material_Gap`
  improves from `max=100`, `mean=0.289350`, `visible=1374` to `max=83`,
  `mean=0.200137`, `visible=1313`; ordinary `Vector_BrushTip_Material` stays
  `0.842944`; exact guards
  (`Vector_Baseline`, `Vector_OpacityPressure`, `Vector_SizeRandom_50`) stay
  exact; `Vector_Texture_50`, paused `Vector_SizePressure`, and
  `test_Filters_Vector_Text` remain unchanged. PSD-layer oracle now has the
  same bbox `[17,16,195,40]` and median RGB `[191,93,97]` vs `[192,93,97]`,
  with alpha abs residual `39368 -> 27496` and alpha sum `170334` vs PSD
  `172314`. Remaining mismatch is now narrow row/final-flush detail, not
  spacing, colour, material dimensions, or single-sample UV coverage.
- 2026-06-04 material Gap dynamic-UV-origin acceptance: reverse-side r2
  evidence for `0x142641C60` confirms the axis material path derives initial
  fixed UV per submitted quad from
  `(integer pixel - float_bbox_min + aaBias) * invScale * 32768`; current
  Material/Gap styles have `AntiAlias=2`, so `aaBias=0`. The importer now
  uses that per-stamp float render origin for wide material tips instead of
  reusing one statically pre-rendered stamp alpha. Verification:
  `Vector_BrushTip_Material_Gap` improves from `max=83`, `mean=0.200137`,
  `visible=1313` to `max=39`, `mean=0.112369`, `visible=740`; ordinary
  `Vector_BrushTip_Material` remains `0.842944`; exact guards
  (`Vector_Baseline`, `Vector_OpacityPressure`, `Vector_SizeRandom_50`) stay
  exact; `Vector_Texture_50`, `Vector_Texture`, `Vector_NoTexture`,
  `Vector_AA_None`, `Vector_AA_Medium`, `Test_Vector`, and paused
  `Vector_SizePressure` retain their prior metrics. Remaining Gap work is
  now the true retained material row writer / lane0-lane1 color mapping /
  final flush, not point spacing, material dimensions, average color, native
  mip size, or static UV phase.
- Follow-up dynamic-UV probes reject two tempting shortcuts. Forcing the right
  material lane as coverage regresses Gap strongly (`mean=1.426213`,
  `visible=2385`), so coverage still comes from the accepted left lane. A
  tiny extra `u=-0.25` native-texel offset improves Gap mean only
  (`0.112369 -> 0.110806`, `visible=740 -> 674`) while worsening max
  (`39 -> 54`) and lacks support in the `0x142641C60` `(pixel - floatMin +
  aaBias) * invScale` formula, so keep zero extra phase.
- 2026-06-04 material Gap AA sample-position acceptance: reverse-side r2
  evidence for `0x142641C60` plus `0x14263E970` shows the accepted fixed-UV
  four-sample footprint was still slightly wrong for nonzero AA.
  `0x142641C60` stores `ctx+0x210` and `ctx+0x220` as half of the x UV
  increment when `ctx+0x1d0 != 0`; `0x14263E970` then samples `(u,v)`,
  `(u+xstep/2,v)`, `(u,v+ystep/2)`, and `(u+xstep/2,v+ystep/2)`. The importer
  now mirrors that half-x AA footprint for wide material tips. Verification:
  `Vector_BrushTip_Material_Gap` improves from `max=39`, `mean=0.112369`,
  `visible=740` to `max=10`, `mean=0.024044`, `visible=258`; ordinary
  `Vector_BrushTip_Material` remains `0.842944`; exact guards
  (`Vector_Baseline`, `Vector_OpacityPressure`, `Vector_SizeRandom_50`) stay
  exact; `Vector_Texture_50`, `Vector_Texture`, `Vector_AA_Medium`, and
  paused `Vector_SizePressure` keep prior metrics. A same-turn no-edit
  retained-row probe says simple `max` and build-up 16-bit coverage are
  neutral with average material color, while per-pixel left/right-lane color
  mixing regresses (`mean=0.795606` and `0.414100`), so do not chase those as
  the remaining Gap residual.
- Post-fix layer oracle: current Gap layer now matches PSD bbox
  `[17,16,195,40]`, has no missing PSD alpha pixels, and has only `96`
  extra alpha pixels; alpha abs residual is `2870`, alpha sum is `173652` vs
  PSD `172314`, median RGB remains `[191,93,97]` vs PSD `[192,93,97]`.
  Floor/16-bit final flush variants are rejected after the half-x fix:
  `alpha_over_floor_each` regresses to `mean=0.024481`, `visible=267`, while
  retained `max_floor` / `buildup_floor` give `mean=0.024213`,
  `visible=274`; retained round variants are exactly neutral. Remaining
  `Vector_BrushTip_Material_Gap` error is a small coverage-distribution
  residual, not layer placement, color, final alpha flush, or simple retained
  coverage accumulation.
- 2026-06-04 native material coverage-scale acceptance: `0x14263E970` does
  not convert the four AA coverage bytes with `(sum+2)//4`. It sums active
  byte-1 coverage samples, multiplies by the incoming fixed coverage
  (`0x8000` for this fixture), then uses `0x80808081` and `shr 9`, i.e. the
  native fixed-point equivalent of `sum / 1020 * 0x8000`, before the final
  byte conversion. The importer now mirrors this fixed coverage scale for wide
  material tips. Verification: `Vector_BrushTip_Material_Gap` improves from
  `max=10`, `mean=0.024044`, `visible=258` to `max=10`, `mean=0.023456`,
  `visible=258`; ordinary `Vector_BrushTip_Material`, exact guards, texture
  guards, AA guard, and paused `Vector_SizePressure` remain unchanged. Layer
  oracle after this change: bbox still matches, missing PSD alpha is `0`,
  extra alpha drops to `74`, alpha abs residual is `2797`, alpha sum is
  `173205` vs PSD `172314`, and median RGB is still `[191,93,97]` vs
  `[192,93,97]`.
- 2026-06-04 ordinary material path acceptance: after the half-x and native
  fixed-coverage fixes, the old rejection of applying the material quad path to
  ordinary `Vector_BrushTip_Material` is superseded. That fixture has
  `PatternStyle=2`, `TexturePattern=0`, `AntiAlias=2`, and
  `IntervalBase=0.1`; native material dispatch still routes it through the
  same PatternStyle material path, so the importer must not require
  `IntervalBase > 0.1` to use the material quad/UV fallback. Changing the
  local gate to include `IntervalBase >= 0.1` moves ordinary Material from the
  old static stamp path to the accepted native-like material path and improves
  `Vector_BrushTip_Material` from `max=127`, `mean=0.842944`, `visible=3117`
  to `max=11`, `mean=0.061113`, `visible=663`; `Vector_BrushTip_Material_Gap`
  remains `max=10`, `mean=0.023456`, `visible=258`. Exact guards
  (`Vector_Baseline`, `Vector_OpacityPressure`, `Vector_SizeRandom_50`) stay
  exact, and `Vector_Texture_50`, `Vector_Texture`, `Vector_AA_Medium`, and
  paused `Vector_SizePressure` retain prior metrics.
- 2026-06-04 ordinary texture native-geometry acceptance: reverse-side r2
  evidence says textured ordinary dabs still enter `0x14263F410` and are then
  forwarded by `0x14263AC30` into retained texture row writers; the old importer
  was instead drawing these samples with the legacy polyline fallback and only
  multiplying texture after the stroke. The importer now admits the narrow
  texture fixtures (`PatternStyle=0`, `TexturePattern>0`, `TextureFlag=0x201`,
  `TextureComposite=0`, neutral texture transform/brightness/contrast, no
  retained state/spray) into the native dab geometry path, draws them into a
  per-stroke buffer, applies the accepted average-baseline `0x200` texture
  factor, and composites that buffer back. This remains a preview bridge rather
  than the full native retained row writer, but it supersedes the older
  "no-pattern dab graft regresses" note, which had tested native geometry
  without the texture reapply step. Verification improves `Vector_Texture` from
  `max=226`, `mean=0.222173`, `visible=8111` to `max=4`, `mean=0.018159`,
  `visible=6952`; `Vector_Texture_50` improves from `max=182`,
  `mean=0.359719`, `visible=533` to `max=2`, `mean=0.020188`, `visible=3`.
  Exact guards (`Vector_Baseline`, `Vector_OpacityPressure`,
  `Vector_SizeRandom_50`) stay exact; `Vector_NoTexture`, paused
  `Vector_SizePressure`, `Vector_AA_Medium`, material brush-tip samples, and
  `test_Filters_Vector_Text` keep prior metrics.
- 2026-06-04 follow-up rejections after fixed-UV coverage: simple material
  row-accumulation/flush swaps (`float_round_each`, `native16_floor_flush`,
  `native16_round_flush`) are exactly neutral on Gap, so the remaining mismatch
  is not 8-bit-over-vs-16-bit accumulation for this fixture. A placement sweep
  found only a tiny metric-only right-shift improvement (`mean=0.198150`) that
  does not match the recovered `center +/- native extent -> floor/ceil` bbox
  well enough to accept. Per-pixel material mix from the same lane or the
  apparent right lane regresses (`mean=0.43..0.92`), so keep the accepted
  PSD/native average material colour until the true cached byte0/byte1 plane
  mapping is implemented.
- 2026-06-04 post-AA triage: `Vector_FlowVelocity_50` and
  `Vector_OpacityTilt_50` now verify as practical guards (`max=1`,
  `visible=0`). Fresh Material_Gap no-edit probes reject alternative fixed-UV
  sample sets, floor/ceil placement, half-pixel shifts, and opacity changes.
  A native-mip width `43` gives a small metric-only improvement
  (`mean ~=0.190000`) but contradicts the captured native material context
  (`w=42`, `h=112`, `fmt=17`), so keep `169x449 -> 42x112`.
- 2026-06-04 AA compact `0x41` rejection update: the AA family residual remains
  isolated to the second compact filled-curve layer (`VectorNormalStrokeIndex=14`);
  the first ordinary vector layer is exact or within alpha `1..2`. Layer 2 has
  mixed residual direction: `Weak` and `Medium` are too light, while `Strong`
  is too heavy, so a single global opacity/coverage edit is not valid. No-edit
  probes reject two native-looking shortcuts: interpreting the compact
  forward-control as an iswCore-style quadratic Bezier regresses all four AA
  files badly, and reducing subdivision toward the iswCore V2 `length/6px,
  clamp 1..32` direction regresses Weak/Medium while only helping Strong
  diagnostically. A metric-only radius `*1.05` / feather `*0.85` surface lowers
  summed mean but worsens visible pixels for Medium/Strong and has no native
  support. Keep the current compact fallback constants until the real
  filled-curve scan conversion / pen-head polygon rasterizer is recovered.
- 2026-06-04 balloon fallback color-mode acceptance: `Test_Ballon` has no
  decodable native layer cache for its three balloon layers; their
  `LayerRenderMipmap` offscreen `BlockData` external IDs are missing from the
  file's Exta bodies, so fallback rendering must use `VectorObjectList`
  headers/points. The recovered semantic gap is that balloon fallback colors
  must respect layer binary color mode. `Balloon 2` and `Balloon 3` have
  `LayerColorTypeIndex=1` and PSD as nearly pure white shapes, even though the
  vector object headers store pastel RGB; `Balloon 1` has `LayerColorTypeIndex=0`
  and should keep header colors. The importer now quantizes balloon fallback
  header colors to black/white for `LayerColorTypeIndex=1`. This improves
  `Test_Ballon` from `max=255`, `mean=0.988340`, `visible=47104` to
  `max=255`, `mean=0.713922`, `visible=19603`. Guards stayed stable:
  `Vector_Baseline` and `Vector_OpacityPressure` exact, `Vector_Texture_50`
  still `max=2`, `mean=0.020188`, `visible=3`, and `Test_Frames` /
  `test_Filters_Vector_Text` unchanged. Remaining balloon mismatch is
  pattern/outline and fallback geometry, especially sparse black edge pattern
  data that is not available through the missing `Sampled Brush 1` cache.
  Follow-up no-edit geometry probes for `Balloon 1` / index `4` reject simple
  tuning: `bbox_expand=1` aligns the bbox and reduces missing alpha but worsens
  premul mean/visible (`1.666352/17879 -> 1.853602/19899`), and point-weight,
  outline-width, and ellipse-power variants also regress. The raw 16-bit
  header colours round back to the current `(243,243,243)` /
  `(228,224,237)`, so there is no accepted 243->244 colour-quantisation fix.
- 2026-06-04 `Test_Frames` fallback pass: `ComicFrameLineMipmap` IDs `39` and
  `46` are valid Mipmap table IDs, but every linked Offscreen `BlockData`
  external ID is absent from Exta, so this fixture has no decodable native
  frame-line cache. Fallback must use `VectorObjectList.VectorData`. Layer `21`
  (`Frame 1`) has three rectangular family-`0x410` objects with four points,
  width `2.5`, styles `4/4/5`, fill style `2`, and a child gradation
  background. Layer `25` (`Frame 2`) has one empty rectangular frame with
  style `7`, fill style `2`, width `2.5`, points `(857.001,310.001)` ->
  `(971.001,701.001)`, and bbox `(854,307,974,704)`. The bbox is the point
  path expanded by roughly the stored width. PSD layer evidence shows CSP
  exports frame lines as separate pixel layers: `Frame 2` is `(244,244,244)`
  with AA alpha mostly `156/157/64` plus a small `255` core, while current
  fallback draws opaque `(243,243,243)`. No code change accepted: a narrow
  semi-transparent empty-frame probe improves only modestly
  (`Test_Frames mean 1.085583 -> 1.031334`, `visible 11817 -> 11617`), closed
  polyline stroke probes improve mean but worsen visible (`>=12407`), and
  simple inset/outline-width probes confirm current `FRAME_FALLBACK_BBOX_INSET=2`
  plus existing outline-width logic remains best by visible pixels. Remaining
  mismatch is missing native frame-line brush/AA style rendering, especially
  style `4/5` patterned frame lines and style `7` empty-frame AA, not bbox
  inset or integer outline-width.
- 2026-06-04 broad non-exact triage over `Vector_*`, `Test_*`, `test_*`, and
  `Bubble_*` fixtures (skipping heavy art/4K samples): largest current targets
  are `Test_AddGlowMultiply` (`max=117`, `mean=0.067933`, `visible=355502`),
  `test_Filters_Vector_Text` (`0.568335/48586`), `Test_Gradiation`
  (`0.310779/45489`), `Test_Vector` (`0.923822/25848`), `Test_Ballon`
  (`0.713922/19603`), `Test_Frames` (`1.085583/11817`), `Vector_Texture`
  (`0.018159/6952`), and `Test_ToneCurve` (`0.018200/6107`). This supersedes
  older priority notes that predate later vector/material/filter fixes.
  `Test_AddGlowMultiply` was rechecked: stack is Normal base, non-clipped
  `ADD_GLOW`, then clipped `MULTIPLY`/`NORMAL` siblings. Alpha is exact and
  error is confined to the AddGlow base bbox, mostly 1-5 RGB levels too dark.
  Final Normal, regular clipped siblings, sequential no-group, Add-mode
  substitution, numerator rounding/ceil/bias, and naive `+1` RGB probes all
  reject; keep the isolated clipping-group structure and current
  `_blend_add_glow_u8` without native evidence. `Test_Color` / `Test_Saturation`
  non-separable blend probes also reject: current Photoshop `0.3/0.59/0.11`
  luminosity weights plus byte-domain source/destination remain best or tied;
  BT.601, integer-601, Rec.709, and no-prequantization variants regress.
  `Test_Clipping` recheck rejects switching Normal clipped preserve to
  effective alpha because it barely helps ordinary clipping and regresses
  `Test_ClippingEdge` to roughly `19189` visible pixels; threshold sweeps do
  not explain the residual.
- 2026-06-04 `test_Filters_Vector_Text` re-triage: the single-filter matrix is
  already mostly closed. Hue/Saturation, Posterization, Reverse Gradient,
  Level, Tone Curve, Color Balance, and Threshold are all `visible=0`;
  Brightness/Contrast has only `28` visible; the remaining single-filter
  outlier is Gradient Map at `max=8`, `mean=0.052949`, `visible=2102`.
  Therefore the full-image residual (`max=225`, `mean=0.568335`,
  `visible=48586`) is not primarily a filter-stack failure. Spatial
  attribution puts about `46318` visible pixels in Layer `5`'s vector area,
  `2999` in the text/balloon area, and `1748` in the frame area; masks overlap
  `10688`, `1354`, and `1748` visible pixels respectively.
- 2026-06-04 Layer `5` in `test_Filters_Vector_Text`: four compact `0x2081`
  strokes use BrushStyle `5` (`AntiAlias=1`, `AutoIntervalType=3`,
  `PatternStyle=0`, `TexturePattern=3`, `TextureFlag=0x101`,
  `TextureComposite=0`). The layer render mipmap external ID is absent from
  Exta, so fallback reconstructs it. Texture pattern `3` is BrushPatternImage
  `Paper`; mipmap `59` has present Exta, but decodes as a 512x512 two-plane
  grayscale tile stream (`524288` bytes), not RGBA or alpha-only. The gray
  plane has alpha all `255`, gray min/max `0/255`, mean about `121.9`, and
  existing `_brush_texture_preview_alpha()` returns `None` for this format.
  No code change accepted: treating the gray plane or inverse gray as texture
  alpha, with current/linear/weak/remap formulas, worsens
  `test_Filters_Vector_Text` and/or `Vector_Texture(_50)`. Including
  `TextureFlag=0x101` in the simple texture native-dab gate is metric-neutral.
  Treat this as unresolved `0x101` paper-texture semantics, not the accepted
  `0x201` texture path.
- 2026-06-04 `0x101` native-dab/texture follow-up: Layer `5` isolated against
  `test_Filters_Vector_Text_Vector.png` is `max=173`, `mean=0.159297`,
  `visible=8857`; the full image is `0.568335/48586`, but this is not a simple
  filter-amplification story. A controlled replay that applies the full filter
  chain `[6,10,11,12,13,14,7,8,9]` to both the current Layer `5` over paper and
  the Layer `5` PNG oracle reduces the layer-only diff to `mean=0.117077`,
  `visible=6555`; intermediate ToneCurve/ColorBalance/Threshold/GradientMap
  stages temporarily raise visible pixels, then Posterization collapses many
  differences. The full residual therefore likely involves base-raster
  interaction, spatial attribution, or other fallback layers as well. The layer
  residual has alpha effectively fixed by paper compositing and is mostly RGB,
  especially red-channel coverage/color. Stroke headers at offsets
  `56/8596/27080/46972` all use style `5`, width `15`, and colors
  `[226,227,227]` for the first stroke then `[53,227,226]` for the other
  three. A no-edit route probe that lets
  `TextureFlag=0x101` enter the native-dab feedback branch rejects simple
  fixes: `native_dab_no_texture` worsens full to `mean=0.703173`,
  `visible=51432`; the only lower-full-mean variant (`lane0` alpha plane,
  effectively all-255, separate texture target) gets `mean=0.544444`,
  `visible=48574` but worsens isolated Layer `5` visible count to `11777`;
  all real gray-lane (`lane1`) mode-0/current/raw texture formulas regress
  badly. Do not implement the lane0/native-dab side effect. The remaining
  `0x101` problem is native row integration, geometry, opacity/flow, or
  texture context interaction, not applying the Paper gray lane over the
  completed fallback mask.
- Layer `5` route probes also reject simple scalar fixes. Scaling only matching
  Layer `5` stroke alpha has a tiny metric-only direction (`alpha=0.8` gives
  full `mean=0.565456`, `visible=48846` and Layer `5` `mean=0.154416`,
  `visible=10235`; baseline is full `0.568335/48586` and Layer `5`
  `0.159297/8857`), but visible pixels and isolated-layer quality both reject a
  FlowBase-as-alpha shortcut. Simple stroke-color channel offsets are rejected
  too: `+1R/+1B` lowers Layer `5` mean to `0.118032` and visible to `8838`, but
  full mean worsens to `0.568581`; larger offsets explode full visible/mean.
  Keep searching in native row/texture/geometry integration, not scalar alpha
  or RGB offsets.
- Fresh single-function r2 extraction of `0x142664760` / `0x142664C00` confirms
  the precise `0x101` texture shape. `TextureFlag&0x100` sets context `+0x54`;
  `0x142664760` inverts the sampled byte when `+0x54==0` and keeps the raw byte
  when `+0x54!=0`, then converts it with `(byte*257+1)>>1`. `ctx+0x88` gates a
  piecewise-linear remap through `+0x8c/+0x90/+0x94/+0x98/+0x9c`. For Layer `5`
  (`TextureBrightness=0.22`, `TextureContrast=0.5`, `TextureFlag=0x101`), the
  native setup is approximately input `<=0.36 -> 0`, input `>=0.86 -> 0x8000`,
  and slope `2.0` in between. Since `0x200` is absent, the remapped texture is
  then scaled by `trunc(TextureDensityBase*1024)>>10` before
  `TextureComposite=0` uses the non-remap baseline `0x4000`:
  `coverage *= 0x8000 + 0x4000 - tex`. Applying this exact-ish row formula
  after the completed polyline fallback is still rejected: using mipmap `59`
  gray lane (`512x512`, min/max `0/255`, mean `188.451`) on changed Layer `5`
  stroke pixels worsens full to `mean=0.575934`, `visible=48750` and Layer `5`
  to `mean=0.165657`, `visible=8835`. This reinforces that the missing behavior
  belongs inside native 16-bit row coverage / geometry integration, not as a
  post-stroke alpha or RGB operation.
- Follow-up route audit: Layer `5` is not the importer `0x41` filled-curve
  family. A fresh `inspect_vector_dynamics.py` dump confirms four `stroke_92`
  records with shape `(92,76,88,88)`, flags `0x2081`, point counts
  `96/209/225/55`, BrushStyle `5`, width `15.0`, and texture-descriptor
  compiled-line evidence. Style `5` is `StyleFlag=0x1c200` (`0x10000`
  texture/color family set, `0x20` retained-state path clear),
  `PatternStyle=0`, `TexturePattern=3`, `TextureFlag=0x101`,
  `TextureDensityBase=0.4`, `FlowBase=0.5`, `AntiAlias=1`,
  `AutoIntervalType=3`. Forcing this style into the ordinary native dab route
  is still rejected even with the row-time `0x101` texture modifier inserted
  before 16-bit accumulation: forced native no-texture is full
  `0.703173/51432`, Layer `5` `0.253532/14638`; forced native row-texture is
  full `0.710222/51165`, Layer `5` `0.259203/12839`. Row texture reduces some
  visible pixels versus no-texture, but the route remains much worse than the
  current fallback (`0.568335/48586`, Layer `5` `0.159297/8857`). Therefore the
  missing native semantics are not simply `0x2081` ordinary dab plus
  `0x142664760` texture; the unresolved layer is the compiled `PWBrushStyle` /
  `PWBrushDraw` sink state (`0x1424A4450 -> 0x1422D9BE0 -> 0x1422DA100`,
  `0x1422CC1E0` sink `+0x10/+0x18/+0x20`) that sits between compact stroke
  records and row rendering.
- 2026-06-04 material row follow-up after returning from
  `test_Filters_Vector_Text`: current metrics are
  `Vector_BrushTip_Material_Gap max=10 mean=0.023456 visible=258`,
  `Vector_BrushTip_Material max=11 mean=0.061113 visible=663`,
  `Vector_Texture max=4 mean=0.018159 visible=6952`, and
  `Vector_Texture_50 max=2 mean=0.020188 visible=3`. A no-edit scan over
  material gap width/height/alpha/anchor/native-mip-scale keeps the current
  constants as the local optimum; every non-baseline candidate worsens the
  material pair. The Gap PSD-layer oracle remains tightly bounded: bbox
  `[17,16,195,40]` matches, alpha sum is `173205` vs PSD `172314`,
  alpha nonzero `1801` vs `1727`, alpha max `253`, no missing PSD alpha
  pixels, and only `74` extra alpha pixels. Median RGB is still `[191,93,97]`
  vs PSD `[192,93,97]`. A diagnostic `+1` red offset in the dynamic material
  mix lowers mean for both material samples (`Gap 0.023456 -> 0.020594`,
  ordinary Material `0.061113 -> 0.051981`) but leaves visible pixels
  essentially unchanged and lacks native support; do not implement it. The
  remaining material issue is still retained coverage/BGR row writing or exact
  lane/color rounding inside `0x14263C3A0 -> 0x14263DDB0`, not preview
  geometry constants.
- 2026-06-04 Layer `16` / Layer `30` in `test_Filters_Vector_Text`: Layer `16`
  is a text/balloon layer with `TextLayerAttributes` bytes. Its layer render
  mipmap is absent from Exta, but text record `50` points to a present `101x21`
  offscreen cache; `_text_cache_fallback_image()` pastes that cache and draws
  a fallback balloon. The exported `bubble` PNG is not a single-layer oracle
  because its non-paper area extends across other document regions, so do not
  tune Layer `16` from that PNG alone. Layer `30` frame render/cache data is
  also absent from Exta and remains under the broader frame fallback issue.
- 2026-06-04 `Test_Gradiation` rejection update: current metrics are
  `max=10`, `mean=0.310779`, `visible=45489`. The Gradient Map filter payload
  header is `(276,24,28,9,16,0,3)`, followed by nine 28-byte nodes and a final
  raw stop of `32768`. No-edit probes support the existing
  `_GRADIENT_STOP_DENOMINATOR = 32768*256/255`; changing the denominator to
  `32768` or `32767` makes visible pixels explode to roughly `1,030,000`.
  BT.601/int601 luminance weights give only tiny mean-only diagnostics while
  worsening visible pixels, and 257-domain colour quantisation regresses.
  Keep the current gradient-map LUT conversion without native evidence.
- 2026-06-04 `Test_ToneCurve` rejection update: current metrics are
  `max=17`, `mean=0.018200`, `visible=6107`. The payload is compact 16-bit
  curve data: 32 records of stride `0x82`, first four curves active
  (master/R/G/B), remaining curves identity. No-edit probes support the current
  LUT behavior: compact points use `ceil(value/257)`, RGB channel curves apply
  first, and the master curve applies last. Reversing the order, using only
  master/channel curves, or changing point conversion to floor/round/256-domain
  variants regresses heavily, often to more than 1,000,000 visible pixels.
  Keep `_tone_curve_bspline_lut` / `_apply_tone_curve` as-is without native
  evidence.
- 2026-06-04 `Test_Vector` residual split/update: accepted a narrow wide-`0x41`
  capsule branch. Current metrics are `max=187`, `mean=0.923822`,
  `visible=25848` (previously `mean=0.976390`, `visible=26223`). The only
  vector layer has three objects: ordinary no-pattern `0x2081` style `4`
  (`AA=1`, `Hardness=0.95`, `AutoIntervalType=1`, interval scalar `0.135`),
  one-point `0x41` Koa_Lace1 stamp style `5`, and wide two-point `0x41`
  capsule style `6`. File structure proves the wide capsule bbox is expanded
  bounds: bbox `(210,378,994,715)`, points `(374,542)` and `(830,551)`, width
  `163.6`, with `374-163.6 ~= 210`, `830+163.6 ~= 994`,
  `551+163.6 ~= 715`. The importer branch is intentionally narrow:
  compact layout `(92,76,120,88)`, `flags == 0x41`, `point_count == 2`,
  `width > 16`, style present, `PatternStyle == 0`, `TexturePattern == 0`;
  it draws the explicit centre segment with `radius=width`, `feather=0.0`.
  Guards are unchanged: `Vector_AA_Medium` `0.016958/1560`,
  `Vector_AA_None` `0.036532/226`, `Vector_Baseline` exact,
  `Vector_OpacityPressure` exact, `Vector_Texture_50` `0.020188/3`,
  `Test_Ballon` `0.713922/19603`, and `Test_Frames` `1.085583/11817`.
  Rejected probes stay rejected: style-4 interval `0.15` and AA1 cap `1.75`
  help diagnostically but contradict native-backed interval/AA mappings and
  exact type-1 guards; combining them worsens visible pixels. Expanding/removing
  the old capsule ellipse inset `(9,5,21,5)` or changing ellipse power from
  `3.6` regresses, and metric-only `radius=160` is rejected because evidence
  supports stored `width=163.6`. Koa_Lace1 mipmap `10` decodes correctly as
  RGBA `732x82`, and alpha-only pattern decoder experiments are neutral.
  Remaining mismatch is ordinary circular-AA/profile/coordinate distribution,
  lace stamp phase, and residual capsule scan-conversion details.
- Superseded 2026-06-04 SizeTilt/thickness rejection pass: at that checkpoint
  `Vector_SizeTilt_50` remained `max=2`, `mean=0.001306`, `visible=14`; AA coverage round/ceil, tiny
  coverage scales, center bias, final flush variants, and AA cap nudges either
  regress or lack native support. r2/WSL re-read of `0x142640420` confirms the
  stretched-AA helper is a per-row rotated-ellipse root solver with half-open
  spans and one-dimensional edge ramps, but a first no-edit row-scan prototype
  worsens 0/90-degree thickness samples and only slightly helps 45 degrees when
  sin is flipped. The thickness half is now superseded by the accepted
  `0x142640420` row solver, and the SizeTilt half is superseded by per-dab
  `0x21` effector evaluation.
- Superseded 2026-06-03 rejection pass: `Vector_SizeTilt_50` was a tiny tail-edge
  residual (`max=2`, `visible=14`). PSD `Layer 2` has `2911` alpha pixels; the
  importer has `2912` and alpha sum only `+133` high. Do not add the old
  `SizeEffector=0x21 * 0.9999` shrink without native evidence.
- Superseded 2026-06-04 `Vector_SizeTilt_50` recheck: the metric was
  `max=2`, `mean=0.001306`, `visible=14`. No-edit probes over `0x21` numeric
  details were rejected: source `float32`, graph `float32`, all-`float32`, and
  one-ULP-lower source are exactly neutral; 15-bit source quantization slightly
  worsens; 8-bit source quantization regresses heavily. A layer-alpha-minus-one
  postprocess lowers final visible pixels to `2` but creates large missing
  alpha (`alpha_abs=2835` vs current `133`), so it is overfit and not native.
  The accepted fix is per-emitted-sample graph evaluation, not numeric scaling.
- 2026-06-03/04 `Vector_NoTexture` pass: the original hard-mask residual was
  `63` output-only pixels plus `78` missing pixels, with most missing pixels at
  the terminal tail component `[693,720,720,750]`. Point rounding/phase and
  hard-span shrink remain rejected; hard-span shrink improves `NoTexture` and
  `SizePressure` diagnostically but breaks exact `Vector_OpacityPressure`.
  The accepted fix is the native terminal short-segment endpoint sample at
  `0x1422CC595..0x1422CC5B9`, not a change to the hard row span formula
  `sqrt(radius^2-dy^2)-0.4`.
- `Vector_OpacityVelocity_50`: solved by the new PSD transparent-layer oracle.
  The old final RGB deltas came from an importer-only `0.02` floor on
  `0x41`/random opacity dynamics. PSD `Layer 2` has `1618` nonzero-alpha pixels;
  before the fix the importer emitted `4459` with tail alpha mostly `1..5`.
  Removing the floor makes the final PNG exact and the layer alpha/premul RGB
  exact against PSD. Keep the accepted `0x41` auxiliary-lane curve formula;
  the native opacity value may reach `0.0`.
- Simple no-edit variants do not solve the size-only footprint nuance: radius fixed-point truncation has the right direction but creates missing pixels or stalls above zero, centre quantisation is wrong, and span endpoint-exclusive variants are wrong. r2 still shows `0x14263F410 -> 0x142640150` passing double radius to the hard circular row formula.
- Per-dab shrink-window probe `tmp_vector_probe/sizepressure_per_dab_shrink_window_v1.json` shows 78 dabs contribute the 151 extra pixels and 77 have a radius-shrink window that removes their attributed extras without deleting uniquely native black pixels. Many late dabs have `protected=0`, so the residual is concentrated in overlapped segment-end/tail footprints rather than row accumulation.
- More diagnostics are now rejected: a segment-tail lookahead emission gate barely moves SizePressure (`151 -> 147`) and breaks exact guards if generic; adjacent-radius drawing (`avg_prev`, `avg_next`, `prev`, `next`) does not solve and introduces missing pixels. The native 1px feedback floor is accepted now, but it is not a SizePressure fix.
- r2 re-read of `0x1422CC1E0` around `0x1422CC5E5..0x1422CC831` reconfirmed compact lane interpolation uses the same refined `xmm6` returned by vtable `+0x68`; distance-fraction interpolation remains rejected despite its diagnostic improvement.

Rejected shortcuts:

- Do not change generic `0x31` semantics or `amount1`.
- Do not shrink radius, hard-span margin, graph output, or interval scalar without new native evidence.
- Do not replace spline parameter interpolation with distance fraction.
- Do not add duplicate-centre, accumulated-alpha, whole-dab, bbox, row-clip, or tile-split skips as a renderer rule.
- Do not use metric-only probes such as `amount1=1.4`, radius scale `0.997`, span `-0.5`, linear sampling, or secondary-channel bias.
- Do not force linear/all-corner sampling: it improves SizePressure diagnostically, but breaks exact baseline/flow/opacity guards and contradicts the confirmed `0x2081 -> PWVectorSplineCurve` route.
- In particular, do not reinterpret `radius_scale=0.997` as native `draw+264`; IDA maps the saved-route scale setter to `draw+272`, not the primary radius scale.
- Do not reopen `StyleFlag&0x40` as a hidden thickness-axis split for current SizePressure. `ThicknessBase=1.0` and `ThicknessEffector=0x01` keep plot `+24` / queue `+320` ratio at `1.0`, so `0x14263F410` bypasses stretched/rotated helpers.
- Do not reopen `StyleFlag&0x1004` flow rescaling for this fixture. Current `StyleFlag=0x1c240`, and `0x1c240 & 0x1004 == 0x1000`, not `4`, so the branch is inactive.
- Do not switch SizeEffector to endpoint-result interpolation. A no-edit source-exec variant that evaluated SizeEffector at endpoints and interpolated the result regressed `Vector_SizePressure` from `visible=151` to `156`.
- Do not pursue a simple segment-first dab skip without new native evidence:
  current-trace replay shows `residual_in == 0` first-sample skip is neutral
  (`visible=153`), while skipping every segment's first dab only reaches
  `visible=152` and creates `8` missing pixels.
- Do not change feedback spacing alone for SizePressure. Current variants do
  not remove the footprint residual and conflict with the accepted native
  one-size plot/step handoff.
- Do not use radius/center float casts, fixed-point radius truncation, or
  segment-local center shifts as semantics. They are diagnostic at best and
  lack native support.
- Do not add further hard spacing floors beyond the accepted native 1px
  feedback clamp; larger/tuned floors are metric-only and can break guards.
- Do not reopen graph evaluator root order / endpoint inclusion for current graph5/graph6; matching native root order is now implemented and metric-neutral.
- Do not reopen polyline-centre sampling for `PWVectorSplineCurve`. IDA `sub_143200C90` walks a polyline only to locate the target distance, then recomputes the emitted x/y with `sub_143200940` at the refined spline `t`; the same `t` drives compact lane interpolation.
- Do not reopen generic sampler float/double precision at the feedback boundary; float32 residual, walk, `t`, size scalars, point, effective size, and next step probes are exactly neutral.
- Do not chase a tail-only segment-26 phase fix by itself; segment length/residual perturbations in that one segment stay essentially at `visible=151`.
- Do not reopen sampler `0x1000/0x2000` fixed-field logic for current SizePressure. The stroke has object flag `0x2000`, but all point flags are zero and compact `f32+56/+60` are constant `1.0`.
- Do not expect `LayerRenderMipmap` to provide a native oracle for this fixture; the referenced render cache has no decodable external body.
- Do not treat `0x14260DB90` queue `+648` / `0.3*size` as an unconditional cropper. The optional rect-helper path is gated by the water-color row-writer flag at draw context `+628`; current SizePressure leaves it off.
- Do not reopen single-parameter shrink fixes without new native evidence.
  Against the PSD layer oracle, `amount1~=1.3995` bottoms out near
  `visible=92`, hard-span shrink `0.4 -> 0.5` bottoms out near `visible=93`,
  and a combined grid still does not reach zero.
- Do not remap the `SizeEffector` second lane away from compact `f32+40` /
  sample `+0x18`. Diagnostic variants using primary, constants, inverted
  secondary, squared secondary, or sqrt secondary are all worse than current.
- Do not chase a whole-dab skip/gate as the remaining answer. A greedy PSD-mask
  probe can remove 39 current dabs without creating missing pixels, but still
  leaves 80 extras, so the mismatch is not a duplicate-centre or accumulated
  hard-dab predicate.
- Do not reopen arc-distance-fraction interpolation as native semantics. It is
  a useful shape diagnostic, but r2 shows the same refined spline `t` drives
  both compact lanes and point sampling.
- Do not reopen rough-vs-refined t-search step count for this fixture. A
  targeted no-edit probe using rough quarter-length for `_native_spline_t_at_distance`
  was exactly neutral on SizePressure and guards.
- Do not reopen segment-tail lookahead, generic/adaptive feedback floors, or
  adjacent-sample radius averaging as native semantics without new evidence.
  Fresh no-edit probes either barely improve SizePressure or introduce missing
  pixels and/or guard regressions.
- Do not use `draw+272` as a geometry scale or expect bbox clipping to finish
  this fixture. The active submit path uses `draw+264`; simple stroke/reference
  bboxes leave most extras behind.

## Next Native Targets

For `Vector_SizePressure`, continue from native-backed causes that can make only this size-dynamic hard stroke a tiny superset while leaving baseline, flow, opacity random, and opacity pressure exact:

- Size-only branches in style/draw setup before `sub_1422D8550`.
- Upstream construction of emitted sample values/positions around `sub_1423E8640` and `sub_1422CC1E0`.
- Per-dab comparison against `tmp_vector_probe/sizepressure_extra_dab_attribution_v3.json` and tail feedback audit `tmp_vector_probe/sizepressure_tail_feedback_audit_v1.json`.
- Any transform/export-space state proven active for this saved vector layer but not for the exact 200x200 guards.

## Verification Commands

Use targeted checks while iterating:

```powershell
python verify_one_clip.py img/Vector_SizePressure.clip
python verify_one_clip.py img/Vector_Hardness_50.clip
python verify_one_clip.py img/Vector_OpacityPressure.clip
python verify_one_clip.py img/Vector_OpacityRandom_50.clip
python verify_one_clip.py img/Vector_SizeRandom_50.clip
python verify_one_clip.py img/Vector_FlowPressure_50.clip
python verify_one_clip.py img/Vector_Flow_50.clip
python verify_one_clip.py img/Vector_Baseline.clip
```

Use broader filters only when touching adjustment layers:

```powershell
python verify_filter_exports.py img/test_Filters_Vector_Text.clip --max-max 8 --max-mean 0.06 --max-visible-px 2200
```

## Doc Map

- `docs/AI_MEMORY.md`: current agent-readable state.
- `docs/plan.md`: short next-action plan for new agents.
- `docs/analysis.md`: append-only technical evidence and rejected hypotheses.
- `docs/design.md`: Blender UX and product direction.
- `README.md`: user-facing install/use overview.
- Adjacent `R2_COMMANDLINE_WORKFLOW.md`: current native-analysis workflow and preserved address/fact map.
- Adjacent `R2_HYBRID_DECOMP_WORKFLOW.md`: Windows runtime plus WSL `pdg`/`pdd` static decompiler workflow.
- `.claude/skills/csp-ida-re/`: historical IDA workflow notes; paused during the r2 trial.

## Latest Test_Vector Wide 0x41 Update

2026-06-04 continuation: `Test_Vector.clip` exposed a narrow active wide
compact `0x41` branch. The large capsule is
`(header_len, point_header_len, stride_a, stride_b) == (92,76,120,88)`,
`flags=0x41`, `point_count=2`, `width=163.6`, BrushStyle `6`,
`AntiAlias=3`, `PatternStyle=0`, and `TexturePattern=0`. The object bbox is
expanded bounds, while points `(374,542)` and `(830,551)` are the centre
segment. The importer now draws only this wide no-pattern/no-texture shape with
`radius=width*0.99`, but the rasterizer is now a circular pen-head envelope
polygon with `16` half-cap segments and a `4x` mask resolve for AA styles. This
matches the native V4 evidence shape (`StorePenHeadPoint -> FillPolygon`,
fixed-point polygon scan conversion, and simple 4x AA) more closely than the
previous distance-field capsule/feather bridge. Current full metric improves
from `max=187`, `mean=0.923822`, `visible=25848` to `max=187`,
`mean=0.610175`, `visible=24477`.

Residual split after the polygon change: layer-vs-PSD mean is `0.861408`; the
capsule region visible count drops to `10420`. The signed capsule residual is
still output-heavy, so exact support still needs CSP's real scan-conversion and
runtime stroke representation rather than more scalar radius fitting. Remaining visible residuals
are still split across ordinary stroke phase/AA, Koa_Lace1 stamp phase/alpha,
and capsule scan conversion. Guard checks were unchanged:
`Vector_OpacityRandom_50` exact, `Vector_OpacityPressure` exact,
`Vector_Hardness_50` exact, `Vector_Texture_50` `0.020188/3`,
`Vector_Texture` `0.018159/6952`, `Vector_SizePressure` `0.024732/153`,
`Vector_AA_None` `0.036532/226`, `Vector_AA_Medium` `0.016958/1560`,
`Vector_NoTexture` `0.010992/68`, `test_Filters_Vector_Text`
`0.568335/48586`, `Test_Ballon` premul `2.060759/19603`, and `Test_Frames`
premul `1.470779/11817`. Corpus scan found this active wide branch only in
`Test_Vector`; AA-family compact `0x41` samples are width `3.0` and remain on
the existing dark filled-curve path.

Rejected lace-placement follow-up: the one-point `0x41` Koa_Lace1 record also
stores extra doubles near `(707,215)`, but a local no-edit sweep over target
width and paste dx/dy found the current placement is already best
(`target_w=328`, `dx=0`, `dy=0`, centered from primary point `(709,195)`).
Alternatives, including the extra-coordinate-like anchor, worsen full mean and
lace-region mean. Remaining lace residual is texture/rasterization phase or
alpha decode, not a simple placement offset.

Rejected ordinary-style follow-up: the same `Test_Vector` layer also contains a
style-4 ordinary `0x2081` stroke (`AA=1`, `Hardness=0.95`, `AutoIntervalType=1`,
`interval_scalar=0.135`). No-edit monkeypatch probes show scalar dab tweaks can
improve `Test_Vector` diagnostically (`radius*0.96` reaches
`max=187`, `mean=0.572845`, `visible=22412`; smaller flow, AA cap, and hardness
tweaks also reduce mean). Do not implement those tweaks as semantics. A global
`radius*0.96` breaks established guards badly: `Vector_Hardness_50` regresses
from exact to `mean=0.407256`, `visible=1657`; `Vector_OpacityPressure` from
exact to `mean=0.132267`, `visible=4849`; `Vector_OpacityRandom_50` from exact
to `mean=0.613800`, `visible=1527`; and `Vector_Thickness_50` worsens to
`mean=0.195738`, `visible=764`. Treat this only as a sign that ordinary soft-AA
dabs still have a local footprint/phase nuance, not as a radius rule.

2026-06-05 native recheck from the reverse workspace: the wide-`0x41` capsule
should not be chased through more radius/cap scalar tuning. Saved r2 dumps are
`E:\Documents\Claude\Projects\rizum-clip-studio-paint\tmp_r2_iswcore\v4_renderstroke_curve_fill_store_pdf_20260605_codex.txt`
and
`E:\Documents\Claude\Projects\rizum-clip-studio-paint\tmp_r2_iswcore\v4_initialize_pdf_20260605_codex.txt`.
`CSVec4Draw::Initialize @ 0x124667C0` sets
`draw+0x158 = max(1, int(offscreen_resolution * 0.15 / 25.4 * 16))`, and
`CSVec4Draw::StorePenHeadPoint @ 0x1246BD30` clamps the default circular
pen-head point count to `n=32` for large radii. Therefore the importer's
current 16 half-cap segments are native-backed for this large capsule. The
`-1.5` constant near `0x1256B788` is bbox expansion, not a footprint rule.
The next meaningful gap is `CSVec4Draw::FillPolygon @ 0x12465250`: native
12-bit fixed scanline conversion, half-open edge/row behavior, and alpha
composite branches. Keep the current `radius=width*0.99`, 16 half-cap, 4x-AA
bridge until a narrow native scanline implementation or probe beats it while
preserving exact guards.

No-edit probes saved as `tmp_vector_probe/test_vector_wide41_fillpolygon_probe_codex_20260605.json`
and `tmp_vector_probe/test_vector_wide41_fillpolygon_phase_grid_codex_20260605.json`
support the scanline/phase boundary but are too weak for a code change. Current
baseline is `0.610175/24477`; the best simplified 4x mask phase diagnostic
(`ox=1.0`, `oy=0.625`) reaches only `0.604394/24432`. Treat that as a native
`FillPolygon` clue, not an accepted importer constant.

Further native continuation from 2026-06-05: `FillPolygon` was reread through
r2 disassembly plus WSL `pdg`. Native builds an active-edge table from
`draw+0x1e8` points and `draw+0x204` count. It uses ceil-like y activation,
fixed x at `floor(x * 4096)` (scaled by `4` when `draw+0x1a0` AA scratch is
present), and span emission `left = current_x >> 12`,
`right = (next_x >> 12) - 1`. `CSVec4AntiAlias::OutputFrom32Bit` resolves 4x
scratch by `alpha=sum_alpha>>4` and alpha-weighted RGB; mode `1` replaces only
when new alpha is at least destination alpha. No-edit probes saved as
`tmp_vector_probe/test_vector_wide41_native_scan_probe_codex_20260605.json`,
`tmp_vector_probe/test_vector_wide41_native_scan_region_metrics_codex_20260605.json`,
and `tmp_vector_probe/test_vector_wide41_aaresolve_probe_codex_20260605.json`
show only a weak improvement: full `0.610175/24477 -> ~0.6057/24447`, wide bbox
`1.924541/9323 -> 1.906870/9293`; floor resolve and max-replace do not improve
further. Do not patch this yet.

## Latest AA Compact 0x41 Native Direction

2026-06-04 `iswCoreTG::RenderCurve` re-read: r2 plus a read-only IDA helper
confirm the native V4 curve renderer is quadratic, not the importer's current
cubic-like fallback. In `CSVec4Draw::RenderCurve @ 0x12466E50`, the block
around `0x12467A8C..0x12467B5B` computes sample count as
`int(CSBezier::CalcLengthFast() / *(draw+0x158))`, clamps it to at least `2`,
then evaluates `(1-t)^2`, `2*(1-t)*t`, and `t^2` over three control points
before `CSBezier::CalcTangent(t)`, `CSVPenHead::CalcFarestPoint`, `Store`, and
`FillPolygon`. Non-fill hull points are appended through `draw+0x1A8/+0x1B8`
with count `draw+0x1D4`; fill paths call `FillPolygon` at `0x12467736`,
`0x1246799F`, `0x12467D80`, and `0x1246826D`.

Do not yet replace the compact `.clip` `0x41` fallback with a guessed
quadratic. The `.clip` header flag `0x41` is not the same layer as the native
`CSVCurve` internal `flags&0x10` branch. The missing evidence is now the
`CLIPStudioPaint.exe` compact-reader mapping from 120-byte point tails
(`+88/+96`, `+104/+112`) into native `CSVCurve` `P0/P1/P2` and any internal
flag bits.

2026-06-04 follow-up in `CLIPStudioPaint.exe`: compact `.clip` `flags=0x41`
does not take the ordinary/spline node family. `0x1422D0510` dispatches on the
low flag byte: `0x20 -> 0x142567730`, `0x40 -> 0x1425677C0`, signed/high-bit
`-> 0x142567A00`, else `0x142567850`. Therefore compact `0x41` takes the
`0x40` factory, not the `0x2081` spline factory. That factory uses vtable
`0x1444E93B0`, allocates `0x98`-byte nodes, and its `+8` allocator reaches
`0x142624430`, which installs node vtable `0x1444EFB10`.

The `0x41` point-reader mapping is now native-confirmed. The shared reader
`0x1422CBAE0` reads the normal compact point fields, then calls node vtable
`+0xD8 = 0x142625DE0`, which reads four additional doubles: compact
`+88/+96 -> node+0x78/+0x80` and compact `+104/+112 -> node+0x88/+0x90`.
Node vtable `+0x60 = 0x1426253E0` evaluates the segment as a cubic-like curve
through `(node+8)`, `(node+0x78)`, `(node+0x88)`, and `(next+8)` via
`0x1431F9640`. However, a scoped importer probe that directly changed the AA
compact fallback from `p1=p0, p2=+104/+112` to full native cubic
`p1=+88/+96, p2=+104/+112` regressed the active AA targets: `Weak` became
`0.044102/1207`, `Medium` `0.027404/1794`, and `Strong` `0.025769/2142`
versus current `0.036818/1163`, `0.016958/1560`, and `0.020742/2070`; `None`
was unchanged at `0.036532/226`, and unrelated guards were unchanged. Rejected
as a direct importer patch. The useful conclusion is narrower: compact `0x41`
tail1 is real native cubic state, but the missing preview semantics are later
native flattening/curve-to-brush rendering details, not the compact reader
mapping itself.

2026-06-04 native walker follow-up: the active saved-curve walker
`0x1422CC1E0` uses the point vtable slots after length computation, not just
the simple `+0x60` sample slot. For compact `0x41`, vtable `+0x70 =
0x1426252B0` calls `0x1431F87A0 -> 0x1431F8330` to estimate cubic length from
quarter points and, for longer curves, a 4px-bucket polyline capped at 255
steps. Vtable `+0x68 = 0x142625370` calls `0x1431F9AF0 -> 0x1431F9770`, which
walks the same cubic by approximate arc length: `steps =
clamp(int(segment_length * 0.25), 4, 255)`, accumulates chord lengths, then
linearly interpolates within the crossing chord and calls cubic evaluator
`0x1431F9490`. Vtable `+0x78 = 0x142625450` calls tangent helper
`0x1431F9F90`, with endpoint-near branches gated by point flags `0x400` and
`0x800`. The isolated `Vector_AA_*` compact `0x41` objects have two points
with point flags `[0x401, 0x0]`, so the first-point `0x400` tangent endpoint
special case is active for AA, while position sampling through `+0x68` remains
the arc-length cubic route.

A second scoped importer probe used full native cubic controls plus the native
arc-length walking rule for the dark compact `0x41` fallback. It improved some
AA metrics but still failed the guard shape: `Vector_AA_Weak` became
`0.034807/1143`, `Vector_AA_Medium` `0.009327/1418`, but `Vector_AA_None`
worsened to `0.040897/253` and `Vector_AA_Strong` to `0.014118/2134`.
Unrelated guards stayed unchanged. The probe was reverted. Treat the result as
diagnostic evidence that native cubic/arc-length is part of the path, but the
current importer polyline/AA fallback is not the same consumer as native
Planeswalker/CSVec4 curve-to-brush rendering.

## Latest Material Colour Probe

2026-06-04 material colour follow-up: r2 re-read of
`0x14263C3A0 -> 0x142637A70 -> 0x14263DDB0` reconfirms the native retained
material row writer uses byte lane `1` for coverage, byte lane `0` for material
mix, expands the mix byte with `(byte0 * 257 + 1) >> 1`, blends BGR fixed-point
with `+0x4000 >> 15`, and then alpha-over blends retained row bytes in
`0x14263DDB0`. A current no-edit monkeypatch using the same fixed-point colour
blend but the importer's existing preview mix source is exactly neutral for
`Vector_BrushTip_Material_Gap` (`0.023456/258`) and ordinary
`Vector_BrushTip_Material` (`0.061113/663`). The diagnostic `+1` red offset
still lowers mean (`Gap 0.020594`, ordinary `0.051981`) but remains rejected:
it is not explained by the native fixed-point formula and likely points to the
unimplemented per-pixel byte0/byte1 cached row mapping or retained row flush.
Probe saved as `tmp_vector_probe/material_mix_fixed_probe_codex_20260604.json`.

2026-06-04 retained row-over follow-up: a no-edit monkeypatch replaced only
`_draw_material_stamp_rgba()` with a native-like 15-bit retained alpha-over
derived from `0x14263DDB0` (`dst_alpha + (0x8000-dst_alpha)*src_alpha >> 15`,
then straight-RGB mix ratio `(src_alpha*0x8000)/out_alpha` with
`+0x4000 >> 15`). Rounded 15-bit and `(u8*257+1)>>1` variants are exactly
neutral for current material samples (`Gap 0.023456/258`, ordinary
`0.061113/663`) and keep guards unchanged; a floor flush regresses the pair.
Therefore the remaining material mismatch is not a simple float-vs-native
alpha-over or final flush issue. Continue toward real per-pixel UV/cached
byte0/byte1 row mapping. Probe saved as
`tmp_vector_probe/material_native_over_probe_codex_20260604.json`.

2026-06-04 material UV-footprint recheck: a current no-edit monkeypatch varied
only the wide material four-sample offsets. The accepted native half-x/half-y
footprint `(u,v)`, `(u+xstep/2,v)`, `(u,v+ystep/2)`,
`(u+xstep/2,v+ystep/2)` remains the local and native-backed optimum. Older or
alternative footprints regress strongly: full-x/half-y moves Gap to
`0.113162/775` and ordinary Material to `0.259887/1334`; full-square and
centered-quarter variants are worse. Keep the current footprint; the remaining
material target is the full cached-lane row pipeline, not another UV sample
offset. Probe saved as
`tmp_vector_probe/material_uv_footprint_probe_codex_20260604.json`.

## Latest AA Compact 0x41 Consumer Probe

2026-06-04 AA compact `0x41` consumer-shape follow-up: after the native reader,
cubic, and arc-length facts were accepted as upstream evidence, current no-edit
probes tested whether the missing piece was simply replacing the importer's
distance-field polyline consumer with native-looking pen-head envelope
consumers. It is not. Monkeypatching `_draw_polyline_rgba()` for the dark
compact `0x41` family to draw a one-shot envelope polygon, per-segment hulls,
or capped envelope polygons regresses the isolated AA family badly. Example:
one-shot envelope moves `Vector_AA_None` from `0.036532/226` to
`0.111214/688`, `Weak` from `0.036818/1163` to `0.083151/1342`, and `Medium`
from `0.016958/1560` to `0.084422/1848`. Guards stay unchanged only because
they do not hit this branch. A second probe replacing the feathered capsule
with 2x/4x/8x supersampled hard capsule coverage also regresses Weak/Medium/
Strong strongly (`ss4` gives `Weak 0.080030/1439`, `Medium 0.076334/1857`,
`Strong 0.079029/2260`). Therefore do not patch AA compact `0x41` by changing
only the raster consumer to envelope, segment hull, or supersampled capsule.
The unresolved native gap remains deeper: the true `CSVStroke`/`CSVCurve`/
corner/pen-head state or Planeswalker compact consumer, not a shallow
replacement for `_draw_polyline_rgba()`. Probes saved as
`tmp_vector_probe/aa41_consumer_shape_probe_codex_20260604.json` and
`tmp_vector_probe/aa41_supersample_polyline_probe_codex_20260604.json`.

2026-06-04 AA compact `0x41` branch-placement follow-up: r2 vtable dump of
`0x1444EFB10` confirms extra slots beyond the sampled reader/walker:
`+0x100=0x142625E40`, `+0x108=0x142625260`, `+0x110=0x142625820`, and
`+0x118=0x1426244B0`. The `+0x118` helper performs cubic control split/rewrite
work through `0x1431FCB70` and propagates endpoint flags `0x400/0x800`, but
direct EXE byte search found only two `call [rax+0x118]` sites, around
`0x14018E228` and `0x140193FDA`, both in editor/command-like coordinate
manipulation code rather than the saved render route. Treat `+0x118` as a real
editing/control-point helper, not current evidence for the AA residual.

The importer branch placement is also clarified. The active narrow dark
compact `0x41` renderer is the early filled-curve branch for
`92/76/120/88`, `flags=0x41`, `width<=16`, dark RGB; it currently samples a
cubic-like fallback with `p1=p0`, `p2=compact+104/+112`, branch-specific
`sample_shift/radius_scale/feather`. It is not the later no-pattern native-dab
path, which only accepts `0x2011/0x2081`. A temporary probe that added cubic
tail handling to the later native-dab path showed zero calls on
`Vector_AA_Weak` (`cubic=0`, `dab=220`) and was reverted. Therefore do not
look for the AA `0x41` residual in `_native_spline_segment_controls()` or
native-dab feedback unless the filled-curve branch is first rerouted there with
new native evidence.

Fresh inspector output for the AA family is saved at
`tmp_vector_probe/aa41_detail_codex_20260604.json`. It shows the four target
`0x41` objects are all `brush_style_id=10`, `width=3.0`, point flags
`[0x401, 0x0]`, and true native cubic tails are present:
point0 `+88/+96` equals point0, point0 `+104/+112` is the forward control,
point1 `+88/+96` is the far return control, point1 `+104/+112` equals point1.
The second point's compact `f32+76` correlates with AA level in this fixture
(`0`, `0.129946`, `0.602505`, `0.820052`), but older native evidence maps it
to internal `+0x6c` sampler persistence/carry and `0x1422CC1E0(a8=1)`
overwrites the active carry, so keep it diagnostic only.

## Latest Test_Ballon Retained Material Row Finding

2026-06-05 retained/material quad closure from the native workspace: the
remaining `Test_Ballon` layer-8 rough line is not recoverable by directly
stamping the decoded pattern bitmap, alpha-modulating the current fallback
outline, drawing object points as polygons, or drawing synthetic connected
quads. Native `0x142636CC0` builds a four-vertex material quad from retained
previous/current segment transform state, then calls
`0x1426410B0(ctx, quad, 1)`. `0x1426410B0` is a scanline quad rasterizer: it
clips/sorts active edges, interpolates x and two material coordinates per row,
writes UV start/step to `ctx+0x208..+0x214` (plus secondary fields
`+0x218..+0x224` when `ctx+0x1d0 != 0`), and calls
`0x14263B7F0(ctx, x0, x1, y)`.

`0x14263B7F0` is a retained row dispatcher/coverage writer. It chooses writer
families from `ctx+0x268/+0x274/+0x270/+0x26c`, row/profile masks, and material
format flags; flag `0x20` is RGBA, flag `0x10` is coverage/mix, and format `1`
is single-channel. The dynamic material branch calls `0x14263C3A0`, which in
turn calls `0x142637A70` or `0x142637C70` before final row write
`0x14263DDB0`. Treat the current PatternStyle-11 low-alpha preview and other
balloon material approximations as scoped preview bridges only. A future exact
implementation must reproduce the retained segment transform and row pipeline
narrowly, with `Vector_OpacityPressure`, `Vector_OpacityVelocity_50`, material
brush guards, and `Vector_SizePressure` preserved.

2026-06-05 retained row disambiguation: `0x14263DF20` is now identified as the
four-edge repair helper used by `0x14263B270`. It evaluates the two quad edges
not already chosen for the active span at the current `y + 0.5` row and may
replace the left/right span endpoints and their `u/v` values before fixed-15
setup. Native material-AA rendering is therefore not a simple parallel strip
or one rectangle parameterization; it can split and repair each row from the
full edge table. A tiny low-opacity retained-state probe still regressed badly
(`best ~= 3.17 premul mean / 28k visible` vs current `1.181103/14326`), so do
not keep tuning opacity/radius on direct material-quad replacement.

Follow-up native detail: saved dump
`tmp_r2_csp/material_quad_builder_1426442B0_pdf_20260605_codex.txt` in the
reverse workspace shows `0x1426442B0` only fills material UVs for an already
built four-vertex screen quad. It copies screen `x/y` vertices at 0x20-byte
stride, writes material coords at vertex `+0x10/+0x18`, uses `ctx+0x1F8` and
`ctx+0x1FC` as material width/height, and switches four UV layouts from the
retained transform flag (`queue+0x218`) plus orientation flag (`queue+0x154`).
Caller `0x142636CC0` builds the screen quad from previous retained point
`queue+0x1E0/+0x1E8`, current point `queue+0x258/+0x260`, previous/current
size and angle fields, then calls `0x1426442B0` before rasterizing through
`0x1426410B0(ctx, quad, 1)`. This confirms importer work should not add more
bitmap-stamp tuning; the missing native surface is retained segment quad UV
orientation plus row coverage/sampling.

No-edit probe saved as
`tmp_vector_probe/test_ballon_retained_segment_quad_probe_codex_20260605.json`
rejects implementing only this segment-quad/UV layer. Replacing the
PatternStyle-10 balloon outline with native-shaped material segment quads keeps
the same fill but worsens full `Test_Ballon`: baseline is `1.181103/14326`;
the best tested quad variant is `1.248549/14449`, and lower-opacity/max
variants mostly expand visible pixels into the `27k..30k` range. Do not patch
the importer with retained segment quads alone. The next meaningful probe must
include closer `0x1426410B0` scan conversion, `0x14263B7F0` material row
sampling, and retained 16-bit accumulation.

Branch refinement from direct fixture inspection: `Test_Ballon` layer 8's
primary rough outlines use brush style 4 (`StyleFlag=0x1c230`,
`PatternStyle=10`, `AntiAlias=2`, `IntervalBase=1.0`, `AutoIntervalType=2`,
`FlowBase=1.0`, `Hardness=1.0`, `RotationBase=0.0`). The retained-state bit
`0x20` is set and the direct/max bit `0x1000` is clear. Seven layer-8 balloon
records use this line style with flags `0x128` / `0x188`, width `2.5`, and
opacity `1.0`. Treat the relevant native branch as build-up retained/material
row accumulation, not a single-lane fast path or direct-max overwrite.

Focused native pseudo-code for `0x1426410B0`: the input material quad is four
double vertices `(x, y, u, v)` at 0x20-byte stride. The function clips bounds
against the row context, builds four edge records with `dx/dy`, `du/dy`, and
`dv/dy`, walks integer y rows, evaluates and x-sorts up to four active edge
intersections, and dispatches one or two inclusive spans to
`0x14263B7F0(ctx, x0, x1, y)`. In the direct branch (`ctx+0x1d0 == 0`) with
`modeFlag == 0`, CSP applies a `+0.5` half-pixel bias before writing fixed-15
material start/step to `ctx+0x208/+0x20c/+0x210/+0x214`. The alternate
`ctx+0x1d0 != 0` path goes through `0x14263B270` and also uses secondary
material fields `+0x218..+0x224`. The next no-edit probe should emulate this
row scan and UV setup before attempting an importer patch.
Native constants checked from r2: `0x1444DED58 = 32768.0` for fixed-15 scale,
`0x144334820 = 0.5` for the conditional half-pixel bias, and
`0x144333E90 = 1e-8` for near-equality edge tests.

IDA MCP read-only cross-check via Claude Code confirmed `0x14263C3A0` is
reached from `0x14263B7F0` when `ctx+0x268 == 0` and `ctx+0x150 != NULL`;
`ctx+0x2A0` only enables extra row-buffer setup before the same call. The
`ctx+0x268 != 0` side routes to alternate pattern/material writer families
instead, so do not invert this branch when porting the row pipeline.

Follow-up no-edit probes from the reverse workspace reject three tempting
shortcuts. `tmp_vector_probe/test_ballon_retained_row_fixed15_probe_codex_20260605.json`
implements a closer fixed-15 scanline plus 16-bit build-up segment-row
prototype, but still worsens full `Test_Ballon`; best tested row variant is
about `1.329556/15619` versus current `1.181103/14326`. The interval-position
material-dab probe
`tmp_vector_probe/test_ballon_retained_interval_material_dab_probe_codex_20260605.json`
is a major regression (`20+` premul mean and `100k+` visible pixels for the
least bad material-stamp family). A diagnostic style-4 feather probe
`tmp_vector_probe/test_ballon_style4_softness_probe_codex_20260605.json` can
reduce premul mean slightly (`1.123839` best tested), but expands visible pixels
to roughly `17k+`, so it must not be patched without native support.

PSD layer oracle: exported `Test_Ballon.psd` layer `Balloon 1` has bbox
`[110,159,889,912]`, the same as the full residual. Comparing importer layer 8
alone against that PSD layer gives the known `0.786705/12602` premul metric.
Residual split is roughly `out_only=882`, `ref_only=2228`, `both=9492`; alpha
nonzero is slightly low (`239759` importer vs `241101` PSD), yet high-alpha
residual pixels are too common in the importer. Current preview is therefore
too hard in the core while missing outer retained/phase coverage. Continue with
native retained sample phase, quad construction, or object geometry; do not
try more direct material-stamp, fixed-row segment, opacity, radius, or feather
tuning.

Retained state pseudocode refinement from the reverse workspace:
`0x142558A90` now reads as `PW_DrawRetainedMaterialSample`. It selects
`draw+0x180` or `draw+0x1E0` retained state, sets `plot+0x50` to that state,
calls `0x1422D8BB0` to fill the current plot and phase/resource outputs, then
submits only if `state+0x58 != 0` and the visibility gate passes. Submission
therefore draws the segment from the previous retained snapshot to the current
point; the first sample only seeds state. After submission it writes current
center, size, phase, wrap flag, and resource pair back into the retained state
and increments the count.

Two tempting helper branches are now ruled out as primary causes.
`0x14255C2A0` is a segment visibility gate: it rounds the transformed current
center, applies draw offsets, expands by `1.5` when the plot has transform
state, and queries the active region. `0x1425597A0` builds descriptor/color
state: it derives fixed-15 `0x8000` weights from the plot, directly reads
resource colors on the style/material `0x20` path, otherwise blends fixed-15
colors, and uses 16-bit coverage scaling in the secondary-opacity branch.
Importer work should still target the retained segment transform and native
`0x1426410B0 -> 0x14263B7F0` row accumulation, not more bbox, descriptor,
opacity, or material-stamp tuning.

No-edit state-machine shortcut probe: monkeypatching only the current
`PatternStyle=10` balloon point-family preview rejected two tempting importer
patches. Full `Test_Ballon` baseline is `premul_mean=1.181103`,
`premul_visible=14326`; skipping the first dab of each retained path worsens to
`1.183447/14336`, and connecting retained resample points with ordinary
capsule segments worsens to `1.202441/14499`. Do not implement either shortcut.
The next useful work is still native material quad UV/row sampling, or more
exact transform-field recovery before another probe.

Reverse-workspace field-chain update: `0x14260F550` mechanically copies
`plot+0x2c` to `queue+0x154`, and `0x14260DB90` passes that value as `r9d` to
`0x142636CC0`, which immediately stores it at material row context
`ctx+0x1d0`. `0x1422D8BB0` fills `plot+0x2c` from `BrushStyle+0x190`
(`AntiAlias`) when its suppress-AA argument is zero, otherwise it writes zero.
For `Test_Ballon` layer 8 the style table says `AntiAlias=2`, so the retained
rough balloon route should use the nonzero material-AA branch:
`0x1426410B0` alternate row setup and `0x14263C3A0 -> 0x142637A70`, not the
direct sampler `0x142637C70`. Importer work should therefore focus on the
material-AA sampler/16-bit accumulation behavior before more geometry or
opacity tuning.

Reverse-workspace branch-map refinement: `0x142644180` initializes material
raster context flags. It maps incoming `r9d` to `ctx+0x268/0x274/0x270`, then
stores stack args into `ctx+0x90`, `ctx+0x1e8`, `ctx+0x1ec`, `ctx+0x248`,
`ctx+0x1f0`, and `ctx+0x1f4`. For `Test_Ballon` style 4
(`StyleFlag=0x1c230`, `PatternStyle=10`, `TexturePattern=0`, `AntiAlias=2`),
the caller path implies active material branch, `ctx+0x248=0`,
`ctx+0x90=1`, and default `ctx+0x228=1`. So the rough balloon route is the
AA4 single-channel mask sampler (`0x142637A70 -> 0x14263E5D0`) plus retained
16-bit row accumulation, not `0x10/0x20` lane mixing or RGBA material
sampling. Existing direct stamp, segment-quad-only, and fixed-row probes remain
rejected because their geometry/phase/row scan is still too shallow.

Reverse-workspace material-AA row detail: `ctx+0x1d0 != 0` in `0x1426410B0`
calls `0x14263B270`, which writes both primary UV fields
`ctx+0x208/+0x20c/+0x210/+0x214` and secondary UV fields
`ctx+0x218/+0x21c/+0x220/+0x224` before dispatching `0x14263B7F0`. The direct
branch only writes the primary stream. For `Test_Ballon` style 4, the next
meaningful no-edit prototype should therefore port this dual-UV AA row setup
and `0x14263E5D0` mask averaging; one-UV fixed-row, segment-quad-only, and
material-stamp shortcuts are already rejected.

Reverse-workspace AA4 sampler refinement: `0x14263E5D0` samples bytes at the
current primary UV, primary UV plus `ctx+0x210/+0x214`, current secondary UV,
and secondary UV plus `ctx+0x220/+0x224`, then advances both streams by another
step. Its fixed-point coverage is effectively
`incoming_0x8000 * (a0+a1+a2+a3) / 1020`, implemented with multiply by
`0x80808081` and `shr 9`; the non-AA one-sample sibling uses `shr 7`, i.e.
division by `255`. Existing true-resource/AA4 probes still regress
`Test_Ballon`, so do not patch from the sampler formula alone; the missing
piece is likely upstream segment edge-table input or descriptor feedback state.

Reverse-workspace retained edge-table refinement: WSL `pdg` plus r2 disassembly
of `0x1426410B0` shows the upstream row builder constructs a scanline edge
table from all four material-quad edges before calling `0x14263B270`. For each
row it evaluates active edge intersections, sorts by x, merges near-equal x
pairs with epsilon `1e-8`, collapses some 3-intersection cases, and emits two
spans when four intersections remain. In the non-AA direct path it applies a
`0.5` pixel-center offset before fixed-15 conversion; in the material-AA branch
it passes the intersection pair, row, and edge indexes into `0x14263B270`,
which handles dual-UV setup and `0x14263DF20` four-edge disambiguation. This is
the strongest current reason true-resource/AA4 segment-quad probes still
overdraw: they approximate the quad footprint but not native edge sorting,
near-equal merge, split-span emission, or descriptor retry state.

Reverse-workspace retained/material quad UV and AA4 mask semantics:
`0x142636CC0` builds the screen quad from previous retained sample and current
sample with this sign pattern:

```c
sincos(prev_angle, &sin0, &cos0);
sincos(cur_angle,  &sin1, &cos1);
v0 = { prev.x - cos0 * prev_size, prev.y + sin0 * prev_size };
v1 = { prev.x + cos0 * prev_size, prev.y - sin0 * prev_size };
v2 = { cur.x  + cos1 * cur_size,  cur.y  - sin1 * cur_size  };
v3 = { cur.x  - cos1 * cur_size,  cur.y  + sin1 * cur_size  };
```

The `sin/cos` labels are inferred; the sign pattern is the stable native fact.
The `0x14260DB90 -> 0x142636CC0` call-site maps the callee's stack-view fields
as: previous center `queue+0x1e0`, previous `size/angle` pair
`queue+0x1f0/+0x1f8`, previous phase distance `queue+0x208`, current center
`queue+0x258`, current size `queue+0x128`, current angle `queue+0x200`,
current phase distance `queue+0x210`, UV orientation `queue+0x218`, and
flip/secondary flag from `0x14260DB90`'s `edx` argument. In
`0x1426442B0`, output vertices are `{x,y,u,v}` with stride `0x20`. It copies
the four screen vertices, then fills material UVs from `width=ctx+0x1f8` and
`height=ctx+0x1fc`:

```c
mode 0: U across width, V = phaseA for v0/v1 and phaseB for v2/v3; clamp V.
mode 1: V across height, U = phaseA for v0/v1 and phaseB for v2/v3; clamp U.
mode 2: U across width, V = height - phaseA/phaseB; clamp V.
mode 3: V across height, U = width  - phaseA/phaseB; clamp U.
flip swaps which side gets 0 vs width/height.
```

`0x14263B7F0` confirms the style-4 `Test_Ballon` route enters the
single-channel material-mask row loop with `ctx+0x90 == 1` and `ctx+0x228 == 1`
and calls `0x14263E5D0`, not RGBA/lane mixers. The loop reads a 16-bit
destination coverage word; if it is below `ctx+0x1e0`, it starts `coverage =
ctx+0x1e4`, calls `0x14263E5D0`, and writes:

```c
dst16 += ((ctx->limit16 - dst16) * coverage) >> 15;
```

`0x14263E5D0` samples four mask bytes: primary UV current, primary UV advanced
by both primary row/span steps, secondary UV current, and secondary UV advanced
by both secondary row/span steps. It then multiplies incoming fixed-15 coverage
by the byte sum with the `0x80808081` reciprocal trick, equivalent to roughly:

```c
coverage = (coverage * (m0 + m1 + m2 + m3)) / 1024;
```

This is the native reason single-UV material-row probes were only diagnostic:
the real retained/material AA path combines two UV edge streams and scales
coverage before the 16-bit coverage interpolation.

Reverse-workspace retained queue field-source correction: in `0x142636CC0`,
local references like `[rbp+0x8d8]`, `[rbp+0x8f8]`, `[rbp+0x900]`, and
`[rbp+0x908]` are stack-argument views, not stable queue offsets. The stable
mapping from `0x14260DB90` is:

```c
prev_center      = queue+0x1e0;  // passed as pointer
prev_size_angle  = queue+0x1f0;  // size at +0x1f0, angle at +0x1f8
prev_phase       = queue+0x208;
cur_center       = queue+0x258;  // passed as pointer
cur_size         = queue+0x128;
cur_angle        = queue+0x200;
cur_phase        = queue+0x210;
uv_orientation   = queue+0x218;
flip_or_secondary = dispatch argument edx to 0x14260DB90;
opacity_pair     = queue+0x130;  // becomes ctx+0x1e0/+0x1e4 after *32768
```

This also explains why the material-strip probe was weak: it used guessed path
phase and tangent fields, while native has retained previous/current phase and
angle fields already in the queue. The next native target is where
`0x14260F550` fills `queue+0x1f0/+0x1f8/+0x200/+0x208/+0x210/+0x218` from the
retained plot/descriptor, not more importer-side strip sweeps.

No-edit retained/material strip probe rejected: script
`tmp_vector_probe/probe_test_ballon_native_material_aa.py` monkeypatched only
`Test_Ballon` `PatternStyle=10` balloon outline, kept the current polygon fill,
and swept approximate material-strip modes/flips/phases using the native
`0x1426442B0` UV orientation table and a two-sample approximation of
`0x14263E5D0`. Best full metric was `premul_mean=1.428421`,
`premul_visible=21660`, worse than the current formal baseline
`1.181103/14326`. Do not patch direct retained material strips over the current
resampled point path. The failure suggests the remaining gap is more likely in
the upstream retained sample list/phase fields or exact row-edge selection than
in simply replacing point dabs with a material-textured segment.

Reverse-workspace retained queue phase/angle source: `0x14260F550` is mostly a
structure copier. In the retained-material call from `0x142558A90` /
`0x14255C680`, its inputs are:

```c
PW_CopyPlotToBrushQueue(queue, plot, bounds_desc, current_center,
                        quantized_or_split_bbox, material_desc,
                        aux_a, aux_b, extra_int,
                        secondary, descriptor_empty_flag, invalidate_flag);
```

The exact stable field facts are:

```c
queue+0x128..0x180 = plot+0x00..0x58;      // current size/color/resource flags
queue+0x188..0x1d8 = bounds_desc+0x00..0x50;
queue+0x258..0x267 = current_center xy;
queue+0x268/+0x270 = aux_a double/int pair;
queue+0x274/+0x27c = aux_b double/int pair;
queue+0x280        = *extra_int_ptr;
queue+0x284        = descriptor_empty_flag at this retained callsite;
queue+0x118        = invalidate_flag;
if (plot+0x50 != NULL) {
    queue+0x1e0..0x21f = *(plot+0x50);     // retained state snapshot
    queue+0x178 = queue+0x1e0;             // dispatch uses retained snapshot
}
```

At the normal `0x142558A90` callsite, the exact stack mapping into
`0x14260F550` is:

```c
PW_CopyPlotToBrushQueue(draw->queue140, &plot, &bounds_desc, &current_center,
                        NULL, &aux_a, &aux_b, &extra_int,
                        secondary_flag, descriptor_empty_flag, 0);
```

`aux_a` is written by `0x1425597A0` from `draw+0x78/+0x80`,
`aux_b` from `draw+0x84/+0x8c`, and `extra_int` is written through
`0x1408E5840(ptr,value)`. `descriptor_empty_flag` is `1` when
`0x1425590B0` returns `0`. `0x142614D00`, the return source inside
`0x1425590B0`, returns `0` only when the selected retained descriptor has
`descriptor+0x28 == 0`; otherwise it blends fixed-point descriptor channels
`+0x30/+0x34/+0x38/+0x3c` by the plot weight and returns `1`. If
`descriptor_empty_flag != 0` and retained `sample_count >= 2`,
`0x142558A90` loops back and runs the `0x1425590B0/0x1425597A0` descriptor
build once more (bounded to two attempts). This is native evidence that rough
retained submission is descriptor-state driven, not a simple one-quad-per
sample-pair strip.

Reverse-workspace retained descriptor pass continuation: `0x14260DB90`
immediately passes queue fields `+0x268`, `+0x274`, `+0x280`, and `+0x284` to
`0x142643C80(row_ctx, aux_a, aux_b, extra_int, descriptor_empty_flag)`. If
`descriptor_empty_flag != 0`, `0x142643C80` sets `row_ctx+0x26c = 1` and skips
the normal primary descriptor/color extraction from `aux_a`; it still loads
the secondary color bytes from `aux_b` into `row_ctx+0x25c..0x264`. For the
known `Test_Ballon` style-4 state (`row_ctx+0x268 != 0`, `+0x274 == 0`,
`+0x270 == 0`), `0x14263B7F0` checks `row_ctx+0x26c` at `0x14263B92E`. If it
is set, the row dispatcher jumps to `0x142643670` instead of the inline
single-channel material-AA build-up loop.

`0x142643670` is a descriptor-accumulation row pass. It reads row coverage
from the paired row planes at `row_ctx+0x120` and `row_ctx+0x150`, calls
`0x142637930` to scale the candidate by the material/mask sample, and then
accumulates nonzero coverage into the descriptor aggregator at `row_ctx+0x20`
through `0x1426154F0`/`0x142615510`. This is not the final pixel-writing pass.
Native implication: when the retained descriptor is empty, rough/material
segments run a gather pass that builds descriptor state; `0x142558A90` can then
loop and submit a second pass with a non-empty descriptor. Previous Python
segment-quad probes skipped this two-pass descriptor feedback, so their
`20k..33k` visible-pixel overdraw is expected and does not falsify the native
row writer.

2026-06-05 descriptor gather refinement: saved r2 evidence in the analysis
repo at `tmp_r2_csp/retained_descriptor_gather_chain_20260605_codex.txt` gives
the first implementable sketch of the retained descriptor struct.
`0x1426154F0` is the narrow single-channel weighted accumulator: nonzero
`weight` adds to `desc+0x08` and adds `sample16 * weight` to `desc+0x18`, then
increments `desc+0x28`. `0x142615510` is the three-channel weighted
accumulator: it adds `weight` to `+0x08`, then accumulates source words
`r8+4/r8+2/r8+0` into `+0x10/+0x18/+0x20`, optionally after conversion through
`0x14256E1A0` when `desc+0x40 == 1`. `0x140F5C620` is just
`return *(uint32_t *)(surface+0x80)`, so `0x142643670` chooses these by the row
surface format at `row_ctx+0x150`: flag `0x20` uses the three-channel path,
flag `0x10` uses the single-channel weighted path, and format value `1`
accumulates coverage only into `+0x08/+0x28`.

`0x142614D00(desc, plot+0x40, flags, mode)` returns 0 only if
`desc+0x28 == 0`; otherwise it averages the gather sums, clears
`+0x08/+0x10/+0x18/+0x20/+0x28`, and writes or blends stable output fields
`+0x30/+0x34/+0x38/+0x3c`. If `flags & 0x20` is clear it derives one channel
from `+0x18` and mirrors it to all three output channels; if set it uses the
three-channel path, with a special `mode==0 && desc+0x40==1` conversion path
through `0x14256E150`. For `Test_Ballon` style 4 this makes the missing native
layer look like descriptor-mediated coverage/strength feedback, not direct RGB
material painting. Do not patch importer semantics from the old direct material
quad probes.

Read-only IDA/Hex-Rays audit from the adjacent reverse workspace confirmed the
same mechanics against active `CLIPStudioPaint.exe` image base `0x140000000`.
Keep the two "empty" concepts separate: `desc+0x28 == 0` means no gathered
samples, so `0x142614D00` returns `0` and the retained caller treats the
descriptor as empty; `desc+0x2c != 0` means a non-empty descriptor is doing its
first direct output fill instead of later EMA smoothing.

`0x1422D8BB0` is the native source for the retained angle/orientation fields,
not the polyline tangent guessed in the rejected importer probe. At return it
writes the plot and also mutates the retained state at `plot+0x50`:

```c
plot+0x00 = scaled_size_or_spacing;
plot+0x08 = axis/opacity_a;
plot+0x10 = axis/opacity_b;
plot+0x28 = style_flag_0x40;
plot+0x2c = BrushStyle+0x190 AntiAlias unless suppressed;
plot+0x48 = spacing_or_texture_phase_scale;
retained_state+0x20 = sample+0x48;         // current angle used by 0x142636CC0
retained_state+0x38 = orientation_bucket;  // later queue+0x218
```

After a segment submit, `0x142558A90` rolls the retained state forward:

```c
state+0x00/+0x08 = current_center;
state+0x10       = current_size;
state+0x18       = state+0x20;             // current angle becomes previous
state+0x28       = previous/wrapped phase value;
state+0x30       = current phase output;
state+0x5c       = wrap/orientation flag;
state+0x58++;
```

Native implication for `Test_Ballon`: the importer should not infer retained
segment angle solely from resampled path tangent. CSP carries an evaluated
sample angle (`sample+0x48`) through the retained state and separately derives
the material UV orientation bucket in `0x1422D8BB0`; missing those two fields
can alter the large hard residuals even if the pixel writer is correct.

Reverse-workspace retained-state roll-forward correction: re-reading
`0x142558A90` around `0x142558e30..0x142558fc7` shows that segment submit is
gated by `state+0x58 != 0`, `r14d != 0`, `0x14255C2A0(draw,current,&plot,xmm3)`,
and a small positive current bbox/size test. After the submit-or-skip block,
the caller always rolls the retained state forward:

```c
state->resource_pair = rbp-0x78;
state->x/y           = transformed_current_center;
state->size          = current_size;
state->prev_angle    = state->angle_current;
state->phase0        = (old_wrap_flag == 0) ? old_phase1 : 0.0;
state->phase1        = phase_out_from_0x1422D8BB0;
state->wrap_flag     = new_wrap_flag_from_0x1422D8BB0;
state->sample_count++;
```

So the queue segment uses the retained snapshot produced before this final
roll-forward. In particular, after a wrap the next `phase0` becomes `0.0`; a
simple modulo/clamp phase stream over all current samples is not native enough.
No-edit probe `tmp_vector_probe/probe_test_ballon_retained_state_trace.py`
added first-submit suppression plus clamp/mod phase modes and still badly
overdrew (`best premul_mean=3.366387`, `premul_visible_px=28896`). Reject
"native-like state trace over all connected current samples" as a patch.

Reverse-workspace Test_Ballon retained sample-list diagnostic: no-edit probe
`tmp_vector_probe/probe_test_ballon_retained_sample_angles.py` compared the
accepted importer `PatternStyle=10` point-family sample list with a native-like
quadratic walk over the three style-4 control-point records. The current
`record.width * interval_base` density matches native-like counts exactly
(`324 + 244 + 314 = 882` samples), with tiny position deltas (`mean/max` about
`0.043/0.076`, `0.016/0.027`, `0.021/0.033` pixels by record). A
`2 * width * interval_base` diameter-spacing variant gives half the count
(`441`) and is rejected. Tangent/angle deltas exist but are not large enough
to explain the current error by primary point density alone. Treat the
remaining `Test_Ballon` gap as true retained segment transform, AA material
row-edge selection, dual-UV four-sample mask coverage, or 16-bit accumulation
semantics, not as a simple sample-count bug.

Reverse-workspace material-AA sampler refinement and rejection: fresh r2
evidence in the adjacent reverse workspace sharpens the style-4 branch.
`0x14263B270` is the `ctx+0x1d0 != 0` row helper from `0x1426410B0`; it
evaluates two active polygon edges for the scan row, clips the integer span,
writes two fixed-15 UV streams at `ctx+0x208..0x224`, and uses constants
`32768.0` for starts and `16384.0` for per-pixel deltas before calling
`0x14263B7F0(ctx, x0, x1, y)`. For `Test_Ballon` style 4 (`ctx+0x268 != 0`,
`ctx+0x274 == 0`, `ctx+0x270 == 0`, no `ctx+0x150` material-format flags,
`ctx+0x90 == 1`, `ctx+0x228 == 1`) the row dispatcher enters the lightweight
single-channel material-AA loop, not the RGBA `0x20/0x10` material writer
families. If `ctx+0x1d0 != 0`, each not-yet-full pixel calls `0x14263E5D0`;
that helper samples four mask bytes through `0x14263E170`, multiplies incoming
coverage by `sum(mask) / 1024`, and the caller updates the 16-bit row plane as
`dst += ((limit - dst) * coverage) >> 15`. `0x14263E170` clamps or rejects
fixed-15 UVs according to `ctx+0x22c/+0x230`, then indexes
`ctx+0x238 + (v >> 15) * ctx+0x244 + (u >> 15) * ctx+0x240`.

No-edit probe `tmp_vector_probe/probe_test_ballon_retained_quad_aa4.py` used
the current `PatternStyle=10` point-family samples, connected them into
approximate retained segment quads, used the true style-4 material resource,
four sub-samples, accumulated phase step `18.4`, and native-like 16-bit
build-up. The narrowed sweep still regressed badly: best full `Test_Ballon`
metric was `premul_mean=1.639960`, `premul_visible_px=20566` versus current
formal `1.181103/14326`. Reject "connect current retained samples into
material-AA quads" as an importer patch. The row writer formula is real, but
exact rough balloons need the native retained segment inputs:
previous/current phase, orientation bucket, angle, and edge selection as
actually produced by `0x1422D8BB0` / retained state, not guessed from the
current resampled path.

Reverse-workspace retained submit/descriptor gate reading: saved r2 evidence
in `tmp_r2_csp/retained_submit_gates_14255C2A0_1425590B0_1425597A0_pd_20260605_codex.txt`
and `tmp_r2_csp/rough_retained_descriptor_1425597A0_pd_20260605_codex.txt`
shows why direct segment-quad probes overdraw. `0x14255C2A0` is a visibility
query: if `draw+0x120 == NULL` it returns true, otherwise it rounds current
center with a `+-0.5` bias plus draw offsets, expands the query radius from
`plot+0x00` plus caller `xmm3`, optionally scales by `plot+0x30` and
`plot+0x18`, then calls `0x142619200`. `0x1425597A0` copies draw descriptor
bounds from `draw+0x78/+0x80` and `draw+0x84/+0x8c`, optionally refreshes them
through `0x14255A020`, then converts `plot+0x00` to fixed-15:

```c
weight = int(plot+0x00 * 32768.0 + 0.5);
inverse = 0x8000 - weight;
```

It uses that pair to blend descriptor/color channels from the two retained
descriptors and later updates draw/resource descriptors through
`0x14260D3D0`, `0x1426155F0`, and `0x14206C350`. Current conclusion:
`0x1425597A0` is not a mere color helper for retained/material balloons; it is
part of the descriptor/gating state that decides what reaches `0x14260F550`.
The next useful prototype should emulate or at least measure these descriptor
gates before drawing material quads. Do not do more "connect every current
sample into a quad" sweeps.

Reverse-workspace retained plot evaluator field refinement: re-reading
`0x1422D8BB0` gives a more implementable core. It first evaluates effective
retained size from the normal brush dynamics path and multiplies by
`sample+0x38`, with a floor of `0.1`. The output plot written at the end is:

```c
plot+0x00 = axis_scale_or_1 * effective_size;
plot+0x08 = opacity_or_axis_limit;
plot+0x10 = min(1.0, opacity_like_value);
plot+0x18 = 0;
plot+0x20 = 0;
plot+0x28 = style->StyleFlag & 0x40;
plot+0x2c = caller_suppressed ? 0 : style->AntiAlias;
plot+0x40 = *arg_resource_or_style_word;
plot+0x44 = local_material_error_flag;
plot+0x48 = optional texture/material scalar from style+0x2f8 path;
plot+0x58 = caller passthrough;
plot+0x50->angle_current = sample+0x48;       // state+0x20
plot+0x50->orientation   = orientation_bucket; // state+0x38
```

The orientation bucket is derived from `style+0x268` rotation, optionally
adjusted by a wrap/phase flag, then compared against literal quadrant
thresholds `45`, `135`, `225`, and `315` degrees to produce buckets `0..3`.
The retained phase advance block selects resource axes from the cached material
object, swaps which axis is the phase axis depending on the bucket, then
computes:

```c
phase_next = phase_prev + resource_phase_axis * 4.0
             / (2.0 * effective_size * axis_scale);
if (phase_next > resource_phase_limit) {
    phase_delta = (resource_phase_limit - phase_prev)
                  * 2.0 * effective_size * axis_scale / resource_phase_axis;
    phase_next = resource_phase_limit;
    *wrap_flag = 1;
} else {
    phase_delta = 4.0;
    *wrap_flag = 0;
}
```

For `Test_Ballon` style 4 (`resource 23x2511`, size `2.5`, axis scale near
`1.0`), choosing the `23` axis gives the observed `23*4/(2*2.5)=18.4` phase
advance. Exact rough-balloon rendering now needs a trace/emulation of this
state machine over the native sample order, including when `state+0x58==0`
suppresses the first submit and when the caller rolls `state+0x28/+0x30/+0x5c`.

2026-06-05 retained descriptor-to-row confirmation: direct PE call scanning
found three callers of `0x142644230`: `0x142637138`, `0x14263F81A`, and
`0x1426423D8`. The helper calls `0x1426155F0(row_ctx+0x20, ...)` and writes
the finalized descriptor high-16 channels into:

```c
row_ctx+0x284 = desc_out_34 >> 16;
row_ctx+0x288 = desc_out_38 >> 16;
row_ctx+0x28c = desc_out_3c >> 16;
row_ctx+0x290 = desc_out_30 >> 16; // descriptor weight/gate
```

At all three call sites, the caller then optionally calls
`0x14263E200(row_ctx, &row_ctx+0x294, &row_ctx+0x298, &row_ctx+0x29c)` when
the row/material mode gates allow it. `0x14263E200` is the descriptor-channel
mixer: if `row_ctx+0x290 == 0`, it leaves the destination triple unchanged; if
`row_ctx+0x27c == 0`, it copies `+0x284/+0x288/+0x28c` directly to the three
output pointers; otherwise it blends each channel with

```c
out = (old * row_ctx+0x27c + desc_channel * mix_weight)
      / (row_ctx+0x27c + mix_weight);
mix_weight = ((0x8000 - row_ctx+0x27c) * row_ctx+0x290) / 0xffff;
```

with a special color-space path through `0x14256DF40` when the surface format
has `0x20` and `row_ctx+0x1f0 == 1`. IDA audit session
`a5e49c13-590d-494a-af42-a7e0e0a59eb1` confirmed the structure and corrected
the exact weight divisor from an initial r2-style `>> 15` sketch to `/ 0xffff`.
The larger sibling row loop `0x14263A780` also calls `0x14263E200` at
`0x14263AA78` before writing row pixels. In the `row_ctx+0x228 == 1` path it
directly uses `row_ctx+0x294/+0x298/+0x29c`; otherwise it expands sampled
material channels to 16-bit, mixes them through `0x14263E200`, and then writes
the resulting triple to the 8-byte pixel lane (`word [dst+2]`, `[dst]`,
`[dst-2]`) alongside the 16-bit coverage plane. This confirms the retained
descriptor path is real row/material color-state input, not dead metadata and
not a direct alpha-only coverage formula.

2026-06-05 retained descriptor gate refinement: `0x14260F8B0` fills the queue
mode flags later consumed by `0x14260DB90`. If the segment mode word
`src+0x1c == 2`, it sets `queue+0x288 = 1`, writes `queue+0x28c` and
`queue+0x290`, and clears `queue+0x294`; if `src+0x1c == 3`, it clears
`queue+0x288` and sets `queue+0x294 = 1`; otherwise both flags are clear.
`0x142644180` maps that same mode to row context flags:

```c
row_ctx+0x268 = mode != 0;
row_ctx+0x274 = mode == 2;
row_ctx+0x270 = mode == 3;
```

For the previously observed style-4 state (`+0x268=1`, `+0x274=0`,
`+0x270=0`), the native mode is therefore `1`. In that mode
`0x14260DB90` does not call either descriptor-weight updater:

- `0x142643BF0`, used only when `queue+0x288 != 0`, writes
  `row_ctx+0x278 = edx`, `+0x27c = round(xmm2 * 32768)`, and
  `+0x280 = round(xmm3 * 32768)`;
- `0x142643C40`, used only when `queue+0x294 != 0`, writes
  `row_ctx+0x27c = round(pow(xmm1, 2.5) * 32768)`.

The setup path around `0x142636783` clears the row-context qwords covering
`+0x278/+0x27c`, so mode 1 should usually enter `0x14263E200` with
`row_ctx+0x27c == 0`. Combined with the previous note, this means the style-4
rough retained/material path most likely direct-copies finalized descriptor
channels into `row_ctx+0x294/+0x298/+0x29c` before row pixels are written,
rather than using the mode-2/mode-3 weighted descriptor blend.

Reverse-workspace retained split-gate recheck: `0x14255D810` clears
`draw+0x160` and sets it to `2` only when all runtime split gates pass:
`local+0x258 != 0`, `draw+0xd4 == 0`, `draw+0x120 == 0`,
`0x1423C8FE0() > 1`, and `0x1422DC1F0(style, draw+0x68, draw+0x100)` returns
true. `0x1422DC1F0` is a large/simple-brush eligibility test and rejects
complex retained/material style states with texture/pattern/secondary/resource
flags. `Test_Ballon` style 4 is serialized as complex retained/material
(`StyleFlag=0x1c230`, `PatternStyle=10`, `AntiAlias=2`, pattern image 3,
`OrderType=3`, `Reverse2=34`). Keep `0x14255C680` as a real alternate path,
but do not treat split submission as the current rough-balloon explanation
without a runtime trace proving `draw+0x160 == 2`.

Reverse-workspace `0x1425597A0` descriptor bridge refinement: the helper first
copies `draw+0x78/+0x80` into `aux_a` and `draw+0x84/+0x8c` into `aux_b`, then
uses `plot+0x00` as `round(size * 32768)` with `inverse = 0x8000 - weight` to
refresh/blend descriptor channels and write them back via `0x14206C350`. In
`draw+0xc0 == 1` mode it also finalizes the current row descriptor through
`0x14260D3D0` / `0x1426155F0`, writes an extra scalar gate via `0x1408E5840`,
and may perform another descriptor/color mix. Treat `aux_a`, `aux_b`, and
`extra_int` as retained descriptor feedback state, not static line color
constants.

Reverse-workspace `0x14255A020` refinement: the helper is a 24-bit HSV-like
descriptor combiner, not a footprint or bitmap sampler. It quantizes six
double inputs by `16777215.0` with `+/-0.50000001`; wraps
`q(arg3)+q(arg0)` as a hue-like channel; clamps `q(arg1)+q(stack0)` and
`q(arg2)+q(stack1)` as saturation/value-like channels; calls `0x142085980`,
whose disassembly is a six-sector HSV-to-RGB integer conversion; then writes
the three channels through `0x14206C350`. This keeps retained rough-balloon
work focused on descriptor feedback and row setup, not another hidden outline
radius rule.

2026-06-05 `Test_Ballon` retained/material scanline probe rejection:
`tmp_vector_probe/probe_test_ballon_retained_scanline.py` is a diagnostic-only
monkeypatch renderer for rough balloon `PatternStyle=10`. It builds explicit
native-style quad vertices, scans per-row edge intersections, interpolates UVs,
samples material alpha four times, and accumulates 16-bit coverage. Do not move
this into `clip_loader.py`: all tested resource/preview-lane variants regressed
to roughly `premul_mean=3.13..3.20` and `~26k..27k` visible pixels compared
with the accepted full baseline `1.181103/14326`. Increasing retained sample
spacing (`step_scale` 1/2/4/8) and radius changes did not help because a
continuous strip still fills the whole outline. The accepted narrow
`PatternStyle=10` interval-dab approximation remains the safer importer model
until native evidence proves the concrete balloon writer slot and submit
density.

Reverse-side audit note for importer decisions: IDA MCP confirmed
`0x1422CC1E0` submits balloon/saved-stroke node sample records through its
writer parameter `a2`, using virtual slots `+0x10` and `+0x20`. The `+0x20`
call has a 6-argument submit shape (`writer, record*, index/mode, gate,
active_flag, accumulator/state*`). This does not match `0x14260DB90`, which is
a 2-argument no-pattern queue consumer/flush with xrefs from `0x142558A90`,
`0x14260D060`, `PW_SubmitSplitNoPatternDabIfNeeded`, and
`PW_DrawOneVectorSampleNoPattern`. A PE scan found the `0x14260DB90` /
`0x142636CC0` / `0x142642010` / `0x14263F410` RVAs only in `.pdata`, not a
vtable. Treat direct balloon-object-to-retained-material-strip rendering as
rejected unless the upstream concrete writer passed as `a2` is later proven to
enqueue that path.

Reverse xref cleanup relevant to importer work: `0x142629750` has only data
xrefs and is a virtual render-slot entry. The real `.rdata` vtable entry at
`0x1444EFF80` contains full VAs beginning with `0x142629750`; the apparent
`0x14566E840` xref is `.pdata` unwind data with 32-bit RVAs. Likewise
`0x1425A3D60` / `0x1425A4320` have `.rdata` table entries at `0x1444EAA68` /
`0x1444EAA58`, while nearby `0x14566B...` addresses are `.pdata`. The next
native entry points for tracing the unknown writer `a2` are the code callers
of `0x1425A4100`: `0x14255A912` (`PW_RenderPreparedVectorDrawList`) and
`0x143F99194`.

Reverse follow-up: in the `0x14255A7E0` prepared vector draw-list path,
`0x14255A912` calls `0x1425A4100` with `rdx = original rcx`; `0x1425A4100`
stores that as `rsi` and passes it unchanged as writer `rdx` to all
`0x1422CC1E0` calls. So the writer consumed by `0x1422CC1E0` is the
draw/prepared-render object, not the no-pattern queue consumer. Keep the
current rough-balloon interval-dab approximation until the draw-object writer
slot is proven to enqueue a different native sink.
