#!/usr/bin/env python3
"""Materialize CLI behavior inputs directly from the pinned Python tests AST.

Sources and scalar call arguments are read, never retyped. Case metadata adds
only file placement and selection advice. Expected JSON/EDN remains generated
by the parent from the original tools, not from this script.
"""

from __future__ import annotations

import argparse
import ast
import json
import subprocess
import sys
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
GOLDENS = ROOT / "fixtures" / "crap" / "goldens"
SOURCE = GOLDENS / "SOURCE"


def pinned_commit() -> str:
    """The crapper commit `goldens/SOURCE` pins for every CLI expectation."""
    return SOURCE.read_text(encoding="utf-8").split()[1]


def default_reference() -> Path:
    """The pinned crapper tests, wherever this worktree sits in the tree."""
    for base in (ROOT, *ROOT.parents):
        candidate = base / "spec203-notes" / "ref" / "crapper" / "tests"
        if candidate.is_dir():
            return candidate
    raise SystemExit(
        "--reference is required: no spec203-notes/ref/crapper/tests beside "
        f"{ROOT} or its parents"
    )


def verify_reference(reference: Path) -> None:
    """Refuse a checkout that is not the pinned crapper commit."""
    checkout = reference.parent
    if not (checkout / ".git").exists():
        print(
            f"note: {checkout} is not a git checkout; cannot verify crapper "
            f"{pinned_commit()}",
            file=sys.stderr,
        )
        return
    result = subprocess.run(
        ["git", "-C", str(checkout), "rev-parse", "HEAD"],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(f"cannot read the crapper commit at {checkout}: {result.stderr.strip()}")
    head = result.stdout.strip()
    if head != pinned_commit():
        raise SystemExit(f"crapper is at {head}, expected {pinned_commit()}")


class PinnedTests:
    def __init__(self, directory: Path):
        self.directory = directory
        self.trees = {
            name: ast.parse((directory / f"{name}.py").read_text(encoding="utf-8"))
            for name in ("test_go", "test_java", "test_clojure", "test_edges")
        }

    def value(self, node: ast.AST, constants: dict[str, str], written: dict[str, str] | None = None):
        if isinstance(node, ast.Constant):
            return node.value
        if isinstance(node, ast.Name) and node.id in constants:
            return constants[node.id]
        if isinstance(node, ast.JoinedStr):
            return "".join(
                self.value(item.value, constants)
                if isinstance(item, ast.FormattedValue)
                else item.value
                for item in node.values
            )
        if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
            return self.value(node.left, constants, written) + self.value(node.right, constants, written)
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr == "read_text" and isinstance(node.func.value, ast.Name) and written:
            return written[node.func.value.id]
        raise ValueError(f"expected a scalar literal: {ast.dump(node)}")

    def assignments(self, node: ast.AST, constants: dict[str, str]):
        found = dict(constants)
        for statement in ast.walk(node):
            if not isinstance(statement, ast.Assign):
                continue
            for target in statement.targets:
                if isinstance(target, ast.Name):
                    try:
                        found[target.id] = self.value(statement.value, found)
                    except ValueError:
                        pass
        return found

    def calls(self, module: str, function: str):
        tree = self.trees[module]
        constants = self.assignments(tree, {})
        for owner in tree.body:
            if not isinstance(owner, ast.FunctionDef):
                continue
            local = self.assignments(owner, constants)
            written = {}
            for statement in ast.walk(owner):
                if not isinstance(statement, ast.Call) or not isinstance(statement.func, ast.Attribute) or statement.func.attr != "write_text" or not statement.args:
                    continue
                target = statement.func.value
                if isinstance(target, ast.Name):
                    try:
                        written[target.id] = self.value(statement.args[0], local)
                    except ValueError:
                        pass
            for node in ast.walk(owner):
                if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Name) or node.func.id != function:
                    continue
                values = []
                for arg in node.args:
                    try:
                        values.append(self.value(arg, local, written))
                    except ValueError:
                        values.append(None)
                yield owner.name, node.lineno, values


CASES: list[dict] = []


def case(language: str, group: str, source: str, filename: str, test: str, source_root: str = ".", **metadata):
    CASES.append({
        "language": language,
        "case": f"{language}/{group}/{PurePosixPath(filename).stem}",
        "group": group,
        "path": filename,
        "source_root": source_root,
        "source_test": test,
        "source": source,
        **metadata,
    })


def go_cases(tests: PinnedTests):
    for _, _, values in tests.calls("test_go", "functions_in_source"):
        source, path = values[:2]
        path = path or "board.go"
        if "Branchy" in source:
            group = "branchy"
        elif "Widget" in source:
            group = "receivers"
        else:
            group = "import-path"
        case("go", group, source, PurePosixPath(path).name, f"test_go.py:{group}", module="example.com/behavior/go")
    for _, line, values in tests.calls("test_edges", "go_functions"):
        source, path = values[:2]
        if "Orphan" in source:
            group = "orphan"
        elif "Declared" in source:
            group = "declarations"
        else:
            group = "root-module"
        case("go", group, source, path or "board.go", f"test_edges.py:{line}", module="example.com/behavior/root" if group == "root-module" else None, original_path=path)
    # Extra original Go complexity calls (closure/type-switch/select) are added
    # by the parent fixture only when they have a source in the pinned tests.


def java_cases(tests: PinnedTests):
    for _, _, values in tests.calls("test_java", "functions_in_source"):
        source, path = values[:2]
        if "int score(boolean" in source:
            group = "score"
        elif "int nested(boolean" in source:
            group = "nested"
        elif "new Runnable" in source:
            group = "anonymous"
        elif "package demo.pkg" in source:
            group = "inner"
        elif "String text" in source:
            group = "comments"
        else:
            group = "loose"
        filename = PurePosixPath(path).name
        case("java", group, source, filename, f"test_java.py:{group}")
    for _, line, values in tests.calls("test_edges", "java_functions"):
        source, path = values[:2]
        case("java", "local-lambda", source, PurePosixPath(path).name, f"test_edges.py:{line}")


