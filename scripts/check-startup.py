#!/usr/bin/env python3
"""Start the desktop app and check that its native window stays open."""

import ctypes
from ctypes import wintypes
from pathlib import Path
import subprocess
import sys
import time


def windows_window(process_id: int) -> bool:
    user32 = ctypes.windll.user32
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
    found = []

    def inspect_window(handle, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(handle, ctypes.byref(owner))
        title = ctypes.create_unicode_buffer(256)
        user32.GetWindowTextW(handle, title, len(title))
        if owner.value == process_id and user32.IsWindowVisible(handle):
            found.append(title.value == "ytamp")
        return True

    user32.EnumWindows(callback_type(inspect_window), 0)
    return any(found)


def linux_window(process_id: int) -> bool:
    result = subprocess.run(
        ["xdotool", "search", "--onlyvisible", "--pid", str(process_id), "--name", "^ytamp$"],
        capture_output=True,
        check=False,
    )
    return result.returncode == 0 and bool(result.stdout.strip())


def check_window(process: subprocess.Popen) -> None:
    window_exists = windows_window if sys.platform == "win32" else linux_window
    deadline = time.monotonic() + 15
    seen_window = False
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Application exited early with code {process.returncode}")
        seen_window = seen_window or window_exists(process.pid)
        time.sleep(0.25)
    if not seen_window or not window_exists(process.pid):
        raise RuntimeError("No visible ytamp window survived the startup check")


def stop(process: subprocess.Popen) -> None:
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()


def main() -> None:
    executable = Path(sys.argv[1]).resolve(strict=True)
    log = Path("startup.log")
    with log.open("w", encoding="utf-8") as output:
        process = subprocess.Popen([str(executable)], stdout=output, stderr=subprocess.STDOUT)
        try:
            check_window(process)
        finally:
            stop(process)
    contents = log.read_text(encoding="utf-8", errors="replace")
    if "panicked at" in contents:
        raise RuntimeError(f"The application reported a panic. See {log}")
    print("PASS: native ytamp window stayed open for 15 seconds without a panic")


if __name__ == "__main__":
    main()
