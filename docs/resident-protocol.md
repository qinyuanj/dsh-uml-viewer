# Resident protocol, version 1

The **resident mode** runs the viewer session (`crates/viewer-session`) as a
long-running child of the plugin host (dsh-uml-viewer#21). The host writes
one JSON **action** per line to the child's stdin; the child writes one JSON
**event** per line to stdout. Frames are the [scene frame](scene-frame.md)
unchanged. The interaction boundary this protocol implements (which work
stays on the client, and which goes through the host) is the appendix of
[ADR 0013](adr/0013-repository-migration.md) (附录：交互边界).

```text
uml-viewer resident [--project <dir>]
```

The project is the working directory (the session's working directory)
unless `--project` names one. Closing stdin ends the session with exit 0;
diagnostics go to stderr, never to stdout. Usage errors exit 2.

The integration tests in `crates/uml-viewer/tests/resident.rs` drive the
real binary through this protocol and are its executable contract.

## Stability

- `schema` is `"dsh-uml-viewer/resident"`; `version` is `1`. Every action
  carries `"version": 1`; every event carries `"version": 1`.
- Adding an action, an event, or a field is compatible and keeps the
  version. The host ignores events and fields it does not know; the child
  answers an action it does not know with an `error` event.
- Removing or renaming an action, event or field, or changing its meaning,
  bumps `version`.
- Each line is one compact JSON object, UTF-8, ending in `\n`. Object keys
  are sorted.
- Ids and hits are EDN text (`":core.order"`, `"{:kind :class :id :x}"`),
  exactly as in the scene frame, so they round-trip into panel requests.
- World points are `[x, y]` in the frame's world units.

## Lines

An action:

```json
{"version": 1, "seq": 7, "action": "drill-in", "id": ":core"}
```

`seq` is the host's correlation value (any JSON value; an integer is
usual). Every event that answers an action echoes its `seq`; an event no
action asked for (a hot reload) has `"seq": null`. Actions are applied in
order, one at a time, and each action's events are written before the next
action is read, so the host can match answers by order as well.

An event:

```json
{"version": 1, "seq": 7, "event": "frame", "frame": {"schema": "dsh-uml-viewer/scene-frame", ...}}
```

## Events

| event | when | fields |
|---|---|---|
| `ready` | first line, before any action | `schema`, `frameSchema`, `frameVersion`, `stateSchema`, `stateVersion`, `actions` (the action names this child understands) |
| `frame` | after every action that changes the view; after a hot reload (`seq` null) | `frame`: a scene frame, version 1, including the optional bounded `hover` metadata (`docs/scene-frame.md`) the panel needs for local hover |
| `search` | answers `search` | `query`, `results`: `[{id, name, ns, label}]`, sorted by namespace then id |
| `context` | answers `context`, before its `frame` | `context`: a [discussion context](#discussion-context) |
| `state` | answers `save` | `state`: a [session state](#session-state) |
| `inspector` | answers `inspector` | `inspector`: the [inspector model](#inspector) (#27) |
| `card` | answers `card` | `card`: the open [class card](#class-card) (#27) |
| `source` | answers `open-source` | `source`: the [opened source](#source-window) (#27) |
| `quality-target` | answers `quality-target` | `target`: the [resolved quality target](#quality-target) (#28) |
| `ack` | answers `camera` and `quit` | — |
| `error` | a line that is not an object, has another `version`, names no or an unknown action, or has invalid fields | `message`; `seq` when the line had one, else null |

An `error` changes nothing; the session keeps running.

## Actions

| action | fields | answer | effect |
|---|---|---|---|
| `open` | `viewport?` `{width, height}`, `diagram?`, `state?`, `waiting?` | `frame` | open the session. The diagram is `diagram` (relative to the project), else the state's `diagram`, else the project default (`diagram.edn`, `examples/<project>.edn`, any example). With `state` the saved view and discussion context are restored; otherwise the diagram is fitted to the viewport. `waiting: true` shows the empty waiting canvas until `display`, except that a `state` naming a displayed `diagram` restores that view (a reopened session, a restarted DSH). When that diagram does not exist yet the canvas waits, `save` answers the saved state unchanged, and the first `display` of the same diagram restores the view instead of fitting it. A missing diagram opens with the canvas hint; an unreadable IR is an `error`. Another `open` replaces the session. |
| `viewport` | `viewport` `{width, height}` (points) | `frame` | the client's canvas size; a pending fit uses it |
| `camera` | `x`, `y`, `zoom` | `ack` | the client panned or zoomed locally: the frame's `camera` (top-left world point and zoom) the client now shows. Saved with the state; no frame is sent back. |
| `display` | `path` | `frame` | show this diagram (what `uml_display` asks for): relative to the project or absolute. The same diagram keeps the view; another one is fitted. Ends the waiting canvas. |
| `reload` | — | `frame` | check the diagram and quality snapshots now instead of at the next poll |
| `drill-in` | `id` (EDN) or `at` (world point) | `frame` | enter a component: a namespace, a proposal package or a nested group. With `at`, what is there is entered when it is drillable, else nothing changes. |
| `back` | — | `frame` | leave one level (close an open proposal layer, else shorten the focus) |
| `namespace-tree` | — | `frame` | **P**: the source diagram's namespace tree |
| `select` | `at` (world point, or `null`) | `frame` | select what is at the point; `null` clears. The frame then carries the 依赖链高亮 and 关系淡化 of the selection. |
| `search` | `query` | `search` | search class names and namespaces (ASCII case-insensitive substring); a blank query has no results. The view does not change. |
| `locate` | `id` (EDN) | `frame` | go to a search result: enter its namespace or proposal layer, select it, open its card and centre it |
| `context` | `proposalId` (EDN, or `null` for the source diagram) | `context`, then `frame` | the user chose the 讨论上下文; the view switches to that diagram. The host turns the `context` event into the panel request. |
| `declutter` | — | `frame` | cycle the arrow mode |
| `save` | — | `state` | the session state to persist |
| `quit` | — | `ack` | end the session (exit 0), like closing stdin |
| `inspector` | — | `inspector` | the diagram/proposal list and the selected element's summary (#27) |
| `card` | `width?`, `viewHeight?` (points) | `card` | the open class card's paint plan; `width` is the card content width (native default 624, floor 560), `viewHeight` the card viewport its scroll clamps against (default 700). No open card is an `error`. |
| `open-card` | `id` (EDN) or `at` (world point) | `frame` | **double-click**: a namespace or proposal layer is entered, a class is selected and its card opens (`scene::events::on_double_press`). With `id` the class's card opens without moving the view. |
| `close-card` | — | `frame` | close the class card; the selection stays |
| `scroll-card` | `delta` (wheel notches), `viewHeight` | `frame` | a wheel over the card: `frame::inspector::scroll` owns the 48-point step and both clamps |
| `quality-target` | `id` (canonical EDN text) | `quality-target` | resolve the class or component to source files from the CURRENT loaded session document and shown proposal, without changing view or discussion state. Unknown, foreign, omitted or sourceless targets are an `error`; a partial or empty success is never returned. |
| `open-source` | `id` (EDN), `member?` | `source` | open the class's source: the whole module, or one member's declaration line. The resident reads the file from the session project; an unknown class, file, language or member, or a source over the [source window limits](#source-window-limits), is an `error`. |

The child polls the diagram and the `.metrics/` quality snapshots every
250 ms while no action is pending; a change produces a `frame` with
`"seq": null` (热重载).

## Local hover (#26, additive version 1)

The panel does hover hit testing and restyling locally (ADR 0013); it never sends
one. Frames carry the optional bounded `hover` field described in
[scene-frame.md](scene-frame.md#hover-metadata-optional-version-1): per visible
relation its `viaIds` and dependency tooltip `rows`, per 精简依赖标记 its
triangle and rows. The resident derives it from the scene; the client renders it
verbatim and, when the field is absent or `complete: false`, falls back to the
relation's own endpoints and no tooltip.

## Overlay message examples (#27, additive version 1)

Each example is one complete JSONL line; `seq` is echoed as usual. The checked-in
real-binary lines in `packages/contracts/tests/protocol-samples/` are validated
by the contracts suite. The compact event examples below show empty item arrays
for space; real card/source events carry all rows.

```jsonl
{"version":1,"seq":1,"action":"inspector"}
{"version":1,"seq":2,"action":"open-card","at":[120,200]}
{"version":1,"seq":3,"action":"card","width":624,"viewHeight":700}
{"version":1,"seq":4,"action":"scroll-card","delta":4,"viewHeight":700}
{"version":1,"seq":5,"action":"close-card"}
{"version":1,"seq":6,"action":"open-source","id":":catalog","member":"target"}
{"version":1,"seq":1,"event":"inspector","inspector":{"entries":[{"id":null,"label":"Shop","active":true}],"summary":{"kind":"empty","lines":[{"type":"label","text":"单击类或命名空间查看摘要。","color":{"role":"muted"}}]},"declutterLabel":"精简：无","detail":null,"selected":null,"labels":{"inspector":"检查器","proposals":"提案","pHint":"P：返回命名空间树"}}}
{"version":1,"seq":3,"event":"card","card":{"id":":catalog","title":"Catalog","contentHeight":32,"viewHeight":700,"scroll":0,"maxScroll":0,"items":[]}}
{"version":1,"seq":6,"event":"source","source":{"file":"D:/project/src/catalog.ts","title":"src/catalog.ts:82","line":82,"lang":"typescript","rows":[]}}
```

`open-source` resolves the requested class file under the session project root.
Parent traversal and resolved symlinks/junctions outside that root are refused
with an `error`; no source text is read and view/discussion/card state stays
unchanged. The class id/member are the only client inputs; clients cannot supply
an arbitrary filename to this action.

## Inspector

`{"event": "inspector", "inspector": {...}}` is `frame::inspector` serialized
(#27). The panel renders it; it does not compute any of it.

| key | type | meaning |
|---|---|---|
| `entries` | `[{id, label, active}]` | the source diagram (`id` null) then each proposal, in display order; `active` is the current discussion context. Choosing one is the `context` action; **P** is `namespace-tree`. |
| `summary` | `{kind, lines}` | `kind` is `class`, `package` or `empty`. `lines` are `class_summary` / `package_summary` output: `{type: "label", text, color}` or `{type: "segments", parts, color}`; segments wrap only between whole statistics. `empty` carries the no-selection help (or the select-a-class hint) as one muted label. |
| `declutterLabel` | string | the arrow-mode control's label |
| `detail` | string \| null | the class whose card is open (EDN) |
| `selected` | string \| null | the selected hit (EDN) |
| `labels` | `{inspector, proposals, pHint}` | the sidebar's UI copy, from `frame::strings` |

Summary text, member text, class counts, CRAP/mutation figures and the 无数据
line are the shared model's, exactly as the egui window prints them; a class
with no quality data keeps its red 无数据 line in the `no-data` role.

## Class card

`{"event": "card", "card": {...}}` is `frame::detail`'s paint plan for the open
card: `id`, `title`, `contentHeight` (`content_h`), `viewHeight`, `scroll`
(clamped) and `maxScroll` (`max_scroll`), plus `items`, the `detail::view`
rows in content points. Each item carries `kind`, `y`, `h`, `text`, `color`,
`titleFont`, `x`, `clipRight`, `cells` (`{id, text, left, right, center,
color, tip}`: the fixed right-aligned metric columns and their tooltips),
`bars` (`{id, left, right, fraction, color}`: Cov and kill-rate bars),
`stripe`, `swatch`, `clickable`, `module`, `opName` and `rel` (the related
class id, EDN). Colors are scene-frame roles (`{"role": …, "grade"|"ratio"}`).

A `module` or `opName` row opens the source window (`open-source` with the
card's `id` and the row's `opName`); a `rel` row locates that class (`locate`).

## Source window

`{"event": "source", "source": {...}}` is `frame::source::member_source` plus
its presentation model: `file` (the resolved path), `title` (project-relative,
with `:line` for a member), `line` (1-based declaration line, `null` for a
whole module), `lang`, and `rows`: `{number, text, display, marked, tokens}`.
`text` is the source line (CR trimmed), `display` its tab-expanded painting,
`marked` is true on the declaration row alone, and `tokens` are the
`{text, kind}` runs (`keyword`, `string`, `comment`, `ink`) of `display`.
A trailing newline leaves one empty last row, as the native window shows.

The resident reads the file from the session project. The client never reads
source from disk; windows, their scrolling and their z-order are panel-local.

### Source window limits

`open-source` is bounded so one large or minified file cannot become an
unbounded event, host buffer or DOM. The resident
(`crates/viewer-session/src/overlays.rs`, `SOURCE_*`) and the contracts
validator (`packages/contracts/src/overlays.ts`, `SOURCE_LIMITS`) use the same
numbers; byte counts are UTF-8:

| bound | limit | checked |
|---|---|---|
| file size | 262144 bytes (256 KiB) | from file metadata before reading; the read stops one byte past the limit |
| rows | 4000 (a trailing newline counts its empty last row) | on the body before declaration lookup |
| one row (`text`, CR trimmed) | 4096 bytes | on the body before declaration lookup |
| one painted row (`display`, a tab paints two spaces) | 8192 bytes | on the body before declaration lookup |
| token runs in one row | 512 | while tokenizing, before allocating run 513 |
| token runs in the source | 20000 | while tokenizing, before allocating run 20001 |
| `source` event line | 2097152 bytes (2 MiB), newline excluded | measured against a stopping writer before the event is written (256 bytes reserved for the envelope) |
| `file`, `title` | 4096 bytes each | before rows are built |
| `lang` | 64 bytes | before rows are built |

A source over any bound is an `error` (`source too large to open: …`) and the
view, discussion context and card stay unchanged, as for any failed
`open-source`. The window is never silently truncated; a non-UTF-8 file stays
"could not open". The contracts validator also refuses the same bounds, row
numbers that are not exactly `1..n`, a `line` that is not `null` or an
existing row, a marked row other than `line`, an unknown token kind, and
token runs that do not join to the row's `display`.

The host additionally caps every resident stdout JSONL record at 16777216
bytes (16 MiB, `RESIDENT_LINE_MAX_BYTES`, newline and CR excluded). It is a
transport safety cap far above normal frames, not a source limit: the
unterminated tail is counted as chunks arrive, so memory stops at the cap. A
record over it stops that resident: every pending request rejects with the
reason, every watching panel receives one `error` and its stream ends, the
child is terminated, and the next panel open starts a fresh resident. Stdout
bytes are decoded incrementally, so a UTF-8 character split between chunks is
not corrupted.

The host's Remote action returns `inspector`, `card` or `source` as an optional
field on that action's `PanelAck` (`{accepted, reason, ...payload}`), validated by
contracts. It does not broadcast these payloads to the panel watch streams:
opening source in one panel must not create a window in another panel of the
same session. View mutations still return and broadcast the existing `frame`
event. Panels request a fresh inspector/card model after a new frame.

A panel's watch stream is not the session's lifetime. Ending one — the gateway
aborting the method signal and returning the iterator on a client unload, a
carrier reconnect, or the sidebar/main reparent — keeps that session's resident
for one bounded window (`WITHDRAWAL_GRACE_MS`, 30 s), so the same session
re-attaches to the same child and the same plugin version. The definite close is
the additive `umlViewer/close` Remote (`PanelCloseAck {closed}`, validated by
contracts): its call, a window expiring with nobody attached, plugin uninstall
and DSH shutdown stop the resident, and each saves the view first.

## Quality target

The host sends this read-only action through its quiet request path and validates
its answer before admitting the visible panel request. It never reads the IR
file again and never computes namespace or proposal membership in TypeScript.
The client menu and `quality-request` operation still carry only the op and id;
client-supplied filenames are ignored. The approved `sessionController.prompt`
call remains one visible message with `mode: "queue"`, admitted FIFO per session,
without touching the composer draft.

```jsonl
{"version":1,"seq":17,"action":"quality-target","id":":core"}
{"version":1,"seq":17,"event":"quality-target","target":{"id":":core","kind":"component","files":["src/core/a.rs","src/core/b.rs"]}}
{"version":1,"seq":18,"action":"quality-target","id":":proposal.domain"}
{"version":1,"seq":18,"event":"quality-target","target":{"id":":proposal.domain","kind":"component","files":["src/api.rs","src/core/a.rs"]}}
{"version":1,"seq":19,"action":"quality-target","id":":quil"}
{"version":1,"seq":19,"event":"error","message":"no class or namespace component :quil in the shown diagram"}
```

These are authored examples, not real-binary captures. IDs are canonical EDN
text as carried by the scene frame. A class takes its own source file. A
namespace component includes its namespace subtree, including its own module
when it has one. Proposal layers, nested groups and `:proposal.unassigned` use
the existing Rust scene hierarchy's actual members: exact module first,
otherwise subtree, with policy/proposal `:omit` and foreign classes excluded.
The answer uses the session's loaded document and current view context, so
replacing the IR file after load cannot change a target until the resident
accepts a reload. No action-specific on-disk IR read occurs.

`target` has `id`, `kind` (`class` or `component`) and `files`. Every file is an
existing canonical project-relative path with forward slashes. The resident
resolves symlinks/junctions before checking project containment; path escapes,
absolute paths, control characters, dot segments and command options are
refused. Files are sorted by UTF-8 path bytes and deduplicated by canonical
path. A missing file or any member without source refuses the whole target.

The resident (`crates/viewer-session/src/targets.rs`, `QUALITY_TARGET_*`) and the
contracts validator (`packages/contracts/src/requests.ts`, `QUALITY_TARGET_LIMITS`)
share these bounds. The id is checked when the answer arrives, because the
resident only echoes an id it already resolved:

| bound | limit |
|---|---|
| files | 4096 |
| each file path | 4096 UTF-8 bytes |
| one `quality-target` payload | 1048320 bytes (1 MiB minus 256 bytes reserved for the envelope) |
| the canonical EDN id (contracts) | 4096 UTF-8 bytes |

An oversized answer is an explicit refusal and is never truncated. The host
checks the returned id matches the requested id, bounds, paths, uniqueness and
order; it neither adds nor removes files. The answer and expected refusals are
private to the requesting action and do not broadcast onto panel watch streams.

## Discussion context

```json
{"kind": "proposal", "proposalId": ":p-shop", "name": "2026-10-24 10:00:00",
 "edn": "{:context :proposal :name \"2026-10-24 10:00:00\" :proposal-id :p-shop}"}
{"kind": "real", "edn": "{:context :real}"}
```

`kind` is `real` (源码图) or `proposal`; `edn` is the `:context` map a panel
request quotes and the one a restore reads.

## Session state

What the host persists with its session (视图状态和讨论上下文由宿主的会话存储持久化),
and passes back as `open`'s `state` to restore it. It is data: the host
stores the resident snapshot and validates its schema. For automatic reopening,
the host also compares `loadedDiagram` with the current file bytes.

| key | type | meaning |
|---|---|---|
| `schema` | string | `"dsh-uml-viewer/session-state"` |
| `version` | integer | `1` |
| `diagram` | string \| null | the diagram shown (absolute path); `null` while the canvas is still waiting for its first display |
| `loadedDiagram` | `{version: 1, sha256: string}` \| null \| absent | additive fingerprint metadata: lowercase SHA-256 of the exact UTF-8 buffer accepted by the resident parser; `null` before any display, absent from older residents |
| `context` | object \| null | the [discussion context](#discussion-context) |
| `focus` | string[] | the namespace path drilled into (EDN keywords) |
| `proposalId` | string \| null | the proposal shown (EDN) |
| `openLayer` | string \| null | the open proposal layer (EDN) |
| `declutter` | string \| null | the arrow mode (EDN keyword) |
| `camera` | `{x, y, zoom}` | as in the scene frame |
| `selected` | string \| null | the selected hit (EDN map) |
| `detail` | string \| null | the class whose card is open (EDN) |

A restore applies only what is still valid: a focus without boxes
shortens, a deleted proposal or layer falls back to the source diagram, a
vanished selection or card is dropped. A state with another `schema` or
`version` is refused with an `error`; unknown keys are ignored.

The fingerprint is captured at initial load, successful display and accepted
hot reload, using the same buffer as parsing. Saving only carries it forward;
the host must never hash a later path read to label that snapshot. An automatic
restore requires matching metadata and matching bytes. Missing/legacy metadata,
changed bytes or a missing file reopens waiting until an authorized display.
The resident checks the bytes it actually loaded again before restoring, so a
replacement between the host's comparison and resident open also fails closed.
This adds no new display grant: the IR-to-display gate remains process-local.

The state carries the restoration dimensions of the egui window's restart
snapshot (`.uml-viewer/session.edn`) without the window chrome and the mail
cursor, which belong to the window and the companion mail.
