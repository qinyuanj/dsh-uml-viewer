"""Compare complete Rust grammar node walks with language-pack 1.21.0.

Developer utility: `python scripts/check-grammar-parity.py --ref-root PATH`.
Requires the oracle Python package and a Rust toolchain. The temporary Rust
program uses the exact dependencies from quality-core's manifest and lockfile;
no outputs are retained in the repository or used as product test oracles.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

import tree_sitter_language_pack

VERSION = "1.21.0"
LANGUAGES = {
    "python": {".py"},
    "typescript": {".ts", ".mts", ".cts"},
    "tsx": {".tsx"},
    "lua": {".lua"},
}
SKIPPED = {".git", ".venv", "node_modules", "target"}


def sources(root: Path, suffixes: set[str]) -> list[str]:
    return [
        str(path)
        for path in sorted(root.rglob("*"))
        if path.suffix in suffixes and not SKIPPED.intersection(path.parts)
    ]


def oracle(language: str, paths: list[str], output: Path) -> None:
    parser = tree_sitter_language_pack.get_parser(language)
    grammar = tree_sitter_language_pack.get_language(language)
    print(
        f"{language}: ABI {grammar.abi_version}, {grammar.node_kind_count} kinds, "
        f"{grammar.parse_state_count} states"
    )
    with output.open("w", encoding="utf-8", newline="\n") as out:
        for path in paths:
            data = Path(path).read_bytes()
            out.write(path + "\n")
            stack = [parser.parse(data).root_node]
            while stack:
                node = stack.pop()
                start, end = node.start_point, node.end_point
                out.write(
                    f"{node.type} {node.start_byte} {node.end_byte} "
                    f"{start[0]} {start[1]} {end[0]} {end[1]}\n"
                )
                stack.extend(reversed(node.children))


def rust_program(manifest: Path) -> str:
    import tomllib

    dependencies = tomllib.loads(manifest.read_text(encoding="utf-8"))["dependencies"]
    selected = [
        name
        for name in dependencies
        if name in {"tree-sitter", "tree-sitter-python", "tree-sitter-typescript", "tree-sitter-lua"}
    ]
    entries = []
    for name in selected:
        spec = dependencies[name]
        if isinstance(spec, str):
            entries.append(f'{name} = "{spec}"')
        else:
            entries.append(
                f"{name} = {{ "
                + ", ".join(f'{key} = "{value}"' for key, value in spec.items())
                + " }"
            )
    return (
        '[package]\nname = "grammar-parity"\nversion = "0.0.0"\nedition = "2024"\n'
        "[dependencies]\n" + "\n".join(entries) + "\n[workspace]\n"
    )


RUST = r'''
use std::{io::Write, path::Path};
fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap();
    let output = args.next().unwrap();
    let language: tree_sitter::Language = match name.as_str() {
        "python" => tree_sitter_python::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "lua" => tree_sitter_lua::LANGUAGE.into(),
        _ => panic!("unknown grammar"),
    };
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).unwrap();
    let mut out = std::io::BufWriter::new(std::fs::File::create(output).unwrap());
    let paths = std::fs::read_to_string(args.next().unwrap()).unwrap();
    for path in paths.lines() {
        let data = std::fs::read(path).unwrap();
        let tree = parser.parse(&data, None).unwrap();
        writeln!(out, "{}", Path::new(&path).display()).unwrap();
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            let start = node.start_position();
            let end = node.end_position();
            writeln!(out, "{} {} {} {} {} {} {}", node.kind(), node.start_byte(),
                node.end_byte(), start.row, start.column, end.row, end.column).unwrap();
            let mut cursor = node.walk();
            let children: Vec<_> = node.children(&mut cursor).collect();
            stack.extend(children.into_iter().rev());
        }
    }
}
'''


def rust(manifest: Path, language: str, paths: list[str], output: Path) -> None:
    env = dict(os.environ, CARGO_TARGET_DIR=os.environ.get("CARGO_TARGET_DIR", str(output.parent / "target")))
    subprocess.run(
        ["cargo", "run", "--quiet", "--locked", "--manifest-path", str(manifest), "--", language, str(output), str(output.with_suffix(".files"))],
        check=True,
        env=env,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ref-root", type=Path, required=True, help="Pinned crapper/dryer/mutator reference root")
    args = parser.parse_args()
    if tree_sitter_language_pack.__version__ != VERSION:
        raise SystemExit(f"Install tree-sitter-language-pack=={VERSION} first")
    if shutil.which("cargo") is None:
        raise SystemExit("Rust toolchain is not on PATH")
    root = Path(__file__).resolve().parents[1]
    corpus = [root, args.ref_root]
    with tempfile.TemporaryDirectory(prefix="grammar-parity-") as temporary:
        temp = Path(temporary)
        (temp / "src").mkdir()
        (temp / "Cargo.toml").write_text(rust_program(root / "crates/quality-core/Cargo.toml"), encoding="utf-8")
        (temp / "src/main.rs").write_text(RUST, encoding="utf-8")
        subprocess.run(["cargo", "generate-lockfile", "--manifest-path", str(temp / "Cargo.toml")], check=True)
        for language, suffixes in LANGUAGES.items():
            paths = [path for source in corpus for path in sources(source, suffixes)]
            (temp / "rust.files").write_text("\n".join(paths), encoding="utf-8")
            oracle(language, paths, temp / "oracle.txt")
            rust(temp / "Cargo.toml", language, paths, temp / "rust.txt")
            if (temp / "oracle.txt").read_bytes() != (temp / "rust.txt").read_bytes():
                raise SystemExit(f"{language}: node walks differ")
            print(f"{language}: {len(paths)} files; complete node walks match")


if __name__ == "__main__":
    main()
