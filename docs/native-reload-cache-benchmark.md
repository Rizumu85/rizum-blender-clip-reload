# Native Persistent Reload Cache Benchmark

Last updated: 2026-06-20

This benchmark validates the current persistent-worker reload cache shape
against three external architecture ideas:

- OIIO-style tile cache reuse and invalidation
- Chromium-style render task graph diagnostics
- libvips-style demand-driven region execution as a future experiment

It does not change CSP compositing semantics, tile-event semantic lowering,
THROUGH/container guards, or fallback policy.

## Command

Release binary:

```powershell
cd native/rust
cargo build --release -p clip_cli
```

Benchmark:

```powershell
python scripts\benchmark_persistent_reload_cache.py `
  --clip-cli native\rust\target\release\clip_cli.exe `
  --fixture-root E:\Documents\Claude\Projects\rizum-blender-clip-reload\img `
  --output-json native\rust\target\reload-cache-benchmark.json
```

Use `--request-timeout-seconds <seconds>` to fail a single worker request
explicitly instead of blocking forever. The script writes worker stderr to a
temporary file instead of a pipe so verbose Rust diagnostics cannot fill the
pipe and stall the child process.

`RIZUM_CLIP_RENDER_PROFILE=1` is set by the script so worker JSON includes
`render_task_graph`, render profile counters, sparse atlas cache stats, and
checkpoint cache stats.

Each sample uses one persistent `clip_cli --blender-render-server` process:

1. Render baseline with no previous manifest.
2. Run five reload requests using a previous manifest whose one compressed
   raster-tile hash has been mutated.
3. Record reload diff, task graph, sparse atlas cache, checkpoint cache, and
   worker timing fields.

Private `.clip` fixtures are read from the local untracked `img/` directory and
are not copied into git.

## Task A: Persistent Reload Cache Results

### Test_Clipping

| request | mode | patch renderer | worker ms | sparse reused/inserted/changed/evicted | resident MiB | checkpoint delta h/m/s/e | dirty segs | dirty ranges | readback bytes | RegionFallback |
| --- | --- | --- | ---: | --- | ---: | --- | ---: | ---: | ---: | --- |
| baseline | full | full | 39 | 0/8/0/0 | 2.0 | 0/0/0/0 | 0 | 0 | 1048576 | no |
| reload 1 | patch | region | 5 | 8/0/0/0 | 2.0 | 0/0/0/0 | 1 | 1 | 262144 | yes |
| reload 2 | patch | region | 5 | 8/0/0/0 | 2.0 | 0/0/0/0 | 1 | 1 | 262144 | yes |
| reload 3 | patch | region | 6 | 8/0/0/0 | 2.0 | 0/0/0/0 | 1 | 1 | 262144 | yes |
| reload 4 | patch | region | 8 | 8/0/0/0 | 2.0 | 0/0/0/0 | 1 | 1 | 262144 | yes |
| reload 5 | patch | region | 4 | 8/0/0/0 | 2.0 | 0/0/0/0 | 1 | 1 | 262144 | yes |

Derived details:

- Mutated tile: `layer=10 resource=15 tile=(0,0)`.
- Dirty segment: `1:RasterClippingRun:tile-local`.
- First later barrier: none.
- Task graph skipped sparse reason: `sparse atlas segment execution was not selected`.
- Patch fallback reason: `sparse_atlas_initial_segments and sparse_atlas_reconstructed_segments returned no executable patch`.

Interpretation: atlas tile reuse works, but sparse patch cannot execute this
shape because there is no selected segment-before checkpoint for the dirty
segment. The current task graph does not expose that precise checkpoint
eligibility reason yet.

### Test_RealArt

| request | mode | patch renderer | worker ms | sparse reused/inserted/changed/evicted | resident MiB | checkpoint delta h/m/s/e | dirty segs | dirty ranges | readback bytes | RegionFallback |
| --- | --- | --- | ---: | --- | ---: | --- | ---: | ---: | ---: | --- |
| baseline | full | full | 1205 | 0/2443/0/8 | 566.6 | 0/0/0/0 | 0 | 0 | 97600000 | no |
| reload 1 | patch | region | 282 | 2443/0/0/0 | 566.6 | 0/0/0/0 | 1 | 1 | 262144 | yes |
| reload 2 | patch | region | 300 | 2443/0/0/0 | 566.6 | 0/0/0/0 | 1 | 1 | 262144 | yes |
| reload 3 | patch | region | 282 | 2443/0/0/0 | 566.6 | 0/0/0/0 | 1 | 1 | 262144 | yes |
| reload 4 | patch | region | 271 | 2443/0/0/0 | 566.6 | 0/0/0/0 | 1 | 1 | 262144 | yes |
| reload 5 | patch | region | 298 | 2443/0/0/0 | 566.6 | 0/0/0/0 | 1 | 1 | 262144 | yes |

Derived details:

- Mutated tile: `layer=35 resource=2475 tile=(5,4) segment=4`.
- Dirty segment: `4:RasterRun:tile-local`.
- First later barrier: `11:LegacySource:ScopeDepthLimitExceeded`.
- RegionFallback is the dominant timed task on every reload.
- Reload 1 render profile recorded `legacy_barrier_segment_count=55` and
  `legacy_barrier_segment_ms=52`.

Interpretation: sparse atlas reuse is effective, but the affected window crosses
a later unsupported scope-depth barrier, so product reload falls back to the
region renderer. No sparse upload was measured before the region fallback.

### Ref_Terra404_Live2D

| request | mode | patch renderer | worker ms | sparse reused/inserted/changed/evicted | resident MiB | checkpoint delta h/m/s/e | dirty segs | dirty ranges | readback bytes | RegionFallback |
| --- | --- | --- | ---: | --- | ---: | --- | ---: | ---: | ---: | --- |
| baseline | full | full | 2673 | 0/6859/0/2443 | 1330.2 | 0/0/0/0 | 0 | 0 | 117120000 | no |
| reload 1 | patch | region | 600 | 6859/0/0/0 | 1330.2 | 0/0/0/0 | 2 | 2 | 262144 | yes |
| reload 2 | patch | region | 490 | 6859/0/0/0 | 1330.2 | 0/0/0/0 | 2 | 2 | 262144 | yes |
| reload 3 | patch | region | 605 | 6859/0/0/0 | 1330.2 | 0/0/0/0 | 2 | 2 | 262144 | yes |
| reload 4 | patch | region | 487 | 6859/0/0/0 | 1330.2 | 0/0/0/0 | 2 | 2 | 262144 | yes |
| reload 5 | patch | region | 468 | 6859/0/0/0 | 1330.2 | 0/0/0/0 | 2 | 2 | 262144 | yes |

Derived details:

- Mutated tile: `layer=10 resource=4787 tile=(8,9) segment=25`.
- Dirty segments: `0:SimpleThroughScope:tile-local` and
  `25:RasterRun:tile-local`.
- First later barrier after the first dirty segment:
  `6:LegacySource:IsolatedContainerRequiresIntermediate`.
- RegionFallback is the dominant timed task on every reload.
- Reload 1 render profile recorded `legacy_barrier_segment_count=31` and
  `legacy_barrier_segment_ms=110`.

Interpretation: the persistent sparse atlas cache keeps the large tile set
resident across repeated reloads, but the current sparse patch executor still
falls back when the affected window crosses an unsupported isolated-container
barrier. No sparse upload was measured before the region fallback.

## Task B: Render Task Graph Findings

The current graph is useful for high-level execution shape:

- Full render executes `RunSegment`.
- Patch reload attempts sparse execution but records `RegionFallback` as the
  dominant executed task when sparse patch cannot produce a payload.
- No tested reload executed `DecodeTile` or `UploadAtlasSlot` before
  `RegionFallback`; `sparse_atlas_update_ms` was `0` and sparse
  inserted/changed tiles were also `0`.
- Persistent atlas cache reuse is visible through `reused_tiles`, resident
  bytes, and stable resident atlas memory.
- Checkpoint cache did not participate in these controlled mutations:
  hit/miss/store/evict deltas stayed `0/0/0/0`.

Current diagnostic gap:

- `render_task_graph` reports the first skipped sparse task as the generic
  `sparse atlas segment execution was not selected`.
- The graph does not yet name the exact sparse feasibility failure, such as
  missing segment-before checkpoint, affected-window unsupported barrier, or
  unsupported overlapping segment kind.
- The benchmark script therefore derives dirty segment and first later barrier
  from the reload manifest, but this should eventually be emitted directly by
  the worker task graph.

There is no evidence in this run for an early-exit optimization that avoids
decode/upload work before a known fallback. The more important finding is that
the sparse path cannot currently execute past specific legacy barriers even
when all source tiles are resident and unchanged.

## Dirty-Rect Internal Cost Drilldown

Command:

```powershell
python scripts\benchmark_persistent_reload_cache.py `
  --clip-cli native\rust\target\release\clip_cli.exe `
  --fixture-root E:\Documents\Claude\Projects\rizum-blender-clip-reload\img `
  --samples Ref_Terra404_Live2D Test_RealArt `
  --iterations 1 `
  --output-json native\rust\target\dirty-rect-drilldown.json
```

