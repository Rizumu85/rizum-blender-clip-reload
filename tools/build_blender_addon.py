from __future__ import annotations

import argparse
from pathlib import Path
import zipfile


PACKAGE_FILES = (
    "__init__.py",
    "clip_loader.py",
    "native_bridge.py",
)

NATIVE_LIBRARY_NAMES = (
    "clip_capi.dll",
    "libclip_capi.so",
    "libclip_capi.dylib",
)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def parse_args() -> argparse.Namespace:
    root = repo_root()
    parser = argparse.ArgumentParser(description="Build the Blender add-on zip.")
    parser.add_argument(
        "--output",
        type=Path,
        default=root / "clip_studio_importer.zip",
        help="Output zip path.",
    )
    parser.add_argument(
        "--no-native",
        action="store_true",
        help="Do not include locally built native renderer libraries.",
    )
    return parser.parse_args()


def native_library_candidates(root: Path) -> list[Path]:
    release_dir = root / "native" / "rust" / "target" / "release"
    return [release_dir / name for name in NATIVE_LIBRARY_NAMES]


def build_zip(output: Path, *, include_native: bool) -> list[str]:
    root = repo_root()
    package_dir = root / "clip_studio_importer"
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    native_libraries = [
        candidate for candidate in native_library_candidates(root) if candidate.exists()
    ]
    if include_native and not native_libraries:
        names = ", ".join(NATIVE_LIBRARY_NAMES)
        raise SystemExit(
            "No release native renderer library found. Run "
            "`cargo build --release -p clip_capi` under native/rust first, "
            f"or pass --no-native. Expected one of: {names}"
        )

    written: list[str] = []
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name in PACKAGE_FILES:
            source = package_dir / name
            if not source.exists():
                raise SystemExit(f"Missing package file: {source}")
            arcname = f"clip_studio_importer/{name}"
            archive.write(source, arcname)
            written.append(arcname)

        if include_native:
            for source in native_libraries:
                arcname = f"clip_studio_importer/native/{source.name}"
                archive.write(source, arcname)
                written.append(arcname)

    return written


def main() -> None:
    args = parse_args()
    written = build_zip(args.output, include_native=not args.no_native)
    print(f"Wrote {args.output.resolve()}")
    for name in written:
        print(f"  {name}")


if __name__ == "__main__":
    main()
