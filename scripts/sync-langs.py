#!/usr/bin/env python3
"""Sync arborium language features from crates.io.

Fetches available lang-* features from arborium crate and updates:
- crates/plotnik-langs/Cargo.toml (features + dependencies)
- crates/plotnik-cli/Cargo.toml (feature re-exports)

Usage:
    python scripts/sync-langs.py [--version VERSION] [--dry-run]
"""

import argparse
import json
import re
import urllib.request
from pathlib import Path


def parse_builtin_langs(path: Path) -> set[str]:
    """Extract lang-* features defined in builtin.rs."""
    content = path.read_text()
    langs = set()
    for line in content.splitlines():
        line = line.strip()
        if line.startswith("feature:"):
            # feature: "lang-foo",
            rest = line[len("feature:"):].strip()
            if rest.startswith('"'):
                rest = rest[1:]
                if '"' in rest:
                    lang = rest[:rest.index('"')]
                    if lang.startswith("lang-"):
                        langs.add(lang)
    return langs


def fetch_lang_features(version: str | None) -> tuple[str, list[str]]:
    """Fetch lang-* features from crates.io API."""
    url = "https://crates.io/api/v1/crates/arborium"
    req = urllib.request.Request(url, headers={"User-Agent": "plotnik-sync-langs"})
    with urllib.request.urlopen(req) as resp:
        data = json.load(resp)

    versions = data["versions"]
    if version:
        ver = next((v for v in versions if v["num"] == version), None)
        if not ver:
            available = [v["num"] for v in versions[:10]]
            raise ValueError(f"Version {version} not found. Available: {available}")
    else:
        ver = versions[0]

    features = ver["features"]
    langs = sorted(k for k in features if k.startswith("lang-"))
    return ver["num"], langs


def replace_section(content: str, start_marker: str, end_marker: str, new_content: str) -> str:
    """Replace content between markers (exclusive of markers)."""
    pattern = rf"({re.escape(start_marker)}\n).*?(\n{re.escape(end_marker)})"
    replacement = rf"\g<1>{new_content}\g<2>"
    result, count = re.subn(pattern, replacement, content, flags=re.DOTALL)
    if count == 0:
        raise ValueError(f"Markers not found: {start_marker!r} ... {end_marker!r}")
    return result


def update_plotnik_langs(path: Path, version: str, langs: list[str], dry_run: bool) -> bool:
    """Update plotnik-langs/Cargo.toml."""
    content = path.read_text()
    original = content

    # Generate all-languages array
    all_langs_items = "\n".join(f'    "{lang}",' for lang in langs)
    content = replace_section(
        content,
        "# @generated:all-languages:begin",
        "# @generated:all-languages:end",
        all_langs_items,
    )

    # Generate individual features
    features = "\n".join(f'{lang} = ["dep:arborium-{lang[5:]}"]' for lang in langs)
    content = replace_section(
        content,
        "# @generated:lang-features:begin",
        "# @generated:lang-features:end",
        features,
    )

    # Generate dependencies
    deps = "\n".join(
        f'arborium-{lang[5:]} = {{ version = "{version}", optional = true }}'
        for lang in langs
    )
    content = replace_section(
        content,
        "# @generated:lang-deps:begin",
        "# @generated:lang-deps:end",
        deps,
    )

    if content == original:
        print(f"  {path}: no changes")
        return False

    if dry_run:
        print(f"  {path}: would update")
    else:
        path.write_text(content)
        print(f"  {path}: updated")
    return True


def update_plotnik_cli(path: Path, langs: list[str], dry_run: bool) -> bool:
    """Update plotnik-cli/Cargo.toml."""
    content = path.read_text()
    original = content

    # Generate all-languages array
    all_langs_items = "\n".join(f'    "{lang}",' for lang in langs)
    content = replace_section(
        content,
        "# @generated:all-languages:begin",
        "# @generated:all-languages:end",
        all_langs_items,
    )

    # Generate individual features (re-exports)
    features = "\n".join(f'{lang} = ["plotnik-langs/{lang}"]' for lang in langs)
    content = replace_section(
        content,
        "# @generated:lang-features:begin",
        "# @generated:lang-features:end",
        features,
    )

    if content == original:
        print(f"  {path}: no changes")
        return False

    if dry_run:
        print(f"  {path}: would update")
    else:
        path.write_text(content)
        print(f"  {path}: updated")
    return True


def check_builtin_consistency(builtin_path: Path, langs: list[str]) -> tuple[list[str], list[str]]:
    """Check if builtin.rs defines all langs from crates.io.

    Returns (added, removed) where:
    - added: langs new in arborium, need to add to builtin.rs
    - removed: langs removed from arborium, need to delete from builtin.rs
    """
    defined = parse_builtin_langs(builtin_path)
    expected = set(langs)

    added = sorted(expected - defined)
    removed = sorted(defined - expected)
    return added, removed


def post_pr_comment(added: list[str], removed: list[str]) -> None:
    """Post a comment to the current PR about builtin.rs inconsistency."""
    import subprocess

    lines = ["Update `crates/plotnik-langs/src/builtin.rs`:", ""]
    if added:
        lines.append("Add: " + ", ".join(f"`{lang}`" for lang in added))
    if removed:
        lines.append("Remove: " + ", ".join(f"`{lang}`" for lang in removed))

    body = "\n".join(lines)
    subprocess.run(["gh", "pr", "comment", "--body", body], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", help="arborium version (default: latest)")
    parser.add_argument("--dry-run", action="store_true", help="print changes without writing")
    parser.add_argument("--ci", action="store_true", help="CI mode: post PR comment on mismatch")
    args = parser.parse_args()

    version, langs = fetch_lang_features(args.version)
    print(f"Found {len(langs)} languages in arborium {version}")

    root = Path(__file__).resolve().parent.parent
    langs_toml = root / "crates/plotnik-langs/Cargo.toml"
    cli_toml = root / "crates/plotnik-cli/Cargo.toml"
    builtin_rs = root / "crates/plotnik-langs/src/builtin.rs"

    changed = False
    changed |= update_plotnik_langs(langs_toml, version, langs, args.dry_run)
    changed |= update_plotnik_cli(cli_toml, langs, args.dry_run)

    if args.dry_run and changed:
        print("\nRun without --dry-run to apply changes")

    added, removed = check_builtin_consistency(builtin_rs, langs)
    if added or removed:
        print("\nbuiltin.rs is out of sync with arborium:")
        for lang in added:
            print(f"  + {lang} (new in arborium, add to define_langs!)")
        for lang in removed:
            print(f"  - {lang} (removed from arborium, delete from define_langs!)")
        if args.ci:
            post_pr_comment(added, removed)
        raise SystemExit(1)


if __name__ == "__main__":
    main()
