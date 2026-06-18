from __future__ import annotations

import tempfile
import unittest
import importlib.util
import sys
import types
from pathlib import Path


def _load_worker_protocol():
    package_dir = Path(__file__).resolve().parents[1] / "clip_studio_importer"
    package = types.ModuleType("clip_studio_importer")
    package.__path__ = [str(package_dir)]
    sys.modules["clip_studio_importer"] = package

    image_state_spec = importlib.util.spec_from_file_location(
        "clip_studio_importer.image_state",
        package_dir / "image_state.py",
    )
    image_state = importlib.util.module_from_spec(image_state_spec)
    assert image_state_spec.loader is not None
    sys.modules[image_state_spec.name] = image_state
    image_state_spec.loader.exec_module(image_state)

    worker_protocol_spec = importlib.util.spec_from_file_location(
        "clip_studio_importer.worker_protocol",
        package_dir / "worker_protocol.py",
    )
    worker_protocol_module = importlib.util.module_from_spec(worker_protocol_spec)
    assert worker_protocol_spec.loader is not None
    sys.modules[worker_protocol_spec.name] = worker_protocol_module
    worker_protocol_spec.loader.exec_module(worker_protocol_module)
    return worker_protocol_module


worker_protocol = _load_worker_protocol()


class WorkerProtocolTests(unittest.TestCase):
    def test_prepares_one_shot_command_and_persistent_request(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            files = worker_protocol.prepare_render_files(
                Path(tmp_dir),
                '{"abi":1}',
            )

            command = worker_protocol.one_shot_command(
                "C:/addon/clip_cli.exe",
                "C:/art/sample.clip",
                files,
            )
            request = worker_protocol.persistent_request("C:/art/sample.clip", files)

            self.assertEqual(files.previous_manifest_path.read_text(encoding="utf-8"), '{"abi":1}')
            self.assertEqual(
                command,
                [
                    "C:/addon/clip_cli.exe",
                    "C:/art/sample.clip",
                    "--blender-render-rgba",
                    str(files.rgba_path),
                    "--blender-render-json",
                    str(files.json_path),
                    "--blender-reload-old-json",
                    str(files.previous_manifest_path),
                ],
            )
            self.assertEqual(
                request,
                {
                    "clip_path": "C:/art/sample.clip",
                    "rgba_path": str(files.rgba_path),
                    "json_path": str(files.json_path),
                    "previous_manifest_path": str(files.previous_manifest_path),
                },
            )
            self.assertEqual(
                worker_protocol.persistent_request_line(request),
                (
                    '{"clip_path":"C:/art/sample.clip",'
                    f'"rgba_path":"{str(files.rgba_path).replace(chr(92), chr(92) * 2)}",'
                    f'"json_path":"{str(files.json_path).replace(chr(92), chr(92) * 2)}",'
                    f'"previous_manifest_path":"{str(files.previous_manifest_path).replace(chr(92), chr(92) * 2)}"'
                    "}\n"
                ),
            )

    def test_parses_patch_reload_output(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            source = Path(tmp_dir) / "sample.clip"
            source.write_bytes(b"clip")
            pixels = bytes([255, 0, 0, 255, 0, 255, 0, 255])
            metadata = {
                "width": 4,
                "height": 4,
                "root_layer_id": 2,
                "layer_count": 3,
                "external_data_count": 4,
                "renderer_version": "0.1.0-test",
                "reload_diff": {
                    "mode": "patch",
                    "reason": "source tile payload changed",
                    "manifest": {"z": 2, "a": 1},
                    "rects": [{"x": 1, "y": 2, "width": 2, "height": 1}],
                },
                "support": {
                    "source_count": 1,
                    "unsupported_count": 1,
                    "unsupported": [
                        {
                            "layer_id": 9,
                            "layer_name": "Tone curve",
                            "node_id": 4,
                            "kind": "Filter",
                            "reason": "unsupported",
                        }
                    ],
                    "report": "1 unsupported node(s).",
                },
                "resources": {"raster_count": 2, "mask_count": 3},
            }

            result = worker_protocol.render_result_from_worker_output(
                str(source),
                metadata,
                pixels,
                expected_abi_version=1,
                worker_seconds=0.5,
                output_read_seconds=0.1,
            )

        self.assertEqual(result.reload_diff_mode, "patch")
        self.assertEqual(result.reload_manifest_json, '{"a":1,"z":2}')
        self.assertEqual(result.patches[0].byte_offset, 0)
        self.assertEqual(result.patches[0].width, 2)
        self.assertEqual(result.support_summary.unsupported_count, 1)
        self.assertEqual(
            result.support_summary.details,
            ("- layer 9 [Tone curve] node 4 Filter: unsupported",),
        )

    def test_rejects_mismatched_patch_buffer_length(self) -> None:
        metadata = {
            "width": 4,
            "height": 4,
            "root_layer_id": 2,
            "layer_count": 3,
            "external_data_count": 4,
            "reload_diff": {
                "mode": "patch",
                "rects": [{"x": 1, "y": 2, "width": 2, "height": 1}],
            },
        }

        with self.assertRaisesRegex(
            worker_protocol.WorkerProtocolError,
            "invalid RGBA buffer length",
        ):
            worker_protocol.render_result_from_worker_output(
                "missing.clip",
                metadata,
                b"\x00\x00\x00\x00",
                expected_abi_version=1,
                worker_seconds=0.5,
                output_read_seconds=0.1,
            )


if __name__ == "__main__":
    unittest.main()
