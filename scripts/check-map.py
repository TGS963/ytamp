#!/usr/bin/env python3
"""Generate and check the ICM routing twins and source links."""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
MAP = ROOT / "docs" / "map"
CATALOG = MAP / "CLAUDE.md"
TWINS = (MAP / "AGENTS.md", MAP / "routing.md")
ROOT_CATALOG = ROOT / "CLAUDE.md"
ROOT_TWINS = (ROOT / "AGENTS.md",)
LINK = re.compile(r"\[[^]]+\]\(([^)]+)\)")


def write_twins() -> None:
    text = CATALOG.read_text(encoding="utf-8")
    for twin in TWINS:
        twin.write_text(text, encoding="utf-8")
    root_text = ROOT_CATALOG.read_text(encoding="utf-8")
    for twin in ROOT_TWINS:
        twin.write_text(root_text, encoding="utf-8")


def check_pair(catalog: Path, twin: Path) -> list[str]:
    if not twin.exists() or twin.read_bytes() != catalog.read_bytes():
        return [f"generated twin differs: {twin.relative_to(ROOT)}"]
    return []


def check_links(files: list[Path]) -> list[str]:
    errors: list[str] = []
    for file in files:
        for target in LINK.findall(file.read_text(encoding="utf-8")):
            target = target.split("#", 1)[0]
            target = re.sub(r":\d+$", "", target)
            if not target or target.startswith(("http://", "https://")):
                continue
            path = (file.parent / target).resolve()
            if not path.exists():
                errors.append(f"missing link: {file.relative_to(ROOT)} -> {target}")
    return errors


def check_twins() -> list[str]:
    errors: list[str] = []
    for twin in TWINS:
        errors.extend(check_pair(CATALOG, twin))
    for twin in ROOT_TWINS:
        errors.extend(check_pair(ROOT_CATALOG, twin))
    errors.extend(check_links(list(MAP.rglob("*.md")) + list(ROOT.glob("*.md"))))
    return errors


def main() -> int:
    if "--write" in sys.argv:
        write_twins()
    errors = check_twins()
    if errors:
        print("\n".join(errors))
        return 1
    print("map check passed: twins and links resolve")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
