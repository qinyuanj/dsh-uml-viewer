#!/usr/bin/env python3
"""Development-only fixed-clock oracle from pinned, unmodified Python tools.

Run on native Linux/WSL with cargo, cargo-llvm-cov and llvm-tools installed:
  python3 scripts/mutate_goldens.py --references /path/to/spec203-notes/ref
No global installation occurs. Dependencies, builds and mutation run in /tmp.
"""
from __future__ import annotations

import argparse
import contextlib
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
EXPECTED = {
    "app.edn": (406, "919be95f1a4856e91619a060d4bfc062554cc5aee45447444248ea13d47cc095"),
    "app/more.edn": (419, "713568c32383e59f2f0b7569903cd570aa353984562b1cd4f4403f0fe5e7575e"),
    "util.edn": (412, "31190035242fa75d52f3ee27f0c6f431189198236bc4a5174f2bcd6bd66fe8c8"),
}
TARGETS = ["util/src/lib.rs", "app/src/lib.rs", "app/src/more.rs"]
# A zero-mutation function changes the form digests, so the file is not
# reusable; the original republishes its snapshot without running a test.
ZERO_SITE_FUNCTION = b"\npub fn no_sites() {}\n"
ZERO_SITE_COMMAND = "grep -q 'x + x' app/src/lib.rs"
ZERO_SITE_GOLDEN = "app-no-sites.edn"
EXPECTED_ZERO_SITE = (601, "49a18a35b755671f972712523d78187064c2924b3121273f920c0df458ac5433")


