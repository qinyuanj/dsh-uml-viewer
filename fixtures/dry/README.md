# Python dryer oracle fixtures (ticket 6)

These expected outputs come from actual runs of the original Python CLI.
No expected snapshot, stdout, stderr, exit code or help text is hand-authored.
The source project is intentionally small but exercises Rust normalization,
extraction, naming, selection, ordering and report rendering together.

## Provenance and regeneration

- dryer: `b3f72ad9681a6574c5714d3c54d50e2ea3eff91a`
- crapper: `4068857bc44e6383cf4a5e4d0b5c1111b49ce0b4`
- Captured with CPython 3.12.10, win32, MSC v.1943 64 bit AMD64.
- Packages: `tree-sitter==0.26.0`, `tree-sitter-language-pack==1.21.0`.
- `goldens/provenance.json` records the full Python version and SHA-256 hashes
  of every Python source file in both pinned oracle checkouts.

Use a Python environment with the exact package versions above (Python 3.11+
can run the script; the capture interpreter is recorded explicitly). For example:

```sh
python -m pip install tree-sitter==0.26.0 tree-sitter-language-pack==1.21.0
python -I scripts/dry_goldens.py /path/to/dryer /path/to/crapper
python -I scripts/dry_goldens.py /path/to/dryer /path/to/crapper --check
```

The script verifies both full Git hashes, checks their tracked source is clean
and has no untracked Python source, and enforces the package versions. Each
scenario runs `dryer.cli.main` from those original sources in a separate
`python -I` subprocess, with a fresh copy of `rust/` in an automatically removed
temporary directory. No oracle checkout is modified. The two source directories
are passed explicitly so an installed or sibling crapper cannot replace the
pinned naming/discovery implementation. `PYTHONHASHSEED=0` and UTF-8 output are
set for the subprocesses. No Rust binary participates in generation.

Three more oracles cover the parts the CLI scenarios cannot enumerate:

- `python -I scripts/dry_count_oracle.py` (any CPython 3.11+; captured with
  3.12.10) writes `fixtures/dry/count/{cases,whitespace,limits}.tsv` and the
  generated digit table `crates/quality-core/src/digits.rs`. It starts from the real
  `int()` conversion, so accepted spellings (whitespace, signs, underscores,
  every Unicode Nd digit), the 4300-digit limit and the trimmed White_Space set
  are all the interpreter's own answers.
- `python -I scripts/dry_threshold_oracle.py <dryer-checkout> <crapper-checkout>`
  writes `fixtures/dry/threshold/cases.tsv` by calling pinned dryer `_number`
  (CPython `float()`) on 104 spellings, and refuses to write unless pinned
  crapper `_threshold` gives the same answer on every one. Both Rust tools
  parse `--threshold` with `quality_core::number::python_float`, which this
  table checks at both CLI seams.
- `python -I scripts/dry_platform_oracle.py <dryer-checkout>` writes
  `fixtures/dry/platform/cases.tsv` from importing the pinned
  `dryer.platform`. Run it where the warning path is POSIX, i.e. WSL2 or Linux
  (captured with CPython 3.12.3 under WSL2), because the original formats the
  host `Path` object. All three scripts verify the checkout hash and refuse a dirty
  source tree; the status check normalizes CRLF so a Windows checkout is still
  accepted under WSL git. All three support `--check`.

The exact `--help` stdout is saved as `help.txt`, including its original wording
and final newline. Every `goldens/<scenario>/` contains `scenario`, `stdout`,
`stderr` and decimal `exit` followed by LF. `dry.edn` exists only when the CLI
actually publishes `.metrics/dry.edn`. `stdout` and snapshots are raw bytes;
stderr alone replaces the absolute temporary project root with `<ROOT>`,
backslashes with slashes, and CRLF with LF. There are no timing or shell-echo
lines to discard. `--check` reruns every CLI invocation and compares these
files and their presence byte for byte, without modifying provenance or outputs.

Scenario syntax is `project rust` followed by zero or more `arg <literal argv
item>` lines. Empty/comment lines after the project line are ignored. The
runner prepends `--root <temporary project>`; all other argv items come directly
from the scenario file. Optional `git init`, `git add`, `commit <paths>`,
`modify <path>` and `rename <source> <target>` lines set up a real temporary git
project before the invocation. Paths in reports remain project-relative POSIX paths.

## Scenarios

