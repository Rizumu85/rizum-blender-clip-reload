from __future__ import annotations

import argparse
import collections
import importlib.util
import json
import math
import struct
import sys
from pathlib import Path
from typing import Any


POINT_U32_FIELDS = (16, 20, 24, 28, 32, 80, 84)
POINT_FLOAT_FIELDS = tuple(range(36, 80, 4))
LAYER_VECTOR_KEYS = (
    "LayerType",
    "LayerFolder",
    "LayerOpacity",
    "LayerComposite",
    "LayerClip",
    "LayerMasking",
    "LayerVisibility",
    "LayerRenderMipmap",
    "LayerLayerMaskMipmap",
    "LayerRenderOffscrOffsetX",
    "LayerRenderOffscrOffsetY",
    "LayerOffsetX",
    "LayerOffsetY",
    "LayerColorTypeIndex",
    "LayerColorTypeBlackChecked",
    "LayerColorTypeWhiteChecked",
    "VectorNormalStrokeIndex",
    "VectorNormalFillIndex",
    "VectorNormalBalloonIndex",
    "VectorNormalType",
    "MixSubColorForEveryPlot",
    "ComicFrameLineMipmap",
    "ComicFrameColorTypeIndex",
    "ComicFrameColorTypeBlackChecked",
    "ComicFrameColorTypeWhiteChecked",
)
BRUSH_STYLE_KEYS = (
    "MainId",
    "_PW_ID",
    "CanvasId",
    "NextIndex",
    "StyleFlag",
    "PenRadius",
    "SizeEffector",
    "OpacityEffector",
    "FlowBase",
    "FlowEffector",
    "AntiAlias",
    "MipmapIndexToPlot",
    "Hardness",
    "IntervalBase",
    "IntervalEffector",
    "AutoIntervalType",
    "ThicknessBase",
    "ThicknessEffector",
    "RotationBase",
    "RotationInSprayBase",
    "RotationEffector",
    "RotationRandom",
    "RotationEffectorInSpray",
    "RotationRandomInSpray",
    "PatternStyle",
    "CompositeMode",
    "DualCompositeMode",
    "TexturePattern",
    "TextureFlag",
    "TextureComposite",
    "TextureScale",
    "TextureRotate",
    "TextureOffsetX",
    "TextureOffsetY",
    "TextureBrightness",
    "TextureContrast",
    "TextureDensityBase",
    "TextureDensityEffector",
    "UseWaterColor",
    "WaterColorType",
    "MixColorBase",
    "MixColorEffector",
    "MixAlphaBase",
    "MixAlphaEffector",
    "ColorExtension",
    "BrushColorMixingMode",
    "BrushLMSLinearity",
    "BlurKind",
    "BlurBase",
    "BlurEffector",
    "SubColorBase",
    "SubColorEffector",
    "HueChangeBase",
    "HueChangeEffector",
    "SaturationChangeBase",
    "SaturationChangeEffector",
    "ValueChangeBase",
    "ValueChangeEffector",
    "ChangeDrawColorTarget",
    "SprayFlag",
    "SpraySizeBase",
    "SpraySizeEffector",
    "SprayDensityBase",
    "SprayDensityEffector",
    "SprayBias",
    "FixedSpray",
    "WaterEdgeFlag",
    "WaterEdgeRadius",
    "WaterEdgeAlphaPower",
    "WaterEdgeValuePower",
    "WaterEdgeBlur",
)
EFFECTOR_KEYS = tuple(key for key in BRUSH_STYLE_KEYS if key.endswith("Effector"))
FILL_STYLE_KEYS = (
    "MainId",
    "_PW_ID",
    "CanvasId",
    "NextIndex",
    "StyleFlag",
    "AntiAlias",
    "CompositeMode",
    "TextureDensity",
)
NATIVE_PWBRUSHSTYLE_EVALUATOR = {
    "function": "0x1422D8550",
    "role": "compiled PWBrushStyle + sampler record -> plot record",
    "input_channels": {
        "compact_f32_36": "sample_context+0x10 primary scalar, used by effector bit 0x10",
        "compact_f32_40": "sample_context+0x18 angle/tilt-derived scalar, used by effector bit 0x20",
        "compact_f32_44": "sample_context+0x20 auxiliary scalar, used by effector bit 0x40",
        "compact_f32_52": "sample_context+0x30 velocity factor, used by effector bit 0x100",
        "compact_f32_56": "sample_context+0x38 size factor, multiplied into evaluated size/spray size",
        "compact_f32_60": "sample_context+0x40 flow factor, multiplied into evaluated flow",
    },
    "plot_record": {
        "+0": "effective dab size",
        "+8": "opacity cap from OpacityEffector",
        "+16": "flow/coverage multiplier",
        "+24": "thickness/stretch ratio",
        "+32": "rotation angle",
        "+40": "style material/rotation flag",
        "+44": "AntiAlias ordinal",
        "+64/+68": "pattern selector/flip outputs",
        "+72": "texture/color auxiliary scalar",
        "+88": "caller plane/pass flag propagated from draw+616",
    },
    "importer_policy": (
        "diagnose all compact channels, but keep renderer formulas sample-gated "
        "until isolated samples and the native row/dab path agree"
    ),
}
NATIVE_VECTOR_SAMPLER_SPACING = {
    "role": "PWVector sampler distance state and per-sample interval feedback",
    "functions": {
        "0x1422CC1E0": (
            "curve/node sampler; keeps residual distance in the caller state at +8, "
            "emits records through sink +0x20, then advances by the interval written "
            "back to that same state"
        ),
        "0x1422CCA10": (
            "segment sample emitter; passes a compact seed/state block to sink +0x20, "
            "where the evaluator writes the next interval at state+8"
        ),
        "0x14255C980": "PWBrushDraw sink +0x20 dispatcher",
        "0x14255DFE0": "ordinary per-sample path; calls 0x1422D8550 with the sampler state pointer",
        "0x1422D8550": (
            "compiled PWBrushStyle evaluator; writes the next interval to state+8 "
            "after evaluating size, interval, hardness/auto-interval, and dynamic channels"
        ),
        "0x1422D9910": (
            "effective footprint estimate used for bbox/secondary-resource step rescale; "
            "includes size effector, compact size channel, anti-alias width, and spray/secondary contribution"
        ),
        "0x1422D9200": (
            "segment-start helper; seeds sample record +56/+64 from line style, "
            "but does not itself choose the next step interval"
        ),
    },
    "state_layout": {
        "state+0": "seed/style word used by the sampler and brush evaluator",
        "state+4": "secondary seed/style word",
        "state+8": "next sample distance/interval written by 0x1422D8550 and consumed by 0x1422CC1E0/0x1422CCA10",
    },
    "interval_formula": {
        "normal": (
            "state+8 = 2 * max(0.1, effective_size) * evaluated IntervalBase/Effector; "
            "when AA/softness requires it, the interval is raised to at least 0.5..1.0 px style-dependent clamps"
        ),
        "auto_interval_1_2_3": (
            "AutoIntervalType 1/2/3 call 0x1422DBF80 to derive the interval scalar from "
            "auto type, hardness-like softness, and thickness/stretch-like scalar before the same state+8 write"
        ),
        "auto_interval_4_5": (
            "AutoIntervalType 4/5 bypass the normal interval effector and derive spacing from "
            "(1 - hardness_like_value) * effective_size, with optional IntervalBase cap for type 5"
        ),
        "small_size_adjustment": (
            "sub-1px effective sizes can scale flow down or clamp size back toward 1px depending on style flags"
        ),
    },
    "importer_policy": (
        "keep capsule fallback spacing sample-gated until the evaluator-driven interval "
        "feedback loop is modeled; do not treat fixed importer steps as recovered native constants"
    ),
}


def _float_stat_range(stats: dict[str, Any] | None) -> tuple[float, float] | None:
    if not isinstance(stats, dict):
        return None
    min_value = stats.get("min")
    max_value = stats.get("max")
    if not isinstance(min_value, (int, float)) or not isinstance(max_value, (int, float)):
        return None
    if not (math.isfinite(float(min_value)) and math.isfinite(float(max_value))):
        return None
    return float(min_value), float(max_value)


def _constant_effector_multiplier(effector: Any) -> float | None:
    if not isinstance(effector, dict):
        return None
    decoded = effector.get("decoded") or {}
    form = decoded.get("form")
    if form in {"default_or_off_0x01", "zero_or_disabled_0x00"}:
        return 1.0
    if form == "primary_graph_0x11" and decoded.get("graph_refs") == []:
        scalar = decoded.get("scalar")
        return float(scalar) if isinstance(scalar, (int, float)) else None
    return None


def _auto_interval_123_scalar(auto_interval_type: Any, hardness: Any, thickness: Any) -> float | None:
    if auto_interval_type not in (1, 2, 3):
        return None
    if not isinstance(hardness, (int, float)):
        return None
    if not isinstance(thickness, (int, float)):
        thickness = 1.0
    base = 0.08
    if auto_interval_type == 1:
        base = 0.12
    elif auto_interval_type == 3:
        base = 0.04
    scalar = base * (3.5 - max(float(hardness), 0.2) * 2.5)
    if float(thickness) < 1.0:
        scalar *= max(float(thickness), 0.01)
    return scalar


def _graph_points_for_id(graphs: dict[str, Any], graph_id: Any) -> list[tuple[float, float]]:
    graph = graphs.get(str(graph_id))
    points = graph.get("points") if isinstance(graph, dict) else None
    out: list[tuple[float, float]] = []
    for point in points or []:
        if (
            isinstance(point, (list, tuple))
            and len(point) >= 2
            and isinstance(point[0], (int, float))
            and isinstance(point[1], (int, float))
        ):
            out.append((float(point[0]), float(point[1])))
    return out


def _sample_channel_range(point_stats: dict[str, Any] | None, channel_offset: str) -> tuple[float, float] | None:
    return _float_stat_range(((point_stats or {}).get("float_fields") or {}).get(channel_offset))


def _effector_graph_output_range(
    effector: Any,
    point_stats: dict[str, Any] | None,
    graphs: dict[str, Any],
) -> dict[str, Any] | None:
    if not isinstance(effector, dict):
        return None
    decoded = effector.get("decoded") or {}
    refs = decoded.get("graph_refs") or []
    if not refs:
        return None
    branch_inputs = [
        ("0x10", "36", 0),
        ("0x20", "40", 1),
        ("0x40", "44", 2),
    ]
    outputs: list[float] = []
    branches: list[dict[str, Any]] = []
    for branch_bit, point_offset, ref_index in branch_inputs:
        if ref_index >= len(refs):
            continue
        graph_id = refs[ref_index]
        points = _graph_points_for_id(graphs, graph_id)
        input_range = _sample_channel_range(point_stats, point_offset)
        if not points or input_range is None:
            continue
        candidates = [
            _eval_graph_native(points, input_range[0]),
            _eval_graph_native(points, input_range[1]),
        ]
        # Include the midpoint because native quadratic spans can peak between endpoints.
        candidates.append(_eval_graph_native(points, (input_range[0] + input_range[1]) * 0.5))
        outputs.extend(candidates)
        branches.append(
            {
                "branch_bit": branch_bit,
                "compact_point_f32": int(point_offset),
                "graph_id": graph_id,
                "input_range": [round(input_range[0], 6), round(input_range[1], 6)],
                "graph_output_range": [
                    round(min(candidates), 6),
                    round(max(candidates), 6),
                ],
            }
        )
    if not branches:
        return None
    return {
        "branches": branches,
        "combined_graph_output_range": [
            round(min(outputs), 6),
            round(max(outputs), 6),
        ],
        "note": (
            "Graph output range only; floor/random/velocity/native combination details are not folded into state+8 here."
        ),
    }


def _mul_range(a: tuple[float, float], b: tuple[float, float]) -> tuple[float, float]:
    vals = (a[0] * b[0], a[0] * b[1], a[1] * b[0], a[1] * b[1])
    return min(vals), max(vals)


def _blend_floor_range(floor: Any, graph_range: tuple[float, float]) -> tuple[float, float] | None:
    if not isinstance(floor, (int, float)):
        return None
    f = float(floor)
    vals = (f + (1.0 - f) * graph_range[0], f + (1.0 - f) * graph_range[1])
    return min(vals), max(vals)


def _blend_low_high_range(low: Any, high: Any, graph_range: tuple[float, float]) -> tuple[float, float] | None:
    if not isinstance(low, (int, float)) or not isinstance(high, (int, float)):
        return None
    lo = float(low)
    hi = float(high)
    vals = (lo + (hi - lo) * graph_range[0], lo + (hi - lo) * graph_range[1])
    return min(vals), max(vals)


def _effector_multiplier_estimate(
    effector: Any,
    point_stats: dict[str, Any] | None,
    graphs: dict[str, Any],
) -> dict[str, Any] | None:
    if not isinstance(effector, dict):
        return None
    decoded = effector.get("decoded") or {}
    form = decoded.get("form")
    if form in {"default_or_off_0x01", "zero_or_disabled_0x00"}:
        return {
            "combined_multiplier_range": [1.0, 1.0],
            "branches": [],
            "note": "constant/default effector",
        }

    branches: list[dict[str, Any]] = []
    combined = (1.0, 1.0)

    def add_graph_branch(
        *,
        branch_bit: str,
        point_offset: str,
        graph_id: Any,
        factor_range: tuple[float, float] | None,
        graph_range: tuple[float, float] | None,
        input_range: tuple[float, float] | None,
    ) -> None:
        nonlocal combined
        if factor_range is None:
            return
        combined = _mul_range(combined, factor_range)
        branch: dict[str, Any] = {
            "branch_bit": branch_bit,
            "factor_range": [round(factor_range[0], 6), round(factor_range[1], 6)],
        }
        if graph_id is not None:
            branch["graph_id"] = graph_id
        if point_offset:
            branch["compact_point_f32"] = int(point_offset)
        if input_range is not None:
            branch["input_range"] = [round(input_range[0], 6), round(input_range[1], 6)]
        if graph_range is not None:
            branch["graph_output_range"] = [round(graph_range[0], 6), round(graph_range[1], 6)]
        branches.append(branch)

    refs = decoded.get("graph_refs") or []
    if form == "primary_graph_0x11" and refs:
        input_range = _sample_channel_range(point_stats, "36")
        points = _graph_points_for_id(graphs, refs[0])
        graph_range = None
        if input_range is not None and points:
            vals = [
                _eval_graph_native(points, input_range[0]),
                _eval_graph_native(points, input_range[1]),
                _eval_graph_native(points, (input_range[0] + input_range[1]) * 0.5),
            ]
            graph_range = (min(vals), max(vals))
        add_graph_branch(
            branch_bit="0x10",
            point_offset="36",
            graph_id=refs[0],
            factor_range=None if graph_range is None else _blend_floor_range(decoded.get("floor"), graph_range),
            graph_range=graph_range,
            input_range=input_range,
        )
    elif form == "dual_graph_0x31" and len(refs) >= 2:
        input_range = _sample_channel_range(point_stats, "36")
        points = _graph_points_for_id(graphs, refs[0])
        graph_range = None
        if input_range is not None and points:
            vals = [
                _eval_graph_native(points, input_range[0]),
                _eval_graph_native(points, input_range[1]),
                _eval_graph_native(points, (input_range[0] + input_range[1]) * 0.5),
            ]
            graph_range = (min(vals), max(vals))
        add_graph_branch(
            branch_bit="0x10",
            point_offset="36",
            graph_id=refs[0],
            factor_range=None if graph_range is None else _blend_floor_range(decoded.get("zero_or_mode"), graph_range),
            graph_range=graph_range,
            input_range=input_range,
        )
        scalars = decoded.get("scalars") or []
        input_range = _sample_channel_range(point_stats, "40")
        points = _graph_points_for_id(graphs, refs[1])
        graph_range = None
        if input_range is not None and points:
            vals = [
                _eval_graph_native(points, input_range[0]),
                _eval_graph_native(points, input_range[1]),
                _eval_graph_native(points, (input_range[0] + input_range[1]) * 0.5),
            ]
            graph_range = (min(vals), max(vals))
        add_graph_branch(
            branch_bit="0x20",
            point_offset="40",
            graph_id=refs[1],
            factor_range=None if graph_range is None or len(scalars) < 2 else _blend_low_high_range(
                scalars[0],
                scalars[1],
                graph_range,
            ),
            graph_range=graph_range,
            input_range=input_range,
        )
    elif form == "secondary_graph_range_0x21" and refs:
        input_range = _sample_channel_range(point_stats, "40")
        points = _graph_points_for_id(graphs, refs[0])
        graph_range = None
        if input_range is not None and points:
            vals = [
                _eval_graph_native(points, input_range[0]),
                _eval_graph_native(points, input_range[1]),
                _eval_graph_native(points, (input_range[0] + input_range[1]) * 0.5),
            ]
            graph_range = (min(vals), max(vals))
        add_graph_branch(
            branch_bit="0x20",
            point_offset="40",
            graph_id=refs[0],
            factor_range=None if graph_range is None else _blend_low_high_range(
                decoded.get("low"),
                decoded.get("high"),
                graph_range,
            ),
            graph_range=graph_range,
            input_range=input_range,
        )
    elif form == "primary_graph_velocity_0x111" and refs:
        input_range = _sample_channel_range(point_stats, "36")
        points = _graph_points_for_id(graphs, refs[0])
        graph_range = None
        if input_range is not None and points:
            vals = [
                _eval_graph_native(points, input_range[0]),
                _eval_graph_native(points, input_range[1]),
                _eval_graph_native(points, (input_range[0] + input_range[1]) * 0.5),
            ]
            graph_range = (min(vals), max(vals))
        add_graph_branch(
            branch_bit="0x10",
            point_offset="36",
            graph_id=refs[0],
            factor_range=None if graph_range is None else _blend_floor_range(decoded.get("floor"), graph_range),
            graph_range=graph_range,
            input_range=input_range,
        )
        velocity_range = _sample_channel_range(point_stats, "52")
        if velocity_range is not None:
            add_graph_branch(
                branch_bit="0x100",
                point_offset="52",
                graph_id=None,
                factor_range=_blend_floor_range(decoded.get("velocity_floor"), velocity_range),
                graph_range=None,
                input_range=velocity_range,
            )

    if not branches:
        return None
    return {
        "combined_multiplier_range": [round(combined[0], 6), round(combined[1], 6)],
        "branches": branches,
        "note": "Best-effort 0x142568040 multiplier estimate; random branches are not sampled here.",
    }


def _native_spacing_estimate(
    kind: str,
    style_id: Any,
    style: dict[str, Any] | None,
    width: Any,
    point_stats: dict[str, Any] | None,
    graphs: dict[str, Any] | None = None,
) -> dict[str, Any] | None:
    if not isinstance(style, dict):
        return None
    if not isinstance(width, (int, float)) or not math.isfinite(float(width)):
        return None

    auto_interval = style.get("AutoIntervalType")
    thickness_multiplier = _constant_effector_multiplier(style.get("ThicknessEffector"))
    thickness_base = style.get("ThicknessBase")
    if not isinstance(thickness_base, (int, float)):
        thickness_base = 1.0
    thickness_guess = None
    if thickness_multiplier is not None:
        thickness_guess = float(thickness_base) * thickness_multiplier

    interval_scalar = None
    branch = "normal_interval"
    if auto_interval in (1, 2, 3):
        branch = "auto_interval_1_2_3"
        interval_scalar = _auto_interval_123_scalar(auto_interval, style.get("Hardness"), thickness_guess)
    elif auto_interval in (4, 5):
        branch = "auto_interval_4_5"
    else:
        interval_base = style.get("IntervalBase")
        interval_multiplier = _constant_effector_multiplier(style.get("IntervalEffector"))
        if isinstance(interval_base, (int, float)) and interval_multiplier is not None:
            interval_scalar = float(interval_base) * interval_multiplier

    size_factor_range = _float_stat_range(((point_stats or {}).get("float_fields") or {}).get("56"))
    if size_factor_range is None:
        size_factor_range = (1.0, 1.0)

    size_effector_constant = _constant_effector_multiplier(style.get("SizeEffector"))
    size_effector_dynamic = size_effector_constant is None and _effector_is_nondefault(style.get("SizeEffector"))
    size_effector_graph_range = _effector_graph_output_range(style.get("SizeEffector"), point_stats, graphs or {})
    size_effector_multiplier = _effector_multiplier_estimate(style.get("SizeEffector"), point_stats, graphs or {})
    effective_size_without_size_effector = (
        float(width) * size_factor_range[0],
        float(width) * size_factor_range[1],
    )
    estimate: dict[str, Any] = {
        "kind": kind,
        "style_id": style_id,
        "branch": branch,
        "base_size_source": "compact stroke/object width; native a3 source is still treated as a best-effort estimate",
        "base_width": round(float(width), 6),
        "compact_f32_56_size_factor_range": [
            round(size_factor_range[0], 6),
            round(size_factor_range[1], 6),
        ],
        "size_effector": _effector_key(style.get("SizeEffector")),
        "size_effector_dynamic": bool(size_effector_dynamic),
        "size_effector_graph_output": size_effector_graph_range,
        "size_effector_multiplier_estimate": size_effector_multiplier,
        "effective_size_without_size_effector_range": [
            round(effective_size_without_size_effector[0], 6),
            round(effective_size_without_size_effector[1], 6),
        ],
        "interval_scalar": None if interval_scalar is None else round(float(interval_scalar), 6),
        "thickness_guess": None if thickness_guess is None else round(float(thickness_guess), 6),
    }
    if interval_scalar is not None:
        state8_range = (
            2.0 * max(0.1, effective_size_without_size_effector[0]) * float(interval_scalar),
            2.0 * max(0.1, effective_size_without_size_effector[1]) * float(interval_scalar),
        )
        estimate["state8_without_size_effector_range"] = [
            round(state8_range[0], 6),
            round(state8_range[1], 6),
        ]
        multiplier_range = None
        if size_effector_constant is not None:
            multiplier_range = (float(size_effector_constant), float(size_effector_constant))
        elif isinstance(size_effector_multiplier, dict):
            mult = size_effector_multiplier.get("combined_multiplier_range")
            if isinstance(mult, list) and len(mult) == 2:
                multiplier_range = (float(mult[0]), float(mult[1]))
        if multiplier_range is not None:
            effective_size_with_effector = _mul_range(effective_size_without_size_effector, multiplier_range)
            state8_with_effector = (
                2.0 * max(0.1, effective_size_with_effector[0]) * float(interval_scalar),
                2.0 * max(0.1, effective_size_with_effector[1]) * float(interval_scalar),
            )
            estimate["effective_size_with_estimated_size_effector_range"] = [
                round(effective_size_with_effector[0], 6),
                round(effective_size_with_effector[1], 6),
            ]
            estimate["state8_with_estimated_size_effector_range"] = [
                round(state8_with_effector[0], 6),
                round(state8_with_effector[1], 6),
            ]
    else:
        estimate["state8_without_size_effector_range"] = None
        estimate["unestimated_reason"] = (
            "interval branch depends on dynamic effector or AutoIntervalType 4/5 not yet reduced to a scalar"
        )
    return estimate


def _native_spacing_bucket(value: Any) -> str:
    if not isinstance(value, (int, float)) or not math.isfinite(float(value)):
        return "unavailable"
    value = float(value)
    if value < 0.1:
        return "subpixel_lt_0.1"
    if value < 0.5:
        return "tiny_0.1_to_0.5"
    if value < 2.0:
        return "small_0.5_to_2"
    if value < 8.0:
        return "medium_2_to_8"
    return "large_gte_8"


def _native_spacing_range_bucket(value_range: Any) -> dict[str, Any]:
    if not isinstance(value_range, (list, tuple)) or len(value_range) != 2:
        return {
            "min_bucket": "unavailable",
            "max_bucket": "unavailable",
            "range_bucket": "unavailable",
        }
    low, high = value_range
    if not isinstance(low, (int, float)) or not isinstance(high, (int, float)):
        return {
            "min_bucket": "unavailable",
            "max_bucket": "unavailable",
            "range_bucket": "unavailable",
        }
    low_bucket = _native_spacing_bucket(low)
    high_bucket = _native_spacing_bucket(high)
    return {
        "min_bucket": low_bucket,
        "max_bucket": high_bucket,
        "range_bucket": low_bucket if low_bucket == high_bucket else f"{low_bucket}..{high_bucket}",
    }


def _native_spacing_summary_bucket_key(estimate: dict[str, Any] | None) -> tuple[Any, ...] | None:
    if not isinstance(estimate, dict):
        return None
    state8_range = estimate.get("state8_with_estimated_size_effector_range")
    range_source = "with_estimated_size_effector"
    if not isinstance(state8_range, list):
        state8_range = estimate.get("state8_without_size_effector_range")
        range_source = "without_size_effector"
    bucket = _native_spacing_range_bucket(state8_range)
    multiplier_estimate = estimate.get("size_effector_multiplier_estimate")
    has_multiplier_estimate = isinstance(multiplier_estimate, dict) and bool(multiplier_estimate)
    return (
        estimate.get("kind"),
        estimate.get("branch"),
        bucket["range_bucket"],
        bucket["min_bucket"],
        bucket["max_bucket"],
        bool(estimate.get("size_effector_dynamic")),
        has_multiplier_estimate,
        range_source,
        estimate.get("unestimated_reason"),
    )