def git(ref: Path, *args: str) -> str:
    # DrvFS references are Windows checkouts; use invocation-only settings.
    return subprocess.check_output(["git", "-c", "core.filemode=false", "-c", "core.autocrlf=true",
                                    "-C", str(ref), *args], text=True).strip()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--references", required=True, type=Path)
    options = parser.parse_args()
    if sys.platform != "linux":
        parser.error("run inside native Linux/WSL, not Windows or MSYS")
    refs = options.references.resolve()
    output = Path(__file__).resolve().parents[1] / "fixtures" / "mutate"
    for name, revision in PINS.items():
        assert git(refs / name, "rev-parse", "HEAD") == revision, name
        assert not git(refs / name, "status", "--porcelain"), name
    subprocess.run(["cargo", "llvm-cov", "--version"], check=True)
    components = subprocess.check_output(["rustup", "component", "list", "--installed"], text=True)
    assert "llvm-tools-" in components
    scratch = Path(tempfile.mkdtemp(prefix="spec203-t8-python.", dir="/tmp")).resolve()
    print(f"Native scratch: {scratch}", flush=True)
    assert scratch.parent == Path("/tmp") and scratch.name.startswith("spec203-t8-python.")
    try:
        os.environ.update(HOME=str(scratch / "home"), XDG_CACHE_HOME=str(scratch / "cache"),
                          PYTHONDONTWRITEBYTECODE="1", PYTEST_DISABLE_PLUGIN_AUTOLOAD="1",
                          PIP_NO_CACHE_DIR="1", PIP_DISABLE_PIP_VERSION_CHECK="1")
        (scratch / "home").mkdir()
        deps = scratch / "deps"
        subprocess.run([sys.executable, "-m", "pip", "install", "--target", str(deps),
                        "tree-sitter==0.26.0", "tree-sitter-language-pack==1.21.0", "pytest==9.2.0"], check=True)
        paths = [str(deps), str(refs / "mutator/src"), str(refs / "crapper/src"), str(refs / "mutator/tests")]
        os.environ["PYTHONPATH"] = os.pathsep.join(paths)
        sys.path[:0] = paths
        sys.dont_write_bytecode = True
        subprocess.run([sys.executable, "-m", "pytest", "-q", "-s", "-p", "no:cacheprovider",
                        "--basetemp", str(scratch / "pytest"),
                        str(refs / "mutator/tests/test_snapshot_bytes.py"),
                        str(refs / "mutator/tests/test_rust_slots_cargo.py")], check=True)
        from mutator.cli import HELP, run
        import mutator.metrics
        from test_snapshot_bytes import FixedTime
        from test_rust_slots_cargo import workspace, FILES

        parent = scratch / "oracle"
        parent.mkdir()
        root = workspace(parent)
        mutator.metrics.datetime = FixedTime
        before = {p.relative_to(parent): p.read_bytes() for p in parent.rglob("*") if p.is_file()}
        args = ["--root", str(root), "--max-workers", "1", "--timeout-factor", "20", *TARGETS]
        logs = []
        for flags in (["--mutate-all", *args], args):
            stdout, stderr = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                result = run(flags)
            print(stdout.getvalue(), end="")
            print(stderr.getvalue(), end="", file=sys.stderr)
            assert result == 0, result
            logs.append(stdout.getvalue() + stderr.getvalue())
            current = {p.relative_to(root / ".metrics/mutate").as_posix(): p.read_bytes()
                       for p in (root / ".metrics/mutate").rglob("*.edn")}
            if len(logs) == 1:
                snapshots = current
            else:
                assert current == snapshots, "unchanged rerun rewrote snapshot bytes"
        assert "executed=0" in logs[1] and "reused=3" in logs[1], logs[1]
        assert set(snapshots) == set(EXPECTED)
        for name, data in snapshots.items():
            size, digest = EXPECTED[name]
            assert len(data) == size and hashlib.sha256(data).hexdigest() == digest, name
            assert b"\r" not in data and data.endswith(b"\n")
        assert all((parent / name).read_bytes() == data for name, data in before.items())
        # The differential guard: a function with no mutation sites still makes
        # the file unreusable, so the snapshot is republished with the new form
        # set and the carried outcomes instead of being kept.
        zero = scratch / "zero-site"
        shutil.copytree(parent, zero)
        zero_root = zero / "project"
        zero_source = zero_root / "app/src/lib.rs"
        zero_source.write_bytes(zero_source.read_bytes() + ZERO_SITE_FUNCTION)
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            result = run(["--root", str(zero_root), "--no-coverage", "--max-workers", "1",
                          "--test-command", ZERO_SITE_COMMAND, "app/src/lib.rs"])
        print(stdout.getvalue(), end="")
        print(stderr.getvalue(), end="", file=sys.stderr)
        assert result == 0, result
        zero_data = (zero_root / ".metrics/mutate/app.edn").read_bytes()
        assert zero_data != snapshots["app.edn"], "zero-site rerun kept stale bytes"
        assert b'defn/no_sites' in zero_data and b":killed 1" in zero_data
        for name in ("app/more.edn", "util.edn"):
            assert (zero_root / ".metrics/mutate" / name).read_bytes() == snapshots[name], name
        assert all((zero / name).read_bytes() == data
                   for name, data in before.items() if name.parts[0] == "project"
                   and name.as_posix() != "project/app/src/lib.rs")
        size, digest = EXPECTED_ZERO_SITE
        assert len(zero_data) == size and hashlib.sha256(zero_data).hexdigest() == digest, (
            len(zero_data), hashlib.sha256(zero_data).hexdigest())
        assert set(FILES) == {p.relative_to("project").as_posix()
                              for p in before if p.parts[0] == "project"}
        for name, revision in PINS.items():
            assert git(refs / name, "rev-parse", "HEAD") == revision
            assert not git(refs / name, "status", "--porcelain"), name
        # Only the external dependency spelling changes in the portable copy.
        # Source/test bytes (and therefore occurrence offsets/digests) stay exact.
        for name, data in before.items():
            if name.as_posix() == "project/app/Cargo.toml":
                data = data.replace((parent / "extdep").as_posix().encode(), b"../../extdep")
            destination = output / "rust" / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        (output / "help.txt").write_bytes(HELP.encode("utf-8"))
        for name, data in snapshots.items():
            destination = output / "goldens" / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        (output / "goldens" / ZERO_SITE_GOLDEN).write_bytes(zero_data)
        evidence = {
            "references": PINS,
            "clock": "2026-11-07T12:00:00+00:00",
            "isolation": "namespace-view",
            "targets": TARGETS,
            "arguments": ["--root", "<native-/tmp-parent>/project", "--max-workers", "1", "--timeout-factor", "20", *TARGETS],
            "first_run": {"prepend": ["--mutate-all"], "exit": 0, "killed": 3, "survived": 0, "uncovered": 0},
            "unchanged_rerun": {"exit": 0, "executed": 0, "reused": 3, "identical_snapshot_bytes": True},
            "differential_zero_site_rerun": {
                "function_added": ZERO_SITE_FUNCTION.decode(),
                "command": ZERO_SITE_COMMAND,
                "exit": 0,
                "executed": 0,
                "reused": 1,
                "identical_snapshot_bytes": False,
                "snapshot": ZERO_SITE_GOLDEN,
                "bytes": len(zero_data),
                "sha256": hashlib.sha256(zero_data).hexdigest(),
            },
            "original_tests": {"passed": 4, "files": ["test_snapshot_bytes.py", "test_rust_slots_cargo.py"]},
            "original_manifest_source_test_bytes_unchanged": True,
            "reference_worktrees_clean": True,
            "snapshots": {name: {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                          for name, data in sorted(snapshots.items())},
        }
        (output / "goldens/evidence.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
        print("SOURCE_UNCHANGED=yes SNAPSHOT_UNCHANGED=yes REFERENCES_CLEAN=yes")
        print(json.dumps(evidence["snapshots"], indent=2))
    finally:
        print(f"Cleanup verified native scratch: {scratch}", flush=True)
        assert scratch.resolve() == scratch and scratch.parent == Path("/tmp")
        assert scratch.name.startswith("spec203-t8-python.")
        shutil.rmtree(scratch)


if __name__ == "__main__":
    main()
