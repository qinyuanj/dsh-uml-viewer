"""Regenerate fixtures/crap/goldens with the Python crapper (ticket #2).

    python -I scripts/crap_goldens.py <crapper checkout>

The checkout must be at the pinned commit (fixtures/crap/goldens/SOURCE), with
tree-sitter and tree-sitter-language-pack importable. Each
`goldens/<name>/scenario` names a sample project, setup steps and the argv; the
script runs crapper on a fresh copy and writes what it published (`crap.edn`,
absent when nothing was published), `stdout`, `stderr` and `exit`. The Rust
test `crates/uml-viewer/tests/crap_goldens.rs` reads the same scenario files
and compares only these committed bytes; it never runs Python.

Scenario lines: `project rust|rust-fixture`, `file <fixture> <project path>`
(copy a file from fixtures/crap), `crlf <project path>` (rewrite a source with
CRLF line ends, as a Windows autocrlf checkout would), `arg <value>`.
`{COPY_FRESH_LCOV}` in an arg is the host shell command that copies
`fixture.lcov` to `coverage/lcov.info`. stderr is normalized: the project root
becomes `<ROOT>`, and the `+ (cwd) command` echo
lines, which name the host shell, are dropped. Native separators on `Wrote`
and other root-bearing lines become `<SEP>`; all other bytes stay exact.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
CRAP = REPO / "fixtures" / "crap"
GOLDENS = CRAP / "goldens"
PROJECTS = {name: CRAP / name for name in ("rust", "go", "java", "clojure")}
PROJECTS["rust-fixture"] = REPO / "rust-fixture"
for name in ("python-fixture", "typescript-fixture", "lua-fixture"):
    PROJECTS[name] = REPO / name
PROJECTS["language-edges"] = CRAP / "language-edges"
PINNED = (GOLDENS / "SOURCE").read_text(encoding="utf-8").split()[1]


def copy_fresh_lcov() -> str:
    if os.name == "nt":
        return r"mkdir coverage 2>NUL & copy /Y fixture.lcov coverage\lcov.info >NUL"
    return "mkdir -p coverage && cp fixture.lcov coverage/lcov.info"


def read_scenario(path: Path) -> tuple[str, list[tuple[str, str]], list[str]]:
    project, steps, args = None, [], []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        word, _, rest = line.partition(" ")
        if word == "project":
            project = rest
        elif word in ("file", "crlf"):
            steps.append((word, rest))
        elif word == "arg":
            args.append(rest.replace("{COPY_FRESH_LCOV}", copy_fresh_lcov()))
        else:
            raise SystemExit(f"{path}: unknown line {line!r}")
    return project, steps, args


def prepare(project: str, steps: list[tuple[str, str]], target: Path) -> None:
    shutil.copytree(PROJECTS[project], target, ignore=shutil.ignore_patterns("target", "Cargo.lock"))
    for word, rest in steps:
        if word == "file":
            source, destination = rest.split(" ")
            (target / destination).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(CRAP / source, target / destination)
        else:
            path = target / rest
            path.write_bytes(path.read_bytes().replace(b"\n", b"\r\n"))


def normalize(stderr: str, root: Path) -> str:
    for form in (str(root), root.as_posix()):
        stderr = stderr.replace(form, "<ROOT>")
    lines = []
    for line in stderr.replace("\r\n", "\n").split("\n"):
        if line.startswith("+ ("):
            continue
        if line.startswith("Wrote ") or "<ROOT>" in line:
            line = line.replace(os.sep, "<SEP>")
        lines.append(line)
    return "\n".join(lines)


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    checkout = Path(sys.argv[1]).resolve()
    head = subprocess.run(["git", "-C", str(checkout), "rev-parse", "HEAD"],
                          capture_output=True, text=True, check=True).stdout.strip()
    if head != PINNED:
        print(f"crapper is at {head}, expected {PINNED}", file=sys.stderr)
        return 1
    runner = ("import sys; sys.path.insert(0, sys.argv[1]); from crapper.cli import main; "
              "main(sys.argv[2:])")
    for scenario in sorted(GOLDENS.glob("*/scenario")):
        project, steps, args = read_scenario(scenario)
        out = scenario.parent
        with tempfile.TemporaryDirectory() as scratch:
            root = (Path(scratch) / "project").resolve()
            prepare(project, steps, root)
            result = subprocess.run(
                [sys.executable, "-I", "-c", runner, str(checkout / "src"), "--root", str(root), *args],
                cwd=root, capture_output=True, check=False,
            )
            snapshot = root / ".metrics" / "crap.edn"
            (out / "crap.edn").unlink(missing_ok=True)
            if snapshot.is_file():
                shutil.copyfile(snapshot, out / "crap.edn")
            (out / "stdout").write_bytes(result.stdout)
            stderr = normalize(result.stderr.decode("utf-8"), root)
            (out / "stderr").write_bytes(stderr.encode("utf-8"))
            (out / "exit").write_text(f"{result.returncode}\n", encoding="utf-8", newline="\n")
        print(f"{out.name}: exit {result.returncode}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
