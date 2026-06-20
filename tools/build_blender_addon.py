from __future__ import annotations

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path
import zipfile


MANIFEST_FILE = "blender_manifest.toml"

PACKAGE_FILES = (
    "__init__.py",
    "image_state.py",
    "i18n.py",
    "native_bridge.py",
    "worker_protocol.py",
)

PACKAGE_ROOT_FILES = (
    "LICENSE",
    "NOTICE.md",
)

NATIVE_LIBRARY_NAMES = (
    "clip_capi.dll",
    "libclip_capi.so",
    "libclip_capi.dylib",
)

NATIVE_WORKER_NAMES = (
    "clip_cli.exe",
    "clip_cli",
)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def parse_args() -> argparse.Namespace:
    root = repo_root()
    parser = argparse.ArgumentParser(description="Build the Blender extension zip.")
    parser.add_argument(
        "--output",
        type=Path,
        default=root / "clip_studio_importer.zip",
        help="Output extension zip path.",
    )
    parser.add_argument(
        "--no-native",
        action="store_true",
        help="Do not include locally built native renderer libraries.",
    )
    parser.add_argument(
        "--blender",
        type=Path,
        help=(
            "Optional Blender executable. When set, build through "
            "`blender --command extension build` from a staged source tree."
        ),
    )
    return parser.parse_args()


def native_library_candidates(root: Path) -> list[Path]:
    release_dir = root / "native" / "rust" / "target" / "release"
    return [release_dir / name for name in NATIVE_LIBRARY_NAMES]


def native_worker_candidates(root: Path) -> list[Path]:
    release_dir = root / "native" / "rust" / "target" / "release"
    return [release_dir / name for name in NATIVE_WORKER_NAMES]


def _write_zip_from_staging(staging_dir: Path, output: Path) -> list[str]:
    written: list[str] = []
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for source in sorted(path for path in staging_dir.rglob("*") if path.is_file()):
            arcname = source.relative_to(staging_dir).as_posix()
            archive.write(source, arcname)
            written.append(arcname)
    return written


def _run_blender_extension_build(blender: Path, staging_dir: Path, output: Path) -> list[str]:
    command = [
        str(blender),
        "--factory-startup",
        "--background",
        "--command",
        "extension",
        "build",
        "--source-dir",
        str(staging_dir),
        "--output-filepath",
        str(output),
    ]
    subprocess.run(command, check=True)
    with zipfile.ZipFile(output) as archive:
        return archive.namelist()


def _copy_package_sources(staging_dir: Path, package_dir: Path, root: Path) -> list[str]:
    written: list[str] = []
    manifest_source = package_dir / MANIFEST_FILE
    if not manifest_source.exists():
        raise SystemExit(f"Missing extension manifest: {manifest_source}")
    shutil.copy2(manifest_source, staging_dir / MANIFEST_FILE)
    written.append(MANIFEST_FILE)

    for name in PACKAGE_ROOT_FILES:
        source = root / name
        if not source.exists():
            raise SystemExit(f"Missing package root file: {source}")
        shutil.copy2(source, staging_dir / name)
        written.append(name)

    for name in PACKAGE_FILES:
        source = package_dir / name
        if not source.exists():
            raise SystemExit(f"Missing package file: {source}")
        shutil.copy2(source, staging_dir / name)
        written.append(name)

    return written


def build_zip(
    output: Path,
    *,
    include_native: bool,
    blender: Path | None = None,
) -> list[str]:
    root = repo_root()
    package_dir = root / "clip_studio_importer"
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    native_libraries = [
        candidate for candidate in native_library_candidates(root) if candidate.exists()
    ]
    native_workers = [
        candidate for candidate in native_worker_candidates(root) if candidate.exists()
    ]
    if include_native and not native_libraries:
        names = ", ".join(NATIVE_LIBRARY_NAMES)
        raise SystemExit(
            "No release native renderer library found. Run "
            "`cargo build --release -p clip_capi` under native/rust first, "
            f"or pass --no-native. Expected one of: {names}"
        )
    if include_native and not native_workers:
        names = ", ".join(NATIVE_WORKER_NAMES)
        raise SystemExit(
            "No release native renderer worker found. Run "
            "`cargo build --release -p clip_cli` under native/rust first, "
            f"or pass --no-native. Expected one of: {names}"
        )

    with tempfile.TemporaryDirectory(prefix="rizum_clip_extension_") as tmp_dir:
        staging_dir = Path(tmp_dir)
        written = _copy_package_sources(staging_dir, package_dir, root)
        if include_native:
            native_dir = staging_dir / "native"
            native_dir.mkdir()
            for source in native_libraries + native_workers:
                target = native_dir / source.name
                shutil.copy2(source, target)
                written.append(f"native/{source.name}")

        if blender is not None:
            return _run_blender_extension_build(blender, staging_dir, output)

        _write_zip_from_staging(staging_dir, output)

    return written


def main() -> None:
    args = parse_args()
    written = build_zip(
        args.output,
        include_native=not args.no_native,
        blender=args.blender,
    )
    print(f"Wrote {args.output.resolve()}")
    for name in written:
        print(f"  {name}")


if __name__ == "__main__":
    main()
