# 0001 Project quality via configured standard tools

**Status**: accepted; clarified for #24/#26

Standard crapper/mutator configuration is the primary integration. Consumers use the public `uml crap` and `uml mutate` entries with their standard tool configuration; they do not need permanent project adapter scripts. Existing Clojure aliases retain their contracts.

A versioned repository-root `uml-quality.json` persists standard scope in an optional `standardTools` object (`sourceRoots`, `exclude`, `crapperOptions`, `mutatorOptions`, each an array of strings). This uses the installed standard executables and adapts Mutator's missing exclusion flag by filtering selected business files. See the README for glob, target and reserved-option rules. A standard-only manifest is valid; it explicitly selects the standard tools even in a project with `deps.edn`.
An optional `operations` object extends this with a different command contract. Its schema is [uml-quality.schema.json](../uml-quality.schema.json). When the file is absent, default dispatch remains active. When `operations` is present, it takes precedence over standard options, aliases and external tools, and must declare both operations. Invalid JSON, invalid standard settings, missing override operations, invalid declarations and command failures fail explicitly without choosing another tool.

```json
{
  "version": 1,
  "operations": {
    "crap": {
      "argv": ["crapper", "--full-tree"],
      "forwardArgs": "none",
      "scope": "whole-tree",
      "validResultExitCodes": [0]
    },
    "mutate": {
      "argv": ["mutator"],
      "forwardArgs": "append",
      "validResultExitCodes": [0, 3]
    }
  }
}
```

This example illustrates the extension, not a requirement to replace standard configuration. Each operation declares a nonempty string argv, `forwardArgs` (`append` or `none`), and nonempty integer `validResultExitCodes` (0–255). CRAP also declares `scope`: `targets` measures requested targets; `whole-tree` measures the business tree selected by its fixed command/tool configuration and requires `forwardArgs: none`, so right-click targets cannot narrow the snapshot. All configured operations are validated before any command runs. Additional fields are ignored for forward compatibility.

