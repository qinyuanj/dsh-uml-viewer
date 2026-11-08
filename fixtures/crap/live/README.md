# Live coverage CLI fixtures

The tests in `crates/uml-viewer/tests/crap_languages.rs` copy only each
`<language>/project` directory into a temporary `Project`, run real coverage,
and compare `.metrics/crap.edn` byte for byte with an oracle-produced file.
They also run failing tests and require the resulting LCOV and oracle snapshot
to remain available. Missing expected bytes are always a test error.

An unavailable interpreter/tool or a failed local dependency install prints a
`SKIP` explanation and returns. Set `CRAPPER_REQUIRE_TOOLCHAINS=1` to make any
of these conditions fail instead. Never hand-author `expected.edn` or
`failing.edn`.

## Generate oracle snapshots

Requirements: the original crapper checkout at
`D:/dev/github/spec203-notes/ref/crapper`, pinned at
`4068857bc44e6383cf4a5e4d0b5c1111b49ce0b4`; Python 3.11+ with
`tree-sitter-language-pack` and `coverage>=7`; Node.js and npm. For Lua, run in an environment with Lua 5.4, Busted,
LuaCov, luacov-reporter-lcov and cluacov 1.0.0. `CRAPPER_ORACLE` overrides the
checkout path when running in WSL; the commit is always checked. From the
repository root:

```sh
python fixtures/crap/live/oracle.py
# WSL (Lua real cluacov toolchain):
# CRAPPER_ORACLE=/mnt/d/dev/github/spec203-notes/ref/crapper \
#   .scratch/oracle-venv/bin/python fixtures/crap/live/oracle.py lua
```

The optional positional arguments restrict generation to `python` and/or
`typescript`. This is a development-only oracle script, not a product runtime
dependency. It runs the original Python crapper with the temporary project as
`--root`; the Rust test runs `uml-viewer crap` independently.

The generated snapshots were produced using Python 3.12.10, coverage.py 7.16.2,
Node.js 24.19.0, npm 12.2.0, and c8 10.1.3. The fixture's `package.json` pins
c8. Each generator run installs it with `--ignore-scripts` inside a temporary
project, with a temporary `npm_config_cache`; all dependencies and reports are
removed with that directory. Nothing is installed globally or under the user
profile.

`src/calc.js` is the runtime counterpart of the `.ts` source. c8 instruments
JavaScript, not TypeScript, so both forms intentionally contain the same
statements and line locations. Test files are JavaScript for the same reason.
`src/demo/source.py` is intentionally nonempty: coverage.py otherwise emits an
empty `__init__.py` LCOV record, which crapper's required LF/LH validation
rejects.

The Lua fixture has two Busted specs, so `--lua-shards 2` exercises actual
sharded collection and stats merging. The TypeScript fixture intentionally
checks the original's `.ts` versus runtime `.js` distinction: c8 instruments
the JavaScript, while crapper names the TypeScript module; the pinned oracle
therefore records its TypeScript function as N/A. That is a parity result, not
a fabricated claim of measured TypeScript coverage.