def _native_spacing_recommendations(bucket_counts: collections.Counter[tuple[Any, ...]]) -> list[dict[str, Any]]:
    totals: collections.Counter[str] = collections.Counter()
    dynamic_count = 0
    unestimated_count = 0
    for key, count in bucket_counts.items():
        range_bucket = key[2]
        totals[range_bucket] += count
        if key[5]:
            dynamic_count += count
        if key[8]:
            unestimated_count += count

    recommendations: list[dict[str, Any]] = []
    tiny_count = sum(
        count
        for bucket, count in totals.items()
        if bucket.startswith("subpixel_lt_0.1") or bucket.startswith("tiny_0.1_to_0.5")
    )
    if tiny_count:
        recommendations.append(
            {
                "topic": "sampler_step",
                "count": tiny_count,
                "recommendation": (
                    "Some native state+8 intervals are below 0.5 px; keep any importer raster-preview "
                    "sampler adaptive and cap minimum visual impact instead of using one coarse fixed step."
                ),
            }
        )
    if dynamic_count:
        recommendations.append(
            {
                "topic": "size_effector",
                "count": dynamic_count,
                "recommendation": (
                    "Dynamic SizeEffector rows need per-sample width/pressure/tilt/velocity evaluation; "
                    "style-level constants are not enough for pressure-sized vector strokes."
                ),
            }
        )
    if unestimated_count:
        recommendations.append(
            {
                "topic": "unestimated_spacing",
                "count": unestimated_count,
                "recommendation": (
                    "Rows without scalar spacing still need native tracing, especially AutoIntervalType 4/5 "
                    "or dynamic IntervalEffector cases."
                ),
            }
        )
    return recommendations


def _importer_vector_sample_step(style: dict[str, Any] | None, mod: Any | None = None) -> tuple[str, float] | None:
    if not isinstance(style, dict):
        return None
    fallback_step = float(getattr(mod, "VECTOR_FALLBACK_SAMPLE_STEP", 5.0))
    opacity_step = float(getattr(mod, "VECTOR_OPACITY_PRESSURE_SAMPLE_STEP", 2.0))
    if _effector_form(style.get("OpacityEffector")) == "dual_graph_0x31":
        return ("VECTOR_OPACITY_PRESSURE_SAMPLE_STEP", opacity_step)
    return ("VECTOR_FALLBACK_SAMPLE_STEP", fallback_step)


def _native_spacing_range_for_comparison(estimate: dict[str, Any] | None) -> tuple[float, float] | None:
    if not isinstance(estimate, dict):
        return None
    value_range = estimate.get("state8_with_estimated_size_effector_range")
    if not isinstance(value_range, list):
        value_range = estimate.get("state8_without_size_effector_range")
    if not isinstance(value_range, list) or len(value_range) != 2:
        return None
    low, high = value_range
    if not isinstance(low, (int, float)) or not isinstance(high, (int, float)):
        return None
    if not math.isfinite(float(low)) or not math.isfinite(float(high)):
        return None
    return (float(low), float(high))


def _importer_spacing_comparison(
    estimate: dict[str, Any] | None,
    style: dict[str, Any] | None,
    mod: Any | None = None,
) -> dict[str, Any] | None:
    step_info = _importer_vector_sample_step(style, mod)
    native_range = _native_spacing_range_for_comparison(estimate)
    if step_info is None or native_range is None:
        return None
    step_source, sample_step = step_info
    if sample_step <= 0:
        return None
    ratio_range = (native_range[0] / sample_step, native_range[1] / sample_step)
    if native_range[1] < sample_step * 0.5:
        classification = "fallback_step_coarser_than_native_interval"
    elif native_range[0] > sample_step * 2.0:
        classification = "fallback_step_finer_than_native_interval"
    else:
        classification = "fallback_step_overlaps_native_interval_range"
    return {
        "importer_sample_step": round(sample_step, 6),
        "sample_step_source": step_source,
        "native_state8_range": [round(native_range[0], 6), round(native_range[1], 6)],
        "native_state8_bucket": _native_spacing_range_bucket(native_range),
        "native_state8_to_importer_step_ratio_range": [
            round(ratio_range[0], 6),
            round(ratio_range[1], 6),
        ],
        "classification": classification,
        "note": (
            "Diagnostic only: importer fallback draws continuous capsules, while native state+8 "
            "is the PWBrushDraw dab interval feedback value."
        ),
    }


def _importer_spacing_comparison_key(comparison: dict[str, Any] | None) -> tuple[Any, ...] | None:
    if not isinstance(comparison, dict):
        return None
    ratio_range = comparison.get("native_state8_to_importer_step_ratio_range")
    native_range = comparison.get("native_state8_range")
    native_bucket = comparison.get("native_state8_bucket") or {}
    return (
        comparison.get("sample_step_source"),
        comparison.get("importer_sample_step"),
        native_bucket.get("range_bucket"),
        comparison.get("classification"),
        tuple(native_range or ()),
        tuple(ratio_range or ()),
    )


def _importer_spacing_comparison_recommendations(
    comparison_counts: collections.Counter[tuple[Any, ...]]
) -> list[dict[str, Any]]:
    weighted: collections.Counter[str] = collections.Counter()
    for key, count in comparison_counts.items():
        weighted[str(key[5])] += count

    recommendations: list[dict[str, Any]] = []
    coarse_count = weighted.get("fallback_step_coarser_than_native_interval", 0)
    overlap_count = weighted.get("fallback_step_overlaps_native_interval_range", 0)
    fine_count = weighted.get("fallback_step_finer_than_native_interval", 0)
    if coarse_count:
        recommendations.append(
            {
                "topic": "fallback_centerline_sampling",
                "classification": "fallback_step_coarser_than_native_interval",
                "count": coarse_count,
                "recommendation": (
                    "Prioritize an adaptive sampler experiment for rows where the current 5px/2px "
                    "fallback step is much larger than the estimated native dab interval."
                ),
            }
        )
    if overlap_count:
        recommendations.append(
            {
                "topic": "fallback_centerline_sampling",
                "classification": "fallback_step_overlaps_native_interval_range",
                "count": overlap_count,
                "recommendation": (
                    "Keep these rows as guard samples for any adaptive sampler; their native estimate range "
                    "already overlaps the current fallback step."
                ),
            }
        )
    if fine_count:
        recommendations.append(
            {
                "topic": "fallback_centerline_sampling",
                "classification": "fallback_step_finer_than_native_interval",
                "count": fine_count,
                "recommendation": (
                    "Watch for overdraw if the importer begins using native-style dabs for rows whose native "
                    "interval is larger than the current fallback subdivision."
                ),
            }
        )
    return recommendations


def _experimental_adaptive_spacing_candidate(
    estimate: dict[str, Any] | None,
    style: dict[str, Any] | None,
    mod: Any | None = None,
) -> dict[str, Any] | None:
    if not isinstance(style, dict):
        return None
    step_info = _importer_vector_sample_step(style, mod)
    if step_info is None:
        return None
    step_source, fallback_step = step_info
    min_step = float(getattr(mod, "VECTOR_ADAPTIVE_SPACING_MIN_STEP", 0.5))
    env_name = str(
        getattr(
            mod,
            "VECTOR_EXPERIMENTAL_ADAPTIVE_SPACING_ENV",
            "RIZUM_CLIP_EXPERIMENTAL_VECTOR_ADAPTIVE_SPACING",
        )
    )
    size_form = _effector_form(style.get("SizeEffector"))
    no_pattern_candidate = not (
        bool(style.get("retained_state"))
        or bool(style.get("SprayFlag"))
        or bool(style.get("PatternStyle"))
        or bool(style.get("TexturePattern"))
        or bool(style.get("TextureFlag"))
    )
    candidate = bool(no_pattern_candidate)
    out: dict[str, Any] = {
        "enabled_by_env": env_name,
        "candidate": bool(candidate),
        "candidate_rule": "ordinary no-pattern dab candidates only",
        "size_effector_form": size_form,
        "fallback_step_source": step_source,
        "fallback_step": round(float(fallback_step), 6),
        "min_step": round(min_step, 6),
    }
    if not candidate:
        out["inactive_reason"] = "not an ordinary no-pattern dab candidate"
        return out

    native_range = _native_spacing_range_for_comparison(estimate)
    if native_range is None:
        out["inactive_reason"] = "native interval estimate unavailable"
        return out
    adaptive_range = (
        max(min_step, min(float(fallback_step), native_range[0])),
        max(min_step, min(float(fallback_step), native_range[1])),
    )
    out["native_state8_range"] = [round(native_range[0], 6), round(native_range[1], 6)]
    out["estimated_adaptive_step_range"] = [
        round(adaptive_range[0], 6),
        round(adaptive_range[1], 6),
    ]
    out["would_reduce_step"] = adaptive_range[0] < float(fallback_step) or adaptive_range[1] < float(fallback_step)
    out["note"] = (
        "Diagnostic approximation of the default-off loader branch; exact segment steps are computed "
        "from endpoint taper/size-factor values at render time."
    )
    return out


def _experimental_adaptive_spacing_candidate_key(candidate: dict[str, Any] | None) -> tuple[Any, ...] | None:
    if not isinstance(candidate, dict):
        return None
    return (
        candidate.get("candidate"),
        candidate.get("size_effector_form"),
        candidate.get("fallback_step_source"),
        candidate.get("fallback_step"),
        candidate.get("min_step"),
        tuple(candidate.get("native_state8_range") or ()),
        tuple(candidate.get("estimated_adaptive_step_range") or ()),
        candidate.get("would_reduce_step"),
        candidate.get("inactive_reason"),
    )
NATIVE_NO_PATTERN_DAB_PIPELINE = {
    "role": "ordinary no-pattern PWBrushDraw queue -> dab rasterizer -> row accumulation",
    "functions": {
        "0x14255DFE0": "ordinary per-sample path; calls 0x1422D8550 and queues plot records",
        "0x14260F550": "thread-safe queue writer; copies plot +0..+88 into queue +0x128..+0x180",
        "0x14263F410": "no-pattern queue consumer; prepares dab context and selects circular/stretched/rotated helpers",
        "0x14263AC30": "row accumulation writer for coverage, direct/max, hardness, texture/material, and color paths",
        "0x14263FC50": "circular anti-aliased dab span writer",
        "0x142640150": "hard circular dab span writer",
        "0x142640420": "stretched/rotated anti-aliased ellipse span writer",
        "0x142640C90": "stretched/rotated hard ellipse span writer",
        "0x1426427D0": "narrow stretched/rotated fallback polygon span writer",
    },
    "queue_copy": {
        "plot+0 -> queue+0x128": "effective dab size",
        "plot+8 -> queue+0x130": "opacity cap",
        "plot+16 -> queue+0x138": "flow/coverage multiplier",
        "plot+24 -> queue+0x140": "thickness/stretch ratio",
        "plot+32 -> queue+0x148": "rotation angle",
        "plot+40 -> queue+0x150": "material/rotation flag",
        "plot+44 -> queue+0x154": "AntiAlias ordinal",
        "plot+48/+56 -> queue+0x158/+0x160": "pattern/material shared pointer",
        "plot+72 -> queue+0x170": "texture/color auxiliary scalar",
        "plot+88 -> queue+0x180": "caller plane/pass flag",
    },
    "no_pattern_context": {
        "ctx+0x1c0": "effective dab size/radius",
        "ctx+0x1d0": "AntiAlias ordinal",
        "ctx+0x1d4": "hardness/profile enabled",
        "ctx+0x1d8": "stretched/rotated path enabled",
        "ctx+0x1dc": "StyleFlag 0x1000 direct/max accumulation selector",
        "ctx+0x1e0": "32768-scaled opacity cap",
        "ctx+0x1e4": "32768-scaled flow multiplier",
    },
    "row_accumulation": {
        "geometric_flow": "flowCoverage = (ctx+0x1e4 * geometricCoverage) >> 15",
        "build_up": "if old < opacityCap: new = old + ((flowCoverage * (opacityCap - old)) >> 15); otherwise keep old",
        "direct_max": "candidate = opacityCap * flowCoverage >> 15; write max(old, candidate)",
        "hardness_profile": "profile scales flowCoverage before either accumulation formula",
    },
    "anti_alias": {
        "helper": "0x1422DBF30",
        "mapping": "AA 0 hard/no AA; AA 1/2/3 use min(radius, 1.5/2.5/3.5px)",
        "circular_aa": "0x14263FC50 writes center spans at full coverage and edge pixels by radial falloff",
        "hard_circular": "0x142640150 writes full spans with sqrt(radius^2-dy^2)-0.4",
    },
    "importer_policy": (
        "Exact rotation/thickness/flow/hardness belong to the native per-dab row writer. "
        "The importer may keep narrow isolated-sample preview bridges, but broad support should "
        "arrive through a dab/row model."
    ),
}
IMPORTER_VECTOR_FALLBACK_CONSTANT_NAMES = (
    "VECTOR_STROKE_MAX_POINTS",
    "VECTOR_STROKE_MAX_WIDTH",
    "VECTOR_STROKE_MIN_VISIBLE_SPAN",
    "VECTOR_STROKE_RADIUS_SCALE",
    "VECTOR_FALLBACK_SAMPLE_STEP",
    "VECTOR_OPACITY_PRESSURE_SAMPLE_STEP",
    "VECTOR_FILLED_CURVE_DARK_RGB_THRESHOLD",
    "VECTOR_FILLED_CURVE_MAX_SOLID_WIDTH",
    "VECTOR_FILLED_CURVE_ELLIPSE_INSET",
    "VECTOR_FILLED_CURVE_ELLIPSE_POWER",
    "VECTOR_FILLED_CURVE_RADIUS_SCALE",
    "VECTOR_FILLED_CURVE_FEATHER_BASE",
    "VECTOR_FILLED_CURVE_FEATHER_PER_AA",
    "VECTOR_LEGACY_AA_FEATHER",
    "VECTOR_NATIVE_AA_FEATHER_BY_LEVEL",
    "VECTOR_HARDNESS_FEATHER_SCALE",
    "VECTOR_HARDNESS_RADIUS_SOFTEN_SCALE",
    "VECTOR_INTERVAL_RADIUS_SOFTEN_MAX",
    "VECTOR_AUTO_INTERVAL_RADIUS_SCALE",
    "VECTOR_AUTO_INTERVAL_FEATHER_SCALE",
    "VECTOR_FLOW_RADIUS_SOFTEN_MAX",
    "VECTOR_FLOW_DYNAMIC_RADIUS_SOFTEN_MAX",
    "VECTOR_TEXTURE_DENSITY_PREVIEW_SCALE",
    "VECTOR_TEXTURE_PREVIEW_GAMMA",
    "VECTOR_MATERIAL_STAMP_WIDTH_SCALE",
    "VECTOR_MATERIAL_STAMP_HEIGHT_SCALE",
    "VECTOR_MATERIAL_STAMP_GAP_HEIGHT_SCALE",
    "VECTOR_MATERIAL_STAMP_ALPHA",
    "VECTOR_MATERIAL_STAMP_GAP_ALPHA",
    "VECTOR_MATERIAL_STAMP_MIN_STEP",
    "VECTOR_SIMPLE_SIZE_RADIUS_SCALE",
    "VECTOR_SIMPLE_SIZE_AA_FEATHER",
    "VECTOR_PRESSURE_SIZE_RADIUS_SCALE",
    "VECTOR_PRESSURE_OPACITY_RADIUS_SCALE",
    "VECTOR_PRESSURE_OPACITY_SECONDARY_NATIVE_BLEND",
    "VECTOR_PRESSURE_OPACITY_ALPHA_SCALE",
    "VECTOR_PRESSURE_OPACITY_ALPHA_POWER",
    "VECTOR_EXPERIMENTAL_DAB_ENV",
    "VECTOR_EXPERIMENTAL_NATIVE_RANDOM_OPACITY_ENV",
    "VECTOR_EXPERIMENTAL_ADAPTIVE_SPACING_ENV",
    "VECTOR_ADAPTIVE_SPACING_MIN_STEP",
    "TEXT_BALLOON_FALLBACK_INSET",
    "TEXT_BALLOON_FALLBACK_POWER",
    "BALLOON_FALLBACK_BODY_BBOX_INSET",
    "BALLOON_FALLBACK_POINT_BBOX_EXPAND",
)


def _importer_vector_fallback_policy(mod: Any | None) -> dict[str, Any]:
    values: dict[str, Any] = {}
    if mod is not None:
        for name in IMPORTER_VECTOR_FALLBACK_CONSTANT_NAMES:
            if hasattr(mod, name):
                values[name] = getattr(mod, name)
    return {
        "role": "current importer preview guards/tuning, not serialized .clip fields",
        "values": values,
    }


def _importer_vector_renderer_policy() -> dict[str, Any]:
    return {
        "role": "fields that currently affect importer vector fallback pixels",
        "data_backed_pixel_inputs": {
            "layer_routing": [
                "Layer.VectorNormalType",
                "Layer.VectorNormalBalloonIndex",
                "Layer.ComicFrameLineMipmap",
                "Layer.ComicFrameColorTypeWhiteChecked",
                "Layer.ComicFrameColorTypeBlackChecked",
                "Layer.TextLayerAttributes when text cache exists",
                "Layer child/background rows for frame fill fallback",
            ],
            "vector_object_headers": [
                "VectorObjectList.VectorData object bbox",
                "line_rgb",
                "fill_rgb",
                "object opacity",
                "width",
                "line_style_id",
                "fill_style_id",
                "family_id 0x130/0x410",
                "point bbox when available",
            ],
            "stroke_records": [
                "92-byte stroke flags 0x2011/0x2081",
                "filled-curve flag 0x41",
                "stroke bbox",
                "stroke RGB",
                "stroke object opacity from header f64+64",
                "stroke width",
                "brush_style_id",
                "point x/y",
                "compact f32+36 primary scalar for pressure branches",
                "compact f32+40 secondary scalar only in the narrow opacity preview blend",
                "compact f32+52 endpoint taper for legacy/simple fallback",
                "compact u32+80 trailing seed for native-LCG SizeEffector 0x81 random preview",
                "compact 120-byte dark curve point f64+104/+112 as the second cubic control in the filled-curve preview",
            ],
            "brush_style_fields_used": [
                "AntiAlias ordinal",
                "SizeEffector 0x31 with BrushEffectorGraphData for pressure-size sample",
                "OpacityEffector 0x31 with BrushEffectorGraphData for pressure-opacity sample",
                "SizeEffector 0x11 only as a branch selector for the older simple-size taper heuristic",
                "SizeEffector 0x81 random floor with compact point u32+80 as native LCG seed",
                "OpacityEffector 0x81 random floor with stable deterministic pseudo-random source",
                "IntervalBase > 0.1 for ordinary no-pattern/no-texture radius preview",
                "AutoIntervalType 1/2/3 with AntiAlias > 0 for ordinary no-pattern/no-texture radius/feather preview scaled by AA ordinal",
                "ThicknessBase < 1 for ordinary no-pattern/no-texture radius/rotated-ellipse preview",
                "RotationBase only when ThicknessBase < 1 selects the rotated ellipse preview path",
                "Hardness < 1 for ordinary no-pattern/no-texture capsule softness preview",
                "FlowBase < 1 for ordinary no-pattern/no-texture radius preview",
                "FlowEffector 0x11/0x41 floor 0.5 graph 4 for ordinary no-pattern/no-texture dynamic radius preview",
                "TexturePattern single-channel, normal composite, zero transform/color adjustment for ordinary stroke alpha preview",
                "PatternStyle single-image material stamp preview for ordinary strokes",
                "PatternStyle/ImageIndex only for the one-point filled-curve pattern fallback",
            ],
            "fill_style_fields_used": [
                "FillStyle.CompositeMode is diagnostic/context for object fill family",
                "actual frame/bubble fallback fill color comes from object headers or child/background rows",
            ],
        },
        "diagnostic_only_or_native_mapped_not_rendered_yet": [
            "Unhandled FlowEffector/FlowBase combinations and exact flow row accumulation",
            "Unhandled Hardness combinations and exact native hardness profile/dab row sampling",
            "Unhandled TexturePattern/TextureFlag/TextureComposite/TextureDensity* combinations",
            "Unhandled PatternStyle OrderType/Reverse2/material list state",
            "StyleFlag bits including 0x20 retained-state and 0x1000 direct/max accumulation",
            "IntervalEffector/AutoIntervalType and unhandled IntervalBase combinations",
            "ThicknessEffector and unhandled ThicknessBase combinations",
            "Rotation* outside the ThicknessBase < 1 rotated-ellipse preview",
            "Spray* and FixedSpray",
            "MixColor/MixAlpha/UseWaterColor/WaterEdge*",
            "SubColor/Hue/Saturation/Value change effectors",
            "Blur*",
            "native compiled PWBrushStyle resource wrappers",
        ],
        "implementation_boundary": (
            "The current importer draws simplified capsules/superellipses. Exact support still needs "
            "the native PWBrushDraw sink, compiled line-style resource wrappers, dab geometry, "
            "material sampling, and row accumulation paths."
        ),
    }


def _renderer_gap_category(item: Any) -> str:
    text = str(item)
    if (
        text.startswith("SizeEffector")
        or text.startswith("RotationEffector")
        or text.startswith("IntervalBase/")
        or text.startswith("ThicknessEffector")
        or "texture_or_water_family" in text
    ):
        return "native_dab_geometry_spacing"
    if (
        text.startswith("PatternStyle/material list")
        or text.startswith("TexturePattern/TextureFlag")
        or "native_retained_state_path_0x20" in text
    ):
        return "retained_pattern_material_path"
    if (
        text.startswith("FlowBase/")
        or text.startswith("OpacityEffector")
        or text.startswith("Hardness")
        or text.startswith("TextureDensity")
        or "direct_max_accum_0x1000" in text
    ):
        return "coverage_alpha_row_accumulation"
    if text.startswith("CompositeMode"):
        return "fill_composite_context"
    if (
        text.startswith("Mix")
        or text.startswith("UseWaterColor")
        or text.startswith("Water")
        or text.startswith("SubColor")
        or text.startswith("HueChange")
        or text.startswith("SaturationChange")
        or text.startswith("ValueChange")
        or text.startswith("Blur")
        or text.startswith("BrushColorMixing")
    ):
        return "color_water_blur_effectors"
    if text.startswith("Spray") or text.startswith("FixedSpray") or text.startswith("RotationEffectorInSpray"):
        return "spray_dab_distribution"
    return "uncategorized_renderer_gap"


def _renderer_gap_recommendation_specs() -> dict[str, dict[str, Any]]:
    return {
        "native_dab_geometry_spacing": {
            "priority": 1,
            "next_step": (
                "Trace the native PWBrushDraw dab geometry/spacing path before adding importer formulas."
            ),
            "native_targets": [
                "PWBrushDraw sink +0x20 dispatcher",
                "ordinary dab path 0x14255DFE0",
                "retained-state path 0x142558A90",
                "PWBrushStyle evaluator 0x1422D8550 plot +24/+32/+40/+44/+72",
            ],
            "sample_hint": (
                "Best samples vary rotation, interval, thickness, and texture/water style flags one at a time "
                "on the same vector stroke."
            ),
        },
        "retained_pattern_material_path": {
            "priority": 1,
            "next_step": (
                "Model the retained pattern/material row path before trying to tune frame/bubble rough lines."
            ),
            "native_targets": [
                "retained-state path 0x142558A90",
                "pattern selector 0x14256ACE0",
                "compiled line-style resource builder 0x1422D9BE0/0x1422DA100",
            ],
            "sample_hint": (
                "Best samples isolate one rough balloon/frame line material with OrderType/Reverse2/pattern list "
                "changes while keeping geometry fixed."
            ),
        },
        "coverage_alpha_row_accumulation": {
            "priority": 2,
            "next_step": (
                "Recover row coverage and alpha accumulation before wiring Flow/Hardness/0x11 opacity into pixels."
            ),
            "native_targets": [
                "PWBrushStyle evaluator 0x1422D8550 plot +8/+16",
                "generic effector evaluator 0x142568040",
                "dab row accumulation under PWBrushDraw sink +0x20",
            ],
            "sample_hint": (
                "Best samples vary flow, hardness, opacity 0x11, texture density, and StyleFlag 0x1000 separately."
            ),
        },
        "color_water_blur_effectors": {
            "priority": 3,
            "next_step": (
                "Keep these diagnostic until color mixing/watercolor/blur samples show a vector-preview need."
            ),
            "native_targets": [
                "PWBrushStyle color/water writer fields",
                "texture/material auxiliary scalar at plot +72",
            ],
            "sample_hint": (
                "Only make samples if the preview must cover watercolor/color-mix vector brushes."
            ),
        },
        "spray_dab_distribution": {
            "priority": 3,
            "next_step": "Keep spray separate from normal stroke rendering; it needs dab distribution, not line fallback tuning.",
            "native_targets": [
                "SprayFlag branch in PWBrushStyle evaluator",
                "spray size/density effectors",
            ],
            "sample_hint": "Use dedicated spray vector samples only after ordinary brush dabs are closer.",
        },
        "fill_composite_context": {
            "priority": 3,
            "next_step": (
                "Keep fill-style composite fields as context until object fill rendering needs blend-mode accuracy."
            ),
            "native_targets": [
                "FillStyle composite handling",
                "object_100 fill routing",
            ],
            "sample_hint": "Use frame/bubble fill samples with changed blend modes only if composite preview errors show up.",
        },
        "uncategorized_renderer_gap": {
            "priority": 4,
            "next_step": "Inspect manually; this field is not yet mapped to a stable renderer workstream.",
            "native_targets": [],
            "sample_hint": "Create a one-setting sample if this appears often in future corpora.",
        },
    }


