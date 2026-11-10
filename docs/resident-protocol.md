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
| `frame` | after every action that changes the view; after a hot reload (`seq` null) | `frame`: a scene frame, version 1, including the legend worded in the [language](#language) (#50) and the optional bounded `hover` metadata (`docs/scene-frame.md`) the panel needs for local hover |
| `search` | answers `search` | `query`, `results`: `[{id, name, ns, label}]`, sorted by namespace then id |
| `context` | answers `context`, before its `frame` | `context`: a [discussion context](#discussion-context) |
| `state` | answers `save` | `state`: a [session state](#session-state); `breadcrumbs`: the tree path ([breadcrumbs, status codes and proposals](#breadcrumbs-status-codes-and-proposals-45-47-additive-version-1)); `status`: the canvas status code, or null; `proposals`: the shown diagram's proposal list for the discussion-context switcher (#47) |
| `card` | answers `card` | `card`: the open [class card](#class-card) (#27), or `null` when none is open (#47) |
| `source` | answers `open-source` | `source`: the [opened source](#source-window) (#27) |
| `quality-target` | answers `quality-target` | `target`: the [resolved quality target](#quality-target) (#28) |
| `ack` | answers `camera` and `quit` | — |
| `error` | a line that is not an object, has another `version`, names no or an unknown action, or has invalid fields | `message`; `seq` when the line had one, else null |

An `error` changes nothing; the session keeps running.

## Actions

| action | fields | answer | effect |
|---|---|---|---|
| `open` | `viewport?` `{width, height}`, `diagram?`, `state?`, `waiting?`, `language?` | `frame` | open the session. The diagram is `diagram` (relative to the project), else the state's `diagram`, else the project default (`diagram.edn`, `examples/<project>.edn`, any example). With `state` the saved view and discussion context are restored; otherwise the diagram is fitted to the viewport. `waiting: true` shows the empty waiting canvas until `display`, except that a `state` naming a displayed `diagram` restores that view (a reopened session, a restarted DSH). When that diagram does not exist yet the canvas waits, `save` answers the saved state unchanged, and the first `display` of the same diagram restores the view instead of fitting it. A missing diagram opens with the `missing-file` status code; an unreadable IR is an `error`. Another `open` replaces the session. `language` is the [language](#language) the panel passes (#44). |
| `viewport` | `viewport` `{width, height}` (points) | `frame` | the client's canvas size; a pending fit uses it |
| `camera` | `x`, `y`, `zoom` | `ack` | the client panned or zoomed locally: the frame's `camera` (top-left world point and zoom) the client now shows. Saved with the state; no frame is sent back. |
| `display` | `path` | `frame` | show this diagram (what `uml_display` asks for): relative to the project or absolute. The same diagram keeps the view; another one is fitted. Ends the waiting canvas. |
| `reload` | — | `frame` | check the diagram and quality snapshots now instead of at the next poll |
| `drill-in` | `id` (EDN) or `at` (world point) | `frame` | enter a component: a namespace, a proposal package or a nested group. With `at`, what is there is entered when it is drillable, else nothing changes. |
| `back` | `target?` (a breadcrumb id, or null for the source diagram's root) | `frame` | leave one level, or return to the level `target` names ([breadcrumbs, status codes and proposals](#breadcrumbs-status-codes-and-proposals-45-47-additive-version-1)); an id no level of the current path carries is an `error` |
| `namespace-tree` | — | `frame` | **P**: the source diagram's namespace tree |
| `select` | `at` (world point, or `null`) | `frame` | select what is at the point; `null` clears. The frame then carries the 依赖链高亮 and 关系淡化 of the selection. |
| `search` | `query` | `search` | search class names and namespaces (ASCII case-insensitive substring); a blank query has no results. The view does not change. |
| `locate` | `id` (EDN) | `frame` | go to a search result: enter its namespace or proposal layer, select it, open its card and centre it |
| `context` | `proposalId` (EDN, or `null` for the source diagram) | `context`, then `frame` | the user chose the 讨论上下文; the view switches to that diagram. The host turns the `context` event into the panel request. |
| `declutter` | `mode?` | `frame` | the arrow mode: `full`, `arrows`, `triangles`, `elements` or `classes`. A named mode is set (the panel's 「更多」 menu names `triangles`/`full`, #46); without one the resident cycles the modes. An unknown mode is an `error` and changes nothing |
| `save` | — | `state` | the session state to persist |
| `quit` | — | `ack` | end the session (exit 0), like closing stdin |
| `card` | `width?`, `viewHeight?` (points) | `card` | the open class card's paint plan; `width` is the card content width (native default 624, floor 560), `viewHeight` the card viewport its scroll clamps against (default 700). With no class card open the answer is `card: null`, which changes nothing (#47). |
| `open-card` | `id` (EDN) or `at` (world point) | `frame` | **double-click**: a namespace or proposal layer is entered, a class is selected and its card opens (`scene::events::on_double_press`). With `id` the class's card opens without moving the view. |
| `close-card` | — | `frame` | close the class card; the selection stays |
| `scroll-card` | `delta` (wheel notches), `viewHeight` | `frame` | a wheel over the card: `frame::detail::scroll_card` owns the 48-point step and both clamps |
| `quality-target` | `id` (canonical EDN text) | `quality-target` | resolve the class or component to source files from the CURRENT loaded session document and shown proposal, without changing view or discussion state. Unknown, foreign, omitted or sourceless targets are an `error`; a partial or empty success is never returned. |
| `language` | `language` (a locale tag) | `frame` | the DSH language the panel passes (#44); the resident re-lays the frame out in it and answers with that frame, so every watching panel redraws its 无数据 markers and asks again for an open class card. The latest one wins: DSH has one language for every panel. Before `open` it is remembered and answered with an `ack`. |
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

## Breadcrumbs and status codes (#45, additive version 1)

The panel's top bar draws the tree path it is looking at; the panel's own
labels localize, but the levels are data. `save` answers with them beside the
[session state](#session-state), so they arrive after every action that
changes the view (the panel saves after each frame). The lines below abridge
the state object:

```jsonl
{"version":1,"seq":9,"event":"state","state":{"schema":"dsh-uml-viewer/session-state",...},"breadcrumbs":[{"id":null,"name":"Shop"},{"id":":core","name":"core"},{"id":":core.order","name":"order"}],"status":null}
{"version":1,"seq":10,"event":"state","state":{"schema":"dsh-uml-viewer/session-state",...},"breadcrumbs":[{"id":":p-shop","name":"2026-10-24 10:00:00"},{"id":":proposal.domain","name":"Domain"}],"status":null}
{"version":1,"seq":11,"event":"state","state":{...},"breadcrumbs":[],"status":{"code":"missing-file","path":"/project/diagram.edn"}}
```

The TS `PanelEvent.state` type requires `breadcrumbs` and `status`, while the contracts validator defaults absent fields to `[]` and `null` for a resident from before #45; in-repo producers always send both (F11).

`breadcrumbs` is the path from the tree root to the current component, root
first, and is `[]` before any diagram is loaded. Each level's `id` is the
component it shows, in the same canonical EDN text the frames use; `name` is
the name the tree gives it — the diagram title, a namespace segment, the
proposal's name, a layer's label or a nested group's label. The source
diagram's root has no component, so its `id` is `null`. A proposal's path
starts at the proposal (`id` is the proposal id) and continues through the
layers and nested groups that contain the shown layer. The path always names
what the canvas is showing: a layer the document no longer has stops the path
at the proposal rather than naming a level nothing displays.

`back` without `target` leaves one level, as it always did. With `target` it
returns straight to the level whose `id` is that value, at any depth: `null`
is the source diagram's root, and a proposal level is named by its own id.
The target must be on the current path; anything else is an `error` and the
view stays as it is. Naming the level already shown is not a move — no camera,
selection or fit change.

`proposals` (#47) is the shown diagram's proposal list for the panel's
discussion-context switcher, in the document's order: `[{id, name}]`, each id
in the same canonical EDN text as the frames and the breadcrumbs, `name` the
proposal's authored `:name`. It is `[]` for a diagram with no proposals, a
diagram not loaded yet, and a resident that predates #47. Choosing one is the
existing `context` action; 新建提案 is the existing `new-proposal` panel request.

`status` is the canvas status as a code, `null` when there is nothing to say:

| code | parameters | meaning |
|---|---|---|
| `waiting-display` | — | the empty canvas waits for its first `display` |
| `missing-file` | `path` | the diagram file this session names is not there |

The panel localizes the code from its own language files, so the code is an
enum with exactly the parameters its text needs: an unknown code is a broken
resident, not a message to show. The frame no longer carries that text at all
(#46): the status bar draws it, and a view's chrome is the hover popup alone
(`docs/scene-frame.md`). A resident that predates #45 sends neither field, and
the panel then shows no path and no status.

## Overlay message examples (#27, additive version 1)

Each example is one complete JSONL line; `seq` is echoed as usual. The checked-in
real-binary lines in `packages/contracts/tests/protocol-samples/` are validated
by the contracts suite. The compact event examples below show empty item arrays
for space; real card/source events carry all rows.

```jsonl
{"version":1,"seq":2,"action":"open-card","at":[120,200]}
{"version":1,"seq":3,"action":"card","width":624,"viewHeight":700}
{"version":1,"seq":4,"action":"scroll-card","delta":4,"viewHeight":700}
{"version":1,"seq":5,"action":"close-card"}
{"version":1,"seq":6,"action":"open-source","id":":catalog","member":"target"}
{"version":1,"seq":3,"event":"card","card":{"id":":catalog","title":"Catalog","contentHeight":32,"viewHeight":700,"scroll":0,"maxScroll":0,"items":[]}}
{"version":1,"seq":7,"action":"card"}
{"version":1,"seq":7,"event":"card","card":null}
{"version":1,"seq":6,"event":"source","source":{"file":"D:/project/src/catalog.ts","title":"src/catalog.ts:82","line":82,"lang":"typescript","rows":[]}}
```

`open-source` resolves the requested class file under the session project root.
Parent traversal and resolved symlinks/junctions outside that root are refused
with an `error`; no source text is read and view/discussion/card state stays
unchanged. The class id/member are the only client inputs; clients cannot supply
an arbitrary filename to this action.

## Deleted: inspector

The `inspector` action and event were deleted in #47. Its proposal list became
the panel's discussion-context switcher and now rides on the `state` event
([below](#breadcrumbs-status-codes-and-proposals-45-47-additive-version-1));
its summary line (the selected class or component's name, package, level, CRAP,
mutation counts and members) was deleted, not moved: the class card already
carries the class's quality figures, and a second copy elsewhere would have to
stay equal to it (ADR 0015). A resident that still sends `inspector` is
ignored by the panel; one that does not know the action answers `error`.

## Language (#44)

The text the resident lays out — the class card's rows and tooltips, the class
box's 无数据 marker, the source window's exact `未能打开源码：%s` error and the
[legend](scene-frame.md#legend) rows every frame carries — comes in Chinese and
English. Any other locale tag reads English, and so does a resident that was
never told one. The panel passes the DSH language when it opens its stream and
again on every switch; the host sends it on `open` (a fresh resident) or as a
`language` action (a running one).

The language is the resident's, not the session's: DSH has one language for all
panels, the latest write wins, and the [session state](#session-state) never
records it. Metric and tool names (CRAP, CC, Cov, killed, survived, uncovered,
IR) read the same in both languages.

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
A trailing newline leaves one empty last row, as the deleted native window showed.

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

The child caps one inbound action line at the same 16777216 bytes (16 MiB,
`ACTION_LINE_MAX_BYTES`, newline excluded). An action is far smaller in
practice, so a line over it is a host bug, not a normal request: the reader
stops at the cap instead of buffering without limit, the session writes one
`error` event with that reason and exits 1, and the panel reports a stopped
resident. Both directions of the transport are therefore bounded.

The host's Remote action returns `card` or `source` as an optional field on
that action's `PanelAck` (`{accepted, reason, ...payload}`), validated by
contracts. It does not broadcast these payloads to the panel watch streams:
opening source in one panel must not create a window in another panel of the
same session. View mutations still return and broadcast the existing `frame`
event. Panels request a fresh card after a new frame; `card` with none open answers
`card: null`, which clears the card the panel shows (#47).

A panel's watch stream is not the session's lifetime. Ending one — the gateway
aborting the method signal and returning the iterator on a client unload, a
carrier reconnect, or a panel body remount during a right-sidebar presentation
change — keeps that session's resident
for one bounded window (`WITHDRAWAL_GRACE_MS`, 30 s), so the same session
re-attaches to the same child and the same plugin version. A mount disposer
never closes. Panels use the additive `umlViewer/watchOwned` and
`umlViewer/closeOwned`. Both take the `agent` lookup and a strict JSON `owner`
`{pageId, holderId}` (1–128 of `[A-Za-z0-9_-]` each); only `watchOwned` has the
trailing cancellation signal. `closeOwned` releases only its own holder and
answers `PanelCloseAck {closed}`. The version-1 `watch` and `close` keep their
wire shape. A legacy `close` is ignored once a resident has an owned holder.
A distinct `pageId` replaces a resident only when nothing holds it. Owner ids
authorize nothing. A close call, a window expiring with nobody attached, plugin
uninstall and DSH shutdown stop the resident, and each saves the view first.

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
| `diagram` | string \| null | the diagram shown, as its canonical absolute path (the spelling a restore opens, so an unchanged view saves the same state); `null` while the canvas is still waiting for its first display |
| `loadedDiagram` | `{version: 1, sha256: string}` \| null \| absent | additive fingerprint metadata: lowercase SHA-256 of the exact UTF-8 buffer accepted by the resident parser; `null` before any display, absent from older residents |
| `context` | object \| null | the [discussion context](#discussion-context) |
| `focus` | string[] | the namespace path drilled into (EDN keywords) |
| `proposalId` | string \| null | the proposal shown (EDN): set exactly while the canvas shows a proposal layer and dropped when the view returns to the source tree (`namespace-tree`). The panel reads it as the view summary's flag (#46), so a proposal that is displayed from a source discussion reads as a proposal view until **P**. |
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

The state carries the restoration dimensions of the historical window
restart snapshot (`.uml-viewer/session.edn`, gone with the window in #30)
without its window chrome and mail cursor.
