# Original mutation oracle

This fixture is the real Cargo workspace from pinned mutator
`tests/test_rust_slots_cargo.py:22–62`. Copy the entire `rust/` directory to a
native Linux temporary directory, preserving sibling `project/` and `extdep/`.
Use `project/` as the CLI root. Do not build or mutate the checked-in fixture.
The app manifest uses `extdep = { path = "../../extdep" }`; the original helper
substitutes an absolute path to the same sibling. This is the only fixture-byte
adaptation. Every Rust source/test and other manifest byte matches the helper.
There are no crates.io dependencies, generated lockfiles or build artifacts.

Targets, in invocation order:

```text
util/src/lib.rs
app/src/lib.rs
app/src/more.rs
```

Run the CLI with `--root <copy>/project --max-workers 1 --timeout-factor 20` and
all three targets. The first run also supplies `--mutate-all`; the second leaves
that flag out and changes no files. Default coverage and real `cargo test` are
required. The first run exits 0 with three killed sites, zero survivors and zero
uncovered sites. The unchanged differential run exits 0 with `executed=0 reused=3`
and preserves the three snapshot files byte for byte. Original manifests,
source and tests remain unchanged after both runs. The fresh original coverage
run warns that reports are not stamped as fresh because Cargo created files;
mutation still succeeds. The unchanged rerun returns before coverage.

The EDN under `goldens/` is the original Python implementation's raw output,
including the final LF. Each has `:isolation :namespace-view` and
`:tested-at "2026-11-07T12:00:00+00:00"`. The clock comes from the original
`test_snapshot_bytes.FixedTime`, assigned to `mutator.metrics.datetime` before
calling `mutator.cli.run`. An integration test comparing bytes must inject the
same timestamp in Rust. Setting `TZ=UTC` alone does not freeze the clock.
On Windows the original isolation field is `:copy`; these Linux goldens retain
the observed `:namespace-view` bytes.

| Golden path (relative to `goldens/`) | Bytes | SHA-256 |
| --- | ---: | --- |
| `app.edn` | 406 | `919be95f1a4856e91619a060d4bfc062554cc5aee45447444248ea13d47cc095` |
| `app/more.edn` | 419 | `713568c32383e59f2f0b7569903cd570aa353984562b1cd4f4403f0fe5e7575e` |
| `util.edn` | 412 | `31190035242fa75d52f3ee27f0c6f431189198236bc4a5174f2bcd6bd66fe8c8` |
| `app-no-sites.edn` | 601 | `49a18a35b755671f972712523d78187064c2924b3121273f920c0df458ac5433` |

`app-no-sites.edn` is the differential guard oracle. After the covered run and
the unchanged rerun, the regeneration script copies the oracle tree, appends
`\npub fn no_sites() {}\n` to `app/src/lib.rs` and reruns the CLI on that file
without `--mutate-all`, using `--no-coverage --test-command
"grep -q 'x + x' app/src/lib.rs"`. The added function owns no mutation site, but
the function digests differ, so the original does not reuse the snapshot: it
exits 0, runs no test, republishes `app.edn` with both forms and the carried
`:killed` outcome, and leaves the other two snapshots byte-identical. The Rust
integration test performs the same edit and compares the republished bytes.

`help.txt` is **this port's** help text, as UTF-8 bytes starting with `Usage:`
and retaining its final LF; the binary reads it through `include_str!`. It is
the original CLI `HELP` string plus the options this port added, so it is not
the pinned original's bytes and no longer claims that contract:

- `--copy <path>` is this port's option (ADR 0016). The pinned original has no
  such flag, so a text published straight from `mutator.cli.HELP` loses it from
  `uml-viewer mutate --help` and from every usage error.
- `--copy`'s entry also states the bare `..` refusal: `--copy ..` names the
  project's parent directory itself, which is no path in it, so it is a usage
  error like the other `--copy` errors.

