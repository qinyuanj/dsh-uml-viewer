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
| `frame` | after every action that changes the view; after a hot reload (`seq` null) | `frame`: a scene frame, version 1 |
| `search` | answers `search` | `query`, `results`: `[{id, name, ns, label}]`, sorted by namespace then id |
| `context` | answers `context`, before its `frame` | `context`: a [discussion context](#discussion-context) |
| `state` | answers `save` | `state`: a [session state](#session-state) |
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

The child polls the diagram and the `.metrics/` quality snapshots every
250 ms while no action is pending; a change produces a `frame` with
`"seq": null` (热重载).

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
