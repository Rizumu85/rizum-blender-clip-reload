# Native Tile-Event Scoreboard

Last updated: 2026-06-20

This file is the convergence scoreboard for tile-event work. Update it before
choosing another semantic coverage task. Do not use the old implementation log
as an open-ended backlog.

## Fixed Samples

Baseline command:

```powershell
cd native/rust
cargo run -q -p clip_cli -- ..\..\img\<sample>.clip --performance-plan-json
```

Timing command after the debug binary is already built:

```powershell
cd native/rust
.\target\debug\clip_cli.exe ..\..\img\<sample>.clip --compare-png ..\..\img\<sample>.png
```

| Sample | planned_passes | tile_local_segments | barrier_segments | legacy_segments | top barrier reasons | tile_event_abi_version | debug CLI compare time | patch_renderer status |
| --- | ---: | ---: | ---: | ---: | --- | ---: | ---: | --- |
| `Test_Clipping` | 2 | 1 | 1 | 1 | `SolidColorNotLowered=1` | 10 | 1.34s | worker fixture: `region`; fallback reason recorded |
| `Test_ClippingEdge` | 1 | 1 | 0 | 0 | none | 10 | 1.15s | not measured |
| `Test_AddGlowMultiply` | 2 | 2 | 0 | 0 | none | 10 | 8.24s | not measured |
| `Test_ToneCurve` | 1 | 1 | 0 | 0 | none | 10 | 1.45s | not measured |
| `Test_RealArt` | 148 | 139 | 9 | 9 | `RasterBoundsOrResourceNotLowered=3; ThroughGroupNotLowered=3; ScopeDepthLimitExceeded=2; IsolatedContainerRequiresIntermediate=1` | 10 | 26.00s | not measured |
| `Ref_Terra404_Live2D` | 481 | 468 | 13 | 13 | `ThroughGroupNotLowered=5; IsolatedContainerRequiresIntermediate=4; ClippingRunNotLowered=2; ScopeDepthLimitExceeded=2` | 10 | 47.23s | not measured |
| `IllustrationBlendModes` | 2 | 1 | 1 | 1 | `SolidColorNotLowered=1` | 10 | 1.48s | not measured |
| `IllustrationBlendModesB` | 2 | 1 | 1 | 1 | `SolidColorNotLowered=1` | 10 | 1.35s | not measured |
| `Test_Gradiation` | 1 | 1 | 0 | 0 | none | 10 | 1.45s | not measured |

Notes:

- Times are local debug CLI wall times after compilation. They are not Blender
  import timings and should not be compared to user-facing worker timings.
- `patch_renderer` status comes only from a fixed worker reload fixture. Leave it
  as `not measured` for samples that do not have a reload fixture.
- Next semantic coverage work must target the largest barrier reason that
  appears here or in an explicitly added fixed sample.

## Required Fields For New Rows

Every new fixed sample row must include:

- sample name
- `planned_passes`
- `tile_local_segments`
- `barrier_segments`
- `legacy_segments`
- top barrier reasons
- `tile_event_abi_version`
- worker/debug render timing source
- patch renderer attempted/succeeded/fallback status when a reload fixture is
  available
