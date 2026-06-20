from __future__ import annotations

import argparse
from dataclasses import dataclass
import platform as host_platform
import shutil
import subprocess
import tempfile
from pathlib import Path
import sys
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

@dataclass(frozen=True)
class NativePlatformSpec:
    platform_id: str
    cargo_target: str
    library_name: str
    worker_name: str
    tested_status: str


NATIVE_PLATFORMS: dict[str, NativePlatformSpec] = {
    "windows-x64": NativePlatformSpec(
        platform_id="windows-x64",
        cargo_target="x86_64-pc-windows-msvc",
        library_name="clip_capi.dll",
        worker_name="clip_cli.exe",
        tested_status="tested on the maintainer's Windows machine",
    ),
    "linux-x64": NativePlatformSpec(
        platform_id="linux-x64",
        cargo_target="x86_64-unknown-linux-gnu",
        library_name="libclip_capi.so",
        worker_name="clip_cli",
        tested_status="packaging-supported but not maintainer-tested",
    ),
    "macos-x64": NativePlatformSpec(
        platform_id="macos-x64",
        cargo_target="x86_64-apple-darwin",
        library_name="libclip_capi.dylib",
        worker_name="clip_cli",
        tested_status="packaging-supported but not maintainer-tested",
    ),
    "macos-arm64": NativePlatformSpec(
        platform_id="macos-arm64",
        cargo_target="aarch64-apple-darwin",
        library_name="libclip_capi.dylib",
        worker_name="clip_cli",
        tested_status="packaging-supported but not maintainer-tested",
    ),
}

ALL_NATIVE_PLATFORM_IDS = tuple(NATIVE_PLATFORMS)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def current_platform_id() -> str:
    machine = host_platform.machine().lower()
    is_x64 = machine in {"amd64", "x86_64"}
    is_arm64 = machine in {"arm64", "aarch64"}
    if sys.platform == "win32" and is_x64:
        return "windows-x64"
    if sys.platform.startswith("linux") and is_x64:
        return "linux-x64"
    if sys.platform == "darwin" and is_x64:
        return "macos-x64"
    if sys.platform == "darwin" and is_arm64:
        return "macos-arm64"
    raise SystemExit(f"Unsupported build host platform: {sys.platform} {machine}")