def _renderer_gap_recommendations(
    renderer_gap_inputs_by_kind: collections.Counter[tuple[Any, ...]],
    renderer_gap_inputs_by_clip: dict[tuple[Any, ...], set[str]] | None = None,
) -> list[dict[str, Any]]:
    specs = _renderer_gap_recommendation_specs()
    buckets: dict[str, dict[str, Any]] = {}
    for (kind, item), count in renderer_gap_inputs_by_kind.items():
        category = _renderer_gap_category(item)
        bucket = buckets.setdefault(
            category,
            {
                "category": category,
                "total_count": 0,
                "by_kind": collections.Counter(),
                "inputs": collections.Counter(),
                "clips": set(),
            },
        )
        bucket["total_count"] += count
        bucket["by_kind"][kind] += count
        bucket["inputs"][item] += count
        if renderer_gap_inputs_by_clip:
            bucket["clips"].update(renderer_gap_inputs_by_clip.get((kind, item), set()))

    recommendations = []
    for category, bucket in buckets.items():
        spec = specs.get(category, specs["uncategorized_renderer_gap"])
        recommendations.append(
            {
                "priority": spec["priority"],
                "category": category,
                "total_count": bucket["total_count"],
                "by_kind": [
                    {"kind": kind, "count": count}
                    for kind, count in bucket["by_kind"].most_common()
                ],
                "top_inputs": [
                    {"diagnostic_only_input": item, "count": count}
                    for item, count in bucket["inputs"].most_common(8)
                ],
                "sample_clip_count": len(bucket["clips"]),
                "sample_clips": sorted(bucket["clips"])[:12],
                "next_step": spec["next_step"],
                "native_targets": spec["native_targets"],
                "sample_hint": spec["sample_hint"],
            }
        )
    return sorted(
        recommendations,
        key=lambda item: (item["priority"], -item["total_count"], item["category"]),
    )


def _has_active_spray(counter: collections.Counter[tuple[Any, ...]]) -> bool:
    for combo in counter:
        spray_flag = combo[1] if len(combo) > 1 else None
        fixed_spray = combo[7] if len(combo) > 7 else None
        if spray_flag not in (None, 0) or fixed_spray not in (None, "0", 0):
            return True
    return False


def _has_active_color_water_blur(
    color_mix_counter: collections.Counter[tuple[Any, ...]],
    blur_counter: collections.Counter[tuple[Any, ...]],
) -> bool:
    for combo in color_mix_counter:
        use_water_color = combo[1] if len(combo) > 1 else None
        brush_color_mixing_mode = combo[3] if len(combo) > 3 else None
        mix_color_base = combo[5] if len(combo) > 5 else None
        mix_color_effector = combo[6] if len(combo) > 6 else None
        mix_alpha_base = combo[7] if len(combo) > 7 else None
        mix_alpha_effector = combo[8] if len(combo) > 8 else None
        water_edge_flag = combo[10] if len(combo) > 10 else None
        if (
            use_water_color not in (None, 0)
            or brush_color_mixing_mode not in (None, 0)
            or mix_color_base not in (None, "1.0", 1, 1.0)
            or mix_color_effector not in (None, "4:0:00000000")
            or mix_alpha_base not in (None, "1.0", 1, 1.0)
            or mix_alpha_effector not in (None, "4:0:00000000")
            or water_edge_flag not in (None, 0)
        ):
            return True
    for combo in blur_counter:
        blur_kind = combo[1] if len(combo) > 1 else None
        blur_base = combo[2] if len(combo) > 2 else None
        blur_effector = combo[3] if len(combo) > 3 else None
        if (
            blur_kind not in (None, 0)
            or blur_base not in (None, "0.0", 0, 0.0)
            or blur_effector not in (None, "4:0:00000000")
        ):
            return True
    return False


