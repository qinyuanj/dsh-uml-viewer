# Scene frame, version 1

The **scene frame** is one view of the diagram, fully drawable: every box,
badge, port, relation and chrome line with its text, geometry, LOD tier,
highlight state and color **role**. The `frame` crate (`crates/frame`)
builds it from a `scene::ViewState`; the egui window paints the same
typed draw lists (`frame::Layers`, `frame::frame::chrome`) and the viewer
panel (dsh-uml-viewer#21–#27) receives the JSON written by:

```text
uml-viewer scene --frame --ir <file.edn> [--focus <edn>] [--metrics-root <dir>]
                 [--wide [--aspect <w/h>]] [--extra <edn map>]
                 [--width <pt>] [--height <pt>] [--zoom <z> --camera <x>,<y>]
                 [--select <edn hit>] [--hover <edn hit>] [--pointer <x>,<y>]
                 [--hover-metadata] [--output <file>]
```

Without `--zoom`/`--camera` the view is fitted to the viewport (default
1220 x 884 pt, the window canvas) like a freshly opened diagram. `--extra`
is the view's EDN extras (`{:declutter :triangles}`, `{:proposal true
:proposal-id :p-1}`, `{:open-layer :x}`); a hit is the EDN map hit testing
yields (`{:kind :class :id :ir.graph}`, `{:kind :edge :from :a :to :b
:deps [...]}`). Without `--frame` the `scene` subcommand writes the geometry
EDN as before. The resident mode (`uml-viewer resident`) streams the same
frames, one per line, in reply to actions: [resident-protocol.md](resident-protocol.md).

The goldens in `crates/frame/tests/fixtures/frames/*.frame.json` are the
contract, locked byte for byte by `crates/frame/tests/golden_frame.rs`. They do
not carry the optional `hover` capability below; `--hover-metadata` adds it to a
`scene --frame` run for debugging and browser oracles.

## Stability

- `schema` is always `"dsh-uml-viewer/scene-frame"`; `version` is `1`.
- Adding a field, an item `kind` or a `role` is compatible and keeps the
  version; a consumer ignores what it does not know (an unknown item kind
  is skipped, an unknown role draws as `ink`).
- Removing or renaming a field, kind or role, or changing a unit or meaning,
  bumps `version`.
- `hover` is such an optional additive field: a consumer without it still draws
  the frame, and the checked-in goldens omit it.
- Object keys are sorted; numbers are JSON numbers (style constants at their
  decimal value). The output ends with a newline.

## Top level

| key | type | meaning |
|---|---|---|
| `schema` | string | `"dsh-uml-viewer/scene-frame"` |
| `version` | integer | `1` |
| `viewport` | `{width, height}` | canvas size in points |
| `camera` | `{x, y, zoom}` | `screen = (world - (x, y)) * zoom`, relative to the viewport origin |
| `lod` | `"blocks" \| "frames" \| "full"` | the tier this zoom paints at |
| `waiting` | bool | no diagram yet (waiting for `:display`); `world` and `edges` are empty |
| `style` | object | fonts, widths, radii and thresholds below |
| `world` | item[] | scene-space items, painted in order (packages first, then each class with its badges, text and ports) |
| `edges` | edge[] | relation strokes, arrowheads and indicators, painted after `world` |
| `chrome` | item[] | viewport-space items, painted last, in order |
| `hover` | object | optional bounded hover metadata (see Hover metadata); absent from the goldens |

### `style`

| key | value | use |
|---|---|---|
| `fonts.canvas` | 16 | class, member and package text (scaled by zoom) |
| `fonts.level` / `marker` / `badge` / `port` | 11 / 12 / 9 / 10 | level number, α/I, C/M letter, port label (scaled) |
| `fonts.chrome` | 18 | chrome text (not scaled) |
| `widths.band` / `rule` / `dot` / `portLink` | 2 / 1 / 1 / 1.3 | stroke widths in points, never scaled by zoom |
| `widths.dash` / `dashGap` | 6 / 4 | dashed outline of an unmeasured frame |
| `radius.class` / `package` / `port` / `rowWash` | 4 / 8 / 3 / 2 | corner radii in world units (dropped under 2 screen px) |
| `dimAlpha` | 70 | 关系淡化: a `dimmed` item multiplies all color channels, alpha included, by 70/255, except a closed arrowhead's `fill` (see Edges) |
| `lodZoom` | `{frames: 0.15, full: 0.4}` | zoom below `frames` is `blocks`, below `full` is `frames` |
| `minTextPx` | 5 | canvas text under 5 screen px is not drawn |
| `edgeDecimatePx` | 8 | at the `frames` tier stroke points closer than 8 screen px may be dropped (ends kept) |

## Colors: roles

A color is `{"role": <name>}`; the quality roles add `"grade"` (0–10,
`null` = unmeasured, drawn as the worst grade's neutral) or `"ratio"`
(0–1, `null` unknown). The renderer resolves roles through host theme
tokens; 质量色, 违规关系色, 无数据 (`no-data`) and 依赖链高亮
(`dependency-chain`) keep their hue and adapt only to light/dark, so neither a
host accent token nor a skin repaints them.
The dark palette below is what the egui window paints (`frame::theme::rgba`).

| role | meaning | dark palette |
|---|---|---|
| `canvas` | canvas; interior of open arrowheads | `#161c20` |
| `panel` | card and source window background | `#1a2428` |
| `sidebar` | inspector | `#121618` |
| `line` | hairlines, popup frame, bar tracks | `#2a3d36` |
| `ink` | primary text; filled diamond | `#ecece4` |
| `muted` | stereotypes, levels, hints, title, status | `#9db8a8` |
| `accent` | package titles, CRAP lines, banner, return line, card headings | `#e8c448` |
| `marker` | α/I, C/M letters, bright card figures | `#ffffff` |
| `selected` | outline of the selected box or port | `#ffffff` |
| `good` | no survived/uncovered mutants | `#5fb58a` |
| `danger` | destructive control under the pointer | `#ff4030` |
| `chrome-plate` | behind chrome text | canvas, alpha 235 |
| `popup` | dependency hover popup | `#121618`, alpha 240 |
| `row-wash` / `row-bar` | marked row wash / its left rule | gold, alpha 32 / gold |
| `quality-fill` *(grade)* | class box and dot fill (质量色) | red→mid→green ramp; unmeasured `#243430` |
| `quality-stroke` *(grade)* | class frame, title band, rules, dot ring | ramp; unmeasured `#5a6e64` |
| `package-fill` *(grade)* | package fill | fill ramp, alpha 80 |
| `package-stroke` *(grade)* | package frame | stroke ramp, alpha 170 |
| `coverage` *(ratio)* | line coverage ink | ≥0.8 good, ≥0.5 gold, else `#e07a4a`; unknown muted |
| `no-data` | 无数据 marker | `#c42a24` |
| `relation` | 普通关系色 | `#9db8a8` |
| `relation-hover` | 关系悬停高亮 | `#ffffff` |
| `dependency-chain` | 依赖链高亮: chain edges and frames | `#3e98ec` |
| `violation` | 违规关系色: violating edges, indicators, popup rows | `#c42a24` |
| `violation-hot` | violating edge on the chain or hovered | `#ff4030` |
| `indicator` | 精简依赖标记 without violations | `#9db8a8` |
| `port-fill` / `port-fill-marked` | 依赖端口 fill / when selected | `#2a3834` / `#4a604e` |
| `port-outline` / `port-link` | port frame / its arrow to the class | muted |
| `button-hover` / `button-active` | inspector buttons | `#364c42` / `#425a4c` |
| `source-bg` / `source-ink` / `source-gutter-bg` / `source-gutter` / `source-selection` / `source-comment` / `source-string` / `source-keyword` | 源码窗口 | panel / ink / sidebar / muted / `#263646` / `#788c82` / `#e6a06e` / `#7aa2f7` |

The ramps: for grade `g ≤ 5` mix the red end into the midpoint by `g/5`,
above it the midpoint into the green end by `(g-5)/5`; fill red/mid/green
`#4a2818`/`#3d3a18`/`#1e4a38`, stroke `#e07a4a`/`#d4c05a`/`#5fb58a`
(`frame::theme::fill`, `stroke`).

## World items

Every world item has `kind`, `lod` (the coarsest tier that draws it; skip it
when the frame's tier is coarser), `bounds` (`{x,y,w,h}` world extent for
culling, `null` when none) and `dimmed`. Class-owned items carry `owner`,
the class id. Ids are EDN text (`":ir.graph"`) and round-trip into panel
requests. Rects are `{x, y, w, h}`, points `[x, y]`, all in world units.

| kind | lod | fields | drawing |
|---|---|---|---|
| `package` | blocks | `id rect text band grade size maxW missing missingCount fill stroke strokeWidth ink selected related` | rounded rect (`radius.package`) filled `fill`, outlined `stroke` at `strokeWidth` (dashed when `missing`); `text` at `rect.x + 12, rect.y + 8` (scaled), italic, size `fonts.canvas`, fitted to `maxW` |
| `class` | blocks | `id owner rect shape band grade missing fill stroke strokeWidth selected hovered related` | `shape` `"oval"` (外来包椭圆) is an ellipse with a solid outline centred on its edge, even when `missing`; otherwise a rounded rect (`radius.class`), dashed outline when `missing` |
| `quality` | frames | `id owner of text band grade center radius fill stroke ink` | C/M dot: disc `radius` (world) filled `fill`, ring `stroke` at `widths.dot`; the letter `text` in `ink` at `fonts.badge`, centred, at `full` only |
| `marker` | full | `owner text at size ink` | α / I at `at` (top-left), size `size` |
| `level` | full | `owner text at size ink` | 架构级别编号 at `at` |
| `band` | full | `owner from to stroke` | title band, width `widths.band` |
| `rule` | full | `owner from to stroke` | member separator, width `widths.rule` |
| `text` | name `frames`, else full | `owner line text x top slot size maxW italic bold center ink` | `line` is `name`, `stereo`, `crap`, `child`, …; draw at `px = round(slot * zoom)` (skip under `minTextPx`), vertically centred in `[top, top + slot]`; left at `x`, or centred on `x + maxW/2` when `center`; shrink to fit `maxW` counting bold `+0.05·px` and italic `+0.2·px`; bold is a second pass offset `0.05·px` |
| `no-data` | full | `owner text right y size ink` | 无数据, right-aligned to `right` |
| `child-wash` | full | `owner rect marker{wash bar barWidth}` | marked member row: `rect` filled `wash` (radius `rowWash`), a `bar` rule `barWidth` (scaled) at its left |
| `port-link` | full | `owner behind tip stroke` | line `behind → tip` at `widths.portLink`, open arrowhead of 10 screen px at ±20° |
| `port` | full | `id owner rect text marked fill stroke strokeWidth ink` | 依赖端口: rect (`radius.port`), label centred at `fonts.port`, fitted to `rect.w - 6` |

Painting rules shared by the egui window and the panel (`crates/viewer/src/paint.rs`,
`packages/client/src/paint.ts`): a scaled font size is a whole pixel,
`max(1, round(size * zoom))`, and a fitted label shrinks to the largest whole
pixel that fits; marker, level, package title, no-data and port labels are
placed by their top edge (a port label's top is `5 * zoom` above the port's
centre). A rounded rect's solid outline (package, class, port) lies inside the
rect: its centre line and corner radius are inset by half the stroke width. An
unmeasured (`missing`) outline is instead a square dashed path along the rect's
edges over the rounded fill. Boxes under one screen point are not drawn. At
`blocks` and `frames` the class names are painted after every box and dot.

## Edges

| kind | lod | fields | drawing |
|---|---|---|---|
| `stroke` | frames | `from to points stroke width hot warm dimmed` | polyline in world points, `width` screen points |
| `head` | full | `from to head tip behind stroke fill width hot warm dimmed` | arrowhead at `tip`, pointing away from `behind`: sides `16·zoom` at ±20°; `head` is `open` (two strokes), `triangle` (filled `fill`), `diamond` / `diamond-fill` (back vertex `16·zoom` behind the tip, filled `fill`) |
| `indicator` | frames | `id points violating fill dimmed` | 精简依赖标记: filled triangle `points`, outlined in the same color at 1 pt |

`hot` is the edge under the pointer, `warm` an edge touching the hovered
class; both are already folded into `stroke`/`width`. A `via-ids` class counts as
touching (`scene::highlight::touches`), which is why the panel reads `hover`'s
`viaIds` for local hover.

For a `head`, `dimmed` applies only to `stroke`. A closed head's `fill`
remains fully opaque: `canvas` for hollow triangles/diamonds and `ink` for
filled diamonds. Draw the fill over the shaft before the head outline; the
scene's shaft runs through the head to its tip, so the opaque interior
masks it. Canvas clients must resolve the fill role at alpha 1 and apply
`dimAlpha` only to the outline, rather than reducing the whole head's
`globalAlpha`. An `open` head has no fill.

Panel strokes at `frames` follow the native render cache: quantize zoom to a
quarter octave (`2^(round(log2(zoom)*4)/4)`), keep the two ends, discard interior
points less than `edgeDecimatePx` screen points from the last kept point, and
discard points within 0.5 screen points of its chord to the next point. At
`frames`, and at `full` below zoom 0.6, draw each segment with butt ends, as the
native shaft mesh does. Closed heads always paint over their shafts, including
at zoom 0.4–0.6. This deliberately avoids the native window's pre-existing
low-zoom flush-order defect, which paints its shaft mesh after the heads.

## Hover metadata (optional, version 1)

Selection, 依赖链高亮 and 关系淡化 are the resident's: a `select` answers a frame
that already carries them. Hover is the client's (ADR 0013), but two things it
needs are not visible in `edges`: which leaf classes a bundled relation passes
through (`via-ids`), and the dependency tooltip rows. The resident adds them in
this optional field, derived from the scene by `crates/frame/src/hover.rs`; the
goldens omit it, so a consumer must treat it as a capability.

```json
"hover": {
  "complete": true,
  "edges": [{"from": ":api", "to": ":serde", "viaIds": [":core.order"],
             "rows": [{"text": "api -> core.order", "violating": true, "ink": {"role": "violation"}}]}],
  "indicators": [{"id": ":core", "points": [[0, 0], [12, 4], [6, 11]],
                  "rows": [{"text": "core -> util", "violating": false, "ink": {"role": "ink"}}]}]
}
```

| key | meaning |
|---|---|
| `complete` | `true` when the whole capability is present; `false` when a bound was exceeded, and then both arrays are empty |
| `edges[]` | one entry per visible relation: `from`/`to` (EDN ids, as in the strokes), `viaIds` (the classes it passes through, `scene::highlight::touches`), `rows` (the `from -> to` tooltip rows, violations first) |
| `indicators[]` | one entry per 精简依赖标记: `id`, its `points` (the two indicators of one class are told apart by their triangle) and its tooltip `rows` |

The tooltip rows are `frame::commands::hover_rows` output verbatim (`text`,
`violating`, `ink` role); the client renders them and never composes one. A
hovered relation, or its indicator, shows them at the pointer; hovering a class
at the overview tiers names the class, as before.

Bounds are deliberate, so a large diagram cannot make one frame unbounded: at
most 1024 edges and indicators together, 256 `viaIds` per edge, 128 rows per
item and 2048 rows in total, 4096 UTF-8 bytes per id or row text, and 512 KiB of
serialized metadata. Exceeding any bound yields `"complete": false` with empty
arrays rather than silent truncation; without the field the client highlights by
the relation's own endpoints and shows no dependency tooltip.

## Shared inspector summaries

The window-free `frame::inspector` API also supplies `class_summary(&detail::Model)`
and `package_summary(&scene, &id)`. Each returns display-ordered `SummaryLine`s:
`Label { text, color }` or `Segments { parts, color }`. Colors are semantic roles;
metric segments wrap only between complete statistics. These functions own
identity/member text, quality formatting, no-data lines and the current view's
package class count. Native and panel adapters only render the supplied lines.

## Chrome items (viewport points)

| kind | fields | drawing |
|---|---|---|
| `title` / `proposal-banner` / `back` | `text at ink plate` | text at `at`, size `fonts.chrome`, on a `plate` rect expanded 6 x 2 pt, radius 3 |
| `status` | `text at ink plate(null)` | bottom-left status line |
| `hint` | `text ink` | centred on the viewport |
| `popup` | `rect fill border rows[{text violating ink}]` | radius 4, inside 1 pt `border`; row `i` top-anchored at `rect.x + 8, rect.y + 8 + 20·i`, size `fonts.chrome` |
