#!/usr/bin/env python3
"""Stage ytamp for the current platform without building or signing it.

Examples:
  python3 scripts/package.py
  python3 scripts/package.py --platform linux --binary target/release/ytamp
  python3 scripts/package.py --platform macos --output dist/ytamp.app

The tool updates only files it owns in the selected output directory.  It does
not delete the directory, so files placed there by a caller remain untouched.
"""

from __future__ import annotations

import argparse
import json
import tomllib
import shutil
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
APP_ID = "io.github.TGS963.ytamp"
APP_NAME = "ytamp"
PLATFORMS = {"darwin": "macos", "linux": "linux", "win32": "windows"}


def cargo_version() -> str:
    """Read the package version without requiring Cargo or third-party modules."""
    with (ROOT / "Cargo.toml").open("rb") as file:
        return tomllib.load(file)["package"]["version"]


def copy_file(source: Path, destination: Path, executable: bool = False) -> None:
    """Copy one known package file, making parent directories as needed."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    if executable:
        destination.chmod(destination.stat().st_mode | 0o111)


def write_text(destination: Path, content: str) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(content, encoding="utf-8")


def require_file(path: Path, label: str) -> Path:
    if not path.is_file():
        raise FileNotFoundError(f"{label} does not exist or is not a file: {path}")
    return path


def prepare_output(output: Path, platform: str, version: str) -> None:
    """Claim an empty directory, and only update directories previously claimed."""
    marker = output / ".ytamp-package.json"
    if output.exists():
        if not output.is_dir():
            raise FileExistsError(f"output is not a directory: {output}")
        if marker.exists():
            try:
                previous = json.loads(marker.read_text(encoding="utf-8"))
            except json.JSONDecodeError as error:
                raise ValueError(f"invalid package marker: {marker}") from error
            if previous.get("app_id") != APP_ID or previous.get("platform") != platform:
                raise FileExistsError(f"output belongs to a different package layout: {output}")
        elif any(output.iterdir()):
            raise FileExistsError(
                f"refusing to modify non-package output directory: {output}; choose an empty directory"
            )
    else:
        output.mkdir(parents=True)
    write_text(marker, json.dumps({"app_id": APP_ID, "platform": platform,
                                   "version": version}, indent=2) + "\n")


def stage_macos(output: Path, binary: Path, version: str) -> None:
    contents = output / "Contents"
    template = (ROOT / "packaging/macos/Info.plist.in").read_text(encoding="utf-8")
    write_text(contents / "Info.plist", template.replace("@VERSION@", version))
    copy_file(binary, contents / "MacOS" / APP_NAME, executable=True)
    copy_file(require_file(ROOT / "assets/branding/AppIcon.icns", "macOS icon"),
              contents / "Resources" / "AppIcon.icns")
    copy_file(require_file(ROOT / "LICENSE", "license"), contents / "Resources" / "LICENSE")


def stage_linux(output: Path, binary: Path) -> None:
    copy_file(binary, output / "usr/bin" / APP_NAME, executable=True)
    copy_file(ROOT / "packaging/linux" / f"{APP_ID}.desktop",
              output / "usr/share/applications" / f"{APP_ID}.desktop")
    copy_file(require_file(ROOT / "assets/branding/icon.png", "Linux icon"),
              output / "usr/share/icons/hicolor/512x512/apps" / f"{APP_ID}.png")
    copy_file(require_file(ROOT / "LICENSE", "license"),
              output / "usr/share/licenses" / APP_NAME / "LICENSE")


def stage_windows(output: Path, binary: Path) -> None:
    copy_file(binary, output / f"{APP_NAME}.exe", executable=True)
    copy_file(require_file(ROOT / "assets/branding/icon.ico", "Windows icon"), output / "icon.ico")
    copy_file(require_file(ROOT / "LICENSE", "license"), output / "LICENSE")


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=("macos", "linux", "windows"),
                        default=PLATFORMS.get(sys.platform),
                        help="target package layout (defaults to this host platform)")
    parser.add_argument("--binary", type=Path,
                        help="compiled executable (defaults to target/release/ytamp)")
    parser.add_argument("--output", type=Path,
                        help="final staging directory (defaults under dist/)")
    return parser.parse_args()


def main() -> int:
    args = arguments()
    if args.platform is None:
        raise SystemExit(f"unsupported host platform: {sys.platform}; pass --platform")
    version = cargo_version()
    default_binary = ROOT / "target/release" / ("ytamp.exe" if args.platform == "windows" else APP_NAME)
    binary = require_file((args.binary or default_binary).resolve(), "binary")
    suffix = ".app" if args.platform == "macos" else ""
    output = (args.output or ROOT / "dist" / f"{APP_NAME}-{version}-{args.platform}{suffix}").resolve()
    prepare_output(output, args.platform, version)

    if args.platform == "macos":
        stage_macos(output, binary, version)
    elif args.platform == "linux":
        stage_linux(output, binary)
    else:
        stage_windows(output, binary)

    print(output)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (FileNotFoundError, OSError, ValueError) as error:
        raise SystemExit(f"package.py: error: {error}")
