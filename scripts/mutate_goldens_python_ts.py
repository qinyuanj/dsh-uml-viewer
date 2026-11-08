#!/usr/bin/env python3
"""Development-only Python/TypeScript mutation oracle from pinned, unmodified tools.

Run on native Linux/WSL with Node (npm on PATH) and a project virtualenv that
has coverage.py and pytest:
  python3 scripts/mutate_goldens_python_ts.py --references /path/to/spec203-notes/ref \\
      --venv /tmp/spec203-t13-venv
No global installation occurs: grammar/test dependencies go into a disposable
/tmp scratch, and the fixtures are mutated only as /tmp copies.
"""
from __future__ import annotations

import argparse
import contextlib
import dataclasses
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

PINS = {
    "mutator": "f772a8329fba3f2f81bfca0a81abfa3210af9f4a",
    "crapper": "4068857bc44e6383cf4a5e4d0b5c1111b49ce0b4",
}
CLOCK = "2026-11-07T12:00:00+00:00"
PROJECTS = {
    # name: (targets in invocation order, directories that get a .venv link)
    "python": (["src/shapes/geometry.py", "legacy/tally/core.py"], [".", "legacy"]),
    "typescript": (["src/calc.ts", "src/view.tsx"], []),
}
ORIGINAL_TESTS = ["test_snapshot_bytes.py", "test_sites.py", "test_runner.py"]
SCRATCH_PREFIX = "spec203-t13-oracle."


def git(ref: Path, *args: str) -> str:
    # DrvFS references are Windows checkouts; use invocation-only settings.
    return subprocess.check_output(["git", "-c", "core.filemode=false", "-c", "core.autocrlf=true",
                                    "-C", str(ref), *args], text=True).strip()


def check_references(refs: Path) -> None:
    for name, revision in PINS.items():
        assert git(refs / name, "rev-parse", "HEAD") == revision, name
        assert not git(refs / name, "status", "--porcelain"), name


def files_under(base: Path) -> dict[str, bytes]:
    return {p.relative_to(base).as_posix(): p.read_bytes()
            for p in sorted(base.rglob("*")) if p.is_file() and not p.is_symlink()}


def snapshots(root: Path) -> dict[str, bytes]:
    base = root / ".metrics" / "mutate"
    return {p.relative_to(base).as_posix(): p.read_bytes() for p in sorted(base.rglob("*.edn"))}


def run_cli(run, args: list[str]) -> tuple[int, str]:
    stdout, stderr = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
        code = run(args)
    print(stdout.getvalue(), end="")
    print(stderr.getvalue(), end="", file=sys.stderr)
    return code, stdout.getvalue() + stderr.getvalue()