This run adds per top-segment drilldown fields under
`render_profile.top_segments` when `RIZUM_CLIP_RENDER_PROFILE=1`:

- CPU wall time, GPU pass encode ms, queue submit/poll ms, readback contribution
- raster source count, visible source-tile intersections, active dirty tiles,
  and max events per dirty tile
- per-run atlas versus resident sparse atlas status
- child source count, nested container/THROUGH count, raster count, and mask
  count inside legacy barriers
- whether source bounds extend outside the requested dirty target

The values below are release-mode `reload_1` measurements from one local run.
They are diagnostic timings, not fidelity changes.

### Ref_Terra404_Live2D reload_1

Patch shape:

- `patch_renderer=region`
- dirty rect: `2048,2304 256x256`
- sparse resident atlas: `6859` reused tiles, `0` inserted/changed, `1330.2`
  MiB resident
- no sparse upload before region fallback
- dominant task: `RegionFallback:210ms`

Top 10 patch/reload segments:

| rank | kind | reason | first layer | ms | target is dirty rect | rasters | children | masks | active tiles | max events/tile | atlas | queue/poll ms | source bounds exceed target |
| ---: | --- | --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: | --- |
| 1 | LegacySource | ThroughGroupNotLowered | 13 | 19 | yes | 19 | 24 | 0 | 1 | 2 | legacy/not applicable | 0 | yes |
| 2 | RasterRun | - | 31 | 18 | yes | 1 | 1 | 0 | 1 | 1 | per-run atlas | 0 | yes |
| 3 | LegacySource | ThroughGroupNotLowered | 203 | 16 | yes | 48 | 70 | 1 | 1 | 10 | legacy/not applicable | 0 | yes |
| 4 | LegacySource | ThroughGroupNotLowered | 407 | 13 | yes | 21 | 28 | 3 | 1 | 6 | legacy/not applicable | 0 | yes |
| 5 | LegacySource | ThroughGroupNotLowered | 443 | 9 | yes | 10 | 12 | 3 | 1 | 4 | legacy/not applicable | 1 | yes |
| 6 | LegacySource | ClippingRunNotLowered | 416 | 8 | yes | 5 | 5 | 1 | 1 | 3 | legacy/not applicable | 0 | yes |
| 7 | LegacySource | IsolatedContainerRequiresIntermediate | 195 | 6 | yes | 5 | 6 | 0 | 1 | 2 | legacy/not applicable | 0 | yes |
| 8 | RasterRun | - | 196 | 6 | yes | 2 | 2 | 0 | 1 | 2 | per-run atlas | 0 | yes |
| 9 | LegacySource | ThroughGroupNotLowered | 469 | 5 | yes | 20 | 26 | 3 | 1 | 5 | legacy/not applicable | 0 | yes |
| 10 | RasterRun | - | 267 | 5 | yes | 1 | 1 | 0 | 1 | 1 | per-run atlas | 0 | yes |