def _current_platform_id_or_none() -> str | None:
    try:
        return current_platform_id()
    except SystemExit:
        return None


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
        "--platform",
        action="append",
        choices=(*ALL_NATIVE_PLATFORM_IDS, "all"),
        help=(
            "Blender extension platform to package. Defaults to the current "
            "host platform. Repeat to build a multi-platform zip, or pass all."
        ),
    )
    parser.add_argument(
        "--native-artifact-dir",
        action="append",
        default=[],
        metavar="PLATFORM=DIR",
        help=(
            "Override native artifact directory for a platform. Useful when "
            "collecting Linux/macOS artifacts on another release machine."
        ),
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


def _parse_platforms(values: list[str] | None) -> tuple[str, ...]:
    if not values:
        return (current_platform_id(),)
    platforms: list[str] = []
    for value in values:
        if value == "all":
            platforms.extend(ALL_NATIVE_PLATFORM_IDS)
        else:
            platforms.append(value)
    deduped = tuple(dict.fromkeys(platforms))
    if not deduped:
        raise SystemExit("At least one platform is required.")
    return deduped


def _parse_native_artifact_dirs(values: list[str]) -> dict[str, Path]:
    dirs: dict[str, Path] = {}
    for value in values:
        if "=" not in value:
            raise SystemExit(
                f"Invalid --native-artifact-dir value {value!r}; expected PLATFORM=DIR"
            )
        platform_id, directory = value.split("=", 1)
        if platform_id not in NATIVE_PLATFORMS:
            known = ", ".join(ALL_NATIVE_PLATFORM_IDS)
            raise SystemExit(f"Unknown native artifact platform {platform_id!r}; expected one of {known}")
        dirs[platform_id] = Path(directory)
    return dirs


def _native_artifact_dirs(
    root: Path,
    platform_id: str,
    native_artifact_dirs: dict[str, Path] | None,
) -> list[Path]:
    spec = NATIVE_PLATFORMS[platform_id]
    dirs: list[Path] = []
    if native_artifact_dirs and platform_id in native_artifact_dirs:
        dirs.append(native_artifact_dirs[platform_id])
    dirs.extend(
        [
            root / "native" / "artifacts" / platform_id,
            root / "native" / "rust" / "target" / spec.cargo_target / "release",
        ]
    )
    if platform_id == _current_platform_id_or_none():
        dirs.append(root / "native" / "rust" / "target" / "release")
    return dirs


def _native_artifact_candidates(
    root: Path,
    platform_id: str,
    file_name: str,
    native_artifact_dirs: dict[str, Path] | None,
) -> list[Path]:
    return [
        directory / file_name
        for directory in _native_artifact_dirs(root, platform_id, native_artifact_dirs)
    ]


def _find_native_artifact(
    root: Path,
    platform_id: str,
    file_name: str,
    native_artifact_dirs: dict[str, Path] | None,
) -> Path:
    candidates = _native_artifact_candidates(
        root,
        platform_id,
        file_name,
        native_artifact_dirs,
    )
    for candidate in candidates:
        if candidate.exists():
            return candidate
    formatted = "\n  ".join(str(candidate) for candidate in candidates)
    raise SystemExit(
        f"No {platform_id} native artifact found for {file_name}. "
        f"Expected one of:\n  {formatted}"
    )


def _manifest_with_platforms(text: str, platforms: tuple[str, ...]) -> str:
    platform_list = ", ".join(f'"{platform}"' for platform in platforms)
    replacement = f"platforms = [{platform_list}]"
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if line.strip().startswith("platforms"):
            lines[index] = replacement
            return "\n".join(lines) + ("\n" if text.endswith("\n") else "")
    if text and not text.endswith("\n"):
        text += "\n"
    return f"{text}{replacement}\n"


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


def _copy_package_sources(
    staging_dir: Path,
    package_dir: Path,
    root: Path,
    *,
    platforms: tuple[str, ...],
) -> list[str]:
    written: list[str] = []
    manifest_source = package_dir / MANIFEST_FILE
    if not manifest_source.exists():
        raise SystemExit(f"Missing extension manifest: {manifest_source}")
    manifest_text = manifest_source.read_text(encoding="utf-8")
    (staging_dir / MANIFEST_FILE).write_text(
        _manifest_with_platforms(manifest_text, platforms),
        encoding="utf-8",
    )
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
    platforms: tuple[str, ...] | None = None,
    native_artifact_dirs: dict[str, Path] | None = None,
) -> list[str]:
    root = repo_root()
    package_dir = root / "clip_studio_importer"
    platforms = platforms or (current_platform_id(),)
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    for platform_id in platforms:
        if platform_id not in NATIVE_PLATFORMS:
            raise SystemExit(f"Unknown native platform: {platform_id}")

    with tempfile.TemporaryDirectory(prefix="rizum_clip_extension_") as tmp_dir:
        staging_dir = Path(tmp_dir)
        written = _copy_package_sources(
            staging_dir,
            package_dir,
            root,
            platforms=platforms,
        )
        if include_native:
            native_root = staging_dir / "native"
            native_root.mkdir()
            for platform_id in platforms:
                spec = NATIVE_PLATFORMS[platform_id]
                native_dir = native_root / platform_id
                native_dir.mkdir()
                sources = [
                    _find_native_artifact(
                        root,
                        platform_id,
                        spec.library_name,
                        native_artifact_dirs,
                    ),
                    _find_native_artifact(
                        root,
                        platform_id,
                        spec.worker_name,
                        native_artifact_dirs,
                    ),
                ]
                for source in sources:
                    target = native_dir / source.name
                    shutil.copy2(source, target)
                    if target.name == spec.worker_name:
                        target.chmod(target.stat().st_mode | 0o755)
                    written.append(f"native/{platform_id}/{source.name}")

        if blender is not None:
            return _run_blender_extension_build(blender, staging_dir, output)

        _write_zip_from_staging(staging_dir, output)

    return written


def main() -> None:
    args = parse_args()
    platforms = _parse_platforms(args.platform)
    written = build_zip(
        args.output,
        include_native=not args.no_native,
        blender=args.blender,
        platforms=platforms,
        native_artifact_dirs=_parse_native_artifact_dirs(args.native_artifact_dir),
    )
    print(f"Wrote {args.output.resolve()}")
    print(f"Platforms: {', '.join(platforms)}")
    for platform_id in platforms:
        print(f"  {platform_id}: {NATIVE_PLATFORMS[platform_id].tested_status}")
    for name in written:
        print(f"  {name}")


if __name__ == "__main__":
    main()
