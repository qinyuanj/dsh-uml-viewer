# #21 viewer panel skeleton — progress log

Branch `t21-panel-skeleton`, worktree `D:/dev/github/dsh-uml-viewer-wt/t21`, base `spec-203@2bd0320`.

## Plan

1. contracts: scene-frame + resident validators (goldens are the source of truth), panel events/descriptors, `uml_display` record.
2. host: per-session resident owner (`uml-viewer resident` through the session sandbox policy),
   per-session IR→display gate, `uml_display` tool, typed Remote (`ctx.typert`), companion prompt
   section tied to the panel stream, Sarasa font route.
3. client package: `dsh.client` facet, right-sidebar tab + main-area page, Canvas 2D painter over the
   Rust frame, host theme tokens with light/dark, bundled Sarasa font for CJK width.
4. build/tests: `lib/client.js` in ModuleLoader form with no React/primitives payload, policy + manifest
   in place, docs.

## Status

- [x] worktree, baseline green (`pnpm build`, 26 host + 2 contracts tests)
- [ ] contracts
- [ ] host
- [ ] client
- [ ] build/manifest/policy/docs
- [ ] merge spec-203 and re-run checks

## Facts verified before coding

- Real Host contracts (`cordis_inspect_query`): `subprocess.spawn` → `{ stdin: Writable, stdout: Readable,
  done, terminate(), waitForExit() }`; `typert.register(contribution)` descriptors carry
  `result/create(): { parse }`; `systemPrompt.section(section)`.
- `dsh-client-modules` `/plugins/<pkg>/assets/*` serves only `.svg .png .webp .jpg .jpeg`, so the Sarasa
  TTF is served by our own `ctx.webServer` route, not the asset route.
- Goldens: 5 frames, world kinds {package, class, quality, level, band, rule, text, marker, no-data,
  port, port-link}, edge kinds {stroke, head, indicator}, heads {open, triangle, diamond, diamond-fill},
  chrome {title, proposal-banner, back, status, hint, popup}.
- Client Slot inspect times out on this machine (no inspecting page), so client-side Slot facts come from
  the shipped package source (`dsh-client-ui-sidebar-right` README + `lib/client.js` registrations).
- Session log `tool/result` carries rendered content only (no structured tool value), so the display gate
  is a per-session host record, not a log fold. Recorded as a residual difference in the report.
