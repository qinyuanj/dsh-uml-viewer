# Grammar parity and provenance

The original Python tools use `tree-sitter-language-pack 1.21.0`. Its tagged
`sources/language_definitions.json` is the authoritative pin list. It records
these upstream revisions:

| Grammar | Rust dependency | Original upstream revision |
| --- | --- | --- |
| Python | Git `tree-sitter/tree-sitter-python`, `26855eabccb19c6abf499fbc5b8dc7cc9ab8bc64` | Same; `generate: true` |
| TypeScript | `tree-sitter-typescript = 0.23.2`, `LANGUAGE_TYPESCRIPT` | `75b3874edb2dc714fb1fd77a32013d0f8699989f` |
| TSX | `tree-sitter-typescript = 0.23.2`, `LANGUAGE_TSX` | Same |
| Lua | `tree-sitter-lua = 0.2.0` | `4fbec840c34149b7d5fe10097c93a320ee4af053` |

Python is deliberately **not** the crates.io 0.25.0 package. That package has
274 node kinds and 2786 parse states; the oracle has 275 and 2788. The pinned
untagged revision is the upstream source at which language-pack generated the
parser it shipped. Its generated C has 275 node kinds and 2788 parse states;
its ABI is 15, whereas the oracle's regenerated parser is ABI 14. Tree-sitter
runtime 0.26.13 supports both. Matching counts alone are not considered parity
evidence.

## Reproduce

Use Python 3.11+ with `tree-sitter-language-pack==1.21.0` installed, and the
repository Rust toolchain:

```sh
python scripts/check-grammar-parity.py --ref-root <spec203-notes>/ref
```

The script builds a temporary Rust dumper using the exact dependency
specifications in `crates/quality-core/Cargo.toml`. It compares every node in
preorder, including kind, byte span and start/end row and column. It skips
repository metadata, virtual environments, dependencies and build outputs.
Temporary dumps and build artifacts are removed on exit. No scanner semantics,
function results, or hand-authored expected trees are tested here.

On 2026-11-09, the script passed with byte-identical output for this repository
and the complete pinned reference corpus: 197 Python files, 1,121 TypeScript
files, 66 TSX files and 31 Lua files. The smaller crapper/dryer/mutator-only
corpus also passed: 190 Python, 16 TypeScript, 2 TSX and 31 Lua files.
