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
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REFERENCE = ROOT.parent / "spec203-notes" / "ref" / "crapper" / "tests"


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
        case("go", group, source, path or "board.go", f"test_edges.py:{line}", source_root="$CASE" if group == "root-module" else ".", module="example.com/behavior/root" if group == "root-module" else None, original_path=path)
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


def write_module(directory: Path, module: str):
    (directory / "go.mod").write_text(f"module {module}\n\ngo 1.20\n", encoding="utf-8")


def generate(tests: PinnedTests, check: bool):
    CASES.clear()
    go_cases(tests)
    java_cases(tests)
    clojure_cases(tests)
    manifest = {
        "reference": str(tests.directory),
        "policy": "Generated source strings and scalar expectations originate in pinned ASTs. Each case directory is a distinct explicit -s root. Expected CLI artifacts are produced by the original tools.",
        "omitted": "Only test_an_unfinished_cond_counts_the_clauses_it_has: private unfinished complexity cannot be observed through CLI extraction.",
        "cases": CASES,
    }
    rendered = json.dumps(manifest, ensure_ascii=False, indent=2) + "\n"
    manifest_path = ROOT / "fixtures/crap/behavior-cases.json"
    files = {manifest_path: rendered}
    for item in CASES:
        if item.get("omitted"):
            continue
        directory = ROOT / "fixtures/crap" / item["language"] / "testdata/behavior" / item["group"]
        files[directory / item["path"]] = item["source"]
        if item["language"] == "go" and item.get("module"):
            files[directory / "go.mod"] = f"module {item['module']}\n\ngo 1.20\n"
    for path, content in files.items():
        if check:
            if path.read_text(encoding="utf-8") != content:
                raise SystemExit(f"stale generated file: {path}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
    print(f"{'Verified' if check else 'Generated'} {len(CASES)} cases ({sum(item.get('omitted', False) for item in CASES)} omitted).")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, default=DEFAULT_REFERENCE)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    generate(PinnedTests(args.reference), args.check)


if __name__ == "__main__":
    main()