### Test_RealArt reload_1

Patch shape:

- `patch_renderer=region`
- dirty rect: `1280,1024 256x256`
- sparse resident atlas: `2443` reused tiles, `0` inserted/changed, `566.6`
  MiB resident
- no sparse upload before region fallback
- dominant task: `RegionFallback:113ms`

Top 10 patch/reload segments:

| rank | kind | reason | first layer | ms | target is dirty rect | rasters | children | masks | active tiles | max events/tile | atlas | queue/poll ms | source bounds exceed target |
| ---: | --- | --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | --- | ---: | --- |
| 1 | RasterRun | - | 35 | 24 | yes | 1 | 1 | 0 | 1 | 1 | per-run atlas | 0 | yes |
| 2 | LegacySource | IsolatedContainerRequiresIntermediate | 312 | 14 | yes | 14 | 16 | 1 | 1 | 4 | legacy/not applicable | 0 | yes |
| 3 | LegacySource | IsolatedContainerRequiresIntermediate | 315 | 11 | yes | 12 | 13 | 0 | 1 | 3 | legacy/not applicable | 0 | yes |
| 4 | RasterRun | - | 299 | 5 | yes | 1 | 1 | 0 | 1 | 1 | per-run atlas | 0 | yes |
| 5 | RasterRun | - | 319 | 5 | yes | 2 | 2 | 0 | 1 | 2 | per-run atlas | 0 | yes |
| 6 | LegacySource | ThroughGroupNotLowered | 31 | 5 | yes | 4 | 5 | 0 | 1 | 2 | legacy/not applicable | 0 | yes |
| 7 | LegacySource | ThroughGroupNotLowered | 37 | 5 | yes | 4 | 5 | 0 | 1 | 1 | legacy/not applicable | 0 | yes |
| 8 | RasterRun | - | 327 | 3 | yes | 1 | 1 | 0 | 1 | 1 | per-run atlas | 0 | yes |
| 9 | RasterRun | - | 313 | 3 | yes | 1 | 1 | 0 | 1 | 1 | per-run atlas | 0 | yes |
| 10 | RasterRun | - | 323 | 2 | yes | 1 | 1 | 0 | 1 | 1 | per-run atlas | 0 | yes |