def _sample_request_recommendations(
    renderer_recommendations: list[dict[str, Any]],
    spray_combos_by_kind: collections.Counter[tuple[Any, ...]],
    color_mix_combos_by_kind: collections.Counter[tuple[Any, ...]],
    blur_combos_by_kind: collections.Counter[tuple[Any, ...]],
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for rec in renderer_recommendations:
        category = rec.get("category")
        if category not in {
            "native_dab_geometry_spacing",
            "retained_pattern_material_path",
            "coverage_alpha_row_accumulation",
        }:
            continue
        rows.append(
            {
                "target": category,
                "status": "current_corpus_has_evidence",
                "priority": rec.get("priority"),
                "current_clip_count": rec.get("sample_clip_count"),
                "current_sample_clips": rec.get("sample_clips"),
                "why": rec.get("next_step"),
                "recommended_sample_design": rec.get("sample_hint"),
            }
        )

    if not _has_active_spray(spray_combos_by_kind):
        rows.append(
            {
                "target": "spray_dab_distribution",
                "status": "missing_from_current_corpus",
                "priority": 3,
                "current_clip_count": 0,
                "current_sample_clips": [],
                "why": "Current vector styles have SprayFlag off, so spray formulas cannot be validated.",
                "recommended_sample_design": (
                    "Only make this after ordinary dabs are closer: same vector stroke, one brush, "
                    "then vary SprayFlag, spray size, spray density, random, and fixed-spray separately."
                ),
            }
        )
    if not _has_active_color_water_blur(color_mix_combos_by_kind, blur_combos_by_kind):
        rows.append(
            {
                "target": "color_water_blur_effectors",
                "status": "missing_from_current_corpus",
                "priority": 3,
                "current_clip_count": 0,
                "current_sample_clips": [],
                "why": "Current vector styles keep color-mix, watercolor, water-edge, and blur fields disabled.",
                "recommended_sample_design": (
                    "Use only if preview needs these brush classes: same vector stroke over a colored raster "
                    "background, then vary MixColor, MixAlpha, UseWaterColor, WaterEdge, and Blur one at a time."
                ),
            }
        )
    return sorted(rows, key=lambda item: (item["priority"], item["target"]))


def _load_loader(root: Path):
    mod_path = root / "clip_studio_importer" / "clip_loader.py"
    spec = importlib.util.spec_from_file_location("pkg_clip_loader", mod_path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)
    return mod


def _decode_effector_words(words: list[dict[str, Any]]) -> dict[str, Any]:
    if not words:
        return {"form": "empty", "graph_refs": [], "scalars": []}
    flag = int(words[0].get("u32") or 0)
    u32s = [int(word.get("u32") or 0) for word in words]
    f32s = [word.get("f32") for word in words]
    native_flags = _effector_runtime_branch_bits(flag)
    if flag == 0x00 and len(words) == 1:
        return {
            "form": "zero_or_disabled_0x00",
            "type": flag,
            "native_runtime_branches": native_flags,
            "graph_refs": [],
            "scalars": [],
        }
    if flag == 0x01 and len(words) == 1:
        return {
            "form": "default_or_off_0x01",
            "type": flag,
            "native_runtime_branches": native_flags,
            "graph_refs": [],
            "scalars": [],
        }
    if flag == 0x11 and len(words) >= 3:
        return {
            "form": "primary_graph_0x11",
            "type": flag,
            "native_runtime_branches": native_flags,
            "floor": f32s[1],
            "scalar": f32s[1],
            "graph_refs": [u32s[2]],
            "extra_scalars": f32s[3:],
            "payload_u32": u32s[1:],
        }
    if flag == 0x31 and len(words) >= 6:
        return {
            "form": "dual_graph_0x31",
            "type": flag,
            "native_runtime_branches": native_flags,
            "zero_or_mode": u32s[1],
            "graph_refs": [u32s[2], u32s[4]],
            "scalars": [f32s[3], f32s[5]],
            "payload_u32": u32s[1:],
        }
    if flag == 0x41 and len(words) >= 3:
        return {
            "form": "aux_graph_0x41",
            "type": flag,
            "native_runtime_branches": native_flags,
            "floor": f32s[1],
            "scalar": f32s[1],
            "graph_refs": [u32s[2]],
            "extra_scalars": f32s[3:],
            "payload_u32": u32s[1:],
        }
    if flag == 0x21 and len(words) >= 4:
        return {
            "form": "secondary_graph_range_0x21",
            "type": flag,
            "native_runtime_branches": native_flags,
            "low": f32s[1],
            "high": f32s[3],
            "scalar": f32s[1],
            "graph_refs": [u32s[2]],
            "scalars": [f32s[1], f32s[3]],
            "payload_u32": u32s[1:],
        }
    if flag == 0xC1 and len(words) >= 4:
        return {
            "form": "aux_graph_random_0xc1",
            "type": flag,
            "native_runtime_branches": native_flags,
            "aux_floor": f32s[1],
            "random_floor": f32s[2],
            "scalars": [f32s[1], f32s[2]],
            "graph_refs": [u32s[3]],
            "payload_u32": u32s[1:],
        }
    if flag == 0x81 and len(words) >= 2:
        return {
            "form": "random_floor_0x81",
            "type": flag,
            "native_runtime_branches": native_flags,
            "random_floor": f32s[1],
            "scalar": f32s[1],
            "graph_refs": [],
            "payload_u32": u32s[1:],
        }
    if flag == 0x111 and len(words) >= 4:
        return {
            "form": "primary_graph_velocity_0x111",
            "type": flag,
            "native_runtime_branches": native_flags,
            "floor": f32s[1],
            "velocity_floor": f32s[3],
            "scalar": f32s[1],
            "graph_refs": [u32s[2]],
            "extra_scalars": [f32s[3]],
            "payload_u32": u32s[1:],
        }
    graph_refs = [value for value in u32s[1:] if 0 < value < 100000]
    return {
        "form": f"unknown_0x{flag:x}",
        "type": flag,
        "native_runtime_branches": native_flags,
        "graph_refs": graph_refs,
        "payload_u32": u32s[1:],
        "payload_f32": f32s[1:],
    }


def _effector_runtime_branch_bits(flag: int) -> dict[str, Any]:
    """Name the native PWBrushParameterEffector branch bits.

    The compact .clip blob starts with the same branch-family bits that the
    compiled runtime evaluator consumes at 0x142568040. Bit 0x1 marks a
    compiled/non-default effector, while the high bits select dynamic inputs.
    """
    flag = int(flag or 0)
    known = 0x1 | 0x10 | 0x20 | 0x40 | 0x80 | 0x100
    return {
        "value": flag,
        "hex": f"0x{flag:x}",
        "compiled_or_active_0x1": bool(flag & 0x1),
        "graph_context_primary_0x10": bool(flag & 0x10),
        "graph_context_primary_0x10_sample_f32_36": bool(flag & 0x10),
        "graph_context_secondary_0x20": bool(flag & 0x20),
        "graph_context_secondary_0x20_sample_f32_40": bool(flag & 0x20),
        "graph_context_aux_0x40": bool(flag & 0x40),
        "graph_context_aux_0x40_sample_f32_44": bool(flag & 0x40),
        "random_0x80": bool(flag & 0x80),
        "velocity_0x100": bool(flag & 0x100),
        "velocity_0x100_sample_f32_52": bool(flag & 0x100),
        "unknown_bits": flag & ~known,
    }


def _decode_balloon_family_flags(value: int) -> dict[str, Any]:
    """Decode PWVectorBalloon tail +76 flags seen in compact 100-byte records."""
    flags = int(value)
    known_bits = 0x1 | 0x2 | 0x10 | 0x20 | 0x40 | 0x100 | 0x200 | 0x400 | 0x1000
    return {
        "value": flags,
        "hex": f"0x{flags:x}",
        "inherits_from_previous_0x1": bool(flags & 0x1),
        "chain_update_propagation_0x2": bool(flags & 0x2),
        "uses_width_0x10": bool(flags & 0x10),
        "flag_0x20": bool(flags & 0x20),
        "skip_point_geometry_0x40": bool(flags & 0x40),
        "chain_hit_test_mode_0x100": bool(flags & 0x100),
        "use_own_fill_list_0x200": bool(flags & 0x200),
        "sibling_hit_blocker_0x400": bool(flags & 0x400),
        "enabled_or_visible_0x1000": not bool(flags & 0x1000),
        "unknown_bits": flags & ~known_bits,
    }


def _effector_words(blob: Any) -> dict[str, Any] | None:
    if not isinstance(blob, bytes):
        return None
    words: list[dict[str, Any]] = []
    for off in range(0, len(blob), 4):
        chunk = blob[off : off + 4]
        if len(chunk) < 4:
            break
        f32 = struct.unpack(">f", chunk)[0]
        words.append(
            {
                "u32": struct.unpack(">I", chunk)[0],
                "f32": None if not math.isfinite(f32) else round(float(f32), 6),
            }
        )
    return {
        "len": len(blob),
        "hex": blob.hex(),
        "words": words,
        "decoded": _decode_effector_words(words),
    }


def _effector_key(summary: dict[str, Any] | None) -> str:
    if not summary:
        return "none"
    if not isinstance(summary, dict):
        return json.dumps(summary, ensure_ascii=False, sort_keys=True)
    words = summary.get("words") or []
    flag = words[0]["u32"] if words else None
    return f"{summary.get('len')}:{flag}:{summary.get('hex')}"


def _style_value_key(value: Any) -> str:
    if isinstance(value, dict) and "words" in value:
        return _effector_key(value)
    if value is None:
        return "none"
    return json.dumps(value, ensure_ascii=False, sort_keys=True)


def _effector_graph_refs(summary: dict[str, Any] | None) -> list[int]:
    """Extract graph ids referenced by compact BrushStyle effector blobs.

    Observed compact forms encode graph ids as integer words. This is kept
    diagnostic: the renderer still decides per family which refs are active.
    """
    if not isinstance(summary, dict):
        return []
    decoded = summary.get("decoded")
    if isinstance(decoded, dict):
        return [
            int(graph_id)
            for graph_id in decoded.get("graph_refs", [])
            if isinstance(graph_id, int) and graph_id > 0
        ]
    words = (summary or {}).get("words") or []
    if not words:
        return []
    flag = int(words[0].get("u32") or 0)
    refs: list[int] = []
    if flag == 0x11 and len(words) >= 3:
        refs.append(int(words[2].get("u32") or 0))
    elif flag == 0x31 and len(words) >= 6:
        refs.extend([int(words[2].get("u32") or 0), int(words[4].get("u32") or 0)])
    elif len(words) >= 3:
        # Unknown compact families still often carry small graph-id-like words.
        for word in words[1:]:
            value = int(word.get("u32") or 0)
            if 0 < value < 100000:
                refs.append(value)
    return refs


def _row_value(row, key: str, default: Any = None) -> Any:
    return row[key] if key in row.keys() else default


def _clean_value(value: Any) -> Any:
    if isinstance(value, bytes):
        return {"len": len(value), "hex": value.hex()}
    if isinstance(value, float):
        return None if not math.isfinite(value) else round(float(value), 6)
    return value


def _u32_be_values(value: Any) -> list[int]:
    if not isinstance(value, bytes):
        return []
    values: list[int] = []
    for off in range(0, len(value), 4):
        chunk = value[off : off + 4]
        if len(chunk) < 4:
            break
        values.append(int(struct.unpack(">I", chunk)[0]))
    return values


def _brush_style_flag_bits(value: int) -> dict[str, Any]:
    value = int(value or 0)
    known_bits = 0x8 | 0x20 | 0x40 | 0x200 | 0x1000 | 0x10000 | 0x20000
    return {
        "value": value,
        "hex": f"0x{value:x}",
        "native_subpixel_min_size_0x8": bool(value & 0x8),
        "native_retained_state_path_0x20": bool(value & 0x20),
        "native_thickness_axis_split_0x40": bool(value & 0x40),
        "native_segment_start_flag_0x200": bool(value & 0x200),
        "direct_max_accum_0x1000": bool(value & 0x1000),
        "texture_or_water_family_0x10000": bool(value & 0x10000),
        "texture_or_water_family_0x20000": bool(value & 0x20000),
        "unknown_bits": value & ~known_bits,
    }


def _texture_flag_bits(value: int) -> dict[str, Any]:
    value = int(value or 0)
    return {
        "value": value,
        "hex": f"0x{value:x}",
        "enabled_bit_0x1": bool(value & 0x1),
        "density_effector_gate_0x10": bool(value & 0x10),
        "invert_sample_0x100": bool(value & 0x100),
        "average_baseline_remap_0x200": bool(value & 0x200),
        "unknown_bits": value & ~(0x1 | 0x10 | 0x100 | 0x200),
    }


def _pattern_order_type_label(value: Any) -> str | None:
    if value is None:
        return None
    labels = {
        0: "sequence_modulo",
        1: "ping_pong_sequence",
        2: "clamp_to_last",
        3: "random_lcg",
        4: "stop_when_exhausted",
        5: "explicit_index_list",
    }
    return labels.get(int(value), f"unknown_{int(value):#x}")


def _pattern_reverse2_bits(value: Any) -> dict[str, Any] | None:
    if value is None:
        return None
    flags = int(value or 0)
    known_bits = 0x1 | 0x2 | 0x4 | 0x10 | 0x20 | 0x40
    return {
        "value": flags,
        "hex": f"0x{flags:x}",
        "axis_a_invert_or_fixed_0x1": bool(flags & 0x1),
        "axis_a_random_0x2": bool(flags & 0x2),
        "axis_a_pingpong_parity_0x4": bool(flags & 0x4),
        "axis_b_invert_or_fixed_0x10": bool(flags & 0x10),
        "axis_b_random_0x20": bool(flags & 0x20),
        "axis_b_pingpong_parity_0x40": bool(flags & 0x40),
        "unknown_bits": flags & ~known_bits,
    }


def _byte_lane_stats(blob: bytes, bytes_per_pixel: int) -> dict[str, Any]:
    if bytes_per_pixel <= 0:
        return {}
    lanes: dict[str, Any] = {}
    lane_values: list[bytes] = []
    for lane in range(bytes_per_pixel):
        values = blob[lane::bytes_per_pixel]
        lane_values.append(values)
        if not values:
            continue
        total = sum(values)
        lanes[str(lane)] = {
            "min": min(values),
            "max": max(values),
            "avg": round(total / len(values), 6),
            "nonzero": sum(1 for value in values if value != 0),
            "count": len(values),
        }
    if len(lane_values) > 1:
        pairs: dict[str, Any] = {}
        for left in range(len(lane_values)):
            for right in range(left + 1, len(lane_values)):
                a = lane_values[left]
                b = lane_values[right]
                count = min(len(a), len(b))
                if count <= 0:
                    continue
                abs_diff_sum = 0
                equal = 0
                max_diff = 0
                for av, bv in zip(a[:count], b[:count]):
                    diff = abs(av - bv)
                    abs_diff_sum += diff
                    if diff == 0:
                        equal += 1
                    if diff > max_diff:
                        max_diff = diff
                pairs[f"{left}-{right}"] = {
                    "count": count,
                    "equal": equal,
                    "equal_ratio": round(equal / count, 6),
                    "mean_abs_diff": round(abs_diff_sum / count, 6),
                    "max_abs_diff": max_diff,
                }
        if pairs:
            lanes["pairwise"] = pairs
    return lanes


def _native_material_selector_guess(bytes_per_pixel: float | None) -> dict[str, Any] | None:
    if bytes_per_pixel == 1.0:
        return {
            "selector": 0x1,
            "selector_hex": "0x1",
            "selector_family": "single_lane",
            "coverage_lane": 0,
            "native_initializer": "0x1432BDA20 selector family 0x1",
            "reason": "one decoded byte per pixel",
        }
    if bytes_per_pixel == 2.0:
        return {
            "selector": 0x11,
            "selector_hex": "0x11",
            "selector_family": "two_lane",
            "coverage_lane": 1,
            "native_initializer": "0x1432BDA20 selector family 0x11",
            "reason": "0x10 selector family samples byte lane 1",
        }
    if bytes_per_pixel == 4.0:
        return {
            "selector": 0x21,
            "selector_hex": "0x21",
            "selector_family": "four_lane",
            "coverage_lane": 3,
            "native_initializer": "0x1432BDA20 selector family 0x21",
            "reason": "0x20 selector family samples byte lane 3",
        }
    return None


def _native_material_format_note(bytes_per_pixel: float | None) -> dict[str, Any] | None:
    selector = _native_material_selector_guess(bytes_per_pixel)
    if selector is None:
        return None
    return {
        "observed_selector_guess": selector["selector_hex"],
        "observed_coverage_lane": selector["coverage_lane"],
        "source": "decoded material bytes-per-pixel",
        "native_selector_getter": "0x1419BCBD0 reads material image object+0x98",
        "native_storage_getter": "0x1419BCDA0 reads material image object+0x9c",
        "native_material_image_builder": "0x1432BDE10 -> 0x1432BBA90 PWVOffscreen/material image",
        "native_copy_or_convert_path": (
            "0x14251C280 direct-copies compatible storage with 0x1432BE2F0, "
            "otherwise initializes 0x1432BDA20 and converts pixels with 0x1432CD910."
        ),
        "native_region_copy_convert": (
            "0x1432D4260 fast-paths matching selector/storage by memcpy, "
            "otherwise dispatches 0x1432D4100 -> 0x1432CDA40 per-pixel conversion."
        ),
        "native_pixel_format_flags": (
            "0x1432CDA40 reads unit size from object+0x84 and flags from object+0x80; "
            "flag 0x10 is a single extra/material lane and 0x20 is RGB-like lanes."
        ),
        "native_channel_helpers": (
            "0x14206C3A0/0x14206C300/0x14206C350 build packed 3-channel values "
            "from 8/16/32-bit lanes; 0x142085BA0 converts RGB-like lanes to "
            "luminance with weights 0.298912/0.586611/0.114478."
        ),
        "native_stamp_buffer_bridge": (
            "0x142642E90 initializes draw-time material/stamp buffers with "
            "0x141AA4BD0, then copies/resamples through 0x141AA5730 or 0x141AA6130 "
            "and publishes pixel base/strides to draw context +0x238/+0x240/+0x244."
        ),
        "native_stamp_buffer_copy_paths": (
            "0x141AA5730 is the general multi-lane path: 0x1 copies base lane, "
            "0x11 writes extra/material lane plus base lane, and 0x21 writes three "
            "RGB-like extra lanes plus base lane. 0x141AA6130 is a fast matching-format "
            "path for base-lane-only buffers."
        ),
        "native_sampler_lane_rule": (
            "0x142637930 samples draw context +0x228: flag 0x20 uses pixel byte 3, "
            "flag 0x10 uses byte 1, otherwise byte 0, then multiplies coverage by "
            "that byte / 255 before row accumulation."
        ),
        "native_row_writer_dispatch": (
            "0x14263B7F0 dispatches material rows by the same flags: 0x20 to RGB-like "
            "row writers, 0x10 to two-lane/material-mix writers, and 1 to base-lane writers."
        ),
        "native_four_sample_material_helpers": (
            "Cached-buffer helpers 0x14263E5D0/0x14263E970/0x14263EE20 sample four "
            "material pixels and scale coverage by sum(alpha)/(4*255). For 0x10, "
            "byte1 is alpha and byte0 is averaged mix; for 0x20, byte3 is alpha and "
            "bytes2/1/0 are averaged RGB-like channels. 0x14263E700/0x14263EB10/"
            "0x14263F070 are the source-image/block-accessor variants."
        ),
        "native_final_material_writer": (
            "0x14263DDB0 writes final 16-bit coverage plus BGR bytes: candidate "
            "coverage is min(0x40000000, sampledCoverage * opacityCap(ctx+0x1e0)) >> 15; "
            "ctx+0x1dc selects direct/max versus build-up color blending."
        ),
        "native_row_accumulator_variants": (
            "0x14263C060 resolves brush profile/mask/texture coverage then calls "
            "0x14263DDB0; 0x14263C3A0 resolves dynamic material color through "
            "0x142637A70/0x142637C70 before the same final writer; 0x14263C5C0 is "
            "coverage-only. 0x1426388D0/0x142638F10 read source coverage/BGR from the "
            "ctx+0x50 temp-buffer span and blend it with brush flow/color weights."
        ),
        "native_texture_coverage_modifier": (
            "0x142664760 is the texture/coverage modifier used by row writers: it samples "
            "a wrapped, rotated 8-bit texture (0x142664550 is the bilinear path), optional "
            "invert/remap/density scaling converts it to 0..0x8000, then mode ctx+0x48 "
            "selects one of ten coverage blend formulas before clamping."
        ),
        "native_texture_descriptor_source": (
            "0x1422DC680 copies the 72-byte BrushStyle texture descriptor from runtime "
            "style+0x2b0 when style+0x2c0 bit0 is set. 0x14255D810 stores it in "
            "writer-state +0x88..+0xc8, 0x14260F8B0 passes it to 0x142664C00, and "
            "0x142644180 enables the row modifier."
        ),
        "native_texture_descriptor_sqlite_map": (
            "The 72-byte descriptor matches BrushStyle texture columns: resource pair "
            "+0/+8 from TexturePattern, TextureFlag at +0x10, TextureComposite at +0x14, "
            "TextureScale/+Rotate/+OffsetX/+OffsetY at +0x18/+0x20/+0x28/+0x30, and "
            "TextureBrightness/+Contrast at +0x38/+0x40. Flag 0x200 enables the remap "
            "path using whole-texture average helper 0x1426642D0."
        ),
        "native_texture_composite_modes": (
            "TextureComposite feeds 0x142664760 mode ctx+0x48. Modes 0..9 apply distinct "
            "coverage formulas using input coverage, sampled texture coverage, baseline, "
            "and density fields; 0x142664260 stores density at ctx+0x4c and scales it by "
            "10 for remap or mode 9. Current samples only exercise mode 0."
        ),
        "native_texture_density_effector_path": (
            "0x1422D8550 writes plot +72 from runtime style+0x2f8/+0x300 "
            "(TextureDensityBase/Effector area) only when texture flags include both "
            "0x1 and 0x10; queue +0x170 then becomes 0x14263F410 a11 and is passed to "
            "0x142664260(textureCtx, density, flowFixed). Current 0x101/0x201 samples "
            "do not set 0x10."
        ),
        "native_boundary": (
            "Exact rendering should use the resolved material-image selector/storage pair "
            "initialized by native 0x1432BDA20, not TextureFlag alone."
        ),
    }


def _rgb16_at(blob: bytes, offsets: tuple[int, int, int]) -> list[int]:
    return [
        max(0, min(int(struct.unpack_from(">H", blob, off)[0] // 257), 255))
        for off in offsets
    ]


def _flag_bits(value: int) -> dict[str, Any]:
    return {
        "value": int(value),
        "bit0_line_lens": bool(value & 1),
        "bit1_fill_lens": bool(value & 2),
        "bit2_special_lens": bool(value & 4),
        "low_nibble": int(value) & 0xF,
    }


def _compact_semantic_probe(
    blob: bytes,
    off: int,
    header_len: int,
    *,
    object_kind: str,
) -> dict[str, Any]:
    """Expose known compact fields and native draw-flag candidate lenses.

    This intentionally does not claim compact SQLite headers are native
    CSRulerObject records. It just makes one-setting sample diffs easy to read.
    """
    probe: dict[str, Any] = {"object_kind": object_kind}
    candidate_offsets = [20, 72, 76] if header_len >= 100 else [20]
    candidates: dict[str, Any] = {}
    for rel in candidate_offsets:
        if rel + 4 <= header_len and off + rel + 4 <= len(blob):
            candidates[str(rel)] = _flag_bits(struct.unpack_from(">I", blob, off + rel)[0])
    if candidates:
        probe["native_draw_flag_candidate_lens"] = candidates

    if header_len >= 92 and off + 92 <= len(blob):
        try:
            opacity = struct.unpack_from(">d", blob, off + 64)[0]
            alpha = int(round(max(0.0, min(float(opacity), 1.0)) * 255)) if math.isfinite(opacity) else None
            line_rgb = _rgb16_at(blob, (off + 40, off + 44, off + 48))
            fill_rgb = _rgb16_at(blob, (off + 52, off + 56, off + 60))
            probe["argb_candidates_from_compact_color"] = {
                "line": None if alpha is None else [alpha, *line_rgb],
                "fill": None if alpha is None else [alpha, *fill_rgb],
                "opacity_alpha": alpha,
            }
        except struct.error:
            pass
    if header_len >= 100 and off + 100 <= len(blob):
        try:
            probe["known_100_fields"] = {
                "point_count": int(struct.unpack_from(">I", blob, off + 16)[0]),
                "unknown_u32_20": int(struct.unpack_from(">I", blob, off + 20)[0]),
                "unknown_u32_72": int(struct.unpack_from(">I", blob, off + 72)[0]),
                "family_u32_76": int(struct.unpack_from(">I", blob, off + 76)[0]),
                "brush_style_id": int(struct.unpack_from(">I", blob, off + 80)[0]),
                "subtype": int(struct.unpack_from(">I", blob, off + 84)[0]),
                "width": round(float(struct.unpack_from(">d", blob, off + 88)[0]), 6),
                "object_id_like_u32_96": int(struct.unpack_from(">I", blob, off + 96)[0]),
            }
        except struct.error:
            pass
    return probe


def _compact_native_reader_probe(blob: bytes, off: int, header_len: int) -> dict[str, Any]:
    """Name compact record fields according to CLIPStudioPaint.exe's reader.

    The common reader at 0x1422cf5f0 consumes the first 76 bytes, then dispatches
    to the PWVector* subclass reader at the offset stored in u32+4.
    """
    try:
        object_header_len, subclass_tail_offset, point_stride, point_tail_offset = struct.unpack_from(
            ">IIII", blob, off
        )
        point_count, object_flags_or_type = struct.unpack_from(">II", blob, off + 16)
        bbox = struct.unpack_from(">IIII", blob, off + 24)
        line_triplet = struct.unpack_from(">III", blob, off + 40)
        fill_triplet = struct.unpack_from(">III", blob, off + 52)
        opacity = struct.unpack_from(">d", blob, off + 64)[0]
    except struct.error:
        return {"valid": False}

    probe: dict[str, Any] = {
        "valid": True,
        "record_control": {
            "object_header_len": int(object_header_len),
            "subclass_tail_offset": int(subclass_tail_offset),
            "point_record_stride": int(point_stride),
            "point_record_tail_offset": int(point_tail_offset),
            "point_count": int(point_count),
            "object_flags_or_type": int(object_flags_or_type),
        },
        "common_object": {
            "bbox": [int(v) for v in bbox],
            "line_color_triplet_u32": [int(v) for v in line_triplet],
            "line_rgb": _rgb16_at(blob, (off + 40, off + 44, off + 48)),
            "fill_color_triplet_u32": [int(v) for v in fill_triplet],
            "fill_rgb": _rgb16_at(blob, (off + 52, off + 56, off + 60)),
            "opacity": None if not math.isfinite(opacity) else round(float(opacity), 6),
        },
        "point_payload": {
            "start": int(off + object_header_len),
            "end": int(off + object_header_len + point_stride * point_count),
        },
    }
    if subclass_tail_offset > 72 and off + 76 <= len(blob):
        probe["common_object"]["extra_u32_72"] = int(struct.unpack_from(">I", blob, off + 72)[0])

    if header_len == 92 and off + 92 <= len(blob):
        width = struct.unpack_from(">d", blob, off + 80)[0]
        probe["subclass_tail"] = {
            "class": "PWVectorStroke",
            "list_index_u32_76": int(struct.unpack_from(">I", blob, off + 76)[0]),
            "width_or_scale_f64_80": None if not math.isfinite(width) else round(float(width), 6),
            "extra_u32_88": int(struct.unpack_from(">I", blob, off + 88)[0]),
        }
    elif header_len == 100 and off + 100 <= len(blob):
        family_id = int(struct.unpack_from(">I", blob, off + 76)[0])
        width = struct.unpack_from(">d", blob, off + 88)[0]
        probe["subclass_tail"] = {
            "class": "PWVectorBalloon",
            "family_u32_76": family_id,
            "family_flags": _decode_balloon_family_flags(family_id),
            "list1_index_u32_80": int(struct.unpack_from(">I", blob, off + 80)[0]),
            "list2_index_u32_84": int(struct.unpack_from(">I", blob, off + 84)[0]),
            "width_or_scale_f64_88": None if not math.isfinite(width) else round(float(width), 6),
            "extra_u32_96": int(struct.unpack_from(">I", blob, off + 96)[0]),
        }
    return probe


def _external_header_probe(blob: bytes) -> dict[str, Any] | None:
    """Parse the 56-byte Exta wrapper seen before compact vector records."""
    if len(blob) < 56:
        return None
    try:
        header_len = struct.unpack_from(">Q", blob, 0)[0]
        tag = blob[8:16].decode("ascii")
        identifier = blob[16:48].decode("ascii")
        payload_size = struct.unpack_from(">Q", blob, 48)[0]
    except (struct.error, UnicodeDecodeError):
        return None
    if header_len != 40 or tag != "extrnlid":
        return None
    payload_offset = 56
    payload_end = payload_offset + int(payload_size)
    return {
        "header_len": int(header_len),
        "tag": tag,
        "identifier": identifier,
        "external_id": f"{tag}{identifier}",
        "payload_offset": payload_offset,
        "payload_size": int(payload_size),
        "payload_end": payload_end,
        "payload_size_matches_body": payload_end == len(blob),
    }


def _compact_record_scan_start(blob: bytes) -> int:
    header = _external_header_probe(blob)
    if header is None:
        return 0
    return int(header["payload_offset"])


def _compact_record_spans(blob: bytes) -> list[dict[str, Any]]:
    spans: list[dict[str, Any]] = []
    for off in range(_compact_record_scan_start(blob), len(blob) - 16, 4):
        try:
            header_len, point_header_len, stride_a, stride_b = struct.unpack_from(
                ">IIII", blob, off
            )
            point_count = struct.unpack_from(">I", blob, off + 16)[0]
        except struct.error:
            continue
        shape = [int(header_len), int(point_header_len), int(stride_a), int(stride_b)]
        kind = None
        if header_len == 100 and point_header_len == 76 and stride_b == 88 and stride_a in (88, 104):
            kind = "object_100"
        elif header_len == 92 and point_header_len == 76 and stride_b == 88 and stride_a in (88, 120):
            kind = "stroke_92"
        if kind is None or not (1 <= point_count <= 5000):
            continue
        end = off + header_len + stride_a * point_count
        if end > len(blob):
            continue
        spans.append(
            {
                "kind": kind,
                "offset": int(off),
                "end": int(end),
                "shape": shape,
                "point_count": int(point_count),
            }
        )
    spans.sort(key=lambda item: item["offset"])
    deduped: list[dict[str, Any]] = []
    last_end = -1
    for span in spans:
        if span["offset"] < last_end:
            continue
        deduped.append(span)
        last_end = span["end"]
    return deduped


def _header_probe(blob: bytes, off: int, header_len: int) -> dict[str, Any]:
    """Dump stable raw header fields so sample sweeps can reveal unknown flags."""
    u32_offsets = [
        rel
        for rel in range(0, min(header_len, 104), 4)
        if rel + 4 <= header_len and off + rel + 4 <= len(blob)
    ]
    f32_offsets = [
        rel
        for rel in range(0, min(header_len, 104), 4)
        if rel + 4 <= header_len and off + rel + 4 <= len(blob)
    ]
    f64_offsets = [
        rel
        for rel in range(0, min(header_len, 104), 8)
        if rel + 8 <= header_len and off + rel + 8 <= len(blob)
    ]
    u32 = {str(rel): int(struct.unpack_from(">I", blob, off + rel)[0]) for rel in u32_offsets}
    f32 = {}
    for rel in f32_offsets:
        value = struct.unpack_from(">f", blob, off + rel)[0]
        if math.isfinite(value) and abs(value) < 1e6:
            f32[str(rel)] = round(float(value), 6)
    f64 = {}
    for rel in f64_offsets:
        value = struct.unpack_from(">d", blob, off + rel)[0]
        if math.isfinite(value) and abs(value) < 1e9:
            f64[str(rel)] = round(float(value), 6)
    return {
        "hex": blob[off : min(off + header_len, len(blob))].hex(),
        "u32_be": u32,
        "f32_be": f32,
        "f64_be": f64,
    }


def _point_summary(body: bytes, start: int, stride: int, count: int) -> dict[str, Any]:
    fields = {rel: [] for rel in POINT_FLOAT_FIELDS if rel + 4 <= stride}
    point_u32_fields: dict[int, collections.Counter[int]] = {
        rel: collections.Counter() for rel in POINT_U32_FIELDS if rel + 4 <= stride
    }
    native_u32_fields: dict[int, collections.Counter[int]] = {
        rel: collections.Counter() for rel in (16, 20, 24, 28) if rel + 4 <= stride
    }
    cubic_f64_fields = {rel: [] for rel in (0, 8, 16, 24, 32, 40) if stride >= 64 and rel + 8 <= stride}
    compact120_tail_f64_fields = {
        rel: [] for rel in (88, 96, 104, 112) if stride >= 120 and rel + 8 <= stride
    }
    cubic_u32_fields: dict[int, collections.Counter[int]] = {
        rel: collections.Counter() for rel in (48, 52, 56, 60) if stride >= 64 and rel + 4 <= stride
    }
    bbox_widths: list[int] = []
    bbox_heights: list[int] = []
    xs: list[float] = []
    ys: list[float] = []
    first_points: list[dict[str, Any]] = []
    valid = True
    for idx in range(count):
        point_off = start + idx * stride
        if point_off + stride > len(body):
            valid = False
            break
        try:
            x = struct.unpack_from(">d", body, point_off)[0]
            y = struct.unpack_from(">d", body, point_off + 8)[0]
        except struct.error:
            valid = False
            break
        if not (math.isfinite(x) and math.isfinite(y)):
            valid = False
            break
        xs.append(float(x))
        ys.append(float(y))
        point_info: dict[str, Any] = {
            "x": round(float(x), 6),
            "y": round(float(y), 6),
        }
        if point_off + 32 <= len(body):
            bx0, by0, bx1, by1 = struct.unpack_from(">IIII", body, point_off + 16)
            bbox_widths.append(int(bx1) - int(bx0))
            bbox_heights.append(int(by1) - int(by0))
            point_info["pen_bbox"] = [int(bx0), int(by0), int(bx1), int(by1)]
            point_info["native32_probe"] = {
                "vertex_select_id_16": int(bx0),
                "line_select_id_20": int(by0),
                "is_vertex_selected_24": int(bx1),
                "unused_or_extra_28": int(by1),
            }
        for rel in fields:
            try:
                fields[rel].append(struct.unpack_from(">f", body, point_off + rel)[0])
            except struct.error:
                pass
        for rel in point_u32_fields:
            try:
                point_u32_fields[rel][int(struct.unpack_from(">I", body, point_off + rel)[0])] += 1
            except struct.error:
                pass
        for rel in native_u32_fields:
            try:
                native_u32_fields[rel][int(struct.unpack_from(">I", body, point_off + rel)[0])] += 1
            except struct.error:
                pass
        for rel in cubic_f64_fields:
            try:
                cubic_f64_fields[rel].append(struct.unpack_from(">d", body, point_off + rel)[0])
            except struct.error:
                pass
        for rel in compact120_tail_f64_fields:
            try:
                compact120_tail_f64_fields[rel].append(struct.unpack_from(">d", body, point_off + rel)[0])
            except struct.error:
                pass
        for rel in cubic_u32_fields:
            try:
                cubic_u32_fields[rel][int(struct.unpack_from(">I", body, point_off + rel)[0])] += 1
            except struct.error:
                pass
        if idx < 4:
            point_info["float_fields"] = {
                str(rel): round(float(struct.unpack_from(">f", body, point_off + rel)[0]), 6)
                for rel in fields
            }
            if stride >= 80 and point_off + 80 <= len(body):
                f32_fields = {
                    str(rel): round(float(struct.unpack_from(">f", body, point_off + rel)[0]), 6)
                    for rel in (36, 40, 44, 48, 52, 56, 60, 64, 68, 72, 76)
                    if rel + 4 <= stride
                }
                point_info["compact_point_reader_probe"] = {
                    "x_f64_0": round(float(x), 6),
                    "y_f64_8": round(float(y), 6),
                    "u32_16_28": {
                        str(rel): int(struct.unpack_from(">I", body, point_off + rel)[0])
                        for rel in (16, 20, 24, 28)
                    },
                    "u32_32": int(struct.unpack_from(">I", body, point_off + 32)[0]),
                    "f32_36_76": f32_fields,
                    "writer_grouped_f32": {
                        "source_double_derived_36_60": {
                            str(rel): f32_fields[str(rel)]
                            for rel in (36, 40, 44, 48, 52, 56, 60)
                            if str(rel) in f32_fields
                        },
                        "native_channel_names_36_60": {
                            "36": "sample_context_0x10_primary_scalar",
                            "40": "sample_context_0x18_angle_over_90",
                            "44": "sample_context_0x20_aux_scalar",
                            "48": "sample_context_0x28_angle_like",
                            "52": "sample_context_0x30_velocity_factor",
                            "56": "sample_context_0x38_size_factor",
                            "60": "sample_context_0x40_flow_factor",
                        },
                        "other_serialized_f32_64_76": {
                            str(rel): f32_fields[str(rel)]
                            for rel in (64, 68, 72, 76)
                            if str(rel) in f32_fields
                        },
                        "source_flags_bit0_bit1_gate_internal_flags_0x1000_0x2000": True,
                    },
                    "native_sampler_record_0x1422CC1E0": {
                        "record_xy_0_8": "compact point f64+0/+8",
                        "record_dynamic_channels_16_64": {
                            "16": "compact f32+36, primary scalar",
                            "24": "compact f32+40, angle/tilt-derived scalar",
                            "32": "compact f32+44, auxiliary scalar",
                            "40": "compact f32+48, angle-like value interpolated by wrapped 0..360 helper 0x14206D6E0",
                            "48": "compact f32+52, velocity/direction factor",
                            "56": "compact f32+56, size-like factor; point flag 0x1000 gates fixed-vs-interpolated behavior",
                            "64": "compact f32+60, flow/opacity-like factor; point flag 0x2000 gates fixed-vs-interpolated behavior",
                        },
                        "record_metric_72": "path/neighbor metric from 0x1422CA0D0 -> 0x141A8DD80 when requested",
                        "record_flags_80": "point/list flags derived from internal 0x1000/0x2000",
                        "sampler_step_source": "compact f32+64/+68/+72 appear in the adjacent segment-step/tangent path, not as submitted dynamic channels",
                    },
                    "u32_80_84": {
                        str(rel): int(struct.unpack_from(">I", body, point_off + rel)[0])
                        for rel in (80, 84)
                        if rel + 4 <= stride
                    },
                }
                if stride >= 104:
                    tail_offsets = (88, 96) if stride < 120 else (88, 96, 104, 112)
                    point_info["compact_point_reader_probe"]["subclass_tail_f64"] = {
                        str(rel): round(float(struct.unpack_from(">d", body, point_off + rel)[0]), 6)
                        for rel in tail_offsets
                        if rel + 8 <= stride
                    }
                    if stride >= 120:
                        point_info["compact_point_reader_probe"]["compact120_curve_tail_candidate"] = {
                            "in_or_prev_control_xy_88_96": [
                                round(float(struct.unpack_from(">d", body, point_off + 88)[0]), 6),
                                round(float(struct.unpack_from(">d", body, point_off + 96)[0]), 6),
                            ],
                            "out_or_next_control_xy_104_112": [
                                round(float(struct.unpack_from(">d", body, point_off + 104)[0]), 6),
                                round(float(struct.unpack_from(">d", body, point_off + 112)[0]), 6),
                            ],
                            "renderer_status": (
                                "pixel_affecting for compact 120-byte dark curve preview: "
                                "the importer uses current point +104/+112 as the second cubic control "
                                "with the first control fixed at the current point"
                            ),
                        }
            if stride >= 64 and point_off + 64 <= len(body):
                cubic_f64 = []
                for rel in (0, 8, 16, 24, 32, 40):
                    value = struct.unpack_from(">d", body, point_off + rel)[0]
                    cubic_f64.append(None if not math.isfinite(value) else round(float(value), 6))
                cubic_u32 = {
                    str(rel): int(struct.unpack_from(">I", body, point_off + rel)[0])
                    for rel in (48, 52, 56, 60)
                }
                point_info["cubic64_probe"] = {
                    "f64_0_40": cubic_f64,
                    "u32_48_60": cubic_u32,
                }
            first_points.append(point_info)
    if not valid:
        return {"valid": False}
    point_bbox = None
    if xs and ys:
        point_bbox = [
            int(math.floor(min(xs))),
            int(math.floor(min(ys))),
            int(math.ceil(max(xs))),
            int(math.ceil(max(ys))),
        ]
    return {
        "valid": True,
        "point_bbox": point_bbox,
        "xy": {"x": _stats(xs), "y": _stats(ys)},
        "float_fields": {str(rel): _stats(vals) for rel, vals in fields.items()},
        "point_u32_stats": {
            str(rel): [
                {"value": value, "count": count}
                for value, count in counter.most_common(12)
            ]
            for rel, counter in point_u32_fields.items()
        },
        "native32_stats": {
            "u32": {
                str(rel): [
                    {"value": value, "count": count}
                    for value, count in counter.most_common(8)
                ]
                for rel, counter in native_u32_fields.items()
            },
        },
        "cubic64_stats": {
            "f64": {str(rel): _stats(vals) for rel, vals in cubic_f64_fields.items()},
            "u32": {
                str(rel): [
                    {"value": value, "count": count}
                    for value, count in counter.most_common(8)
                ]
                for rel, counter in cubic_u32_fields.items()
            },
        },
        "compact120_tail_f64_stats": {
            str(rel): _stats(vals) for rel, vals in compact120_tail_f64_fields.items()
        },
        "pen_bbox_width": _stats([float(v) for v in bbox_widths]),
        "pen_bbox_height": _stats([float(v) for v in bbox_heights]),
        "first_points": first_points,
    }


def _graph_points(row) -> dict[str, Any]:
    points = []
    blob = row["ControlPoints"]
    stride = int(row["ControlDataSize"] or 0)
    count = int(row["ControlNumber"] or 0)
    if isinstance(blob, bytes) and stride >= 16:
        for idx in range(min(count, len(blob) // stride)):
            off = idx * stride
            x, y = struct.unpack_from(">dd", blob, off)
            points.append([round(float(x), 6), round(float(y), 6)])
    return {
        "control_number": count,
        "control_data_size": stride,
        "points": points,
        "native_eval": _graph_eval_diagnostics(points),
    }


def _graph_eval_diagnostics(points: list[list[float]]) -> dict[str, Any]:
    pts = [(float(point[0]), float(point[1])) for point in points if len(point) >= 2]
    pts.sort(key=lambda item: item[0])
    if len(pts) <= 1:
        return {"rule": "constant_or_empty", "max_delta_vs_linear": 0.0}
    rule = "linear" if len(pts) == 2 else "midpoint_quadratic"
    max_delta = 0.0
    max_x = 0.0
    max_native = 0.0
    max_linear = 0.0
    sample_xs = {0.0, 1.0}
    sample_xs.update(point[0] for point in pts)
    for idx in range(65):
        sample_xs.add(idx / 64.0)
    for x in sorted(sample_xs):
        native = _eval_graph_native(pts, x)
        linear = _eval_graph_linear(pts, x)
        delta = abs(native - linear)
        if delta > max_delta:
            max_delta = delta
            max_x = x
            max_native = native
            max_linear = linear
    return {
        "rule": rule,
        "max_delta_vs_linear": round(max_delta, 6),
        "max_delta_x": round(max_x, 6),
        "native_at_max": round(max_native, 6),
        "linear_at_max": round(max_linear, 6),
    }


def _eval_graph_linear(points: list[tuple[float, float]], x: float) -> float:
    x = max(0.0, min(float(x), 1.0))
    if x <= points[0][0]:
        return points[0][1]
    for a, b in zip(points, points[1:]):
        if x <= b[0]:
            return _eval_graph_line(a, b, x)
    return points[-1][1]


def _eval_graph_native(points: list[tuple[float, float]], x: float) -> float:
    x = max(0.0, min(float(x), 1.0))
    if len(points) <= 1:
        return points[0][1] if points else 0.0
    if len(points) == 2:
        return _eval_graph_line(points[0], points[1], x)
    if len(points) == 3:
        return _eval_graph_quad(points[0], points[1], points[2], x)
    for idx in range(1, len(points) - 1):
        start = points[0] if idx == 1 else _graph_midpoint(points[idx - 1], points[idx])
        end = points[-1] if idx == len(points) - 2 else _graph_midpoint(points[idx], points[idx + 1])
        if x <= end[0] or idx == len(points) - 2:
            return _eval_graph_quad(start, points[idx], end, x)
    return points[-1][1]


def _graph_midpoint(a: tuple[float, float], b: tuple[float, float]) -> tuple[float, float]:
    return ((a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5)


def _eval_graph_line(a: tuple[float, float], b: tuple[float, float], x: float) -> float:
    span = b[0] - a[0]
    if abs(span) <= 1e-9:
        return a[1]
    return a[1] + (b[1] - a[1]) * ((x - a[0]) / span)


def _eval_graph_quad(
    a: tuple[float, float],
    b: tuple[float, float],
    c: tuple[float, float],
    x: float,
) -> float:
    qa = a[0] - 2.0 * b[0] + c[0]
    qb = 2.0 * (b[0] - a[0])
    qc = a[0] - x
    roots: list[float] = []
    if abs(qa) <= 1e-9:
        if abs(qb) > 1e-9:
            roots.append(-qc / qb)
    else:
        disc = qb * qb - 4.0 * qa * qc
        if disc >= -1e-9:
            if disc <= 1e-9:
                roots.append(-qb / (2.0 * qa))
            else:
                sqrt_disc = math.sqrt(max(0.0, disc))
                roots.append((-qb + sqrt_disc) / (2.0 * qa))
                roots.append((-qb - sqrt_disc) / (2.0 * qa))
    for t in roots:
        if 0.0 < t < 1.0:
            omt = 1.0 - t
            return omt * omt * a[1] + 2.0 * omt * t * b[1] + t * t * c[1]
    return _eval_graph_line(a, c, x)


def _stats(values: list[float]) -> dict[str, Any] | None:
    finite = [float(v) for v in values if math.isfinite(v) and abs(v) < 1e6]
    if not finite:
        return None
    rounded = {round(v, 6) for v in finite}
    return {
        "min": round(min(finite), 6),
        "max": round(max(finite), 6),
        "avg": round(sum(finite) / len(finite), 6),
        "uniq": len(rounded),
    }


def _brush_style_summary(clip, style_id: int) -> dict[str, Any] | None:
    cols = {row["name"] for row in clip._db.execute("PRAGMA table_info(BrushStyle)")}
    select_cols = [key for key in BRUSH_STYLE_KEYS if key in cols]
    if not select_cols:
        return None
    row = clip._db.execute(
        f"SELECT {', '.join(select_cols)} FROM BrushStyle WHERE MainId=?",
        (int(style_id),),
    ).fetchone()
    if row is None:
        return None
    out: dict[str, Any] = {}
    for key in row.keys():
        value = row[key]
        if key in EFFECTOR_KEYS and isinstance(value, bytes):
            out[key] = _effector_words(value)
        elif key == "StyleFlag":
            out[key] = _brush_style_flag_bits(value)
        elif key == "TextureFlag":
            out[key] = _clean_value(value)
            out["TextureFlagDecoded"] = _texture_flag_bits(value)
        else:
            out[key] = _clean_value(value)
    out["importer_renderer_status"] = _brush_style_renderer_status(out)
    return out


def _fill_style_summary(clip, style_id: int) -> dict[str, Any] | None:
    table = clip._db.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='FillStyle'"
    ).fetchone()
    if table is None:
        return None
    cols = {row["name"] for row in clip._db.execute("PRAGMA table_info(FillStyle)")}
    select_cols = [key for key in FILL_STYLE_KEYS if key in cols]
    if not select_cols:
        return None
    row = clip._db.execute(
        f"SELECT {', '.join(select_cols)} FROM FillStyle WHERE MainId=?",
        (int(style_id),),
    ).fetchone()
    if row is None:
        return None
    out = {key: _clean_value(row[key]) for key in row.keys()}
    out["importer_renderer_status"] = _fill_style_renderer_status(out)
    return out


def _effector_form(summary: Any) -> str | None:
    if not isinstance(summary, dict):
        return None
    decoded = summary.get("decoded")
    return decoded.get("form") if isinstance(decoded, dict) else None


def _effector_is_nondefault(summary: Any) -> bool:
    form = _effector_form(summary)
    if form is None:
        return bool(summary not in (None, 0, 1))
    return form not in {"default_or_off_0x01", "zero_or_disabled_0x00"}


def _brush_style_renderer_status(style: dict[str, Any]) -> dict[str, Any]:
    pixel_inputs: list[str] = []
    preview_only: list[str] = []
    diagnostic_only: list[str] = []

    if style.get("AntiAlias") not in (None, 0):
        pixel_inputs.append("AntiAlias ordinal -> fallback feather")

    size_form = _effector_form(style.get("SizeEffector"))
    if size_form == "dual_graph_0x31":
        pixel_inputs.append("SizeEffector dual_graph_0x31 -> pressure-size preview")
    elif size_form == "primary_graph_0x11":
        preview_only.append("SizeEffector primary_graph_0x11 -> simple-size branch selector/taper heuristic")
    elif size_form == "random_floor_0x81":
        pixel_inputs.append("SizeEffector random_floor_0x81 -> compact u32+80 native-LCG size-random preview")
    elif _effector_is_nondefault(style.get("SizeEffector")):
        diagnostic_only.append(f"SizeEffector {size_form or 'nondefault'}")

    opacity_form = _effector_form(style.get("OpacityEffector"))
    if opacity_form == "dual_graph_0x31":
        pixel_inputs.append("OpacityEffector dual_graph_0x31 -> pressure-opacity preview")
    elif opacity_form == "random_floor_0x81":
        pixel_inputs.append("OpacityEffector random_floor_0x81 -> stable deterministic opacity-random preview")
    elif _effector_is_nondefault(style.get("OpacityEffector")):
        diagnostic_only.append(f"OpacityEffector {opacity_form or 'nondefault'}")

    interval_base = style.get("IntervalBase")
    try:
        interval_base_f = float(interval_base)
    except (TypeError, ValueError):
        interval_base_f = None
    interval_effector_nondefault = _effector_is_nondefault(style.get("IntervalEffector"))
    auto_interval = style.get("AutoIntervalType")
    if (
        interval_base_f is not None
        and interval_base_f > 0.1
        and not interval_effector_nondefault
        and auto_interval in (None, 0)
        and style.get("PatternStyle") in (None, 0)
        and style.get("TexturePattern") in (None, 0)
    ):
        pixel_inputs.append("IntervalBase > 0.1 -> ordinary no-pattern radius preview")
    elif (
        auto_interval in (1, 2, 3)
        and int(style.get("AntiAlias") or 0) > 0
        and not interval_effector_nondefault
        and style.get("PatternStyle") in (None, 0)
        and style.get("TexturePattern") in (None, 0)
    ):
        pixel_inputs.append("AutoIntervalType 1/2/3 + AA>0 -> ordinary no-pattern radius/feather preview")
    elif (
        (interval_base_f is not None and interval_base_f != 0.1)
        or interval_effector_nondefault
        or auto_interval not in (None, 0)
    ):
        diagnostic_only.append("IntervalBase/IntervalEffector/AutoIntervalType")

    thickness_base = style.get("ThicknessBase")
    try:
        thickness_base_f = float(thickness_base)
    except (TypeError, ValueError):
        thickness_base_f = None
    if (
        thickness_base_f is not None
        and 0.0 < thickness_base_f < 1.0
        and style.get("PatternStyle") in (None, 0)
        and style.get("TexturePattern") in (None, 0)
    ):
        rotation_base = style.get("RotationBase")
        try:
            rotation_base_f = float(rotation_base)
        except (TypeError, ValueError):
            rotation_base_f = 0.0
        rotation_from_horizontal = abs(((rotation_base_f + 90.0) % 180.0) - 90.0)
        if rotation_from_horizontal > 1.0:
            pixel_inputs.append("ThicknessBase < 1 + RotationBase -> ordinary no-pattern rotated ellipse preview")
        else:
            pixel_inputs.append("ThicknessBase < 1 -> ordinary no-pattern stroke radius preview")
    elif thickness_base_f is not None and thickness_base_f != 1.0:
        diagnostic_only.append("ThicknessBase")

    hardness = style.get("Hardness")
    try:
        hardness_f = float(hardness)
    except (TypeError, ValueError):
        hardness_f = None
    if (
        hardness_f is not None
        and 0.0 < hardness_f < 1.0
        and style.get("PatternStyle") in (None, 0)
        and style.get("TexturePattern") in (None, 0)
    ):
        pixel_inputs.append("Hardness < 1 -> ordinary no-pattern capsule softness preview")
    elif hardness_f is not None and hardness_f != 1.0:
        diagnostic_only.append("Hardness")

    pattern_style = style.get("PatternStyle")
    if pattern_style not in (None, 0):
        pixel_inputs.append("PatternStyle -> ordinary stroke material stamp preview")
        diagnostic_only.append("PatternStyle/material list native-mapped; exact retained pattern rendering not implemented")
    texture_pattern = style.get("TexturePattern")
    texture_preview_supported = False
    if texture_pattern not in (None, 0):
        texture_preview_supported = (
            style.get("PatternStyle") in (None, 0)
            and style.get("TextureComposite") in (None, 0)
            and float(style.get("TextureScale") or 1.0) == 1.0
            and float(style.get("TextureRotate") or 0.0) == 0.0
            and float(style.get("TextureOffsetX") or 0.0) == 0.0
            and float(style.get("TextureOffsetY") or 0.0) == 0.0
            and float(style.get("TextureBrightness") or 0.0) == 0.0
            and float(style.get("TextureContrast") or 0.0) == 0.0
        )
    if texture_preview_supported:
        pixel_inputs.append("TexturePattern -> ordinary stroke single-channel texture alpha preview")
    elif texture_pattern not in (None, 0):
        diagnostic_only.append("TexturePattern/TextureFlag native-mapped; texture/material row path not implemented")
    flow_base = style.get("FlowBase")
    try:
        flow_base_f = float(flow_base)
    except (TypeError, ValueError):
        flow_base_f = None
    flow_effector_nondefault = _effector_is_nondefault(style.get("FlowEffector"))
    flow_form = _effector_form(style.get("FlowEffector"))
    if (
        flow_base_f is not None
        and 0.0 < flow_base_f < 1.0
        and not flow_effector_nondefault
        and style.get("PatternStyle") in (None, 0)
        and style.get("TexturePattern") in (None, 0)
    ):
        pixel_inputs.append("FlowBase < 1 -> ordinary no-pattern radius preview")
    elif (
        flow_form in {"primary_graph_0x11", "aux_graph_0x41"}
        and isinstance(style.get("FlowEffector"), dict)
        and abs(float(style["FlowEffector"].get("decoded", {}).get("floor", -1.0)) - 0.5) <= 1e-6
        and style["FlowEffector"].get("decoded", {}).get("graph_refs") == [4]
        and style.get("PatternStyle") in (None, 0)
        and style.get("TexturePattern") in (None, 0)
    ):
        pixel_inputs.append("FlowEffector 0x11/0x41 floor 0.5 graph 4 -> ordinary no-pattern dynamic radius preview")
    elif (flow_base_f is not None and flow_base_f != 1.0) or flow_effector_nondefault:
        diagnostic_only.append("FlowBase/FlowEffector")
    if _effector_is_nondefault(style.get("ThicknessEffector")):
        diagnostic_only.append("ThicknessEffector")

    for key in (
        "RotationEffector",
        "TextureDensityEffector",
        "MixColorEffector",
        "MixAlphaEffector",
        "BlurEffector",
        "SubColorEffector",
        "HueChangeEffector",
        "SaturationChangeEffector",
        "ValueChangeEffector",
    ):
        if _effector_is_nondefault(style.get(key)):
            diagnostic_only.append(key)
    if style.get("SprayFlag") not in (None, 0):
        for key in ("RotationEffectorInSpray", "SpraySizeEffector", "SprayDensityEffector"):
            if _effector_is_nondefault(style.get(key)):
                diagnostic_only.append(key)

    for key, default in (
        ("UseWaterColor", 0),
        ("WaterColorType", 0),
        ("BrushColorMixingMode", 0),
        ("BrushLMSLinearity", 0),
        ("SprayFlag", 0),
        ("FixedSpray", 0),
        ("TextureComposite", 0),
        ("WaterEdgeFlag", 0),
        ("BlurKind", 0),
        ("ChangeDrawColorTarget", 1),
    ):
        value = style.get(key)
        if value not in (None, default):
            diagnostic_only.append(key)

    style_flag = style.get("StyleFlag")
    if isinstance(style_flag, dict):
        flag_notes = []
        for name in (
            "native_retained_state_path_0x20",
            "direct_max_accum_0x1000",
            "texture_or_water_family_0x10000",
            "texture_or_water_family_0x20000",
        ):
            if style_flag.get(name):
                flag_notes.append(name)
        if flag_notes:
            diagnostic_only.append("StyleFlag " + ",".join(flag_notes))

    if pixel_inputs and diagnostic_only:
        coverage = "partial_preview"
    elif pixel_inputs or preview_only:
        coverage = "preview_rendered"
    elif diagnostic_only:
        coverage = "native_mapped_not_rendered"
    else:
        coverage = "default_or_not_vector_relevant"

    return {
        "coverage": coverage,
        "pixel_affecting_inputs": pixel_inputs,
        "preview_only_inputs": preview_only,
        "diagnostic_only_inputs": sorted(set(diagnostic_only)),
    }


def _fill_style_renderer_status(style: dict[str, Any]) -> dict[str, Any]:
    diagnostic_only = []
    for key in ("StyleFlag", "AntiAlias", "CompositeMode", "TextureDensity"):
        value = style.get(key)
        if value not in (None, 0, 1, 1.0):
            diagnostic_only.append(key)
    return {
        "coverage": "context_only",
        "pixel_affecting_inputs": [],
        "preview_only_inputs": [
            "fill_style_id participates in object-family routing, but FillStyle fields are not exact fill rendering yet",
        ],
        "diagnostic_only_inputs": diagnostic_only,
    }


def _graph_signature(graph: dict[str, Any] | None) -> dict[str, Any] | None:
    if not isinstance(graph, dict):
        return None
    return {
        "control_number": graph.get("control_number"),
        "control_data_size": graph.get("control_data_size"),
        "points": graph.get("points") or [],
        "native_eval": graph.get("native_eval"),
    }


def _style_effector_diagnostics(
    style: dict[str, Any] | None,
    graphs: dict[str, Any],
) -> dict[str, Any]:
    if not isinstance(style, dict):
        return {}
    out: dict[str, Any] = {}
    for key in EFFECTOR_KEYS:
        if key not in style:
            continue
        value = style.get(key)
        if isinstance(value, dict):
            decoded = value.get("decoded") or {}
            form = decoded.get("form")
            include = form not in {"zero_or_disabled_0x00", "default_or_off_0x01"}
            graph_refs = [
                int(graph_id)
                for graph_id in decoded.get("graph_refs", [])
                if isinstance(graph_id, int) and graph_id > 0
            ]
            if not include and not graph_refs:
                continue
            out[key] = {
                "key": _effector_key(value),
                "decoded": decoded,
                "graphs": {
                    str(graph_id): _graph_signature(graphs.get(str(graph_id)))
                    for graph_id in graph_refs
                },
            }
        elif value not in (None, 0, 1):
            out[key] = {"value": value}
    return out


def _pattern_resources(clip, style_ids: set[int]) -> dict[str, Any]:
    resources: dict[str, Any] = {"pattern_styles": {}, "pattern_images": {}}
    tables = {
        row["name"]
        for row in clip._db.execute("SELECT name FROM sqlite_master WHERE type='table'")
    }
    if "BrushPatternStyle" in tables:
        for row in clip._db.execute("SELECT * FROM BrushPatternStyle ORDER BY MainId"):
            resources["pattern_styles"][str(row["MainId"])] = {
                key: _clean_value(row[key]) for key in row.keys()
            }
            image_indices = _u32_be_values(_row_value(row, "ImageIndex"))
            if image_indices:
                resources["pattern_styles"][str(row["MainId"])]["decoded_image_indices"] = image_indices
    if "BrushPatternImage" in tables:
        for row in clip._db.execute("SELECT * FROM BrushPatternImage ORDER BY MainId"):
            mipmap = _row_value(row, "Mipmap", 0)
            resources["pattern_images"][str(row["MainId"])] = {
                key: _clean_value(row[key]) for key in row.keys()
            }
            if mipmap:
                probe = _mipmap_probe(clip, mipmap)
                if probe is not None:
                    resources["pattern_images"][str(row["MainId"])]["mipmap_probe"] = probe
                try:
                    rgba = clip._decode_mipmap_rgba(int(mipmap))
                except Exception:
                    rgba = None
                if rgba is not None:
                    resources["pattern_images"][str(row["MainId"])]["decoded_rgba_shape"] = [
                        int(rgba.shape[1]),
                        int(rgba.shape[0]),
                        int(rgba.shape[2]),
                    ]
                    resources["pattern_images"][str(row["MainId"])]["decoded_alpha_nonzero"] = int(
                        (rgba[..., 3] > 0).sum()
                    )
    return resources


def _pattern_material_ref_tuple(
    style: dict[str, Any] | None,
    resources: dict[str, Any],
) -> tuple[Any, ...] | None:
    if not isinstance(style, dict):
        return None
    pattern_style_id = style.get("PatternStyle")
    if not pattern_style_id:
        return None
    pattern_style = (resources.get("pattern_styles") or {}).get(str(pattern_style_id)) or {}
    image_ids = tuple(int(item) for item in pattern_style.get("decoded_image_indices") or [])
    images = resources.get("pattern_images") or {}
    image_names = tuple((images.get(str(image_id)) or {}).get("Name") for image_id in image_ids)
    image_mipmaps = tuple((images.get(str(image_id)) or {}).get("Mipmap") for image_id in image_ids)
    return (
        pattern_style_id,
        image_ids,
        image_names,
        image_mipmaps,
        pattern_style.get("ImageNumber"),
        pattern_style.get("OrderType"),
        pattern_style.get("Reverse2"),
    )


def _pattern_material_ref_detail(
    style: dict[str, Any] | None,
    resources: dict[str, Any],
) -> dict[str, Any] | None:
    ref = _pattern_material_ref_tuple(style, resources)
    if ref is None:
        return None
    return {
        "pattern_style_id": ref[0],
        "pattern_image_ids": list(ref[1]),
        "pattern_image_names": list(ref[2]),
        "pattern_image_mipmaps": list(ref[3]),
        "image_number": ref[4],
        "order_type": ref[5],
        "order_type_label": _pattern_order_type_label(ref[5]),
        "reverse2": ref[6],
        "reverse2_decoded": _pattern_reverse2_bits(ref[6]),
    }


def _texture_resource_ref_tuple(
    style: dict[str, Any] | None,
    resources: dict[str, Any],
) -> tuple[Any, ...] | None:
    if not isinstance(style, dict):
        return None
    texture_pattern_id = style.get("TexturePattern")
    if not texture_pattern_id:
        return None
    image = (resources.get("pattern_images") or {}).get(str(texture_pattern_id)) or {}
    return (
        texture_pattern_id,
        image.get("Name"),
        image.get("Mipmap"),
        style.get("TextureFlag"),
        style.get("TextureComposite"),
        _style_value_key(style.get("TextureDensityBase")),
        _effector_key(style.get("TextureDensityEffector")),
        _style_value_key(style.get("TextureBrightness")),
        _style_value_key(style.get("TextureContrast")),
        _style_value_key(style.get("TextureScale")),
        _style_value_key(style.get("TextureRotate")),
        _style_value_key(style.get("TextureOffsetX")),
        _style_value_key(style.get("TextureOffsetY")),
    )


def _texture_resource_ref_detail(
    style: dict[str, Any] | None,
    resources: dict[str, Any],
) -> dict[str, Any] | None:
    ref = _texture_resource_ref_tuple(style, resources)
    if ref is None:
        return None
    return {
        "texture_pattern_id": ref[0],
        "texture_image_name": ref[1],
        "texture_image_mipmap": ref[2],
        "texture_flag": ref[3],
        "texture_composite": ref[4],
        "texture_density_base": ref[5],
        "texture_density_effector": ref[6],
        "texture_brightness": ref[7],
        "texture_contrast": ref[8],
        "texture_scale": ref[9],
        "texture_rotate": ref[10],
        "texture_offset_x": ref[11],
        "texture_offset_y": ref[12],
    }


def _compiled_line_style_note(
    style: dict[str, Any] | None,
    resources: dict[str, Any],
) -> dict[str, Any] | None:
    if not isinstance(style, dict):
        return None
    pattern_ref = _pattern_material_ref_detail(style, resources)
    texture_ref = _texture_resource_ref_detail(style, resources)
    has_pattern_list = pattern_ref is not None
    has_texture_descriptor = texture_ref is not None
    if not has_pattern_list and not has_texture_descriptor:
        return None
    return {
        "native_compile_chain": [
            "0x1424A4450 line-style cache resolver",
            "0x1422D9BE0 PWBrushStyle builder",
            "0x1422DA100 clone/compile copier",
        ],
        "runtime_object": "PWBrushStyle 0x720 bytes",
        "source_table_evidence": {
            "has_pattern_material_list": has_pattern_list,
            "has_texture_descriptor": has_texture_descriptor,
        },
        "compiled_state_not_serialized_directly": [
            "line/list secondary resource pair at runtime +0x60/+0x68",
            "optional multi-entry list prepared by 0x1422DF860 -> 0x14256B190",
            "deep cache equality over resolved effectors/resources via 0x1422DCAD0",
        ],
    }


def _mipmap_probe(clip, mipmap_id: Any) -> dict[str, Any] | None:
    if not mipmap_id:
        return None
    try:
        mid = int(mipmap_id)
    except (TypeError, ValueError):
        return {"mipmap_id": mipmap_id, "error": "invalid_mipmap_id"}
    offscreen_id = clip._mipmap_offscreen_id(mid)
    external_id = clip._resolve_mipmap_external_id(mid)
    body = clip._exta_bodies.get(external_id) if external_id else None
    pixel_size = clip._offscreen_pixel_size(offscreen_id) if offscreen_id is not None else None
    expected_rgba_tile_len = None
    raw_bytes_per_pixel = None
    decoded_blob = None
    decoded_tile_blob_len = None
    decoded_bytes_per_pixel = None
    if pixel_size is not None:
        cols = (int(pixel_size[0]) + 255) // 256
        rows = (int(pixel_size[1]) + 255) // 256
        expected_rgba_tile_len = cols * rows * 256 * 256 * 4
        if body is not None and pixel_size[0] > 0 and pixel_size[1] > 0:
            raw_bytes_per_pixel = len(body) / float(pixel_size[0] * pixel_size[1])
        if external_id:
            try:
                decoded_blob = clip._get_tile_blob(external_id, expected_len=expected_rgba_tile_len)
            except Exception:
                decoded_blob = None
            if decoded_blob is not None:
                decoded_tile_blob_len = len(decoded_blob)
                if pixel_size[0] > 0 and pixel_size[1] > 0:
                    decoded_bytes_per_pixel = decoded_tile_blob_len / float(
                        pixel_size[0] * pixel_size[1]
                    )
    info: dict[str, Any] = {
        "mipmap_id": mid,
        "offscreen_id": offscreen_id,
        "external_id": external_id,
        "external_present": body is not None,
        "external_len": None if body is None else len(body),
        "pixel_size": None if pixel_size is None else [int(pixel_size[0]), int(pixel_size[1])],
        "expected_rgba_tile_len": expected_rgba_tile_len,
        "raw_bytes_per_pixel": raw_bytes_per_pixel,
        "decoded_tile_blob_len": decoded_tile_blob_len,
        "decoded_bytes_per_pixel": decoded_bytes_per_pixel,
    }
    if (
        decoded_blob is not None
        and decoded_bytes_per_pixel in (1.0, 2.0, 4.0)
    ):
        info["decoded_byte_lane_stats"] = _byte_lane_stats(
            decoded_blob,
            int(decoded_bytes_per_pixel),
        )
        info["native_material_selector_guess"] = _native_material_selector_guess(
            decoded_bytes_per_pixel
        )
        info["native_material_format_note"] = _native_material_format_note(
            decoded_bytes_per_pixel
        )
    if body is not None:
        try:
            rgba = clip._decode_mipmap_rgba(mid)
        except Exception as exc:
            info["decode_error"] = str(exc)
        else:
            if rgba is not None:
                alpha = rgba[..., 3]
                info["decoded_rgba_shape"] = [
                    int(rgba.shape[1]),
                    int(rgba.shape[0]),
                    int(rgba.shape[2]),
                ]
                info["decoded_alpha_nonzero"] = int((alpha > 0).sum())
                info["decoded_alpha_sum"] = int(alpha.sum())
            else:
                info["decoded_rgba_shape"] = None
    return info


def _scan_vector_layers(clip) -> list[dict[str, Any]]:
    layers: list[dict[str, Any]] = []
    table = clip._db.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name='VectorObjectList'"
    ).fetchone()
    if table is None:
        return layers
    vector_rows = {
        int(row["LayerId"]): row
        for row in clip._db.execute("SELECT * FROM VectorObjectList ORDER BY LayerId")
    }
    for layer_id, vector_row in vector_rows.items():
        layer = clip._layer_row(layer_id)
        if layer is None:
            continue
        ext_id = vector_row["VectorData"]
        if isinstance(ext_id, bytes):
            ext_id = ext_id.decode("ascii", errors="replace")
        body = clip._vector_object_body(layer_id)
        info = {
            "layer_id": int(layer_id),
            "layer_name": layer["LayerName"],
            "vector_row": {
                key: _clean_value(vector_row[key])
                for key in ("_PW_ID", "CanvasId", "MainId", "LayerId")
                if key in vector_row.keys()
            },
            "vector_data": ext_id,
            "vector_body_len": None if body is None else len(body),
        }
        if body is not None:
            external_header = _external_header_probe(body)
            if external_header is not None:
                external_header["matches_vector_data"] = external_header.get("external_id") == ext_id
                info["external_header"] = external_header
            spans = _compact_record_spans(body)
            covered = sum(max(0, int(span["end"]) - int(span["offset"])) for span in spans)
            prefix_len = int(spans[0]["offset"]) if spans else None
            trailing_gap = int(len(body) - spans[-1]["end"]) if spans else len(body)
            info["compact_record_spans"] = spans[:32]
            info["compact_record_coverage"] = {
                "record_count": len(spans),
                "covered_bytes": covered,
                "body_len": len(body),
                "prefix_len": prefix_len,
                "trailing_gap": trailing_gap,
                "fully_covered_after_prefix": bool(spans and trailing_gap == 0),
                "prefix_matches_external_header": bool(
                    external_header is not None
                    and prefix_len == int(external_header["payload_offset"])
                    and external_header["payload_size_matches_body"]
                ),
            }
        for key in LAYER_VECTOR_KEYS:
            if key in layer.keys():
                info[key] = _clean_value(layer[key])
        mipmaps: dict[str, Any] = {}
        for key in ("LayerRenderMipmap", "LayerLayerMaskMipmap", "ComicFrameLineMipmap"):
            if key in layer.keys():
                probe = _mipmap_probe(clip, layer[key])
                if probe is not None:
                    mipmaps[key] = probe
        if mipmaps:
            info["mipmap_resources"] = mipmaps
        layers.append(info)
    return layers


def _scan_strokes(clip) -> list[dict[str, Any]]:
    strokes: list[dict[str, Any]] = []
    for layer in clip._db.execute("SELECT MainId, LayerName FROM Layer ORDER BY MainId"):
        body = clip._vector_object_body(layer["MainId"])
        if body is None:
            continue
        for off in range(_compact_record_scan_start(body), len(body) - 92, 4):
            try:
                header_len, point_header_len, stride_a, stride_b = struct.unpack_from(
                    ">IIII", body, off
                )
                point_count, flags = struct.unpack_from(">II", body, off + 16)
            except struct.error:
                continue

            shape = [header_len, point_header_len, stride_a, stride_b]
            if shape not in ([92, 76, 88, 88], [92, 76, 120, 88]):
                continue
            if not (1 <= point_count <= 5000):
                continue

            try:
                bbox = struct.unpack_from(">IIII", body, off + 24)
                line_rgb = _rgb16_at(body, (off + 40, off + 44, off + 48))
                fill_rgb = _rgb16_at(body, (off + 52, off + 56, off + 60))
                opacity = struct.unpack_from(">d", body, off + 64)[0]
                brush_style_id = struct.unpack_from(">I", body, off + 76)[0]
                width = struct.unpack_from(">d", body, off + 80)[0]
            except struct.error:
                continue

            point_stats: dict[str, Any] = {}
            if point_count >= 1:
                point_stats = _point_summary(body, off + header_len, stride_a, point_count)

            strokes.append(
                {
                    "layer_id": int(layer["MainId"]),
                    "layer_name": layer["LayerName"],
                    "body_offset": off,
                    "shape": shape,
                    "flags": flags,
                    "raw_header": _header_probe(body, off, header_len),
                    "compact_native_reader_probe": _compact_native_reader_probe(
                        body,
                        off,
                        header_len,
                    ),
                    "compact_semantic_probe": _compact_semantic_probe(
                        body,
                        off,
                        header_len,
                        object_kind="stroke_92",
                    ),
                    "point_count": point_count,
                    "bbox": [int(v) for v in bbox],
                    "line_rgb": line_rgb,
                    "fill_rgb": fill_rgb,
                    "object_opacity": None if not math.isfinite(opacity) else round(float(opacity), 6),
                    "brush_style_id": int(brush_style_id),
                    "width": None if not math.isfinite(width) else round(float(width), 6),
                    "point_stats": point_stats,
                }
            )
    return strokes


def _scan_object_headers_100(clip) -> list[dict[str, Any]]:
    objects: list[dict[str, Any]] = []
    layer_columns = {row[1] for row in clip._db.execute("PRAGMA table_info(Layer)")}
    optional_layer_columns = [
        key
        for key in (
            "VectorNormalBalloonIndex",
            "VectorNormalType",
            "ComicFrameLineMipmap",
            "ComicFrameColorTypeIndex",
            "ComicFrameColorTypeBlackChecked",
            "ComicFrameColorTypeWhiteChecked",
        )
        if key in layer_columns
    ]
    query_columns = ["MainId", "LayerName", "LayerFirstChildIndex"] + optional_layer_columns
    for layer in clip._db.execute(
        f"SELECT {', '.join(query_columns)} FROM Layer ORDER BY MainId"
    ):
        body = clip._vector_object_body(layer["MainId"])
        if body is None:
            continue
        for off in range(_compact_record_scan_start(body), len(body) - 100, 4):
            try:
                header_len, point_header_len, stride_a, stride_b = struct.unpack_from(
                    ">IIII", body, off
                )
                point_count = struct.unpack_from(">I", body, off + 16)[0]
                bbox = struct.unpack_from(">IIII", body, off + 24)
                line_rgb = _rgb16_at(body, (off + 40, off + 44, off + 48))
                fill_rgb = _rgb16_at(body, (off + 52, off + 56, off + 60))
                opacity = struct.unpack_from(">d", body, off + 64)[0]
                family_id = struct.unpack_from(">I", body, off + 76)[0]
                line_style_id, fill_style_id, width = struct.unpack_from(">IId", body, off + 80)
                extra_id = struct.unpack_from(">I", body, off + 96)[0]
            except struct.error:
                continue
            shape = [header_len, point_header_len, stride_a, stride_b]
            if header_len != 100 or point_header_len != 76 or stride_b != 88:
                continue
            if stride_a not in (88, 104):
                continue
            if not (1 <= point_count <= 5000):
                continue
            if not (0 <= bbox[0] < bbox[2] <= clip.width * 2 and 0 <= bbox[1] < bbox[3] <= clip.height * 2):
                continue
            if not (
                0 <= int(line_style_id) <= 10000
                and 0 <= int(fill_style_id) <= 10000
                and math.isfinite(width)
            ):
                continue
            child_ids = clip._walk_chain(int(layer["LayerFirstChildIndex"] or 0))
            child_layers = []
            for child_id in child_ids[:8]:
                child = clip._layer_row(child_id)
                if child is None:
                    continue
                child_keys = set(child.keys())
                child_layers.append(
                    {
                        "layer_id": int(child_id),
                        "layer_name": child["LayerName"] if "LayerName" in child_keys else None,
                        "layer_type": int(child["LayerType"]) if "LayerType" in child_keys and child["LayerType"] is not None else None,
                        "layer_folder": int(child["LayerFolder"]) if "LayerFolder" in child_keys and child["LayerFolder"] is not None else None,
                        "layer_visibility": int(child["LayerVisibility"]) if "LayerVisibility" in child_keys and child["LayerVisibility"] is not None else None,
                        "layer_render_mipmap": int(child["LayerRenderMipmap"]) if "LayerRenderMipmap" in child_keys and child["LayerRenderMipmap"] else 0,
                        "has_gradation_fill_info": bool(child["GradationFillInfo"]) if "GradationFillInfo" in child_keys else False,
                    }
                )
            objects.append(
                {
                    "layer_id": int(layer["MainId"]),
                    "layer_name": layer["LayerName"],
                    "layer_first_child": int(layer["LayerFirstChildIndex"] or 0),
                    "layer_child_count": len(child_ids),
                    "layer_children": child_layers,
                    "layer_vector_fields": {
                        key: _clean_value(layer[key])
                        for key in optional_layer_columns
                    },
                    "body_offset": off,
                    "shape": shape,
                    "raw_header": _header_probe(body, off, header_len),
                    "compact_native_reader_probe": _compact_native_reader_probe(
                        body,
                        off,
                        header_len,
                    ),
                    "compact_semantic_probe": _compact_semantic_probe(
                        body,
                        off,
                        header_len,
                        object_kind="object_100",
                    ),
                    "point_count": int(point_count),
                    "bbox": [int(v) for v in bbox],
                    "line_rgb": line_rgb,
                    "fill_rgb": fill_rgb,
                    "object_opacity": None if not math.isfinite(opacity) else round(float(opacity), 6),
                    "family_id": int(family_id),
                    "family_flags": _decode_balloon_family_flags(int(family_id)),
                    "line_style_id": int(line_style_id),
                    "fill_style_id": int(fill_style_id),
                    "extra_id": int(extra_id),
                    "brush_style_id": int(line_style_id),
                    "subtype": int(fill_style_id),
                    "width": round(float(width), 6),
                    "point_stats": _point_summary(body, off + header_len, stride_a, point_count),
                }
            )
    return objects


def _inspect_clip(mod, clip_path: Path) -> dict[str, Any]:
    clip = mod.ClipFile(str(clip_path))
    try:
        vector_layers = _scan_vector_layers(clip)
        strokes = _scan_strokes(clip)
        objects_100 = _scan_object_headers_100(clip)
        style_ids = sorted(
            {stroke["brush_style_id"] for stroke in strokes}
            | {obj["line_style_id"] for obj in objects_100}
        )
        fill_style_ids = sorted({obj["fill_style_id"] for obj in objects_100})
        styles = {str(style_id): _brush_style_summary(clip, style_id) for style_id in style_ids}
        fill_styles = {
            str(style_id): _fill_style_summary(clip, style_id)
            for style_id in fill_style_ids
        }
        resources = _pattern_resources(clip, set(style_ids))
        graph_table = clip._db.execute(
            """
            SELECT 1 FROM sqlite_master
            WHERE type='table' AND name='BrushEffectorGraphData'
            """
        ).fetchone()
        if graph_table is None:
            graphs = {}
        else:
            graphs = {
                str(row["MainId"]): _graph_points(row)
                for row in clip._db.execute(
                    """
                    SELECT MainId, ControlNumber, ControlDataSize, ControlPoints
                    FROM BrushEffectorGraphData ORDER BY MainId
                    """
                )
            }
        for stroke in strokes:
            style = styles.get(str(stroke["brush_style_id"]))
            diagnostics = _style_effector_diagnostics(style, graphs)
            if diagnostics:
                stroke["brush_style_effectors"] = diagnostics
            spacing_estimate = _native_spacing_estimate(
                "stroke_92",
                stroke.get("brush_style_id"),
                style,
                stroke.get("width"),
                stroke.get("point_stats"),
                graphs,
            )
            if spacing_estimate is not None:
                stroke["native_spacing_estimate"] = spacing_estimate
            spacing_comparison = _importer_spacing_comparison(spacing_estimate, style, mod)
            if spacing_comparison is not None:
                stroke["importer_spacing_comparison"] = spacing_comparison
            adaptive_candidate = _experimental_adaptive_spacing_candidate(spacing_estimate, style, mod)
            if adaptive_candidate is not None:
                stroke["experimental_adaptive_spacing_candidate"] = adaptive_candidate
            pattern_ref = _pattern_material_ref_detail(style, resources)
            if pattern_ref is not None:
                stroke["pattern_material_resource"] = pattern_ref
            texture_ref = _texture_resource_ref_detail(style, resources)
            if texture_ref is not None:
                stroke["texture_resource"] = texture_ref
            compiled_note = _compiled_line_style_note(style, resources)
            if compiled_note is not None:
                stroke["compiled_line_style_note"] = compiled_note
        for obj in objects_100:
            line_style = styles.get(str(obj["line_style_id"]))
            line_diagnostics = _style_effector_diagnostics(line_style, graphs)
            if line_diagnostics:
                obj["line_style_effectors"] = line_diagnostics
                obj["brush_style_effectors"] = line_diagnostics
            spacing_estimate = _native_spacing_estimate(
                "object_100_line",
                obj.get("line_style_id"),
                line_style,
                obj.get("width"),
                obj.get("point_stats"),
                graphs,
            )
            if spacing_estimate is not None:
                obj["line_native_spacing_estimate"] = spacing_estimate
            spacing_comparison = _importer_spacing_comparison(spacing_estimate, line_style, mod)
            if spacing_comparison is not None:
                obj["line_importer_spacing_comparison"] = spacing_comparison
            adaptive_candidate = _experimental_adaptive_spacing_candidate(spacing_estimate, line_style, mod)
            if adaptive_candidate is not None:
                obj["line_experimental_adaptive_spacing_candidate"] = adaptive_candidate
            line_pattern_ref = _pattern_material_ref_detail(line_style, resources)
            if line_pattern_ref is not None:
                obj["line_pattern_material_resource"] = line_pattern_ref
            line_texture_ref = _texture_resource_ref_detail(line_style, resources)
            if line_texture_ref is not None:
                obj["line_texture_resource"] = line_texture_ref
            compiled_note = _compiled_line_style_note(line_style, resources)
            if compiled_note is not None:
                obj["line_compiled_style_note"] = compiled_note
            fill_style = fill_styles.get(str(obj["fill_style_id"]))
            if fill_style is not None:
                obj["fill_style"] = fill_style
    finally:
        clip.close()

    return {
        "name": clip_path.name,
        "path": str(clip_path),
        "canvas": {"width": clip.width, "height": clip.height},
        "vector_layer_count": len(vector_layers),
        "vector_layers": vector_layers,
        "stroke_count": len(strokes),
        "object_100_count": len(objects_100),
        "strokes": strokes,
        "objects_100": objects_100,
        "brush_styles": styles,
        "fill_styles": fill_styles,
        "brush_resources": resources,
        "effector_graphs": graphs,
    }


def _summary(results: list[dict[str, Any]], mod: Any | None = None) -> dict[str, Any]:
    style_combos: collections.Counter[tuple[Any, ...]] = collections.Counter()
    style_combos_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    native_spacing_combos_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    native_spacing_estimates_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    native_spacing_estimate_bucket_counts: collections.Counter[tuple[Any, ...]] = collections.Counter()
    importer_spacing_comparisons_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    experimental_adaptive_spacing_candidates_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    fill_style_combos_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    color_mix_combos: collections.Counter[tuple[Any, ...]] = collections.Counter()
    color_mix_combos_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    color_change_combos: collections.Counter[tuple[Any, ...]] = collections.Counter()
    color_change_combos_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    blur_combos: collections.Counter[tuple[Any, ...]] = collections.Counter()
    blur_combos_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    rotation_combos: collections.Counter[tuple[Any, ...]] = collections.Counter()
    rotation_combos_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    texture_combos: collections.Counter[tuple[Any, ...]] = collections.Counter()
    texture_combos_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    pattern_material_refs_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    texture_resource_refs_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    compiled_line_style_notes_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    style_flags_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    renderer_status_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    renderer_coverage_counts: collections.Counter[tuple[Any, ...]] = collections.Counter()
    renderer_gap_inputs_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    renderer_gap_inputs_by_clip: dict[tuple[Any, ...], set[str]] = collections.defaultdict(set)
    no_pattern_dab_candidate_counts: collections.Counter[tuple[Any, ...]] = collections.Counter()
    spray_combos: collections.Counter[tuple[Any, ...]] = collections.Counter()
    spray_combos_by_kind: collections.Counter[tuple[Any, ...]] = collections.Counter()
    effector_keys: collections.Counter[tuple[str, str]] = collections.Counter()
    effector_decoded_forms: collections.Counter[tuple[Any, ...]] = collections.Counter()
    effector_runtime_branches: collections.Counter[tuple[Any, ...]] = collections.Counter()
    effector_graph_signatures: collections.Counter[tuple[Any, ...]] = collections.Counter()
    effector_graph_refs: collections.Counter[tuple[Any, ...]] = collections.Counter()
    shape_counts: collections.Counter[tuple[Any, ...]] = collections.Counter()
    object_100_shape_counts: collections.Counter[tuple[Any, ...]] = collections.Counter()
    object_100_layer_counts: collections.Counter[tuple[Any, ...]] = collections.Counter()
    object_100_fill_contexts: collections.Counter[tuple[Any, ...]] = collections.Counter()
    object_100_raw_u32: dict[str, collections.Counter[tuple[Any, ...]]] = collections.defaultdict(collections.Counter)
    object_100_raw_f64: dict[str, collections.Counter[tuple[Any, ...]]] = collections.defaultdict(collections.Counter)
    stroke_raw_u32: dict[str, collections.Counter[tuple[Any, ...]]] = collections.defaultdict(collections.Counter)
    compact_draw_lenses: collections.Counter[tuple[Any, ...]] = collections.Counter()
    point_native32_u32: dict[str, collections.Counter[tuple[Any, ...]]] = collections.defaultdict(collections.Counter)
    point_u32_stats: dict[str, collections.Counter[tuple[Any, ...]]] = collections.defaultdict(collections.Counter)
    point_cubic64_u32: dict[str, collections.Counter[tuple[Any, ...]]] = collections.defaultdict(collections.Counter)
    point_cubic64_f64_stats: dict[str, collections.Counter[tuple[Any, ...]]] = collections.defaultdict(collections.Counter)
    point_float_stats: dict[str, collections.Counter[tuple[Any, ...]]] = collections.defaultdict(collections.Counter)
    effector_point_float_stats: dict[str, collections.Counter[tuple[Any, ...]]] = collections.defaultdict(collections.Counter)
    effector_point_u32_stats: dict[str, collections.Counter[tuple[Any, ...]]] = collections.defaultdict(collections.Counter)
    layer_vector_types: collections.Counter[tuple[Any, ...]] = collections.Counter()
    brush_usage: collections.Counter[tuple[Any, ...]] = collections.Counter()
    external_header_counts: collections.Counter[tuple[Any, ...]] = collections.Counter()

    def _active_effector_context(style: dict[str, Any] | None) -> tuple[str, ...]:
        if not isinstance(style, dict):
            return ()
        parts: list[str] = []
        for key in EFFECTOR_KEYS:
            value = style.get(key)
            if isinstance(value, dict):
                decoded = value.get("decoded") or {}
                form = decoded.get("form")
                if form in {None, "zero_or_disabled_0x00", "default_or_off_0x01"}:
                    continue
                refs = ",".join(str(item) for item in (decoded.get("graph_refs") or []))
                scalars = decoded.get("scalars")
                scalar = decoded.get("scalar")
                scalar_text = ""
                if scalars:
                    scalar_text = ":" + ",".join(str(item) for item in scalars)
                elif scalar is not None:
                    scalar_text = f":{scalar}"
                parts.append(f"{key}:{form}:g={refs}{scalar_text}")
            elif value not in (None, 0, 1):
                parts.append(f"{key}:mode={value}")
        return tuple(parts)

    def _style_combo(style: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(style, dict):
            return None
        style_flag = style.get("StyleFlag")
        if isinstance(style_flag, dict):
            style_flag = (
                style_flag.get("hex"),
                style_flag.get("direct_max_accum_0x1000"),
            )
        return (
            _effector_key(style.get("SizeEffector")),
            _effector_key(style.get("OpacityEffector")),
            _effector_key(style.get("FlowEffector")),
            _effector_key(style.get("ThicknessEffector")),
            style.get("FlowBase"),
            style.get("Hardness"),
            style_flag,
            style.get("TexturePattern"),
            style.get("PatternStyle"),
            style.get("AutoIntervalType"),
            style.get("AntiAlias"),
            _style_value_key(style.get("IntervalBase")),
            _effector_key(style.get("IntervalEffector")),
        )

    def _native_spacing_combo(style: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(style, dict):
            return None
        style_flag = style.get("StyleFlag") if isinstance(style.get("StyleFlag"), dict) else {}
        auto_interval = style.get("AutoIntervalType")
        if auto_interval in (4, 5):
            branch = "auto_interval_4_5"
        elif auto_interval in (1, 2, 3):
            branch = "auto_interval_1_2_3"
        else:
            branch = "normal_interval"
        return (
            branch,
            _style_value_key(style.get("PenRadius")),
            _effector_key(style.get("SizeEffector")),
            _style_value_key(style.get("IntervalBase")),
            _effector_key(style.get("IntervalEffector")),
            auto_interval,
            _style_value_key(style.get("ThicknessBase")),
            _effector_key(style.get("ThicknessEffector")),
            _style_value_key(style.get("Hardness")),
            style.get("AntiAlias"),
            style.get("SprayFlag"),
            style_flag.get("native_subpixel_min_size_0x8"),
            style_flag.get("native_retained_state_path_0x20"),
            style_flag.get("texture_or_water_family_0x10000"),
            style_flag.get("texture_or_water_family_0x20000"),
        )

    def _native_spacing_estimate_key(estimate: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(estimate, dict):
            return None
        return (
            estimate.get("kind"),
            estimate.get("style_id"),
            estimate.get("branch"),
            estimate.get("base_width"),
            tuple(estimate.get("compact_f32_56_size_factor_range") or ()),
            estimate.get("size_effector"),
            estimate.get("size_effector_dynamic"),
            json.dumps(estimate.get("size_effector_graph_output"), ensure_ascii=False, sort_keys=True),
            json.dumps(estimate.get("size_effector_multiplier_estimate"), ensure_ascii=False, sort_keys=True),
            estimate.get("interval_scalar"),
            estimate.get("thickness_guess"),
            tuple(estimate.get("effective_size_without_size_effector_range") or ()),
            tuple(estimate.get("state8_without_size_effector_range") or ()),
            tuple(estimate.get("effective_size_with_estimated_size_effector_range") or ()),
            tuple(estimate.get("state8_with_estimated_size_effector_range") or ()),
            estimate.get("unestimated_reason"),
        )

    def _style_flag_key(style: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(style, dict):
            return None
        style_flag = style.get("StyleFlag")
        if not isinstance(style_flag, dict):
            return None
        return (
            style_flag.get("hex"),
            style_flag.get("native_subpixel_min_size_0x8"),
            style_flag.get("native_retained_state_path_0x20"),
            style_flag.get("native_thickness_axis_split_0x40"),
            style_flag.get("native_segment_start_flag_0x200"),
            style_flag.get("direct_max_accum_0x1000"),
            style_flag.get("texture_or_water_family_0x10000"),
            style_flag.get("texture_or_water_family_0x20000"),
            style_flag.get("unknown_bits"),
        )

    def _fill_style_combo(style: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(style, dict):
            return None
        return (
            style.get("StyleFlag"),
            style.get("AntiAlias"),
            style.get("CompositeMode"),
            _style_value_key(style.get("TextureDensity")),
        )

    def _color_mix_combo(style: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(style, dict):
            return None
        return (
            style.get("UseWaterColor"),
            style.get("WaterColorType"),
            style.get("BrushColorMixingMode"),
            style.get("BrushLMSLinearity"),
            _style_value_key(style.get("MixColorBase")),
            _effector_key(style.get("MixColorEffector")),
            _style_value_key(style.get("MixAlphaBase")),
            _effector_key(style.get("MixAlphaEffector")),
            style.get("ColorExtension"),
            style.get("WaterEdgeFlag"),
            _style_value_key(style.get("WaterEdgeRadius")),
            _style_value_key(style.get("WaterEdgeAlphaPower")),
            _style_value_key(style.get("WaterEdgeValuePower")),
            _style_value_key(style.get("WaterEdgeBlur")),
        )

    def _color_change_combo(style: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(style, dict):
            return None
        return (
            _style_value_key(style.get("SubColorBase")),
            _effector_key(style.get("SubColorEffector")),
            _style_value_key(style.get("HueChangeBase")),
            _effector_key(style.get("HueChangeEffector")),
            _style_value_key(style.get("SaturationChangeBase")),
            _effector_key(style.get("SaturationChangeEffector")),
            _style_value_key(style.get("ValueChangeBase")),
            _effector_key(style.get("ValueChangeEffector")),
            style.get("ChangeDrawColorTarget"),
            style.get("ColorExtension"),
        )

    def _blur_combo(style: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(style, dict):
            return None
        return (
            style.get("BlurKind"),
            _style_value_key(style.get("BlurBase")),
            _effector_key(style.get("BlurEffector")),
            style.get("UseWaterColor"),
            style.get("WaterColorType"),
        )

    def _rotation_combo(style: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(style, dict):
            return None
        rotation_effector = style.get("RotationEffector")
        rotation_effector_in_spray = style.get("RotationEffectorInSpray")
        return (
            _style_value_key(style.get("RotationBase")),
            _effector_key(rotation_effector),
            _rotation_effector_bits(rotation_effector),
            _style_value_key(style.get("RotationRandom")),
            _style_value_key(style.get("RotationInSprayBase")),
            _effector_key(rotation_effector_in_spray),
            _rotation_effector_bits(rotation_effector_in_spray),
            _style_value_key(style.get("RotationRandomInSpray")),
        )

    def _rotation_effector_bits(value: Any) -> tuple[str, ...]:
        if not isinstance(value, int):
            return ()
        bits: list[str] = []
        if value & 0x01:
            bits.append("active_0x01")
        if value & 0x02:
            bits.append("unknown_0x02")
        if value & 0x30:
            bits.append("sample_angle_0x30")
        if value & 0x40:
            bits.append("aux_rotation_0x40")
        if value & 0x80:
            bits.append("random_0x80")
        rest = value & ~(0x01 | 0x02 | 0x30 | 0x40 | 0x80)
        if rest:
            bits.append(f"unknown_{rest:#x}")
        return tuple(bits)

    def _texture_combo(style: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(style, dict):
            return None
        return (
            style.get("TexturePattern"),
            style.get("TextureFlag"),
            style.get("TextureComposite"),
            _style_value_key(style.get("TextureScale")),
            _style_value_key(style.get("TextureRotate")),
            _style_value_key(style.get("TextureOffsetX")),
            _style_value_key(style.get("TextureOffsetY")),
            _style_value_key(style.get("TextureBrightness")),
            _style_value_key(style.get("TextureContrast")),
            _style_value_key(style.get("TextureDensityBase")),
            _effector_key(style.get("TextureDensityEffector")),
        )

    def _pattern_material_ref(
        style: dict[str, Any] | None,
        resources: dict[str, Any],
    ) -> tuple[Any, ...] | None:
        return _pattern_material_ref_tuple(style, resources)

    def _texture_resource_ref(
        style: dict[str, Any] | None,
        resources: dict[str, Any],
    ) -> tuple[Any, ...] | None:
        return _texture_resource_ref_tuple(style, resources)

    def _compiled_line_style_note_key(
        style: dict[str, Any] | None,
        resources: dict[str, Any],
    ) -> tuple[Any, ...] | None:
        note = _compiled_line_style_note(style, resources)
        if note is None:
            return None
        evidence = note.get("source_table_evidence") or {}
        return (
            note.get("runtime_object"),
            bool(evidence.get("has_pattern_material_list")),
            bool(evidence.get("has_texture_descriptor")),
            tuple(note.get("compiled_state_not_serialized_directly") or ()),
        )

    def _spray_combo(style: dict[str, Any] | None) -> tuple[Any, ...] | None:
        if not isinstance(style, dict):
            return None
        return (
            style.get("SprayFlag"),
            _style_value_key(style.get("SpraySizeBase")),
            _effector_key(style.get("SpraySizeEffector")),
            _style_value_key(style.get("SprayDensityBase")),
            _effector_key(style.get("SprayDensityEffector")),
            _style_value_key(style.get("SprayBias")),
            _style_value_key(style.get("FixedSpray")),
        )

    def _no_pattern_dab_candidate_key(
        kind: str,
        style_id: Any,
        style: dict[str, Any] | None,
        flags: Any = None,
    ) -> tuple[Any, ...]:
        if not isinstance(style, dict):
            return (kind, "missing_style", style_id, flags, ())
        if kind == "stroke_92" and flags not in (0x2011, 0x2081):
            return (kind, "not_ordinary_compact_stroke", style_id, flags, ())
        style_flag = style.get("StyleFlag") if isinstance(style.get("StyleFlag"), dict) else {}
        if style.get("SprayFlag") not in (None, 0):
            return (kind, "spray_path_separate", style_id, flags, ())
        if style_flag.get("native_retained_state_path_0x20"):
            return (kind, "retained_state_path", style_id, flags, ())
        if style.get("PatternStyle") not in (None, 0):
            return (kind, "pattern_material_path", style_id, flags, ())
        if style.get("TexturePattern") not in (None, 0) or style.get("TextureFlag") not in (None, 0):
            return (kind, "texture_material_path", style_id, flags, ())

        dynamic_inputs: list[str] = []
        if _effector_is_nondefault(style.get("SizeEffector")):
            dynamic_inputs.append("SizeEffector")
        if _effector_is_nondefault(style.get("OpacityEffector")):
            dynamic_inputs.append("OpacityEffector")
        if style.get("FlowBase") not in (None, 1, 1.0) or _effector_is_nondefault(style.get("FlowEffector")):
            dynamic_inputs.append("Flow")
        if style.get("Hardness") not in (None, 1, 1.0):
            dynamic_inputs.append("Hardness")
        if style.get("AutoIntervalType") not in (None, 0) or _effector_is_nondefault(style.get("IntervalEffector")):
            dynamic_inputs.append("Interval")
        if _effector_is_nondefault(style.get("ThicknessEffector")):
            dynamic_inputs.append("Thickness")
        if _effector_is_nondefault(style.get("RotationEffector")):
            dynamic_inputs.append("Rotation")
        if style.get("AntiAlias") not in (None, 0):
            dynamic_inputs.append("AntiAlias")
        if style_flag.get("direct_max_accum_0x1000"):
            dynamic_inputs.append("DirectMaxAccum")
        category = (
            "ordinary_no_pattern_dynamic_dab_candidate"
            if dynamic_inputs
            else "ordinary_no_pattern_basic_dab_candidate"
        )
        return (kind, category, style_id, flags, tuple(sorted(set(dynamic_inputs))))

    def _water_writer_mode(use_water_color: Any, water_color_type: Any) -> Any:
        if not use_water_color:
            return 0
        if isinstance(water_color_type, int) and 0 <= water_color_type <= 2:
            return water_color_type + 1
        return "unknown"

    def _hardness_profile_info(hardness: Any) -> tuple[bool | None, float | None]:
        if not isinstance(hardness, (int, float)):
            return None, None
        value = float(hardness)
        active = value < 0.9999999899999999
        return active, round(value * 1.3 - 0.3, 6)

    def _collect_cubic64_point_stats(
        point_stats: dict[str, Any] | None,
        context: tuple[Any, ...],
    ) -> None:
        for rel, stats in ((point_stats or {}).get("float_fields") or {}).items():
            if not stats:
                continue
            stat_key = (
                stats.get("min"),
                stats.get("max"),
                stats.get("avg"),
                stats.get("uniq"),
            )
            point_float_stats[str(rel)][context + stat_key] += 1
        for rel, values in ((point_stats or {}).get("point_u32_stats") or {}).items():
            for item in values or []:
                point_u32_stats[str(rel)][context + (item.get("value"),)] += int(item.get("count") or 0)
        native_stats = (point_stats or {}).get("native32_stats") or {}
        for rel, values in (native_stats.get("u32") or {}).items():
            for item in values or []:
                point_native32_u32[str(rel)][context + (item.get("value"),)] += int(item.get("count") or 0)
        cubic_stats = (point_stats or {}).get("cubic64_stats") or {}
        for rel, values in (cubic_stats.get("u32") or {}).items():
            for item in values or []:
                point_cubic64_u32[str(rel)][context + (item.get("value"),)] += int(item.get("count") or 0)
        for rel, stats in (cubic_stats.get("f64") or {}).items():
            if not stats:
                continue
            stat_key = (
                stats.get("min"),
                stats.get("max"),
                stats.get("avg"),
                stats.get("uniq"),
            )
            point_cubic64_f64_stats[str(rel)][context + stat_key] += 1

    def _collect_effector_point_float_stats(
        point_stats: dict[str, Any] | None,
        context: tuple[Any, ...],
        effector_context: tuple[str, ...],
    ) -> None:
        if not effector_context:
            return
        for rel, stats in ((point_stats or {}).get("float_fields") or {}).items():
            if not stats:
                continue
            stat_key = (
                stats.get("min"),
                stats.get("max"),
                stats.get("avg"),
                stats.get("uniq"),
            )
            effector_point_float_stats[str(rel)][context + (effector_context,) + stat_key] += 1

    def _collect_effector_point_u32_stats(
        point_stats: dict[str, Any] | None,
        context: tuple[Any, ...],
        effector_context: tuple[str, ...],
    ) -> None:
        if not effector_context:
            return
        for rel, values in ((point_stats or {}).get("point_u32_stats") or {}).items():
            for item in values or []:
                effector_point_u32_stats[str(rel)][
                    context + (effector_context, item.get("value"))
                ] += int(item.get("count") or 0)

    def _collect_compact_draw_lens(
        probe: dict[str, Any] | None,
        context: tuple[Any, ...],
    ) -> None:
        for rel, lens in ((probe or {}).get("native_draw_flag_candidate_lens") or {}).items():
            compact_draw_lenses[
                context
                + (
                    int(rel),
                    lens.get("value"),
                    lens.get("low_nibble"),
                    bool(lens.get("bit0_line_lens")),
                    bool(lens.get("bit1_fill_lens")),
                    bool(lens.get("bit2_special_lens")),
                )
            ] += 1

    def _collect_style_effector_stats(
        style: dict[str, Any] | None,
        graph_ids: set[int],
    ) -> None:
        if not isinstance(style, dict):
            return
        for key in EFFECTOR_KEYS:
            value_key = _style_value_key(style.get(key))
            effector_keys[(key, value_key)] += 1
            decoded = style.get(key).get("decoded") if isinstance(style.get(key), dict) else None
            if isinstance(decoded, dict):
                branches = decoded.get("native_runtime_branches") or {}
                effector_decoded_forms[(
                    key,
                    decoded.get("form"),
                    decoded.get("type"),
                    tuple(decoded.get("graph_refs") or []),
                    tuple(decoded.get("scalars") or []),
                    decoded.get("scalar"),
                )] += 1
                effector_runtime_branches[(
                    key,
                    decoded.get("form"),
                    decoded.get("type"),
                    bool(branches.get("graph_context_primary_0x10")),
                    bool(branches.get("graph_context_secondary_0x20")),
                    bool(branches.get("graph_context_aux_0x40")),
                    bool(branches.get("random_0x80")),
                    bool(branches.get("velocity_0x100")),
                    branches.get("unknown_bits"),
                )] += 1
            elif style.get(key) is not None:
                effector_decoded_forms[(key, "scalar_or_mode", style.get(key), (), (), None)] += 1
            for graph_id in _effector_graph_refs(style.get(key)):
                effector_graph_refs[(
                    key,
                    value_key,
                    graph_id,
                    graph_id in graph_ids,
                )] += 1

    def _collect_renderer_status(
        clip_name: str,
        kind: str,
        style_id: Any,
        status: dict[str, Any],
    ) -> None:
        if not isinstance(status, dict):
            return
        pixel_inputs = tuple(status.get("pixel_affecting_inputs") or [])
        preview_inputs = tuple(status.get("preview_only_inputs") or [])
        diagnostic_inputs = tuple(status.get("diagnostic_only_inputs") or [])
        coverage = status.get("coverage")
        renderer_status_by_kind[(
            kind,
            style_id,
            coverage,
            pixel_inputs,
            preview_inputs,
            diagnostic_inputs,
        )] += 1
        renderer_coverage_counts[(kind, coverage)] += 1
        for item in diagnostic_inputs:
            renderer_gap_inputs_by_kind[(kind, item)] += 1
            renderer_gap_inputs_by_clip[(kind, item)].add(clip_name)

    def _no_pattern_dab_category_rows() -> list[dict[str, Any]]:
        category_counts: collections.Counter[tuple[Any, ...]] = collections.Counter()
        for combo, count in no_pattern_dab_candidate_counts.items():
            category_counts[(combo[0], combo[1])] += count
        return [
            {
                "count": count,
                "kind": combo[0],
                "category": combo[1],
            }
            for combo, count in category_counts.most_common()
        ]

    for result in results:
        styles = result.get("brush_styles") or {}
        fill_styles = result.get("fill_styles") or {}
        resources = result.get("brush_resources") or {}
        graphs = result.get("effector_graphs") or {}
        graph_ids = {int(graph_id) for graph_id in graphs.keys() if str(graph_id).isdigit()}
        for graph_id, graph in graphs.items():
            points = tuple(tuple(point) for point in (graph.get("points") or []))
            effector_graph_signatures[(
                graph.get("control_number"),
                graph.get("control_data_size"),
                points,
            )] += 1
        for layer in result.get("vector_layers") or []:
            layer_vector_types[(
                layer.get("LayerType"),
                layer.get("LayerFolder"),
                layer.get("VectorNormalBalloonIndex"),
                layer.get("VectorNormalType"),
            )] += 1
            header = layer.get("external_header") or {}
            coverage = layer.get("compact_record_coverage") or {}
            if header:
                external_header_counts[(
                    header.get("header_len"),
                    header.get("tag"),
                    header.get("payload_offset"),
                    header.get("payload_size_matches_body"),
                    header.get("matches_vector_data"),
                    coverage.get("fully_covered_after_prefix"),
                    coverage.get("record_count"),
                    coverage.get("trailing_gap"),
                )] += 1
        for stroke in result.get("strokes") or []:
            shape_counts[(tuple(stroke["shape"]), stroke["flags"])] += 1
            brush_usage[("stroke_92", stroke.get("brush_style_id"))] += 1
            raw_u32 = ((stroke.get("raw_header") or {}).get("u32_be") or {})
            raw_context = (
                tuple(stroke.get("shape") or []),
                stroke.get("flags"),
                stroke.get("brush_style_id"),
            )
            for rel, value in raw_u32.items():
                stroke_raw_u32[str(rel)][raw_context + (value,)] += 1
            _collect_compact_draw_lens(
                stroke.get("compact_semantic_probe"),
                ("stroke_92",) + raw_context,
            )
            _collect_cubic64_point_stats(stroke.get("point_stats"), ("stroke_92",) + raw_context)
            style = styles.get(str(stroke["brush_style_id"]))
            if not style:
                continue
            no_pattern_dab_candidate_counts[
                _no_pattern_dab_candidate_key(
                    "stroke_92",
                    stroke.get("brush_style_id"),
                    style,
                    stroke.get("flags"),
                )
            ] += 1
            status = style.get("importer_renderer_status") or {}
            _collect_renderer_status(
                result.get("name") or "",
                "stroke_92",
                stroke.get("brush_style_id"),
                status,
            )
            combo = _style_combo(style)
            if combo is not None:
                style_combos[combo] += 1
                style_combos_by_kind[("stroke_92",) + combo] += 1
            spacing_combo = _native_spacing_combo(style)
            if spacing_combo is not None:
                native_spacing_combos_by_kind[
                    ("stroke_92", stroke.get("brush_style_id")) + spacing_combo
                ] += 1
            spacing_estimate = _native_spacing_estimate(
                "stroke_92",
                stroke.get("brush_style_id"),
                style,
                stroke.get("width"),
                stroke.get("point_stats"),
                graphs,
            )
            spacing_estimate_key = _native_spacing_estimate_key(spacing_estimate)
            if spacing_estimate_key is not None:
                native_spacing_estimates_by_kind[spacing_estimate_key] += 1
            spacing_bucket_key = _native_spacing_summary_bucket_key(spacing_estimate)
            if spacing_bucket_key is not None:
                native_spacing_estimate_bucket_counts[spacing_bucket_key] += 1
            spacing_comparison_key = _importer_spacing_comparison_key(
                _importer_spacing_comparison(spacing_estimate, style, mod)
            )
            if spacing_comparison_key is not None:
                importer_spacing_comparisons_by_kind[
                    ("stroke_92", stroke.get("brush_style_id")) + spacing_comparison_key
                ] += 1
            adaptive_candidate_key = _experimental_adaptive_spacing_candidate_key(
                _experimental_adaptive_spacing_candidate(spacing_estimate, style, mod)
            )
            if adaptive_candidate_key is not None:
                experimental_adaptive_spacing_candidates_by_kind[
                    ("stroke_92", stroke.get("brush_style_id")) + adaptive_candidate_key
                ] += 1
            style_flag_key = _style_flag_key(style)
            if style_flag_key is not None:
                style_flags_by_kind[("stroke_92", stroke.get("brush_style_id")) + style_flag_key] += 1
            color_combo = _color_mix_combo(style)
            if color_combo is not None:
                color_mix_combos[color_combo] += 1
                color_mix_combos_by_kind[("stroke_92",) + color_combo] += 1
            color_change_combo = _color_change_combo(style)
            if color_change_combo is not None:
                color_change_combos[color_change_combo] += 1
                color_change_combos_by_kind[("stroke_92",) + color_change_combo] += 1
            blur_combo = _blur_combo(style)
            if blur_combo is not None:
                blur_combos[blur_combo] += 1
                blur_combos_by_kind[("stroke_92",) + blur_combo] += 1
            rotation_combo = _rotation_combo(style)
            if rotation_combo is not None:
                rotation_combos[rotation_combo] += 1
                rotation_combos_by_kind[("stroke_92",) + rotation_combo] += 1
            texture_combo = _texture_combo(style)
            if texture_combo is not None:
                texture_combos[texture_combo] += 1
                texture_combos_by_kind[("stroke_92",) + texture_combo] += 1
            pattern_ref = _pattern_material_ref(style, resources)
            if pattern_ref is not None:
                pattern_material_refs_by_kind[("stroke_92", stroke.get("brush_style_id")) + pattern_ref] += 1
            texture_ref = _texture_resource_ref(style, resources)
            if texture_ref is not None:
                texture_resource_refs_by_kind[("stroke_92", stroke.get("brush_style_id")) + texture_ref] += 1
            compiled_note_key = _compiled_line_style_note_key(style, resources)
            if compiled_note_key is not None:
                compiled_line_style_notes_by_kind[
                    ("stroke_92", stroke.get("brush_style_id")) + compiled_note_key
                ] += 1
            spray_combo = _spray_combo(style)
            if spray_combo is not None:
                spray_combos[spray_combo] += 1
                spray_combos_by_kind[("stroke_92",) + spray_combo] += 1
            effector_context = _active_effector_context(style)
            _collect_effector_point_float_stats(
                stroke.get("point_stats"),
                ("stroke_92",) + raw_context,
                effector_context,
            )
            _collect_effector_point_u32_stats(
                stroke.get("point_stats"),
                ("stroke_92",) + raw_context,
                effector_context,
            )
            _collect_style_effector_stats(style, graph_ids)
        for obj in result.get("objects_100") or []:
            obj_context = (
                tuple(obj.get("shape") or []),
                obj.get("family_id"),
                obj.get("line_style_id"),
                obj.get("fill_style_id"),
                tuple(obj.get("line_rgb") or []),
                tuple(obj.get("fill_rgb") or []),
            )
            object_100_shape_counts[(
                obj_context[0],
                obj_context[1],
                obj_context[2],
                obj_context[3],
                obj_context[4],
                obj_context[5],
            )] += 1
            layer_fields = obj.get("layer_vector_fields") or {}
            object_100_layer_counts[(
                obj.get("family_id"),
                obj.get("line_style_id"),
                obj.get("fill_style_id"),
                layer_fields.get("VectorNormalBalloonIndex"),
                layer_fields.get("VectorNormalType"),
                bool(layer_fields.get("ComicFrameLineMipmap")),
                layer_fields.get("ComicFrameColorTypeIndex"),
                layer_fields.get("ComicFrameColorTypeBlackChecked"),
                layer_fields.get("ComicFrameColorTypeWhiteChecked"),
            )] += 1
            brush_usage[("object_100_line", obj.get("line_style_id"))] += 1
            brush_usage[("object_100_fill", obj.get("fill_style_id"))] += 1
            raw_header = obj.get("raw_header") or {}
            for rel, value in (raw_header.get("u32_be") or {}).items():
                object_100_raw_u32[str(rel)][obj_context + (value,)] += 1
            for rel, value in (raw_header.get("f64_be") or {}).items():
                object_100_raw_f64[str(rel)][obj_context + (value,)] += 1
            _collect_compact_draw_lens(
                obj.get("compact_semantic_probe"),
                ("object_100",) + obj_context,
            )
            _collect_cubic64_point_stats(obj.get("point_stats"), ("object_100",) + obj_context)
            obj_style = styles.get(str(obj.get("line_style_id")))
            if isinstance(obj_style, dict):
                no_pattern_dab_candidate_counts[
                    _no_pattern_dab_candidate_key(
                        "object_100_line",
                        obj.get("line_style_id"),
                        obj_style,
                    )
                ] += 1
                status = obj_style.get("importer_renderer_status") or {}
                _collect_renderer_status(
                    result.get("name") or "",
                    "object_100_line",
                    obj.get("line_style_id"),
                    status,
                )
            combo = _style_combo(obj_style)
            if combo is not None:
                style_combos_by_kind[("object_100",) + combo] += 1
            spacing_combo = _native_spacing_combo(obj_style)
            if spacing_combo is not None:
                native_spacing_combos_by_kind[
                    ("object_100_line", obj.get("line_style_id")) + spacing_combo
                ] += 1
            spacing_estimate = _native_spacing_estimate(
                "object_100_line",
                obj.get("line_style_id"),
                obj_style,
                obj.get("width"),
                obj.get("point_stats"),
                graphs,
            )
            spacing_estimate_key = _native_spacing_estimate_key(spacing_estimate)
            if spacing_estimate_key is not None:
                native_spacing_estimates_by_kind[spacing_estimate_key] += 1
            spacing_bucket_key = _native_spacing_summary_bucket_key(spacing_estimate)
            if spacing_bucket_key is not None:
                native_spacing_estimate_bucket_counts[spacing_bucket_key] += 1
            spacing_comparison_key = _importer_spacing_comparison_key(
                _importer_spacing_comparison(spacing_estimate, obj_style, mod)
            )
            if spacing_comparison_key is not None:
                importer_spacing_comparisons_by_kind[
                    ("object_100_line", obj.get("line_style_id")) + spacing_comparison_key
                ] += 1
            adaptive_candidate_key = _experimental_adaptive_spacing_candidate_key(
                _experimental_adaptive_spacing_candidate(spacing_estimate, obj_style, mod)
            )
            if adaptive_candidate_key is not None:
                experimental_adaptive_spacing_candidates_by_kind[
                    ("object_100_line", obj.get("line_style_id")) + adaptive_candidate_key
                ] += 1
            style_flag_key = _style_flag_key(obj_style)
            if style_flag_key is not None:
                style_flags_by_kind[("object_100_line", obj.get("line_style_id")) + style_flag_key] += 1
            color_combo = _color_mix_combo(obj_style)
            if color_combo is not None:
                color_mix_combos[color_combo] += 1
                color_mix_combos_by_kind[("object_100",) + color_combo] += 1
            color_change_combo = _color_change_combo(obj_style)
            if color_change_combo is not None:
                color_change_combos[color_change_combo] += 1
                color_change_combos_by_kind[("object_100",) + color_change_combo] += 1
            blur_combo = _blur_combo(obj_style)
            if blur_combo is not None:
                blur_combos[blur_combo] += 1
                blur_combos_by_kind[("object_100",) + blur_combo] += 1
            rotation_combo = _rotation_combo(obj_style)
            if rotation_combo is not None:
                rotation_combos[rotation_combo] += 1
                rotation_combos_by_kind[("object_100",) + rotation_combo] += 1
            texture_combo = _texture_combo(obj_style)
            if texture_combo is not None:
                texture_combos[texture_combo] += 1
                texture_combos_by_kind[("object_100",) + texture_combo] += 1
            pattern_ref = _pattern_material_ref(obj_style, resources)
            if pattern_ref is not None:
                pattern_material_refs_by_kind[("object_100_line", obj.get("line_style_id")) + pattern_ref] += 1
            texture_ref = _texture_resource_ref(obj_style, resources)
            if texture_ref is not None:
                texture_resource_refs_by_kind[("object_100_line", obj.get("line_style_id")) + texture_ref] += 1
            compiled_note_key = _compiled_line_style_note_key(obj_style, resources)
            if compiled_note_key is not None:
                compiled_line_style_notes_by_kind[
                    ("object_100_line", obj.get("line_style_id")) + compiled_note_key
                ] += 1
            spray_combo = _spray_combo(obj_style)
            if spray_combo is not None:
                spray_combos[spray_combo] += 1
                spray_combos_by_kind[("object_100",) + spray_combo] += 1
            _collect_style_effector_stats(obj_style, graph_ids)
            effector_context = _active_effector_context(obj_style)
            _collect_effector_point_float_stats(
                obj.get("point_stats"),
                ("object_100",) + obj_context[:3],
                effector_context,
            )
            _collect_effector_point_u32_stats(
                obj.get("point_stats"),
                ("object_100",) + obj_context[:3],
                effector_context,
            )
            fill_style = fill_styles.get(str(obj.get("fill_style_id")))
            if isinstance(fill_style, dict):
                status = fill_style.get("importer_renderer_status") or {}
                _collect_renderer_status(
                    result.get("name") or "",
                    "object_100_fill",
                    obj.get("fill_style_id"),
                    status,
                )
            fill_style_combo = _fill_style_combo(fill_style)
            if fill_style_combo is not None:
                fill_style_combos_by_kind[("object_100_fill",) + fill_style_combo] += 1
            object_100_fill_contexts[(
                obj.get("family_id"),
                obj.get("fill_style_id"),
                None if fill_style_combo is None else fill_style_combo[2],
                obj.get("layer_child_count"),
                tuple(obj.get("fill_rgb") or []),
                layer_fields.get("VectorNormalType"),
                bool(layer_fields.get("ComicFrameLineMipmap")),
                layer_fields.get("ComicFrameColorTypeWhiteChecked"),
            )] += 1

    def _raw_u32_summary(
        counters: dict[str, collections.Counter[tuple[Any, ...]]],
        *,
        limit_offsets: int = 40,
        limit_values: int = 12,
    ) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        for rel, counter in sorted(counters.items(), key=lambda item: int(item[0])):
            values = []
            for key, count in counter.most_common(limit_values):
                item = {
                    "count": count,
                    "shape": list(key[0]),
                    "value": key[-1],
                }
                if len(key) >= 7:
                    item.update({
                        "family_or_flags": key[1],
                        "line_style_id": key[2],
                        "fill_style_id": key[3],
                        "line_rgb": list(key[4]),
                        "fill_rgb": list(key[5]),
                    })
                else:
                    item.update({
                        "subtype_or_flags": key[1],
                        "brush_style_id": key[2],
                        "line_rgb": list(key[3]) if len(key) > 5 else None,
                        "fill_rgb": list(key[4]) if len(key) > 5 else None,
                    })
                values.append(item)
            items.append(
                {
                    "offset": int(rel),
                    "unique": len(counter),
                    "values": values,
                }
            )
            if len(items) >= limit_offsets:
                break
        return items

    def _raw_f64_summary(
        counters: dict[str, collections.Counter[tuple[Any, ...]]],
        *,
        limit_offsets: int = 20,
        limit_values: int = 12,
    ) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        for rel, counter in sorted(counters.items(), key=lambda item: int(item[0])):
            values = []
            for key, count in counter.most_common(limit_values):
                item = {
                    "count": count,
                    "shape": list(key[0]),
                    "value": key[-1],
                }
                if len(key) >= 7:
                    item.update({
                        "family_id": key[1],
                        "line_style_id": key[2],
                        "fill_style_id": key[3],
                        "line_rgb": list(key[4]),
                        "fill_rgb": list(key[5]),
                    })
                else:
                    item.update({
                        "subtype": key[1],
                        "brush_style_id": key[2],
                        "line_rgb": list(key[3]),
                        "fill_rgb": list(key[4]),
                    })
                values.append(item)
            items.append(
                {
                    "offset": int(rel),
                    "unique": len(counter),
                    "values": values,
                }
            )
            if len(items) >= limit_offsets:
                break
        return items

    def _point_cubic64_u32_summary(
        counters: dict[str, collections.Counter[tuple[Any, ...]]],
        *,
        limit_offsets: int = 8,
        limit_values: int = 12,
    ) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        for rel, counter in sorted(counters.items(), key=lambda item: int(item[0])):
            values = []
            for key, count in counter.most_common(limit_values):
                kind = key[0]
                shape = key[1]
                subtype_or_flags = key[2]
                brush_style_id = key[3]
                value = key[-1]
                values.append(
                    {
                        "count": count,
                        "kind": kind,
                        "shape": list(shape),
                        "subtype_or_flags": subtype_or_flags,
                        "brush_style_id": brush_style_id,
                        "value": value,
                    }
                )
            items.append({"offset": int(rel), "unique": len(counter), "values": values})
            if len(items) >= limit_offsets:
                break
        return items

    def _point_cubic64_f64_summary(
        counters: dict[str, collections.Counter[tuple[Any, ...]]],
        *,
        limit_offsets: int = 8,
        limit_values: int = 12,
    ) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        for rel, counter in sorted(counters.items(), key=lambda item: int(item[0])):
            values = []
            for key, count in counter.most_common(limit_values):
                kind = key[0]
                shape = key[1]
                subtype_or_flags = key[2]
                brush_style_id = key[3]
                values.append(
                    {
                        "count": count,
                        "kind": kind,
                        "shape": list(shape),
                        "subtype_or_flags": subtype_or_flags,
                        "brush_style_id": brush_style_id,
                        "min": key[-4],
                        "max": key[-3],
                        "avg": key[-2],
                        "uniq": key[-1],
                    }
                )
            items.append({"offset": int(rel), "unique": len(counter), "values": values})
            if len(items) >= limit_offsets:
                break
        return items

    def _effector_point_float_summary(
        counters: dict[str, collections.Counter[tuple[Any, ...]]],
        *,
        limit_offsets: int = 12,
        limit_values: int = 24,
        varying_only: bool = False,
    ) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        for rel, counter in sorted(counters.items(), key=lambda item: int(item[0])):
            values = []
            for key, count in counter.most_common(limit_values):
                if varying_only and key[-4] == key[-3] and key[-1] <= 1:
                    continue
                kind = key[0]
                shape = key[1]
                subtype_or_flags = key[2]
                brush_style_id = key[3]
                effectors = key[4]
                values.append(
                    {
                        "count": count,
                        "kind": kind,
                        "shape": list(shape),
                        "subtype_or_flags": subtype_or_flags,
                        "brush_style_id": brush_style_id,
                        "effectors": list(effectors),
                        "min": key[-4],
                        "max": key[-3],
                        "avg": key[-2],
                        "uniq": key[-1],
                    }
                )
            items.append({"offset": int(rel), "unique": len(counter), "values": values})
            if len(items) >= limit_offsets:
                break
        return items

    def _effector_point_u32_summary(
        counters: dict[str, collections.Counter[tuple[Any, ...]]],
        *,
        limit_offsets: int = len(POINT_U32_FIELDS),
        limit_values: int = 24,
    ) -> list[dict[str, Any]]:
        items: list[dict[str, Any]] = []
        for rel, counter in sorted(counters.items(), key=lambda item: int(item[0])):
            values = []
            for key, count in counter.most_common(limit_values):
                kind = key[0]
                shape = key[1]
                subtype_or_flags = key[2]
                brush_style_id = key[3]
                effectors = key[4]
                values.append(
                    {
                        "count": count,
                        "kind": kind,
                        "shape": list(shape),
                        "subtype_or_flags": subtype_or_flags,
                        "brush_style_id": brush_style_id,
                        "effectors": list(effectors),
                        "value": key[-1],
                    }
                )
            items.append({"offset": int(rel), "unique": len(counter), "values": values})
            if len(items) >= limit_offsets:
                break
        return items

    renderer_recommendations = _renderer_gap_recommendations(
        renderer_gap_inputs_by_kind,
        renderer_gap_inputs_by_clip,
    )

    return {
        "clip_count": len(results),
        "vector_layer_count": sum(int(result.get("vector_layer_count") or 0) for result in results),
        "stroke_count": sum(int(result.get("stroke_count") or 0) for result in results),
        "object_100_count": sum(int(result.get("object_100_count") or 0) for result in results),
        "native_pwbrushstyle_evaluator_0x1422D8550": NATIVE_PWBRUSHSTYLE_EVALUATOR,
        "native_vector_sampler_spacing": NATIVE_VECTOR_SAMPLER_SPACING,
        "native_no_pattern_dab_pipeline": NATIVE_NO_PATTERN_DAB_PIPELINE,
        "importer_vector_fallback_policy": _importer_vector_fallback_policy(mod),
        "importer_vector_renderer_policy": _importer_vector_renderer_policy(),
        "layer_vector_types": [
            {
                "count": count,
                "layer_type": key[0],
                "layer_folder": key[1],
                "vector_balloon_index": key[2],
                "vector_normal_type": key[3],
            }
            for key, count in layer_vector_types.most_common()
        ],
        "external_header_counts": [
            {
                "count": count,
                "header_len": key[0],
                "tag": key[1],
                "payload_offset": key[2],
                "payload_size_matches_body": key[3],
                "matches_vector_data": key[4],
                "fully_covered_after_prefix": key[5],
                "record_count": key[6],
                "trailing_gap": key[7],
            }
            for key, count in external_header_counts.most_common()
        ],
        "shape_counts": [
            {"count": count, "shape": list(shape), "flags": flags}
            for (shape, flags), count in shape_counts.most_common()
        ],
        "object_100_shape_counts": [
            {
                "count": count,
                "shape": list(shape),
                "family_id": family_id,
                "family_flags": _decode_balloon_family_flags(int(family_id)),
                "line_style_id": line_style_id,
                "fill_style_id": fill_style_id,
                "line_rgb": list(line_rgb),
                "fill_rgb": list(fill_rgb),
            }
            for (shape, family_id, line_style_id, fill_style_id, line_rgb, fill_rgb), count
            in object_100_shape_counts.most_common()
        ],
        "object_100_layer_counts": [
            {
                "count": count,
                "family_id": family_id,
                "family_flags": _decode_balloon_family_flags(int(family_id)),
                "line_style_id": line_style_id,
                "fill_style_id": fill_style_id,
                "vector_normal_balloon_index": vector_balloon_index,
                "vector_normal_type": vector_normal_type,
                "has_comic_frame_line_mipmap": has_comic_frame_line_mipmap,
                "comic_frame_color_type_index": comic_frame_color_type_index,
                "comic_frame_black_checked": comic_frame_black_checked,
                "comic_frame_white_checked": comic_frame_white_checked,
            }
            for (
                family_id,
                line_style_id,
                fill_style_id,
                vector_balloon_index,
                vector_normal_type,
                has_comic_frame_line_mipmap,
                comic_frame_color_type_index,
                comic_frame_black_checked,
                comic_frame_white_checked,
            ), count in object_100_layer_counts.most_common()
        ],
        "object_100_fill_contexts": [
            {
                "count": count,
                "family_id": family_id,
                "family_flags": _decode_balloon_family_flags(int(family_id)),
                "fill_style_id": fill_style_id,
                "fill_composite_mode": fill_composite_mode,
                "layer_child_count": layer_child_count,
                "fill_rgb": list(fill_rgb),
                "vector_normal_type": vector_normal_type,
                "has_comic_frame_line_mipmap": has_comic_frame_line_mipmap,
                "comic_frame_white_checked": comic_frame_white_checked,
            }
            for (
                family_id,
                fill_style_id,
                fill_composite_mode,
                layer_child_count,
                fill_rgb,
                vector_normal_type,
                has_comic_frame_line_mipmap,
                comic_frame_white_checked,
            ), count in object_100_fill_contexts.most_common()
        ],
        "object_100_raw_u32": _raw_u32_summary(object_100_raw_u32),
        "object_100_raw_f64": _raw_f64_summary(object_100_raw_f64),
        "stroke_raw_u32": _raw_u32_summary(stroke_raw_u32),
        "compact_draw_flag_candidate_lenses": [
            {
                "count": count,
                "kind": key[0],
                "shape": list(key[1]),
                "family_or_flags": key[2],
                "line_style_id": key[3],
                "fill_style_id": key[4] if key[0] == "object_100" else None,
                "line_rgb": list(key[5]) if key[0] == "object_100" and len(key) > 11 else None,
                "fill_rgb": list(key[6]) if key[0] == "object_100" and len(key) > 11 else None,
                "candidate_offset": key[-6],
                "value": key[-5],
                "low_nibble": key[-4],
                "bit0_line_lens": key[-3],
                "bit1_fill_lens": key[-2],
                "bit2_special_lens": key[-1],
            }
            for key, count in compact_draw_lenses.most_common(80)
        ],
        "point_u32_stats": _point_cubic64_u32_summary(
            point_u32_stats,
            limit_offsets=len(POINT_U32_FIELDS),
            limit_values=20,
        ),
        "point_native32_u32": _point_cubic64_u32_summary(point_native32_u32),
        "point_cubic64_u32": _point_cubic64_u32_summary(point_cubic64_u32),
        "point_cubic64_f64_stats": _point_cubic64_f64_summary(point_cubic64_f64_stats),
        "point_float_stats": _point_cubic64_f64_summary(point_float_stats, limit_offsets=12, limit_values=20),
        "effector_point_float_stats": _effector_point_float_summary(effector_point_float_stats),
        "effector_point_float_varying_stats": _effector_point_float_summary(
            effector_point_float_stats,
            varying_only=True,
        ),
        "effector_point_u32_stats": _effector_point_u32_summary(effector_point_u32_stats),
        "brush_usage": [
            {"count": count, "kind": kind, "style_id": style_id}
            for (kind, style_id), count in brush_usage.most_common()
        ],
        "style_usage": [
            {"count": count, "kind": kind, "style_id": style_id}
            for (kind, style_id), count in brush_usage.most_common()
        ],
        "renderer_status_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "style_id": combo[1],
                "coverage": combo[2],
                "pixel_affecting_inputs": list(combo[3]),
                "preview_only_inputs": list(combo[4]),
                "diagnostic_only_inputs": list(combo[5]),
            }
            for combo, count in renderer_status_by_kind.most_common()
        ],
        "renderer_coverage_counts": [
            {
                "count": count,
                "kind": combo[0],
                "coverage": combo[1],
            }
            for combo, count in renderer_coverage_counts.most_common()
        ],
        "renderer_gap_inputs_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "diagnostic_only_input": combo[1],
            }
            for combo, count in renderer_gap_inputs_by_kind.most_common()
        ],
        "renderer_gap_inputs_by_clip": [
            {
                "kind": combo[0],
                "diagnostic_only_input": combo[1],
                "clip_count": len(clips),
                "clips": sorted(clips),
            }
            for combo, clips in sorted(
                renderer_gap_inputs_by_clip.items(),
                key=lambda item: (item[0][0], str(item[0][1])),
            )
        ],
        "renderer_gap_recommendations": renderer_recommendations,
        "sample_request_recommendations": _sample_request_recommendations(
            renderer_recommendations,
            spray_combos_by_kind,
            color_mix_combos_by_kind,
            blur_combos_by_kind,
        ),
        "no_pattern_dab_candidate_counts": [
            {
                "count": count,
                "kind": combo[0],
                "category": combo[1],
                "style_id": combo[2],
                "flags": combo[3],
                "dynamic_inputs": list(combo[4]),
            }
            for combo, count in no_pattern_dab_candidate_counts.most_common()
        ],
        "no_pattern_dab_candidate_category_counts": [
            row
            for row in _no_pattern_dab_category_rows()
        ],
        "style_combos": [
            {
                "count": count,
                "size": combo[0],
                "opacity": combo[1],
                "flow": combo[2],
                "thickness": combo[3],
                "flow_base": combo[4],
                "hardness": combo[5],
                "hardness_profile_active": _hardness_profile_info(combo[5])[0],
                "hardness_threshold": _hardness_profile_info(combo[5])[1],
                "style_flag": combo[6],
                "texture": combo[7],
                "pattern": combo[8],
                "auto_interval": combo[9],
                "anti_alias": combo[10],
                "interval_base": combo[11],
                "interval_effector": combo[12],
            }
            for combo, count in style_combos.most_common()
        ],
        "style_combos_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "size": combo[1],
                "opacity": combo[2],
                "flow": combo[3],
                "thickness": combo[4],
                "flow_base": combo[5],
                "hardness": combo[6],
                "hardness_profile_active": _hardness_profile_info(combo[6])[0],
                "hardness_threshold": _hardness_profile_info(combo[6])[1],
                "style_flag": combo[7],
                "texture": combo[8],
                "pattern": combo[9],
                "auto_interval": combo[10],
                "anti_alias": combo[11],
                "interval_base": combo[12],
                "interval_effector": combo[13],
            }
            for combo, count in style_combos_by_kind.most_common()
        ],
        "native_spacing_combos_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "style_id": combo[1],
                "branch": combo[2],
                "pen_radius": combo[3],
                "size_effector": combo[4],
                "interval_base": combo[5],
                "interval_effector": combo[6],
                "auto_interval_type": combo[7],
                "thickness_base": combo[8],
                "thickness_effector": combo[9],
                "hardness": combo[10],
                "anti_alias": combo[11],
                "spray_flag": combo[12],
                "styleflag_subpixel_min_size_0x8": combo[13],
                "styleflag_retained_state_path_0x20": combo[14],
                "styleflag_texture_or_water_0x10000": combo[15],
                "styleflag_texture_or_water_0x20000": combo[16],
            }
            for combo, count in native_spacing_combos_by_kind.most_common()
        ],
        "native_spacing_estimates_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "style_id": combo[1],
                "branch": combo[2],
                "base_width": combo[3],
                "compact_f32_56_size_factor_range": list(combo[4]),
                "size_effector": combo[5],
                "size_effector_dynamic": combo[6],
                "size_effector_graph_output": json.loads(combo[7]) if combo[7] != "null" else None,
                "size_effector_multiplier_estimate": json.loads(combo[8]) if combo[8] != "null" else None,
                "interval_scalar": combo[9],
                "thickness_guess": combo[10],
                "effective_size_without_size_effector_range": list(combo[11]),
                "state8_without_size_effector_range": list(combo[12]),
                "effective_size_with_estimated_size_effector_range": list(combo[13]),
                "state8_with_estimated_size_effector_range": list(combo[14]),
                "unestimated_reason": combo[15],
            }
            for combo, count in native_spacing_estimates_by_kind.most_common()
        ],
        "native_spacing_estimate_bucket_counts": [
            {
                "count": count,
                "kind": combo[0],
                "branch": combo[1],
                "range_bucket": combo[2],
                "min_bucket": combo[3],
                "max_bucket": combo[4],
                "size_effector_dynamic": combo[5],
                "has_size_effector_multiplier_estimate": combo[6],
                "range_source": combo[7],
                "unestimated_reason": combo[8],
            }
            for combo, count in native_spacing_estimate_bucket_counts.most_common()
        ],
        "native_spacing_estimate_recommendations": _native_spacing_recommendations(
            native_spacing_estimate_bucket_counts
        ),
        "importer_spacing_comparisons_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "style_id": combo[1],
                "sample_step_source": combo[2],
                "importer_sample_step": combo[3],
                "native_state8_bucket": combo[4],
                "classification": combo[5],
                "native_state8_range": list(combo[6]),
                "native_state8_to_importer_step_ratio_range": list(combo[7]),
            }
            for combo, count in importer_spacing_comparisons_by_kind.most_common()
        ],
        "importer_spacing_comparison_recommendations": _importer_spacing_comparison_recommendations(
            importer_spacing_comparisons_by_kind
        ),
        "experimental_adaptive_spacing_candidates_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "style_id": combo[1],
                "candidate": combo[2],
                "size_effector_form": combo[3],
                "fallback_step_source": combo[4],
                "fallback_step": combo[5],
                "min_step": combo[6],
                "native_state8_range": list(combo[7]),
                "estimated_adaptive_step_range": list(combo[8]),
                "would_reduce_step": combo[9],
                "inactive_reason": combo[10],
            }
            for combo, count in experimental_adaptive_spacing_candidates_by_kind.most_common()
        ],
        "fill_style_combos_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "style_flag": combo[1],
                "anti_alias": combo[2],
                "composite_mode": combo[3],
                "texture_density": combo[4],
            }
            for combo, count in fill_style_combos_by_kind.most_common()
        ],
        "color_mix_combos": [
            {
                "count": count,
                "use_water_color": combo[0],
                "water_color_type": combo[1],
                "water_writer_mode": _water_writer_mode(combo[0], combo[1]),
                "brush_color_mixing_mode": combo[2],
                "brush_lms_linearity": combo[3],
                "mix_color_base": combo[4],
                "mix_color_effector": combo[5],
                "mix_alpha_base": combo[6],
                "mix_alpha_effector": combo[7],
                "color_extension": combo[8],
                "water_edge_flag": combo[9],
                "water_edge_radius": combo[10],
                "water_edge_alpha_power": combo[11],
                "water_edge_value_power": combo[12],
                "water_edge_blur": combo[13],
            }
            for combo, count in color_mix_combos.most_common()
        ],
        "color_mix_combos_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "use_water_color": combo[1],
                "water_color_type": combo[2],
                "water_writer_mode": _water_writer_mode(combo[1], combo[2]),
                "brush_color_mixing_mode": combo[3],
                "brush_lms_linearity": combo[4],
                "mix_color_base": combo[5],
                "mix_color_effector": combo[6],
                "mix_alpha_base": combo[7],
                "mix_alpha_effector": combo[8],
                "color_extension": combo[9],
                "water_edge_flag": combo[10],
                "water_edge_radius": combo[11],
                "water_edge_alpha_power": combo[12],
                "water_edge_value_power": combo[13],
                "water_edge_blur": combo[14],
            }
            for combo, count in color_mix_combos_by_kind.most_common()
        ],
        "color_change_combos": [
            {
                "count": count,
                "sub_color_base": combo[0],
                "sub_color_effector": combo[1],
                "hue_change_base": combo[2],
                "hue_change_effector": combo[3],
                "saturation_change_base": combo[4],
                "saturation_change_effector": combo[5],
                "value_change_base": combo[6],
                "value_change_effector": combo[7],
                "change_draw_color_target": combo[8],
                "color_extension": combo[9],
            }
            for combo, count in color_change_combos.most_common()
        ],
        "color_change_combos_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "sub_color_base": combo[1],
                "sub_color_effector": combo[2],
                "hue_change_base": combo[3],
                "hue_change_effector": combo[4],
                "saturation_change_base": combo[5],
                "saturation_change_effector": combo[6],
                "value_change_base": combo[7],
                "value_change_effector": combo[8],
                "change_draw_color_target": combo[9],
                "color_extension": combo[10],
            }
            for combo, count in color_change_combos_by_kind.most_common()
        ],
        "blur_combos": [
            {
                "count": count,
                "blur_kind": combo[0],
                "blur_base": combo[1],
                "blur_effector": combo[2],
                "use_water_color": combo[3],
                "water_color_type": combo[4],
            }
            for combo, count in blur_combos.most_common()
        ],
        "blur_combos_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "blur_kind": combo[1],
                "blur_base": combo[2],
                "blur_effector": combo[3],
                "use_water_color": combo[4],
                "water_color_type": combo[5],
            }
            for combo, count in blur_combos_by_kind.most_common()
        ],
        "rotation_combos": [
            {
                "count": count,
                "rotation_base": combo[0],
                "rotation_effector": combo[1],
                "rotation_effector_bits": list(combo[2]),
                "rotation_random": combo[3],
                "rotation_in_spray_base": combo[4],
                "rotation_effector_in_spray": combo[5],
                "rotation_effector_in_spray_bits": list(combo[6]),
                "rotation_random_in_spray": combo[7],
            }
            for combo, count in rotation_combos.most_common()
        ],
        "rotation_combos_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "rotation_base": combo[1],
                "rotation_effector": combo[2],
                "rotation_effector_bits": list(combo[3]),
                "rotation_random": combo[4],
                "rotation_in_spray_base": combo[5],
                "rotation_effector_in_spray": combo[6],
                "rotation_effector_in_spray_bits": list(combo[7]),
                "rotation_random_in_spray": combo[8],
            }
            for combo, count in rotation_combos_by_kind.most_common()
        ],
        "texture_combos": [
            {
                "count": count,
                "texture_pattern": combo[0],
                "texture_flag": combo[1],
                "texture_composite": combo[2],
                "texture_scale": combo[3],
                "texture_rotate": combo[4],
                "texture_offset_x": combo[5],
                "texture_offset_y": combo[6],
                "texture_brightness": combo[7],
                "texture_contrast": combo[8],
                "texture_density_base": combo[9],
                "texture_density_effector": combo[10],
            }
            for combo, count in texture_combos.most_common()
        ],
        "texture_combos_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "texture_pattern": combo[1],
                "texture_flag": combo[2],
                "texture_composite": combo[3],
                "texture_scale": combo[4],
                "texture_rotate": combo[5],
                "texture_offset_x": combo[6],
                "texture_offset_y": combo[7],
                "texture_brightness": combo[8],
                "texture_contrast": combo[9],
                "texture_density_base": combo[10],
                "texture_density_effector": combo[11],
            }
            for combo, count in texture_combos_by_kind.most_common()
        ],
        "pattern_material_refs_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "style_id": combo[1],
                "pattern_style_id": combo[2],
                "pattern_image_ids": list(combo[3]),
                "pattern_image_names": list(combo[4]),
                "pattern_image_mipmaps": list(combo[5]),
                "image_number": combo[6],
                "order_type": combo[7],
                "order_type_label": _pattern_order_type_label(combo[7]),
                "reverse2": combo[8],
                "reverse2_decoded": _pattern_reverse2_bits(combo[8]),
            }
            for combo, count in pattern_material_refs_by_kind.most_common()
        ],
        "texture_resource_refs_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "style_id": combo[1],
                "texture_pattern_id": combo[2],
                "texture_image_name": combo[3],
                "texture_image_mipmap": combo[4],
                "texture_flag": combo[5],
                "texture_composite": combo[6],
                "texture_density_base": combo[7],
                "texture_density_effector": combo[8],
                "texture_brightness": combo[9],
                "texture_contrast": combo[10],
                "texture_scale": combo[11],
                "texture_rotate": combo[12],
                "texture_offset_x": combo[13],
                "texture_offset_y": combo[14],
            }
            for combo, count in texture_resource_refs_by_kind.most_common()
        ],
        "compiled_line_style_notes_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "style_id": combo[1],
                "runtime_object": combo[2],
                "has_pattern_material_list": combo[3],
                "has_texture_descriptor": combo[4],
                "compiled_state_not_serialized_directly": list(combo[5]),
            }
            for combo, count in compiled_line_style_notes_by_kind.most_common()
        ],
        "style_flags_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "style_id": combo[1],
                "style_flag_hex": combo[2],
                "native_subpixel_min_size_0x8": combo[3],
                "native_retained_state_path_0x20": combo[4],
                "native_thickness_axis_split_0x40": combo[5],
                "native_segment_start_flag_0x200": combo[6],
                "direct_max_accum_0x1000": combo[7],
                "texture_or_water_family_0x10000": combo[8],
                "texture_or_water_family_0x20000": combo[9],
                "unknown_bits": combo[10],
            }
            for combo, count in style_flags_by_kind.most_common()
        ],
        "spray_combos": [
            {
                "count": count,
                "spray_flag": combo[0],
                "spray_size_base": combo[1],
                "spray_size_effector": combo[2],
                "spray_density_base": combo[3],
                "spray_density_effector": combo[4],
                "spray_bias": combo[5],
                "fixed_spray": combo[6],
            }
            for combo, count in spray_combos.most_common()
        ],
        "spray_combos_by_kind": [
            {
                "count": count,
                "kind": combo[0],
                "spray_flag": combo[1],
                "spray_size_base": combo[2],
                "spray_size_effector": combo[3],
                "spray_density_base": combo[4],
                "spray_density_effector": combo[5],
                "spray_bias": combo[6],
                "fixed_spray": combo[7],
            }
            for combo, count in spray_combos_by_kind.most_common()
        ],
        "effector_keys": [
            {"count": count, "kind": kind, "key": key}
            for (kind, key), count in effector_keys.most_common()
        ],
        "effector_decoded_forms": [
            {
                "count": count,
                "effector": key[0],
                "form": key[1],
                "type": key[2],
                "graph_refs": list(key[3]),
                "scalars": list(key[4]),
                "scalar": key[5],
            }
            for key, count in effector_decoded_forms.most_common(120)
        ],
        "effector_runtime_branches": [
            {
                "count": count,
                "effector": key[0],
                "form": key[1],
                "type": key[2],
                "primary_graph": key[3],
                "secondary_graph": key[4],
                "aux_graph": key[5],
                "random": key[6],
                "velocity": key[7],
                "unknown_bits": key[8],
            }
            for key, count in effector_runtime_branches.most_common(120)
        ],
        "effector_graph_signatures": [
            {
                "count": count,
                "control_number": key[0],
                "control_data_size": key[1],
                "points": [list(point) for point in key[2]],
                "native_eval": _graph_eval_diagnostics([list(point) for point in key[2]]),
            }
            for key, count in effector_graph_signatures.most_common(80)
        ],
        "effector_graph_refs": [
            {
                "count": count,
                "effector": key[0],
                "effector_key": key[1],
                "graph_id": key[2],
                "graph_present": key[3],
            }
            for key, count in effector_graph_refs.most_common(120)
        ],
    }


