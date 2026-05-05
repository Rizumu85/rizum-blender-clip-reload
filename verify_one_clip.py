from __future__ import annotations

import importlib.util
import json
import sys
import time
from pathlib import Path

import numpy as np
from PIL import Image


def main() -> int:
    root = Path(__file__).resolve().parent
    clip_path = Path(sys.argv[1])
    mod_path = root / "clip_studio_importer" / "clip_loader.py"
    spec = importlib.util.spec_from_file_location("pkg_clip_loader", mod_path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = mod
    assert spec.loader is not None
    spec.loader.exec_module(mod)

    started = time.time()
    clip = mod.ClipFile(str(clip_path))
    try:
        out = clip.composite()
    finally:
        clip.close()

    result = {
        "name": clip_path.name,
        "rendered": True,
        "shape": list(out.shape),
        "seconds": round(time.time() - started, 3),
    }

    ref_path = clip_path.with_suffix(".png")
    if ref_path.exists():
        ref = np.array(Image.open(ref_path).convert("RGBA"))
        result["ref"] = ref_path.name
        result["ref_shape"] = list(ref.shape)
        if ref.shape == out.shape:
            diff = np.abs(out.astype(np.int16) - ref.astype(np.int16))
            visible = diff.max(axis=-1) > 1
            result["max"] = int(diff.max())
            result["mean"] = round(float(diff.mean()), 6)
            result["diff_px"] = int((diff.max(axis=-1) > 0).sum())
            result["visible_px"] = int(visible.sum())
            result["visible_pct"] = round(100.0 * int(visible.sum()) / visible.size, 6)
    else:
        result["ref"] = None

    print(json.dumps(result, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