### Classification

Selected class: **E. Sparse resident atlas not used in region path**.

Reasoning:

- The outer region renderer is already using the 256x256 dirty target for every
  top segment in both requested samples. That rules out a broad full-canvas or
  segment-bounds region-demand fix as the next smallest step.
- The measured RasterRun segments are tiny in semantic work: one active dirty
  tile and usually one raster source, yet they still cost up to `18ms` on Terra
  and `24ms` on RealArt.
- The same reload requests already have thousands of resident sparse atlas
  tiles reused, but the region renderer's RasterRun segments report
  `per-run atlas`, not `resident_sparse_atlas`.
- Legacy barriers are still meaningful costs, especially Terra's
  `ThroughGroupNotLowered`, but those are semantic-unsafe barriers. Reducing
  them would require semantic lowering, which is out of scope for this pass.

Unsupported/unknown fields:

- Legacy internal intermediate-cache rectangle and internal child sub-pass timing
  are not measured yet. The profiler records the outer target rect, child source
  counts, raster counts, mask counts, direct streaming pass counts, and
  source-bound over-target status only.
- Per-segment cache reuse count is not currently attributed inside
  `stream_sequence`; sample-level sparse cache reuse is measured by
  `sparse_atlas_cache`.

Recommended smallest prototype:

- Add an opt-in region-renderer RasterRun cache substitution prototype.
- Scope: patch reload only, `RIZUM_CLIP_RENDER_PROFILE=1` / explicit feature
  flag only.
- When the region fallback renderer executes a `RasterRun` segment and the
  persistent sparse atlas has complete resident slot coverage for that segment's
  dirty target, execute that RasterRun through the resident sparse atlas
  executor instead of rebuilding a per-run atlas.
- Keep legacy barriers on the existing faithful path and preserve segment order,
  target origin, transparent-white accumulator convention, and current patch
  payload bytes.
- Validate by comparing patch payload bytes against the current region renderer
  on `Test_RealArt reload_1` and `Ref_Terra404_Live2D reload_1`.

## Resident Sparse Atlas RasterRun Prototype

Prototype flag:

```powershell
$env:RIZUM_CLIP_REGION_RASTER_RESIDENT_ATLAS = "1"
```

The prototype was added behind the env flag and kept fail-closed. It only tries
to substitute resident sparse-atlas execution for region-fallback `RasterRun`
segments when:

- the render is a patch reload dirty-region render
- the segment is a `RasterRun`
- the source and event counts stay under the bounded prototype limits
- all required resident RGBA/R8 atlas slots have matching GPU textures in the
  session sparse atlas pool

The benchmark now supports A/B payload comparison:

```powershell
python scripts\benchmark_persistent_reload_cache.py `
  --clip-cli native\rust\target\release\clip_cli.exe `
  --fixture-root E:\Documents\Claude\Projects\rizum-blender-clip-reload\img `
  --samples Test_Clipping Test_RealArt Ref_Terra404_Live2D `
  --iterations 5 `
  --ab-region-resident-atlas `
  --output-json resident_atlas_ab_results.json
```

Release-mode medians from the local run:

| sample | off median reload ms | on median reload ms | payload equality | resident RasterRun hits | per-run RasterRun median | decision |
| --- | ---: | ---: | --- | ---: | ---: | --- |
| `Test_Clipping` | 6 | 6 | all equal | 0 | 0 | not applicable; dirty path is `RasterClippingRun` |
| `Test_RealArt` | 261 | 325 | all equal | 0 | 11 | no-go |
| `Ref_Terra404_Live2D` | 546 | 573 | all equal | 0 | 4 | no-go |

Conclusion:

- The prototype does not meet the performance acceptance criteria.
- It must not become the default path.
- It remains opt-in and fail-closed for now, but the current controlled reload
  fixture does not exercise a resident GPU atlas path.
- The root cause is cache lifecycle, not compositing semantics: the benchmark's
  baseline full render builds logical sparse atlas cache entries, but it does
  not populate the GPU sparse atlas texture pool. The synthetic reload then
  reports `Reuse` for all atlas entries and produces no upload chunks, so the
  resident GPU textures required by the prototype are absent. The correct
  behavior is to fail closed to the existing region renderer.

Next recommendation:

- Do not continue this prototype until there is a narrow GPU sparse-atlas pool
  warm-up or changed-tile fixture that proves the required resident textures are
  actually present. Without that, the measured optimization target is not
  executable and only adds branch overhead.