def _expand_clip_args(values: list[str]) -> list[Path]:
    paths: list[Path] = []
    for value in values:
        if any(char in value for char in "*?[]"):
            pattern = Path(value)
            parent = pattern.parent if str(pattern.parent) != "." else Path(".")
            matches = sorted(parent.glob(pattern.name))
            paths.extend(path for path in matches if path.is_file())
        else:
            paths.append(Path(value))
    return paths


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("clips", nargs="+")
    parser.add_argument("--out", help="Write JSON to this path instead of stdout.")
    parser.add_argument(
        "--summary",
        action="store_true",
        help="Include corpus-level style/effector counts when scanning clips.",
    )
    parser.add_argument(
        "--summary-only",
        action="store_true",
        help="Only emit the corpus summary, not per-clip details.",
    )
    parser.add_argument(
        "--sample-requests-only",
        action="store_true",
        help="Only emit compact sample-making recommendations derived from the corpus summary.",
    )
    args = parser.parse_args()

    root = Path(__file__).resolve().parent
    mod = _load_loader(root)
    clip_paths = _expand_clip_args(args.clips)
    results = [_inspect_clip(mod, path) for path in clip_paths]
    include_summary = args.summary or args.summary_only or args.sample_requests_only or len(results) > 1

    if args.summary_only or args.sample_requests_only:
        summary = _summary(results, mod)
        if args.sample_requests_only:
            payload = {
                "clip_count": summary.get("clip_count"),
                "vector_layer_count": summary.get("vector_layer_count"),
                "stroke_count": summary.get("stroke_count"),
                "object_100_count": summary.get("object_100_count"),
                "sample_request_recommendations": summary.get("sample_request_recommendations"),
            }
        else:
            payload = summary
    elif len(results) == 1 and not include_summary:
        payload = results[0]
    else:
        payload = {"summary": _summary(results, mod), "clips": results}

    result = json.dumps(payload, ensure_ascii=False, indent=2)
    if args.out:
        Path(args.out).write_text(result + "\n", encoding="utf-8")
    else:
        print(result)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