Regeneration must therefore not copy the pinned module's string over this file.
`scripts/mutate_goldens.py` calls `publish_help`, which refuses to write a text
that lacks `--copy <path>` or the bare `..` rule, and prints what is missing
instead of overwriting. The refusal is pinned by
`scripts/test_mutate_goldens_help.py`, and the published bytes by
`mutate_help_documents_the_copy_option_and_the_bare_parent_refusal` in
`crates/uml-viewer/tests/mutate.rs`: comparing the binary with this fixture
cannot catch a regression, because both read the same file.

The portable sibling layout was additionally copied to a fresh native `/tmp`
directory and passed `cargo test --offline --workspace`.

## Lua project

`lua/project` is a two-module Lua project: a root module (`.busted`, `lpath`
`src/?.lua;src/?/init.lua`) and a nested one (`nested/.busted`, `lib/?.lua`).
Two root spec files cover it, one of them with a space in its name
(`spec/double unit_spec.lua`), and `src/odd name.lua` is an untested source with
a space in its own name. `src/calc.lua` also holds a private `local function`.

The pinned original selects 16 sites over `src/calc.lua`, `src/odd name.lua`
and `nested/lib/mathx.lua`: 3 killed, one survivor (`//` to `/` is the same
function on even input) and 12 uncovered sites, so both the covered run and the
differential rerun exit 3; the rerun reuses the killed sites and republishes
identical bytes. `--select-specs` publishes the same bytes again.

`lua/goldens/` holds the three snapshots byte for byte, `lua/sites.json` the
original's `sites_in_file` output (offsets, originals, mutants, categories,
owners, mutation identities) and form digests, and `lua/evidence.json` the
recorded console run. Regenerate inside WSL with Lua 5.4, Busted, LuaCov,
cluacov 1.0.0 and dkjson installed:

```bash
python3 scripts/mutate_goldens.py --references /mnt/d/dev/github/spec203-notes/ref --lua
```

The fixture is copied to native `/tmp` first and its bytes are checked
afterwards; the Rust tests run the same arguments with the same fixed clock and
compare the published bytes. The Lua case keeps the caller's HOME, because
LuaRocks resolves Busted and cluacov from `~/.luarocks`.

## Provenance and regeneration

Original references (read only):

- mutator `f772a8329fba3f2f81bfca0a81abfa3210af9f4a`
- crapper `4068857bc44e6383cf4a5e4d0b5c1111b49ce0b4`

The development-only script `scripts/mutate_goldens.py` follows the complete
`spec203-notes/EXPLORE-linux-mutation.md` recipe. It verifies reference pins and
cleanliness, preflights installed Cargo coverage tools, installs only pinned
Python test dependencies into disposable native `/tmp`, and runs both original
test files (four tests) before generating the real covered Cargo oracle. It
asserts the unchanged differential behavior, all original fixture bytes, and
the exact sizes/hashes above before publishing anything. `goldens/evidence.json`
records stable assertions from that run. No Python is used by the Rust CLI.

Example from WSL (adjust the home/tool/reference paths for another host):

```bash
export PATH=/home/lzx/.cargo/bin:/home/lzx/.local/share/kami/venv/bin:/usr/local/bin:/usr/bin:/bin
export CARGO_HOME=/home/lzx/.cargo RUSTUP_HOME=/home/lzx/.rustup
export PYTHONDONTWRITEBYTECODE=1
python3 /mnt/d/dev/github/dsh-uml-viewer-wt/t8/scripts/mutate_goldens.py \
  --references /mnt/d/dev/github/spec203-notes/ref
```

Requires native Linux user namespaces plus overlayfs, installed Cargo,
`cargo-llvm-cov`, the Rust `llvm-tools` component and Python with pip. There is no
global installation or tool-install fallback. The temporary Python pins are
`tree-sitter==0.26.0`, `tree-sitter-language-pack==1.21.0`, `pytest==9.2.0`.
Grammar first use may download into the disposable cache. Reference cleanliness
checks use invocation-only Git settings for Windows/DrvFS line endings and modes;
reference files/configuration are never rewritten. HOME/cache and bytecode/test
cache settings prevent run artifacts in the reference clones. Scratch is removed
only after checking its exact resolved native `/tmp` path.

