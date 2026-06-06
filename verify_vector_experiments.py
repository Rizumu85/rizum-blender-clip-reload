from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any


DEFAULT_GUARD_CLIPS = (
    "img/Vector_SizePressure.clip",
    "img/Vector_OpacityPressure.clip",
    "img/Test_Vector.clip",
    "img/Vector_AA_None.clip",
    "img/Vector_AA_Medium.clip",
    "img/Vector_Texture.clip",
    "img/Vector_NoTexture.clip",
)
DEFAULT_EXPERIMENT_ENV = "RIZUM_CLIP_EXPERIMENTAL_VECTOR_ADAPTIVE_SPACING=1"
ADAPTIVE_SPACING_REQUIRED_IMPROVEMENT = "Vector_SizePressure.clip"
ADAPTIVE_SPACING_REQUIRED_UNCHANGED = (
    "Vector_OpacityPressure.clip",
    "Test_Vector.clip",
    "Vector_AA_None.clip",
    "Vector_AA_Medium.clip",
    "Vector_Texture.clip",
    "Vector_NoTexture.clip",
)
METRICS = ("mean", "visible_px", "premul_mean", "premul_visible_px", "max")


def _parse_env(items: list[str]) -> dict[str, str]:
    out: dict[str, str] = {}
    for item in items:
        if "=" not in item:
            raise ValueError(f"Expected NAME=VALUE env assignment, got {item!r}.")
        name, value = item.split("=", 1)
        name = name.strip()
        if not name:
            raise ValueError(f"Empty env name in {item!r}.")
        out[name] = value
    return out


