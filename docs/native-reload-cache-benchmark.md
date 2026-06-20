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

## Task C: Region-Demand Prototype Proposal

Recommended single next implementation:

- Prototype a faithful region-demand path for
  `LegacySource(IsolatedContainerRequiresIntermediate)`.
- Primary fixture: `Ref_Terra404_Live2D`.
- Exact observed barrier candidate:
  `segment=6 kind=LegacySource reason=IsolatedContainerRequiresIntermediate`.

Why it is unsafe to lower semantically:

- This barrier represents isolated container semantics that the current
  tile-event scope model has not proven faithful for all nested shapes.
- The existing Terra guard around nested THROUGH/container relationships must
  remain intact. This experiment should not lower the container to tile events.

Smallest faithful experiment:

- Input: a reload dirty rect and the legacy barrier segment that intersects the
  affected window.
- Execution: invoke the existing faithful legacy source/barrier renderer, but
  restrict its render target and readback to the requested dirty rect clipped to
  the barrier/source bounds when those bounds are known.
- Output: the same patch payload format returned by the current region renderer.
- Fallback: use the current full region renderer if the barrier bounds,
  required child-source bounds, mask bounds, or coordinate translation cannot be
  proven.

Correctness test:

- Use the same controlled previous-manifest tile mutation as the benchmark.
- Compare the prototype barrier-region patch payload against the current
  `patch_renderer=region` payload for `Ref_Terra404_Live2D`.
- Add `Test_RealArt` as a guard where the first later barrier is
  `ScopeDepthLimitExceeded`; it should either match the existing region path or
  explicitly decline and fall back.

Expected metric:

- Primary: reduce Terra patch reload worker time or `RegionFallback` actual ms.
- Secondary: reduce `legacy_barrier_segment_ms` for the patch reload.
- The prototype is accepted only if the payload matches the current region path
  byte-for-byte for the tested patch rects.