## Python and TypeScript oracles (#13)

`python/project` and `typescript/project` are real, dependency-free projects.
Copy the whole `project/` directory to a native Linux temporary directory; never
mutate the checked-in fixture. Python needs a project virtualenv: symlink an
isolated venv that has coverage.py and pytest to `.venv` in both the root and
`legacy/` (the second package deliberately uses unittest discovery). The tests
read that venv from `MUTATOR_TEST_PYTHON_VENV`. TypeScript needs Node with type
stripping and npm on PATH; its `coverage` script is Node's own LCOV reporter, so
no npm install or network is required. A missing toolchain is a loud SKIP, or a
failure with `MUTATOR_REQUIRE_TOOLCHAINS=1`.

Targets, in invocation order:

```text
python:     src/shapes/geometry.py legacy/tally/core.py
typescript: src/calc.ts src/view.tsx
```

Run with `--root <copy> --max-workers 1 --timeout-factor 20` and those targets,
first with `--mutate-all`, then unchanged without it. Both runs exit 3 (the
goldens record surviving mutants); the rerun re-executes the survivors and keeps
every snapshot's bytes. The untested functions stay uncovered, and the untested
TypeScript `.tsx` file publishes only uncovered sites. No `target/`, `coverage/`,
`node_modules/`, `__pycache__` or `.coverage` appears in the project.

The EDN under `python/goldens/` and `typescript/goldens/` is the pinned original
Python implementation's raw output with the same fixed clock as above
(`2026-11-07T12:00:00+00:00`, `:isolation :namespace-view`):

| Golden | Bytes | SHA-256 |
| --- | ---: | --- |
| `python/goldens/legacy.tally.core.edn` | 744 | `5ae9926f33c8eda604407f1a5f3588bcc6e48eb1566b49ad61ac641dc2a3072d` |
| `python/goldens/shapes.geometry.Board.edn` | 1811 | `6cf928366db3f1d4414fb53caf6d14c2ba08a5de14b4ffa808c8f02451d41648` |
| `python/goldens/shapes.geometry.edn` | 2346 | `22795c5177539cec8fc23b921fbfe14ccb383a0c8753cbbc6f2fec5eacf22b26` |
| `typescript/goldens/calc.Counter.edn` | 1519 | `4a9d177c24518f45ca4f5a8f2decd740ce10d029b3858503b2297fb9a1ae648c` |
| `typescript/goldens/calc.edn` | 1975 | `e43e794bb7520ea04cf8de460e1ffe6c5d1bd2a48e5e00e47ba6c53490d18bf0` |
| `typescript/goldens/view.edn` | 332 | `c3506374dc9589e061442b5c0341a3f7ee3de7c0a64d18849ba5cb8bb1cd6c10` |

`sites/sites.json` is the original's `sites_in_file`, `form_spans` and
`form_digests` output for `sites/src/**` (a corpus built from its
`test_sites.py` cases plus edge cases) and for both fixture projects: 30 files,
176 sites, every offset, category, owner, form id, digest and mutation id.

Regenerate with `scripts/mutate_goldens_python_ts.py --references
<spec203-notes>/ref --venv <isolated venv>` on native Linux/WSL with Node/npm.
It verifies both pins and reference cleanliness, runs the original's
`test_snapshot_bytes.py`, `test_sites.py` and `test_runner.py` (57 passed, 1
skipped on the recorded run), publishes the goldens and the site inventory, and
records provenance and tool versions in `python-typescript-evidence.json`.
Grammar and test dependencies install only into a disposable `/tmp` scratch;
nothing is installed globally and the reference clones are never written.

## Go, Java and Clojure oracles (#14)

