"""Capture original Python dryer CLI bytes; never synthesize expected reports.

python -I scripts/dry_goldens.py <dryer-checkout> <crapper-checkout> [--check]
See fixtures/dry/README.md for pinned dependencies and scenario syntax.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

REPO = Path(__file__).resolve().parents[1]
FIXTURES = REPO / "fixtures" / "dry"
PINS = {
    "dryer": "b3f72ad9681a6574c5714d3c54d50e2ea3eff91a",
    "crapper": "4068857bc44e6383cf4a5e4d0b5c1111b49ce0b4",
}
PACKAGES = {"tree-sitter": "0.26.0", "tree-sitter-language-pack": "1.21.0"}
RUNNER = (
    "import sys; sys.path[:0] = sys.argv[1:3]; "
    "from dryer.cli import main; main(sys.argv[3:])"
)


def original(checkout: Path, name: str) -> None:
    result = subprocess.run(
        ["git", "-C", str(checkout), "rev-parse", "HEAD"],
        capture_output=True, text=True, check=True,
    )
    if result.stdout.strip() != PINS[name]:
        raise SystemExit(f"{name}: expected {PINS[name]}, got {result.stdout.strip()}")
    for command in (["diff", "--quiet", "HEAD", "--", "src", "pyproject.toml"],
                    ["ls-files", "--others", "--exclude-standard", "src"]):
        result = subprocess.run(["git", "-C", str(checkout), *command],
                                capture_output=True, check=False)
        if result.returncode or result.stdout:
            raise SystemExit(f"{name}: source checkout must be clean")


def scenario(path: Path) -> list[str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0] != "project rust":
        raise SystemExit(f"{path}: expected project rust")
    args = []
    for line in lines[1:]:
        if not line or line.startswith(("#", "git ", "commit ", "modify ", "rename ")):
            continue
        if not line.startswith("arg "):
            raise SystemExit(f"{path}: unknown line {line!r}")
        args.append(line[4:])
    return args


def setup(root: Path, lines: list[str]) -> None:
    """Real git setup for --changed; never mutate the committed fixture."""
    for line in lines:
        if not line or line.startswith(("#", "arg ")):
            continue
        if line == "git init":
            subprocess.run(["git", "init", "--quiet"], cwd=root, check=True)
        elif line == "git add":
            subprocess.run(["git", "add", "."], cwd=root, check=True)
        elif line.startswith("commit "):
            paths = line[7:].split()
            subprocess.run(["git", "add", "--", *paths], cwd=root, check=True)
            subprocess.run(["git", "-c", "user.name=oracle", "-c",
                            "user.email=<REDACTED_EMAIL>", "commit", "--quiet", "-m", "base"],
                           cwd=root, check=True)
        elif line.startswith("modify "):
            target = root / line[7:]
            target.write_text(target.read_text(encoding="utf-8") +
                              "// Modified for a git status selection.\n", encoding="utf-8")
        elif line.startswith("rename "):
            source, destination = line[7:].split()
            (root / source).rename(root / destination)
        else:
            raise SystemExit(f"unknown setup line {line!r}")


def normalized_stderr(data: bytes, root: Path) -> bytes:
    # CLI output is captured as bytes. Only stderr's host-specific root and
    # separators/line endings are normalized. stdout and snapshot stay exact.
    text = data.decode("utf-8").replace("\r\n", "\n")
    for form in (str(root), root.as_posix()):
        text = text.replace(form, "<ROOT>")
    return text.replace("\\", "/").encode("utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dryer", type=Path)
    parser.add_argument("crapper", type=Path)
    parser.add_argument("--check", action="store_true", help="compare without overwriting")
    options = parser.parse_args()
    dryer, crapper = options.dryer.resolve(), options.crapper.resolve()
    original(dryer, "dryer")
    original(crapper, "crapper")
    for package, expected in PACKAGES.items():
        actual = importlib.metadata.version(package)
        if actual != expected:
            raise SystemExit(f"{package}: expected {expected}, got {actual}")
    prefix = [sys.executable, "-I", "-c", RUNNER, str(dryer / "src"), str(crapper / "src")]
    environment = dict(os.environ, PYTHONIOENCODING="utf-8", PYTHONHASHSEED="0")
    mismatches = []

    def publish(path: Path, expected: bytes | None) -> None:
        if options.check:
            actual = path.read_bytes() if path.is_file() else None
            if actual != expected:
                mismatches.append(str(path.relative_to(REPO)))
        elif expected is None:
            path.unlink(missing_ok=True)
        else:
            path.write_bytes(expected)

    with tempfile.TemporaryDirectory(prefix="dry-oracle-") as scratch:
        help_result = subprocess.run([*prefix, "--help"], cwd=scratch,
                                     env=environment, capture_output=True, check=False)
        if help_result.returncode or help_result.stderr:
            raise SystemExit(f"oracle --help failed: {help_result.stderr!r}")
        publish(FIXTURES / "help.txt", help_result.stdout)
        for path in sorted((FIXTURES / "goldens").glob("*/scenario")):
            root = Path(scratch) / path.parent.name / "project"
            shutil.copytree(FIXTURES / "rust", root,
                            ignore=shutil.ignore_patterns("target", "Cargo.lock"))
            setup(root, path.read_text(encoding="utf-8").splitlines()[1:])
            result = subprocess.run([*prefix, "--root", str(root), *scenario(path)],
                                    cwd=root, env=environment, capture_output=True, check=False)
            out = path.parent
            snapshot = root / ".metrics" / "dry.edn"
            publish(out / "dry.edn", snapshot.read_bytes() if snapshot.is_file() else None)
            publish(out / "stdout", result.stdout)
            publish(out / "stderr", normalized_stderr(result.stderr, root))
            publish(out / "exit", f"{result.returncode}\n".encode("ascii"))
            print(f"{out.name}: exit={result.returncode}, snapshot={snapshot.is_file()}")
    if mismatches:
        raise SystemExit("Oracle differences:\n" + "\n".join(mismatches))
    if not options.check:
        provenance = {
            "oracle_commits": PINS,
            "python": sys.version,
            "platform": sys.platform,
            "packages": {package: importlib.metadata.version(package) for package in PACKAGES},
            "source_sha256": {
                f"{name}/{path.relative_to(checkout).as_posix()}": hashlib.sha256(path.read_bytes()).hexdigest()
                for name, checkout in (("dryer", dryer), ("crapper", crapper))
                for path in sorted((checkout / "src").rglob("*.py"))
            },
        }
        (FIXTURES / "goldens" / "provenance.json").write_bytes(
            (json.dumps(provenance, indent=2, sort_keys=True) + "\n").encode("utf-8"))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
