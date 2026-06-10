#!/usr/bin/env python3
"""UI-aware Clip Studio Paint PNG export helper.

Default mode is a dry run: connect to the current CSP window, take a screenshot,
dump the UI Automation control tree, and print likely controls. It does not
click unless --execute is supplied.

The script prefers pywinauto/UIA controls. Image matching is only used as a
fallback against templates captured from previous UIA-visible controls.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass, asdict
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable

from PIL import Image
import pyautogui
from pywinauto import Desktop
from pywinauto.application import Application
from pywinauto.findwindows import ElementNotFoundError
from pywinauto.timings import TimeoutError as UIATimeoutError


REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CSP_EXE = Path(r"C:\Program Files\CELSYS\CLIP STUDIO 1.5\CLIP STUDIO PAINT\CLIPStudioPaint.exe")
DEFAULT_CLIP = REPO_ROOT / "img" / "Vector_SizePressure.clip"
OUT_DIR = REPO_ROOT / "tmp_vector_probe" / "csp_ui_automation"
TEMPLATE_DIR = OUT_DIR / "templates"

CSP_TITLE_RE = r".*CLIP STUDIO PAINT.*"
VECTOR_TITLE_RE = r".*Vector_SizePressure.*CLIP STUDIO PAINT.*"

FILE_MENU_PATTERNS = [r"^File$", r"^ファイル$", r"^文件$"]
OPEN_PATTERNS = [r"^Open\.\.\.$", r"^Open$", r"開く", r"打开"]
SAVE_PATTERNS = [r"^Save$", r"^保存$", r"保存(&S)?"]
OK_PATTERNS = [r"^OK$", r"^确定$", r"^確認$", r"^OK\\s*\\(&O\\)$"]
PNG_PATTERNS = [r"^png$", r"^PNG$", r".*\\.png.*", r".*PNG.*"]
EXPORT_PATTERNS = [r"Export", r"書き出し", r"导出", r"輸出"]
SINGLE_LAYER_PATTERNS = [r"Single Layer", r"単一レイヤー", r"单层", r"單一圖層"]


@dataclass
class ControlInfo:
    index: int
    name: str
    control_type: str
    automation_id: str | None
    class_name: str | None
    rectangle: list[int]
    visible: bool
    enabled: bool


def stamp() -> str:
    return datetime.now().strftime("%Y%m%d_%H%M%S_%f")[:-3]


def norm_rect(rect: Any) -> list[int]:
    return [int(rect.left), int(rect.top), int(rect.right), int(rect.bottom)]


def rect_center(rect: list[int]) -> tuple[int, int]:
    return ((rect[0] + rect[2]) // 2, (rect[1] + rect[3]) // 2)


def compile_any(patterns: Iterable[str]) -> re.Pattern[str]:
    return re.compile("|".join(f"(?:{p})" for p in patterns), re.IGNORECASE)


def powershell_text(script: str, timeout: int = 30) -> str:
    proc = subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script],
        cwd=str(REPO_ROOT),
        text=True,
        capture_output=True,
        timeout=timeout,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"PowerShell failed ({proc.returncode})\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return proc.stdout.strip()


def get_foreground_pid() -> int | None:
    script = r"""
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32Fg {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
}
"@
$h = [Win32Fg]::GetForegroundWindow()
$pid = 0
[void][Win32Fg]::GetWindowThreadProcessId($h, [ref]$pid)
$pid
"""
    out = powershell_text(script, timeout=10)
    try:
        return int(out.splitlines()[-1].strip())
    except (ValueError, IndexError):
        return None


def start_csp(exe: Path, clip_path: Path | None) -> None:
    if not exe.exists():
        raise FileNotFoundError(f"CSP executable not found: {exe}")
    args = [str(exe)]
    if clip_path is not None:
        args.append(str(clip_path))
    subprocess.Popen(args, cwd=str(exe.parent))


def describe_csp_processes() -> str:
    script = (
        "$p = Get-Process CLIPStudioPaint -ErrorAction SilentlyContinue | "
        "Select-Object Id,StartTime,MainWindowTitle,Path; "
        "if ($null -eq $p) { 'no CLIPStudioPaint process' } "
        "else { $p | ConvertTo-Json -Depth 3 }; exit 0"
    )
    try:
        out = powershell_text(script, timeout=10)
    except Exception as exc:
        return f"could not query CLIPStudioPaint processes: {exc}"
    return out or "no CLIPStudioPaint process"


def visible_window_sample(limit: int = 30) -> list[dict[str, Any]]:
    sample: list[dict[str, Any]] = []
    for win in Desktop(backend="uia").windows(visible_only=True)[:limit]:
        try:
            sample.append({
                "title": win.window_text(),
                "pid": win.element_info.process_id,
                "class": win.element_info.class_name,
            })
        except Exception:
            continue
    return sample


def connect_csp(timeout: float = 15.0, title_re: str = CSP_TITLE_RE):
    deadline = time.time() + timeout
    last_error: Exception | None = None
    while time.time() < deadline:
        try:
            windows = Desktop(backend="uia").windows(title_re=title_re, visible_only=True)
            if windows:
                # Prefer the top-level window with the longest title. It tends
                # to be the main app rather than a small palette/dialog.
                windows.sort(key=lambda w: len(w.window_text() or ""), reverse=True)
                return windows[0]
        except (ElementNotFoundError, UIATimeoutError) as exc:
            last_error = exc
        time.sleep(0.5)
    if last_error:
        raise RuntimeError(f"CSP window not found: {last_error}") from last_error
    raise RuntimeError(
        "CSP window not found\n"
        f"CLIPStudioPaint processes: {describe_csp_processes()}\n"
        f"visible windows sample: {json.dumps(visible_window_sample(), ensure_ascii=True)}"
    )


def set_foreground(win) -> None:
    try:
        win.set_focus()
    except Exception:
        # set_focus is best effort; dry-run still works if focus cannot be set.
        pass


def screenshot_window(win, output_dir: Path) -> Path:
    output_dir.mkdir(parents=True, exist_ok=True)
    rect = norm_rect(win.rectangle())
    width = max(1, rect[2] - rect[0])
    height = max(1, rect[3] - rect[1])
    image = pyautogui.screenshot(region=(rect[0], rect[1], width, height))
    path = output_dir / f"csp_window_{stamp()}.png"
    image.save(path)
    return path


def collect_controls(win) -> list[ControlInfo]:
    controls: list[ControlInfo] = []
    for index, ctrl in enumerate([win, *win.descendants()]):
        try:
            info = ctrl.element_info
            rect = norm_rect(ctrl.rectangle())
            controls.append(
                ControlInfo(
                    index=index,
                    name=ctrl.window_text() or "",
                    control_type=info.control_type or "",
                    automation_id=info.automation_id or None,
                    class_name=info.class_name or None,
                    rectangle=rect,
                    visible=bool(ctrl.is_visible()),
                    enabled=bool(ctrl.is_enabled()),
                )
            )
        except Exception:
            continue
    return controls


def write_control_dump(controls: list[ControlInfo], output_dir: Path) -> Path:
    output_dir.mkdir(parents=True, exist_ok=True)
    path = output_dir / f"csp_controls_{stamp()}.json"
    path.write_text(json.dumps([asdict(c) for c in controls], indent=2, ensure_ascii=False), encoding="utf-8")
    return path


def print_candidates(controls: list[ControlInfo]) -> None:
    groups = {
        "file_menu": compile_any(FILE_MENU_PATTERNS),
        "open": compile_any(OPEN_PATTERNS),
        "png": compile_any(PNG_PATTERNS),
        "export": compile_any(EXPORT_PATTERNS),
        "single_layer": compile_any(SINGLE_LAYER_PATTERNS),
        "save": compile_any(SAVE_PATTERNS),
        "ok": compile_any(OK_PATTERNS),
    }
    for label, pattern in groups.items():
        hits = [c for c in controls if c.visible and pattern.search(c.name)]
        print(f"\n[{label}] {len(hits)} candidate(s)")
        for c in hits[:20]:
            print(
                f"  #{c.index:04d} {c.control_type:<14} "
                f"name={c.name!r} auto_id={c.automation_id!r} rect={c.rectangle}"
            )


def find_control(win, patterns: Iterable[str], *, timeout: float = 5.0):
    pattern = compile_any(patterns)
    deadline = time.time() + timeout
    last_snapshot: list[ControlInfo] = []
    while time.time() < deadline:
        controls = collect_controls(win)
        last_snapshot = controls
        for ctrl in [win, *win.descendants()]:
            try:
                if not ctrl.is_visible() or not ctrl.is_enabled():
                    continue
                if pattern.search(ctrl.window_text() or ""):
                    return ctrl
            except Exception:
                continue
        time.sleep(0.2)
    names = [c.name for c in last_snapshot if c.visible and c.name][:30]
    raise RuntimeError(f"control not found for patterns={list(patterns)!r}; visible names sample={names!r}")


def safe_click_control(ctrl, *, dry_run: bool) -> None:
    rect = norm_rect(ctrl.rectangle())
    x, y = rect_center(rect)
    print(f"{'DRY ' if dry_run else ''}click UIA {ctrl.window_text()!r} at rect={rect} center=({x},{y})")
    if dry_run:
        return
    ctrl.click_input()


def crop_control_template(window_screenshot: Path, win_rect: list[int], control: ControlInfo, name: str) -> Path | None:
    rect = control.rectangle
    if rect[2] <= rect[0] or rect[3] <= rect[1]:
        return None
    left = max(0, rect[0] - win_rect[0])
    top = max(0, rect[1] - win_rect[1])
    right = max(left + 1, rect[2] - win_rect[0])
    bottom = max(top + 1, rect[3] - win_rect[1])
    image = Image.open(window_screenshot)
    crop = image.crop((left, top, right, bottom))
    TEMPLATE_DIR.mkdir(parents=True, exist_ok=True)
    path = TEMPLATE_DIR / f"{name}.png"
    crop.save(path)
    return path


def calibrate_templates(win, controls: list[ControlInfo], screenshot_path: Path) -> dict[str, str]:
    win_rect = norm_rect(win.rectangle())
    targets = {
        "file_menu": compile_any(FILE_MENU_PATTERNS),
        "png_button": compile_any(PNG_PATTERNS),
        "export_menu": compile_any(EXPORT_PATTERNS),
    }
    saved: dict[str, str] = {}
    for label, pattern in targets.items():
        hit = next((c for c in controls if c.visible and pattern.search(c.name)), None)
        if not hit:
            continue
        path = crop_control_template(screenshot_path, win_rect, hit, label)
        if path:
            saved[label] = str(path)
    return saved


def click_template(name: str, *, dry_run: bool, confidence: float) -> bool:
    path = TEMPLATE_DIR / f"{name}.png"
    if not path.exists():
        return False
    location = pyautogui.locateOnScreen(str(path), confidence=confidence)
    if not location:
        return False
    x, y = pyautogui.center(location)
    print(f"{'DRY ' if dry_run else ''}click template {name!r} at ({x},{y}) from {path}")
    if not dry_run:
        pyautogui.click(x, y)
    return True


def click_by_uia_or_template(win, patterns: Iterable[str], template_name: str, *, dry_run: bool, timeout: float = 4.0) -> None:
    try:
        ctrl = find_control(win, patterns, timeout=timeout)
        safe_click_control(ctrl, dry_run=dry_run)
        return
    except RuntimeError as exc:
        print(f"UIA miss for {template_name}: {exc}")
    if click_template(template_name, dry_run=dry_run, confidence=0.87):
        return
    raise RuntimeError(f"neither UIA nor template found target {template_name}")


def wait_for_window(title_re: str, *, timeout: float = 15.0):
    deadline = time.time() + timeout
    while time.time() < deadline:
        wins = Desktop(backend="uia").windows(title_re=title_re, visible_only=True)
        if wins:
            wins.sort(key=lambda w: len(w.window_text() or ""), reverse=True)
            return wins[0]
        time.sleep(0.3)
    raise RuntimeError(f"window not found for title_re={title_re!r}")


def choose_first_edit(dialog):
    edits = []
    for ctrl in dialog.descendants():
        try:
            if ctrl.element_info.control_type == "Edit" and ctrl.is_visible() and ctrl.is_enabled():
                edits.append(ctrl)
        except Exception:
            continue
    if not edits:
        raise RuntimeError("no editable text field found in dialog")
    # In common file dialogs, the file-name edit is usually near the bottom and
    # wider than search boxes.
    edits.sort(key=lambda c: (c.rectangle().top, c.rectangle().width()), reverse=True)
    return edits[0]


def open_clip_via_uia(win, clip_path: Path, *, dry_run: bool) -> None:
    click_by_uia_or_template(win, FILE_MENU_PATTERNS, "file_menu", dry_run=dry_run)
    time.sleep(0.6)
    click_by_uia_or_template(win, OPEN_PATTERNS, "open_menu", dry_run=dry_run)
    if dry_run:
        return
    dialog = wait_for_window(r".*(Open|開く|打开).*", timeout=20.0)
    edit = choose_first_edit(dialog)
    edit.set_edit_text(str(clip_path))
    safe_click_control(find_control(dialog, OPEN_PATTERNS, timeout=8.0), dry_run=False)


def open_clip_shell(clip_path: Path, *, dry_run: bool) -> None:
    print(f"{'DRY ' if dry_run else ''}shell-open {clip_path}")
    if not dry_run:
        os.startfile(str(clip_path))  # type: ignore[attr-defined]


def wait_canvas_visible(timeout: float = 30.0):
    return wait_for_window(VECTOR_TITLE_RE, timeout=timeout)


def export_png(win, output_png: Path, *, dry_run: bool) -> None:
    output_png.parent.mkdir(parents=True, exist_ok=True)
    try:
        click_by_uia_or_template(win, PNG_PATTERNS, "png_button", dry_run=dry_run, timeout=4.0)
    except RuntimeError:
        print("PNG toolbar button not found; trying menu export path")
        click_by_uia_or_template(win, FILE_MENU_PATTERNS, "file_menu", dry_run=dry_run)
        time.sleep(0.5)
        click_by_uia_or_template(win, EXPORT_PATTERNS, "export_menu", dry_run=dry_run)
        time.sleep(0.5)
        click_by_uia_or_template(win, SINGLE_LAYER_PATTERNS, "single_layer_menu", dry_run=dry_run)
        time.sleep(0.5)
        click_by_uia_or_template(win, PNG_PATTERNS, "png_menu", dry_run=dry_run)

    if dry_run:
        return

    save_dialog = wait_for_window(r".*(Save|保存|名前を付けて保存).*", timeout=20.0)
    edit = choose_first_edit(save_dialog)
    edit.set_edit_text(str(output_png))
    safe_click_control(find_control(save_dialog, SAVE_PATTERNS, timeout=8.0), dry_run=False)
    handle_modal_ok(timeout=30.0)
    wait_for_export_done(output_png, timeout=60.0)


def handle_modal_ok(timeout: float = 20.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        for win in Desktop(backend="uia").windows(visible_only=True):
            title = win.window_text() or ""
            if "CLIP STUDIO PAINT" not in title and not re.search(r"Export|PNG|保存|書き出し|出力", title, re.I):
                continue
            try:
                ok = find_control(win, OK_PATTERNS, timeout=0.5)
            except RuntimeError:
                continue
            safe_click_control(ok, dry_run=False)
            time.sleep(0.8)
        time.sleep(0.3)


def wait_for_export_done(path: Path, timeout: float) -> None:
    deadline = time.time() + timeout
    last_size = -1
    stable = 0
    while time.time() < deadline:
        if path.exists():
            size = path.stat().st_size
            if size > 0 and size == last_size:
                stable += 1
                if stable >= 3:
                    return
            else:
                stable = 0
            last_size = size
        time.sleep(0.5)
    raise RuntimeError(f"export did not finish or file stayed unstable: {path}")


def dry_run_report(win, args: argparse.Namespace) -> dict[str, Any]:
    set_foreground(win)
    screenshot_path = screenshot_window(win, OUT_DIR)
    controls = collect_controls(win)
    dump_path = write_control_dump(controls, OUT_DIR)
    print(f"CSP title: {win.window_text()!r}")
    print(f"CSP rect: {norm_rect(win.rectangle())}")
    print(f"foreground_pid: {get_foreground_pid()}")
    print(f"screenshot: {screenshot_path}")
    print(f"control_dump: {dump_path}")
    print_candidates(controls)
    templates = calibrate_templates(win, controls, screenshot_path) if args.calibrate_templates else {}
    if templates:
        print("\n[templates]")
        for label, path in templates.items():
            print(f"  {label}: {path}")
    return {
        "title": win.window_text(),
        "rect": norm_rect(win.rectangle()),
        "foreground_pid": get_foreground_pid(),
        "screenshot": str(screenshot_path),
        "control_dump": str(dump_path),
        "templates": templates,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Dry-run-first CSP UIA PNG export helper.")
    parser.add_argument("--clip", type=Path, default=DEFAULT_CLIP)
    parser.add_argument("--csp-exe", type=Path, default=DEFAULT_CSP_EXE)
    parser.add_argument(
        "--output-png",
        type=Path,
        default=OUT_DIR / f"Vector_SizePressure_export_{stamp()}.png",
    )
    parser.add_argument("--launch", action="store_true", help="start CSP if no visible CSP window exists")
    parser.add_argument("--open", action="store_true", help="open the target clip as part of the run")
    parser.add_argument("--open-method", choices=("uia", "shell"), default="uia")
    parser.add_argument("--execute", action="store_true", help="perform clicks; omitted means dry-run only")
    parser.add_argument("--calibrate-templates", action="store_true", help="crop UIA-visible candidates into templates")
    parser.add_argument("--timeout", type=float, default=30.0)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    args.clip = args.clip.resolve()
    args.output_png = args.output_png.resolve()
    dry_run = not args.execute

    if not args.clip.exists():
        raise SystemExit(f"clip file missing: {args.clip}")

    try:
        win = connect_csp(timeout=5.0)
    except RuntimeError:
        if not args.launch:
            raise
        start_csp(args.csp_exe, None)
        win = connect_csp(timeout=args.timeout)

    report = dry_run_report(win, args)
    summary_path = OUT_DIR / f"csp_automation_summary_{stamp()}.json"
    summary = {"dry_run": dry_run, "report": report, "actions": []}

    if dry_run:
        print("\nDry-run only: no clicks were sent. Re-run with --execute to operate CSP.")
        summary_path.write_text(json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")
        print(f"summary: {summary_path}")
        return 0

    set_foreground(win)
    if args.open:
        if args.open_method == "uia":
            open_clip_via_uia(win, args.clip, dry_run=False)
        else:
            open_clip_shell(args.clip, dry_run=False)
        win = wait_canvas_visible(timeout=args.timeout)
        set_foreground(win)
        summary["actions"].append({"open_clip": str(args.clip), "method": args.open_method})

    export_png(win, args.output_png, dry_run=False)
    summary["actions"].append({"export_png": str(args.output_png)})
    summary["success"] = True
    summary_path.write_text(json.dumps(summary, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"exported: {args.output_png}")
    print(f"summary: {summary_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
