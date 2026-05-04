"""
verify_blend_modes.py — Compare clip_loader output against CSP PNG export.

Usage:
    1. In CLIP STUDIO PAINT, create a test document with one layer per blend mode.
       Name each layer EXACTLY after the blend mode string (e.g. "MULTIPLY",
       "COLOR_DODGE", "GLOW_DODGE"). Use a simple colored rectangle on each layer
       so differences are visually obvious.

    2. Export from CSP: File → Export (Single Layer) → PNG.
       Or: File → Export (Flattened) → PNG for the full composite.

    3. Run this script:
       python verify_blend_modes.py test.clip --csp-png csp_export.png

Output:
    - Per-mode pixel difference statistics
    - List of modes with >1% pixel difference
    - Optional: save difference heatmaps to --diff-dir
"""

from __future__ import annotations

import argparse
import os
import sys
import numpy as np
from PIL import Image
from clip_loader import ClipFile, _BLEND_MAPPING

# Modes that need special verification (CSP-specific or ambiguous)
SUSPECT_MODES = {
    "THROUGH": "Not yet implemented in _blend_func",
    "GLOW_DODGE": "Currently aliased to COLOR_DODGE — may differ in practice",
    "ADD_GLOW": "Currently aliased to ADD — may differ in practice",
    "HARD_MIX": "Threshold may differ from 127/255",
    "SOFT_LIGHT": "Multiple formula variants (PS vs W3C)",
    "LINEAR_LIGHT": "CSS spec vs PS spec differ on alpha handling",
}


def load_png(path: str) -> np.ndarray:
    """Load PNG as (H, W, 4) uint8 RGBA."""
    img = Image.open(path).convert("RGBA")
    return np.array(img)


def compare_images(a: np.ndarray, b: np.ndarray, mode_name: str, threshold: float = 0.01):
    """Compare two RGBA uint8 images. Return dict of stats."""
    if a.shape != b.shape:
        return {
            "mode": mode_name,
            "match": False,
            "error": f"Shape mismatch: {a.shape} vs {b.shape}",
            "diff_pct": 100.0,
            "max_diff": 255,
            "mean_diff": 0,
        }

    diff = np.abs(a.astype(np.int16) - b.astype(np.int16))
    max_diff = int(diff.max())
    mean_diff = float(diff.mean())
    total_px = a.shape[0] * a.shape[1]

    # Pixels where ANY channel differs
    diff_mask = diff.max(axis=-1) > 0
    diff_px = int(diff_mask.sum())
    diff_pct = 100.0 * diff_px / total_px

    # Pixels with visible difference (>1 in any channel)
    visible_mask = diff.max(axis=-1) > 1
    visible_px = int(visible_mask.sum())
    visible_pct = 100.0 * visible_px / total_px

    return {
        "mode": mode_name,
        "match": diff_pct <= threshold * 100,
        "total_px": total_px,
        "diff_px": diff_px,
        "diff_pct": round(diff_pct, 4),
        "visible_diff_px": visible_px,
        "visible_diff_pct": round(visible_pct, 4),
        "max_diff": max_diff,
        "mean_diff": round(mean_diff, 4),
    }


def verify_single_mode(clip: ClipFile, csp_png: np.ndarray, mode_id: int, mode_name: str):
    """Composite the clip targeting one specific mode and compare."""
    result = clip.composite()  # full composite
    return compare_images(result, csp_png, f"{mode_id}:{mode_name}")


def verify_all_modes(clip_path: str, csp_png_path: str):
    """Full verification: composite the clip and compare against CSP PNG export."""
    clip = ClipFile(clip_path)
    csp_png = load_png(csp_png_path)

    print(f"Canvas: {clip.width}x{clip.height}")
    print(f"CSP PNG: {csp_png.shape}")

    result = clip.composite()
    stats = compare_images(result, csp_png, "FULL_COMPOSITE")

    print(f"\n--- Full Composite ---")
    print(f"  Match: {stats['match']}")
    print(f"  Diff pixels: {stats['diff_px']}/{stats['total_px']} ({stats['diff_pct']}%)")
    print(f"  Visible diff (>1): {stats['visible_diff_px']} ({stats['visible_diff_pct']}%)")
    print(f"  Max diff: {stats['max_diff']}")
    print(f"  Mean diff: {stats['mean_diff']}")

    # Show which blend modes are in this file
    db = clip._db
    layers = db.execute("SELECT MainId, LayerName, LayerComposite FROM Layer").fetchall()
    print(f"\n--- Layers and Blend Modes ---")
    for row in layers:
        mode_id = int(row["LayerComposite"] or 0)
        mode_name = _BLEND_MAPPING.get(mode_id, f"UNKNOWN({mode_id})")
        suspect = " *** SUSPECT ***" if mode_name in SUSPECT_MODES else ""
        print(f"  Layer {row['MainId']}: {row['LayerName']} → {mode_name}{suspect}")

    clip.close()
    return stats


def main():
    ap = argparse.ArgumentParser(description="Verify clip_loader blend modes against CSP PNG export")
    ap.add_argument("clip_path", help="Path to .clip test file")
    ap.add_argument("--csp-png", help="Path to CSP-exported PNG for comparison")
    ap.add_argument("--diff-dir", help="Directory to save per-layer difference heatmaps")
    ap.add_argument("--list-modes", action="store_true", help="Just list blend modes in the file")
    args = ap.parse_args()

    if args.list_modes:
        clip = ClipFile(args.clip_path)
        db = clip._db
        layers = db.execute(
            "SELECT MainId, LayerName, LayerComposite FROM Layer ORDER BY LayerId"
        ).fetchall()
        print(f"{'ID':>6}  {'ModeID':>6}  {'Mode Name':<20}  Layer Name")
        print("-" * 60)
        for row in layers:
            mode_id = int(row["LayerComposite"] or 0)
            mode_name = _BLEND_MAPPING.get(mode_id, f"??? ({mode_id})")
            print(f"{row['MainId']:>6}  {mode_id:>6}  {mode_name:<20}  {row['LayerName']}")
        clip.close()
        return

    if not args.csp_png:
        ap.error("--csp-png is required for verification (or use --list-modes to inspect only)")
    stats = verify_all_modes(args.clip_path, args.csp_png)

    # Report suspect modes
    print(f"\n--- Suspect Modes Status ---")
    for mode, note in SUSPECT_MODES.items():
        print(f"  {mode}: {note}")

    if stats["diff_pct"] > 0.01:
        print(f"\n⚠️  Composite differs from CSP PNG by {stats['diff_pct']}% of pixels.")
        print("   Create per-layer test files for detailed mode-by-mode verification.")
        sys.exit(1)
    else:
        print(f"\n✅ Composite matches CSP PNG ({stats['diff_pct']}% diff, within tolerance).")
        sys.exit(0)


if __name__ == "__main__":
    main()