The new projects exercise real test runners: Go 1.26.3 on WSL, JDK
17.0.21+9 with Maven 3.9.11 and JUnit 5.11.4, and Clojure CLI 1.12.6.1673
with Clojure 1.12.0/Cloverage 1.2.4. CPython 3.12.3 with tree-sitter 0.26.0
and language-pack 1.21.0 runs the two unmodified pins listed above. A scratch
`clj` shim invokes the installed `clojure` because this host lacks rlwrap;
nothing is installed globally. Cached Maven dependencies are prerequisites.

Regenerate on Linux/WSL with those tools on PATH:

```bash
python3 scripts/mutate_goldens_go_java_clojure.py --references <spec203-notes>/ref
```

The generator verifies reference pins and cleanliness, installs pinned Python
oracle dependencies into disposable `/tmp`, runs the original
`test_sites.py`, `test_runner.py`, `test_coverage.py` and
`test_snapshot_bytes.py` (60 passed, 1 skipped in the captured run), and uses
`FixedTime` to generate seven raw snapshots. The projects run once with
`--mutate-all --max-workers 1 --timeout-factor 20`, then unchanged without
`--mutate-all`. All three first runs and differential reruns exit 3; all
seven rerun snapshots keep identical bytes. The current generator additionally
checks the run copy's source bytes, as well as the fixture originals. The
captured evidence predates that extra assertion.

| Language | Targets | Oracles |
| --- | --- | --- |
| Go | `demo/calc.go`, `widgets/widget.go` | `demo.edn`, `demo.Counter.edn`, `example.com/widgets.edn`, `example.com/widgets.Box.edn` |
| Java | `src/main/java/demo/Board.java` | `demo.Board.edn`, `demo.Board.Cell.edn` |
| Clojure | `src/demo/core.clj` | `demo.core.edn` |

The Go `demo` module's profile names its source `demo/calc.go`, which joins
by suffix. The second module's import path `example.com/widgets/widget.go`
does not join the project path `widgets/widget.go` under the pinned original:
its three sites stay uncovered despite the real tests running. This is an
observed original behavior, retained in the goldens. The first module has
six killed sites, three survivors and two uncovered sites; Java has twelve
killed, one survivor and four uncovered; Clojure has five killed, three
survivors and one uncovered. All goldens use the fixed clock above and
`:namespace-view`. The Windows test adapts only the expected isolation field
to `:copy`, as #10's tests do; Linux goldens themselves stay raw.

`go-java-clojure-evidence.json` records exact toolchain versions, pins,
arguments, statuses, sizes and SHA-256s. `sites/go-java-clojure.json` records
21 source files, 288 sites and their forms/digests. The site corpus includes
the six original language-specific `test_sites.py` cases, the three
non-bitwise parameter cases, all Clojure head swaps, reader macros, metadata,
quotes, errors, Unicode/CRLF, Go receivers and Java nested classes. The CRLF
fixture is `-text` in `.gitattributes` so a fresh checkout retains its bytes.

The custom Clojure reader was also compared with the original's grammar on
132 library sources extracted from locally cached jars: 7,033 sites matched
exactly. This optional differential runs via the ignored external-corpus test
with `MUTATOR_CLOJURE_CORPUS` and `MUTATOR_CLOJURE_RAW`; the generator's
`--clojure-corpus DIR --raw-output FILE` produces its independent oracle.
The external corpus is scratch and is not committed.

Rust tests cover native report selection from the original
`test_coverage.py` cases, Cloverage precedence/fallback, missing native
reports, malformed reports, fixed-clock real runner snapshots, reruns and
worker-count equality. The original `test_runner.py` Go package argv,
Java/Maven fallback and all Clojure task/alias cases are ported too. Toolchain
skips write directly to real stderr, so libtest capture cannot hide them;
`MUTATOR_REQUIRE_TOOLCHAINS=1` makes them fail.

Final Rust runtime acceptance remains pending under the user's #14 resource
restriction (scoped `cargo check` only). The committed Python oracles and the
passing site-reader differential are evidence for those parts; they do not
substitute for execution of the final engine tests.