def oracle_project(name: str, fixture: Path, scratch: Path, venv: Path, run) -> dict:
    targets, venv_dirs = PROJECTS[name]
    source = fixture / name / "project"
    before = files_under(source)
    root = scratch / name / "project"
    shutil.copytree(source, root)
    for directory in venv_dirs:
        (root / directory / ".venv").symlink_to(venv, target_is_directory=True)
    args = ["--root", str(root), "--max-workers", "1", "--timeout-factor", "20", *targets]
    first, first_log = run_cli(run, ["--mutate-all", *args])
    published = snapshots(root)
    second, second_log = run_cli(run, args)
    rerun = snapshots(root)
    assert published, f"{name}: no snapshot published"
    assert rerun == published, f"{name}: unchanged rerun changed snapshot bytes"
    for relative, data in before.items():
        assert (root / relative).read_bytes() == data, f"{name}: original source changed {relative}"
    for data in published.values():
        assert b"\r" not in data and data.endswith(b"\n")
    output = fixture / name / "goldens"
    if output.exists():
        shutil.rmtree(output)
    for relative, data in published.items():
        (output / relative).parent.mkdir(parents=True, exist_ok=True)
        (output / relative).write_bytes(data)
    return {
        "targets": targets,
        "venv_links": venv_dirs,
        "arguments": ["--root", "<native-/tmp-copy>/project", "--max-workers", "1",
                      "--timeout-factor", "20", *targets],
        "first_run": {"prepend": ["--mutate-all"], "exit": first,
                      "no_coverage_warnings": [line for line in first_log.splitlines()
                                               if line.startswith("No coverage data for")]},
        "unchanged_rerun": {"exit": second, "identical_snapshot_bytes": True},
        "original_source_bytes_unchanged": True,
        "snapshots": {key: {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                      for key, data in sorted(published.items())},
    }


def site_inventory(fixture: Path) -> dict:
    """The original's sites, forms and privacy for every corpus and fixture source."""

    from mutator.functions import form_digests, form_spans, sites_in_file

    found = {}
    bases = [(fixture / "sites", "sites")] + [(fixture / name / "project", name) for name in PROJECTS]
    for base, label in bases:
        for path in sorted(base.rglob("*")):
            if path.suffix not in {".py", ".ts", ".tsx"} or not path.is_file():
                continue
            relative = path.relative_to(base).as_posix()
            source = path.read_bytes().decode("utf-8")
            spans = form_spans(source, path, base)
            digests = form_digests(source, path, base)
            found[f"{label}/{relative}"] = {
                "sites": [dataclasses.asdict(site) for site in sites_in_file(source, path, base, relative)],
                "forms": [{"namespace": key[0], "id": key[1], "line": span[0], "end_line": span[1],
                           "name": span[2], "private": span[3], "hash": digests[key]}
                          for key, span in spans.items()],
            }
    return found


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--references", required=True, type=Path)
    parser.add_argument("--venv", required=True, type=Path)
    options = parser.parse_args()
    if sys.platform != "linux":
        parser.error("run inside native Linux/WSL, not Windows or MSYS")
    refs = options.references.resolve()
    venv = options.venv.absolute()
    fixture = Path(__file__).resolve().parents[1] / "fixtures" / "mutate"
    check_references(refs)
    venv_python = venv / "bin" / "python"
    versions = subprocess.check_output(
        [str(venv_python), "-c", "import coverage, pytest; print(coverage.__version__, pytest.__version__)"],
        text=True).split()
    node = subprocess.check_output(["node", "--version"], text=True).strip()
    npm = subprocess.check_output(["npm", "--version"], text=True).strip()
    scratch = Path(tempfile.mkdtemp(prefix=SCRATCH_PREFIX, dir="/tmp")).resolve()
    print(f"Native scratch: {scratch}", flush=True)
    assert scratch.parent == Path("/tmp") and scratch.name.startswith(SCRATCH_PREFIX)
    try:
        os.environ.update(HOME=str(scratch / "home"), XDG_CACHE_HOME=str(scratch / "cache"),
                          PYTHONDONTWRITEBYTECODE="1", PYTEST_DISABLE_PLUGIN_AUTOLOAD="1",
                          PIP_NO_CACHE_DIR="1", PIP_DISABLE_PIP_VERSION_CHECK="1", TZ="UTC")
        (scratch / "home").mkdir()
        deps = scratch / "deps"
        subprocess.run([sys.executable, "-m", "pip", "install", "--quiet", "--target", str(deps),
                        "tree-sitter==0.26.0", "tree-sitter-language-pack==1.21.0", "pytest==9.2.0"],
                       check=True)
        paths = [str(deps), str(refs / "mutator/src"), str(refs / "crapper/src"), str(refs / "mutator/tests")]
        os.environ["PYTHONPATH"] = os.pathsep.join(paths)
        sys.path[:0] = paths
        sys.dont_write_bytecode = True
        tests = subprocess.run([sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider",
                                "--basetemp", str(scratch / "pytest"),
                                *[str(refs / "mutator/tests" / name) for name in ORIGINAL_TESTS]],
                               capture_output=True, text=True)
        print(tests.stdout[-2000:], tests.stderr[-2000:])
        assert tests.returncode == 0, "original tests failed"
        summary = tests.stdout.strip().splitlines()[-1]
        from mutator.cli import run
        import mutator.metrics
        from test_snapshot_bytes import FixedTime

        mutator.metrics.datetime = FixedTime
        projects = {name: oracle_project(name, fixture, scratch, venv, run) for name in PROJECTS}
        inventory = site_inventory(fixture)
        (fixture / "sites" / "sites.json").write_text(
            json.dumps(inventory, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
        check_references(refs)
        evidence = {
            "references": PINS,
            "clock": CLOCK,
            "isolation": "namespace-view",
            "toolchain": {"python": sys.version.split()[0], "coverage": versions[0],
                          "pytest": versions[1], "node": node, "npm": npm},
            "original_tests": {"files": ORIGINAL_TESTS, "summary": summary},
            "projects": projects,
            "sites_files": len(inventory),
            "sites_total": sum(len(entry["sites"]) for entry in inventory.values()),
            "reference_worktrees_clean": True,
        }
        (fixture / "python-typescript-evidence.json").write_text(
            json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(evidence, indent=2))
    finally:
        print(f"Cleanup verified native scratch: {scratch}", flush=True)
        assert scratch.resolve() == scratch and scratch.parent == Path("/tmp")
        assert scratch.name.startswith(SCRATCH_PREFIX)
        shutil.rmtree(scratch)


if __name__ == "__main__":
    main()