Commands execute at the project root. Fixed argv precedes appended caller arguments, including empty strings, spaces, quotes, newlines and shell metacharacters. No shell expression or environment interpolation occurs. Only native executables are accepted; script and shim entries (`.ps1`, `.cmd`, `.bat`) are rejected because a shell boundary cannot preserve arbitrary argv. *Amended 2026-10-21 (spec #106): `.ps1` entries were dispatchable while Windows native was a delivery platform; ADR 0007 retires that platform, so they are now rejected like the other shims.* stdout/stderr remain the command's streams, and the wrapper returns the real exit code even for declared findings. Exit 3 is a valid result only under the selected tool's declared contract or an explicit project alias/operation declaration; it is never universally treated as success. A launch or configuration error exits 1.

Generic Clojure aliases accept only exit 0. Version 1 also permits an optional
`aliasResultExitCodes` object with nonempty integer arrays for `crap` and/or
`mutate` (0–255); a manifest containing only this declaration is valid. It changes
result classification only on the alias route, retaining portable Clojure command
resolution and argv. Operations overrides and standard-tool selection retain
precedence. The repository declares mutation 0/3 for its pinned clj-mutate
c8c0eedf34dafec270e13e74a106ff1703578d28; the README links that tool's
`exit-status` contract. No executable paths or universal findings codes are added.

The wrapper removes `.uml-viewer/quality-result.json` before dispatch. On command completion it atomically publishes a version-1 companion result with `operation` (`crap`/`mutate`), absolute `projectRoot`, actual `exitCode`, boolean `validResult`, `crapScope` (`targets`/`whole-tree`, null for mutation), boolean `configured`, and `tool` (`configured` for this extension; defaults identify their tool). The record never replaces stdout or the process status. If publication fails, the wrapper reports the failure on stderr while retaining the command's exit; no valid record is available.

Companions serialize quality requests, read only the record produced by the invocation they just awaited, and verify version, root, operation and exit code. Only a matching `validResult: true` permits IR/display. A missing/mismatched record or `validResult: false` stops the sequence. Valid survivor/coverage-gap results keep snapshots and permit IR; they do not justify rerunning a file or forcing full mutation. Direct `clj -M:crap` commands retain their own contracts and do not produce this wrapper record.

Rejected alternatives: configuration inside policy (couples diagram policy to command execution); mandatory project script location (forces consumer adapter scripts).

Automated public-entry acceptance runs controlled commands from a different directory with a project path containing spaces and `&`, checks argv/output/status, scope, findings and fail-closed behavior. Native Windows automated acceptance is distinct from real companion mailbox acceptance. macOS uses the shared PS7 runtime but native macOS acceptance remains unexecuted; wrapper tests do not claim it passed.

Addendum (2026-10-15, spec #32): the final sentence above is historical. Serialized quality dispatch gating (crap/mutate record gate, FIFO mail, IR/display gating) was exercised and passed on native macOS under spec #32 with Herdr 0.9.4/protocol 22; evidence is recorded in [acceptance-evidence/acceptance-log.md](../../acceptance-evidence/acceptance-log.md). Platform terminal attach remains NOT VERIFIED under [#49](https://github.com/qinyuanj/uml-viewer/issues/49).

Addendum (2026-10-18, #72 cutover under [ADR 0005](0005-love2d-rewrite-drop-jvm.md)): the Clojure alias route is removed with the JVM implementation. `deps.edn` no longer selects aliases, and `aliasResultExitCodes` is rejected as a removed declaration instead of being ignored. Undeclared projects use the standard tools. Standard tools, `operations` overrides, the result record and the IR/display gate above are unchanged. This repository now declares `standardTools` with `sourceRoots: ["love/uml"]`.

Addendum (2026-10-20, #89 dryer integration): the third quality tool, dryer, joins the same dispatch as `uml dry`. It adds `dryerOptions` to `standardTools` and an optional `dry` entry to `operations`; `crap` and `mutate` stay mandatory there, and a manifest that omits `dry` still runs the standard dryer. Dryer's only valid result is exit 0 — exit 1 is a usage error and exit 2 an unknown format — so it publishes `validResult: true` for 0 alone. Because dryer has no `--exclude`, declared exclusions are applied by enumerating the matching business files and passing them explicitly; with no exclusions the source roots are forwarded with `-s` and dryer's own walker skips test/spec/vendor. The wrapper record gains `operation: "dry"`. Nothing overlays `.metrics/dry.edn` in the viewer yet.

Addendum (2026-10-22, #144 cutover under [ADR 0008](0008-rust-egui-viewer-drop-lua.md)): the Lua implementation, its Busted/LuaCov analysis path and the LOVE/cimgui provisioning are removed. This repository now declares `standardTools` with `sourceRoots: ["crates"]`; crapper and mutator analyze Rust, with coverage from `cargo llvm-cov`. The dispatch contract, the version-1 result record and the IR/display gate are unchanged. `love` is refused as a runtime the same way `jvm` was.

Addendum (2026-11-01, scope passed to the tools): mutator and dryer now take `-s`/`--source-root` and `--exclude` with crapper's selection and glob rules, so the standard route passes declared scope to them as it does to crapper, and uml-viewer no longer selects files with its own skip and extension lists. Those lists kept `build/`, `dist/`, `testdata/`, `coverage/`, `lua_modules/` and test-named files that crapper skips, so `dry.edn` and mutation snapshots gained rows that joined to nothing in `crap.edn`. This supersedes the filtering adapter described in the second paragraph and the enumeration in the #89 addendum. `uml dry` runs `dryer -s <root>… --exclude <glob>… <dryerOptions> <args>` and never lists files. `uml mutate` runs `mutator -s <root>… --exclude <glob>… <mutatorOptions> <options> <targets>`: `--changed` is forwarded as the flag, so uml-viewer makes no git calls. Because the tools take only the changed set when given both, `--changed` together with a named target is refused without launching. Named files and directories are forwarded unexpanded, as project-relative paths, in the order named. A named file outside the declared scope, or a named directory that is excluded or neither inside nor containing a root, is not forwarded and is named on stderr. Missing paths, symbolic links and paths outside the project still fail. When nothing is left to forward the call is still refused without launching or a record. With `-s`, the tools narrow named paths and `--changed` to the source roots: a named file outside them gets `Skipping <path>; it is outside the source roots`. They report an empty selection themselves (exit 0). Mutator and dryer before this change have no `--exclude` and ignore named paths under `-s`, and the installer clones each tool's `main`. So the tool changes must be released before this one. As a guard, whenever `uml mutate` or `uml dry` passes `-s` or `--exclude`, dispatch first runs `<tool> --help` and refuses to run a tool whose help does not list `--exclude`, pointing to `get-uml-viewer --install-only`. The `operations` route, the result record and the IR/display gate are unchanged.