| Directory under `goldens/` | Purpose |
| --- | --- |
| `changed` | Real git status: modified calls.rs and renamed extraction.rs selected; tracked unchanged lib.rs and tests not selected. |
| `changed-scoped` | Changed untracked integration test narrowed away by -s src; existing positional calls.rs remains a filter rather than overriding --changed. |
| `changed-empty` | Clean git tree yields original empty-selection output and no snapshot. |
| `default-text` | Defaults: threshold 0.82, min-lines 4, min-nodes 20; text uses two decimals while its snapshot uses twelve-place EDN rendering. |
| `all-edn` | Threshold 0.35, min-lines 1, min-nodes 0; broad cross-file pair and small-function coverage with `--format edn`. |
| `exact-only` | Threshold 1.0; exact normalized matches only, using `--edn`. |
| `precision-boundary` | Select lib/extraction with min-lines 9; threshold 0.690909090909 includes their sole qualifying pair, whose raw score is 38/55. |
| `precision-above` | Same selection, threshold 0.6909090909091 excludes that pair. Thresholding uses raw similarity before EDN rounding. |
| `min-lines` | Inclusive min-lines 9 with threshold 0.6; explicit `--text`. |
| `min-nodes` | Min-lines 1, min-nodes 90 and threshold 0.6 retain the 95-node operator duplicates. |
| `empty-candidates` | Source exists, min-nodes 100000 produces and publishes `{:candidates []}`. |
| `empty-selection` | Unmatched filter produces `No source files to analyze.`, exit 0, and no snapshot. |
| `explicit-tests` | Explicitly named integration-test file bypasses directory discovery skip; its inner cfg(test) functions have nil owners. |
| `scoped-excluded` | Source-root/exclusion diagnostics on named files; compares only lib.rs. |
| `unknown-format` | Invalid format exits 2 with no snapshot. |
| `usage-error` | Unknown option exits 1 and prints original help after its diagnostic, with no snapshot. |
| `count-underscore` | `--min-lines 1_0 --min-nodes 0_0`; Python `int()` accepts one underscore between digits, so both options parse as 10 and 0. |
| `count-whitespace-plus` | `--min-lines " 4" --min-nodes +20`; the value strips leading White_Space and accepts a leading plus sign. |
| `count-unicode` | `--min-lines ١٢ --min-nodes ٥`; Arabic-Indic digits are category Nd, so `int()` reads 12 and 5. |
| `count-large` | A 101-digit `--min-lines`; accepted arbitrary precision, compared against small counts, publishes `{:candidates []}`. |
| `count-negative-zero` | `--min-lines " -0"`; `int()` yields 0, which is not negative, so the default-sized report is produced. |
| `count-error-float` | `--min-lines 1.0` exits 1 with `requires an integer` and the original help. |
| `count-error-underscore` | `--min-nodes 1__0` exits 1 naming `--min-nodes`; a doubled underscore is not an integer. |
| `count-error-negative` | `--min-lines " -5"` exits 1 with `requires a non-negative integer`; a leading `-` directly after the option is still `requires a value`. |

## Observed behavior to preserve

`rust/src/lib.rs` contains renamed/literal-varied duplicates, a parenthesized
copy, an operator variant, duplicate impl methods, tiny one-line functions,
a direct nested function, and a recursively nested `mod tests`. Immediate
block-child functions and all functions under `mod tests` are excluded as
independent candidates; nested bodies still contribute to the containing
function's normalized structure.

`rust/src/extraction.rs` covers `#[test]`, `cfg(test)`, `cfg(all(test, ...))`,
`cfg(any(test, ...))`, a cfg(test) module whose name is not tests, and an inner
cfg(test) module. Dryer retains these forms, whereas crapper declines to name
the test-only ones, yielding `:namespace nil, :name nil`. The any(test, ...)
function has a normal owner. A module nested inside a function also contains an
independent candidate: the extraction guard checks the immediate parent only.

`rust/src/calls.rs` covers scoped callees, method names, generic calls, macros,
compound assignment, shifts, bitwise/boolean/unary/comparison operators.
Normal callee paths and called method names matter. Local/field/type argument
names, literal values and redundant parentheses normalize away. Macro names
are ordinary non-call-position identifiers: `debug_assert!` versus `assert!`
therefore normalize identically here. Generic calls differing only in primitive
type arguments i32/i64 also normalize identically. The default snapshot contains
score `0.847826086957` while its text output displays `0.85`.

The fixture is input for structural scanning; regeneration does not compile it,
run its tests, fetch coverage, or write metrics into the repository project.