def clojure_cases(tests: PinnedTests):
    for _, line, values in tests.calls("test_clojure", "cyclomatic_complexity"):
        source = values[0]
        complexity = None  # The parent reads scalar assertions from the original pytest.
        if source == "(defn foo [x] (cond a b":
            case("clojure", "omitted", source, "unfinished.clj", f"test_clojure.py:{line}", omitted=True, reason="Private unfinished complexity test; extraction drops an unclosed defn.")
            continue
        case("clojure", f"complexity-{line}", source, "core.clj", f"test_clojure.py:{line}", expected_complexity=complexity)
    for _, line, values in tests.calls("test_clojure", "functions_in_source"):
        source, path, root = values
        if "core_extra" in path:
            group = "core-extra"
            filename = PurePosixPath(path).name
            source_root = "src"
        else:
            group = "demo-core"
            filename = PurePosixPath(path).name
            source_root = "src"
        case("clojure", group, source, filename, f"test_clojure.py:{line}", source_root=source_root, original_path=path, original_root=root)
    for _, line, values in tests.calls("test_clojure", "extract_functions"):
        source = values[0]
        case("clojure", f"extract-{line}", source, "edge.clj", f"test_clojure.py:{line}")
    for _, line, values in tests.calls("test_edges", "extract_functions"):
        case("clojure", f"edge-{line}", values[0], "edge.clj", f"test_edges.py:{line}")
    for _, line, values in tests.calls("test_edges", "_extract_top_level_defns"):
        case("clojure", f"private-{line}", values[0], "edge.clj", f"test_edges.py:{line}", note="Public extraction gives the same function; private raw text assertion is not CLI observable.")
    for _, line, values in tests.calls("test_clojure", "namespace_from_path"):
        path, root = values
        # Preserve a leading src/ for strip_src; strip an original project
        # prefix or source root because each case is independently selected.
        source_root = PurePosixPath(path).parent.as_posix()
        for prefix in ("/proj/", "/proj", "pkg/", "."):
            if source_root.startswith(prefix):
                source_root = source_root[len(prefix):]
                break
        if source_root.endswith("/src"):
            source_root = "src"
        elif "/src/" in source_root:
            source_root = source_root.split("/src/", 1)[1]
        case("clojure", f"namespace-{line}", "(defn foo [] 1)\n", PurePosixPath(path).name, f"test_clojure.py:{line}", source_root=source_root, original_path=path, original_root=root)
    for _, line, values in tests.calls("test_clojure", "without_strings_and_comments"):
        case("clojure", f"helper-{line}", values[0], "helper.clj", f"test_clojure.py:{line}", helper_only=True, note="String blanking has no direct CLI output; source preserved for manual inspection only.")


def group_directory(item: dict) -> Path:
    return ROOT / "fixtures/crap" / item["language"] / "testdata/behavior" / item["group"]


def placed_source(item: dict) -> Path:
    """Where the source must sit: under its declared `-s` root, not the group root."""
    root = item["source_root"]
    source = group_directory(item) / item["path"]
    if root in ("", "."):
        return source
    return group_directory(item) / root / item["path"]


def scenario_name(item: dict) -> str:
    return f"{item['language']}-behavior-{item['group']}"


def scenario_text(item: dict) -> str:
    root = "." if item["source_root"] in ("", ".") else item["source_root"]
    return (
        f"project {item['language']}\n"
        "arg -s\n"
        f"arg testdata/behavior/{item['group']}/{root}\n"
        "arg --no-coverage\n"
    )


def generate(tests: PinnedTests, check: bool):
    CASES.clear()
    go_cases(tests)
    java_cases(tests)
    clojure_cases(tests)
    manifest = {
        "reference": f"crapper {pinned_commit()} tests",
        "policy": "Generated source strings and scalar expectations originate in pinned ASTs. Each case directory is a distinct explicit -s root. Expected CLI artifacts are produced by the original tools.",
        "omitted": "Only test_an_unfinished_cond_counts_the_clauses_it_has: the private complexity assertion is not ported; its source still materializes so the CLI asserts the unclosed defn yields no function.",
        "cases": CASES,
    }
    rendered = json.dumps(manifest, ensure_ascii=False, indent=2) + "\n"
    manifest_path = ROOT / "fixtures/crap/behavior-cases.json"
    files = {manifest_path: rendered}
    for item in CASES:
        files[placed_source(item)] = item["source"]
        if item["language"] == "go" and item.get("module"):
            files[group_directory(item) / "go.mod"] = f"module {item['module']}\n\ngo 1.20\n"
        if item.get("helper_only"):
            # String blanking has no CLI output; the group has no scenario.
            continue
        name = scenario_name(item)
        if any(
            scenario_name(other) == name and other is not item and not other.get("helper_only")
            for other in CASES
        ):
            raise SystemExit(f"two behavior cases share the golden group {name}")
        files[GOLDENS / name / "scenario"] = scenario_text(item)
    for path, content in files.items():
        if check:
            if path.read_bytes() != content.encode("utf-8"):
                raise SystemExit(f"stale generated file: {path}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8", newline="\n")
    print(f"{'Verified' if check else 'Generated'} {len(CASES)} cases ({sum(item.get('omitted', False) for item in CASES)} omitted).")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    reference = args.reference or default_reference()
    verify_reference(reference)
    generate(PinnedTests(reference), args.check)


if __name__ == "__main__":
    main()