def _run_verify(root: Path, clip_path: Path, extra_env: dict[str, str]) -> dict[str, Any]:
    env = os.environ.copy()
    for name in extra_env:
        env.pop(name, None)
    env.update(extra_env)
    proc = subprocess.run(
        [sys.executable, str(root / "verify_one_clip.py"), str(clip_path)],
        cwd=root,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        return {
            "name": clip_path.name,
            "rendered": False,
            "returncode": proc.returncode,
            "stderr": proc.stderr.strip(),
            "stdout": proc.stdout.strip(),
        }
    try:
        result = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        return {
            "name": clip_path.name,
            "rendered": False,
            "error": f"verify_one_clip output was not JSON: {exc}",
            "stdout": proc.stdout.strip(),
            "stderr": proc.stderr.strip(),
        }
    if proc.stderr.strip():
        result["stderr"] = proc.stderr.strip()
    return result


def _delta(default: dict[str, Any], experiment: dict[str, Any]) -> dict[str, Any]:
    out: dict[str, Any] = {}
    for key in METRICS:
        left = default.get(key)
        right = experiment.get(key)
        if isinstance(left, (int, float)) and isinstance(right, (int, float)):
            out[f"{key}_delta"] = round(float(right) - float(left), 6)
    default_mean = default.get("mean")
    experiment_mean = experiment.get("mean")
    if isinstance(default_mean, (int, float)) and isinstance(experiment_mean, (int, float)):
        if experiment_mean < default_mean:
            out["mean_direction"] = "improved"
        elif experiment_mean > default_mean:
            out["mean_direction"] = "regressed"
        else:
            out["mean_direction"] = "unchanged"
    return out


def _duplicate_clip_basenames(paths: list[Path]) -> dict[str, list[str]]:
    seen: dict[str, list[str]] = {}
    for path in paths:
        seen.setdefault(path.name, []).append(str(path))
    return {name: values for name, values in sorted(seen.items()) if len(values) > 1}


def _summary(
    rows: list[dict[str, Any]],
    max_mean_regression: float,
    max_visible_regression: int | None,
    min_improved: int,
    require_improved: set[str],
    require_unchanged: set[str],
    fail_on_error: bool,
    duplicate_basenames: dict[str, list[str]],
) -> dict[str, Any]:
    counts = {"improved": 0, "unchanged": 0, "regressed": 0, "unknown": 0}
    failures: list[dict[str, Any]] = []
    warnings: list[dict[str, Any]] = []
    improved_names: set[str] = set()
    rows_by_name: dict[str, dict[str, Any]] = {}
    for name, paths in duplicate_basenames.items():
        warning = {
            "type": "duplicate_clip_basename",
            "name": name,
            "paths": paths,
            "message": (
                "Expected-direction rules are keyed by clip basename; duplicate basenames "
                "make those expectations ambiguous."
            ),
        }
        warnings.append(warning)
        if fail_on_error:
            failures.append(
                {
                    "type": "config_error",
                    "name": name,
                    "reasons": ["duplicate clip basename"],
                    "paths": paths,
                    "delta": {},
                }
            )
    for row in rows:
        if isinstance(row.get("name"), str):
            rows_by_name[str(row["name"])] = row
        delta = row.get("delta") or {}
        direction = delta.get("mean_direction")
        if direction in counts:
            counts[direction] += 1
        else:
            counts["unknown"] += 1
        if fail_on_error:
            error_reasons: list[str] = []
            if not row.get("default", {}).get("rendered"):
                error_reasons.append("default render failed")
            if not row.get("experiment", {}).get("rendered"):
                error_reasons.append("experiment render failed")
            if direction not in {"improved", "unchanged", "regressed"}:
                error_reasons.append("mean_direction missing")
            if error_reasons:
                failures.append(
                    {
                        "type": "render_error",
                        "name": row.get("name"),
                        "reasons": error_reasons,
                        "delta": delta,
                    }
                )
        if direction == "improved" and isinstance(row.get("name"), str):
            improved_names.add(str(row["name"]))

        mean_delta = delta.get("mean_delta")
        visible_delta = delta.get("visible_px_delta")
        reasons: list[str] = []
        if isinstance(mean_delta, (int, float)) and float(mean_delta) > max_mean_regression:
            reasons.append(f"mean_delta {mean_delta} > {max_mean_regression}")
        if (
            max_visible_regression is not None
            and isinstance(visible_delta, (int, float))
            and int(visible_delta) > max_visible_regression
        ):
            reasons.append(f"visible_px_delta {visible_delta} > {max_visible_regression}")
        if reasons:
            failures.append(
                {
                    "type": "regression",
                    "name": row.get("name"),
                    "reasons": reasons,
                    "delta": delta,
                }
            )
    if counts["improved"] < min_improved:
        failures.append(
            {
                "type": "activation",
                "name": "__summary__",
                "reasons": [f"improved count {counts['improved']} < required {min_improved}"],
                "delta": {},
            }
        )
    missing_required = sorted(require_improved - improved_names)
    for name in missing_required:
        failures.append(
            {
                "type": "expectation",
                "name": name,
                "reasons": [f"required improved clip {name!r} did not improve"],
                "delta": {},
            }
        )
    for name in sorted(require_unchanged):
        row = rows_by_name.get(name)
        delta = (row or {}).get("delta") or {}
        if delta.get("mean_direction") != "unchanged":
            failures.append(
                {
                    "type": "expectation",
                    "name": name,
                    "reasons": [f"required unchanged clip {name!r} changed"],
                    "delta": delta,
                }
            )
    return {
        "counts_by_mean_direction": counts,
        "thresholds": {
            "max_mean_regression": max_mean_regression,
            "max_visible_regression": max_visible_regression,
            "min_improved": min_improved,
            "require_improved": sorted(require_improved),
            "require_unchanged": sorted(require_unchanged),
            "fail_on_error": fail_on_error,
        },
        "duplicate_clip_basenames": duplicate_basenames,
        "expected_directions": _expected_directions(require_improved, require_unchanged),
        "failure_count": len(failures),
        "failure_counts_by_type": {
            failure_type: sum(1 for failure in failures if failure.get("type") == failure_type)
            for failure_type in sorted(
                {str(failure.get("type") or "unknown") for failure in failures}
            )
        },
        "passed": len(failures) == 0,
        "warnings": warnings,
        "failures": failures,
    }


def _expected_directions(require_improved: set[str], require_unchanged: set[str]) -> dict[str, str]:
    expected = {name: "improved" for name in sorted(require_improved)}
    for name in sorted(require_unchanged):
        if name in expected:
            expected[name] = f"{expected[name]}+unchanged"
        else:
            expected[name] = "unchanged"
    return expected


def _format_table(rows: list[dict[str, Any]], expected: dict[str, str]) -> str:
    headers = ("clip", "expected", "direction", "mean_delta", "visible_delta")
    table_rows: list[tuple[str, str, str, str, str]] = []
    for row in rows:
        delta = row.get("delta") or {}
        name = str(row.get("name") or "")
        table_rows.append(
            (
                name,
                str(row.get("expected_direction") or expected.get(name, "")),
                str(delta.get("mean_direction") or "unknown"),
                "" if delta.get("mean_delta") is None else str(delta.get("mean_delta")),
                "" if delta.get("visible_px_delta") is None else str(delta.get("visible_px_delta")),
            )
        )
    widths = [
        max(len(headers[idx]), *(len(row[idx]) for row in table_rows))
        for idx in range(len(headers))
    ]
    lines = [
        "  ".join(headers[idx].ljust(widths[idx]) for idx in range(len(headers))),
        "  ".join("-" * width for width in widths),
    ]
    for row in table_rows:
        lines.append("  ".join(row[idx].ljust(widths[idx]) for idx in range(len(headers))))
    return "\n".join(lines)


def _run_config(args: argparse.Namespace, clips: list[Path], env: dict[str, str]) -> dict[str, Any]:
    require_improved = set(args.require_improved)
    require_unchanged = set(args.require_unchanged)
    return {
        "clips": [str(path) for path in clips],
        "duplicate_clip_basenames": _duplicate_clip_basenames(clips),
        "experiment_env": env,
        "adaptive_spacing_gate": bool(args.adaptive_spacing_gate),
        "fail_on_regression": bool(args.fail_on_regression),
        "fail_on_error": bool(args.fail_on_error),
        "max_mean_regression": args.max_mean_regression,
        "max_visible_regression": args.max_visible_regression,
        "min_improved": args.min_improved,
        "require_improved": list(args.require_improved),
        "require_unchanged": list(args.require_unchanged),
        "expected_directions": _expected_directions(require_improved, require_unchanged),
    }


def _annotate_expectations(rows: list[dict[str, Any]], expected: dict[str, str]) -> None:
    for row in rows:
        name = row.get("name")
        expected_direction = expected.get(str(name)) if isinstance(name, str) else None
        if not expected_direction:
            row["expected_direction"] = None
            row["matches_expected"] = None
            continue
        actual = (row.get("delta") or {}).get("mean_direction")
        row["expected_direction"] = expected_direction
        row["matches_expected"] = actual == expected_direction


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "clips",
        nargs="*",
        help="Clip files to compare. Defaults to the focused vector guard set.",
    )
    parser.add_argument(
        "--env",
        action="append",
        default=[DEFAULT_EXPERIMENT_ENV],
        help="Experiment env assignment. Can be repeated. Default enables adaptive spacing.",
    )
    parser.add_argument("--out", help="Write JSON to this path instead of stdout.")
    parser.add_argument(
        "--fail-on-regression",
        action="store_true",
        help="Exit nonzero when any metric exceeds the regression thresholds.",
    )
    parser.add_argument(
        "--max-mean-regression",
        type=float,
        default=0.0,
        help="Allowed positive mean delta before a row is considered regressed.",
    )
    parser.add_argument(
        "--max-visible-regression",
        type=int,
        help="Allowed positive visible_px delta before a row is considered regressed.",
    )
    parser.add_argument(
        "--min-improved",
        type=int,
        default=0,
        help="Require at least this many clips to improve before the gate passes.",
    )
    parser.add_argument(
        "--require-improved",
        action="append",
        default=[],
        help="Require a specific clip basename to improve. Can be repeated.",
    )
    parser.add_argument(
        "--require-unchanged",
        action="append",
        default=[],
        help="Require a specific clip basename to remain unchanged. Can be repeated.",
    )
    parser.add_argument(
        "--fail-on-error",
        action="store_true",
        help="Exit nonzero when a default/experiment render fails or a comparable metric is missing.",
    )
    parser.add_argument(
        "--adaptive-spacing-gate",
        action="store_true",
        help=(
            "Use the current adaptive-spacing guard preset: fail on regression, allow no visible-pixel "
            "increase, and require Vector_SizePressure.clip to improve."
        ),
    )
    parser.add_argument(
        "--print-table",
        action="store_true",
        help="Print a compact human-readable delta table to stderr.",
    )
    args = parser.parse_args()

    if args.adaptive_spacing_gate:
        args.fail_on_regression = True
        args.fail_on_error = True
        args.max_visible_regression = 0
        args.min_improved = max(args.min_improved, 1)
        if ADAPTIVE_SPACING_REQUIRED_IMPROVEMENT not in args.require_improved:
            args.require_improved.append(ADAPTIVE_SPACING_REQUIRED_IMPROVEMENT)
        for name in ADAPTIVE_SPACING_REQUIRED_UNCHANGED:
            if name not in args.require_unchanged:
                args.require_unchanged.append(name)

    root = Path(__file__).resolve().parent
    env = _parse_env(args.env)
    clips = [Path(value) for value in (args.clips or DEFAULT_GUARD_CLIPS)]
    clip_paths = [clip if clip.is_absolute() else root / clip for clip in clips]
    duplicate_basenames = _duplicate_clip_basenames(clip_paths)
    rows: list[dict[str, Any]] = []
    for clip_path in clip_paths:
        default = _run_verify(root, clip_path, {})
        experiment = _run_verify(root, clip_path, env)
        rows.append(
            {
                "name": clip_path.name,
                "clip": str(clip_path),
                "default": default,
                "experiment": experiment,
                "delta": _delta(default, experiment),
            }
        )
    expected_directions = _expected_directions(
        set(args.require_improved),
        set(args.require_unchanged),
    )
    _annotate_expectations(rows, expected_directions)

    payload = {
        "experiment_env": env,
        "guard_preset": "adaptive_spacing" if args.adaptive_spacing_gate else None,
        "run_config": _run_config(args, clip_paths, env),
        "summary": _summary(
            rows,
            max_mean_regression=args.max_mean_regression,
            max_visible_regression=args.max_visible_regression,
            min_improved=args.min_improved,
            require_improved=set(args.require_improved),
            require_unchanged=set(args.require_unchanged),
            fail_on_error=args.fail_on_error,
            duplicate_basenames=duplicate_basenames,
        ),
        "clips": rows,
    }
    text = json.dumps(payload, ensure_ascii=False, indent=2)
    if args.out:
        Path(args.out).write_text(text + "\n", encoding="utf-8")
    else:
        print(text)
    if args.print_table:
        print(_format_table(rows, payload["summary"]["expected_directions"]), file=sys.stderr)
    failure_counts = payload["summary"]["failure_counts_by_type"]
    should_exit_for_regression = args.fail_on_regression and payload["summary"]["failure_count"]
    should_exit_for_error = args.fail_on_error and (
        failure_counts.get("config_error", 0) or failure_counts.get("render_error", 0)
    )
    if should_exit_for_regression or should_exit_for_error:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
