# Resident overlay producer samples

These JSON files are exact stdout lines captured from the real existing
`uml-viewer resident` binary on native Windows for ticket #27. The project was
the real IR generated from `packages/e2e/inspector-project.mjs` in the isolated
browser lane. Source line 82 and all summary/card/source formatting come from
Rust; the samples are not hand-written or transformed. The source event keeps
the capture project's absolute filename as emitted.

Capture command (explicit maintenance action, never part of `pnpm test`):

```text
node packages/contracts/tests/capture-overlays.mjs <existing-absolute-binary> <absolute-project>
```

The capture script does not build Rust. The contracts suite reads the checked-in
lines and validates both events (the inspector sample was deleted with the inspector, #47). Current samples predate the subsequent
project-root containment guard; that guard changes refusal behavior only, with
no successful wire-field changes. Its new CLI tests were written but not run
because the user restricted further Rust validation to scoped cargo check.
