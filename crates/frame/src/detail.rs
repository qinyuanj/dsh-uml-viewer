//! Class card model (#141): port of `application/detail.lua` (card model,
//! content-space rows, scroll clamping, row hits) and
//! `application/detail_view.lua` (the paint plan: fixed right-aligned metric
//! columns, colors, swatches, Cov / kill-rate bars, stripes). Window-free;
//! the panel only paints [`Item`]s and reports pointer rows.

use edn::Value;
use edn::record::{flag, get, local_name, num, seq};
use scene::document::ViewState;

use crate::source::Ident;
use crate::strings::{self, Lang};
use crate::theme::{Role, RowMarker};

/// Default card window, in points.
pub const WIDTH: f64 = 640.0;
pub const HEIGHT: f64 = 700.0;
pub const PAD: f64 = 16.0;
pub const SWATCH_W: f64 = 10.0;
pub const SWATCH_GAP: f64 = 4.0;
/// One wheel notch scrolls one step.
pub const SCROLL_STEP: f64 = 48.0;

/// A relationship row: the other class, the relationship kind and its
/// direction; the phrase is laid out in the card's language.
#[derive(Debug, Clone, PartialEq)]
pub struct Rel {
    pub id: Value,
    pub name: String,
    pub label: Option<String>,
    pub kind: String,
    pub outgoing: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub class: Value,
    pub ns: Option<String>,
    pub package: Option<Value>,
    pub lang: Option<Value>,
    pub file: Option<String>,
    pub rels: Vec<Rel>,
}

impl Model {
    /// The label of the package that holds the class, if any.
    pub fn package_label(&self) -> Option<String> {
        self.package
            .as_ref()
            .and_then(|p| string(get(p, "label")))
            .or_else(|| string(get(&self.class, "package")))
    }

    /// A card for a bare class record (no scene context).
    pub fn of(class: Value) -> Model {
        Model {
            class,
            ns: None,
            package: None,
            lang: None,
            file: None,
            rels: Vec::new(),
        }
    }
}

fn string(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Nil => None,
        other => Some(local_name(other).unwrap_or_default().to_owned()),
    }
}

fn phrase(rel: &Rel, lang: Lang) -> &'static str {
    let text = lang.text();
    let (out, inc) = text
        .phrases
        .iter()
        .find(|(k, _, _)| *k == rel.kind)
        .map_or(text.phrase_fallback, |(_, o, i)| (*o, *i));
    if rel.outgoing { out } else { inc }
}

/// The card model of a scene-core neighbourhood: phrasing and names only.
fn assemble(n: scene::Neighbourhood) -> Model {
    let c = n.class;
    Model {
        ns: string(get(&c, "ns"))
            .or_else(|| get(&c, "id").map(|v| local_name(v).unwrap_or_default().to_owned())),
        lang: get(&c, "lang").cloned(),
        file: string(get(&c, "file")),
        package: n.package,
        rels: n
            .relations
            .into_iter()
            .map(|r| Rel {
                name: string(r.other_name.as_ref())
                    .unwrap_or_else(|| local_name(&r.other).unwrap_or_default().to_owned()),
                label: string(r.label.as_ref()),
                kind: r
                    .kind
                    .as_ref()
                    .and_then(local_name)
                    .unwrap_or_default()
                    .to_owned(),
                outgoing: r.outgoing,
                id: r.other,
            })
            .collect(),
        class: c,
    }
}

/// The card of class `id` in a scene, or None when it is not there.
pub fn model(scene: &Value, id: &Value) -> Option<Model> {
    scene::class_card_in(scene, id).map(assemble)
}

/// The card of class `id` in the current view; the scene core keeps its
/// leaf identities and relationships across namespaces.
pub fn for_state(state: &ViewState, id: &Value) -> Option<Model> {
    let mut n = scene::class_card(state, id)?;
    // A crate root module shares its namespace's id; a namespace box on
    // screen reads as the namespace, with its totals.
    if let Some(shown) = scene::hit::class_by_id(&state.scene, id).filter(|c| flag(c, "drill?")) {
        n.class = shown.clone();
    }
    Some(assemble(n))
}

/// What a source jump from this card opens: the module, or one member.
pub fn member_ident(model: &Model, member: Option<&str>) -> Ident {
    Ident {
        lang: model
            .lang
            .as_ref()
            .map(|v| local_name(v).unwrap_or_default().to_owned()),
        file: model.file.clone(),
        ns: model.ns.clone(),
        name: member.filter(|m| !m.is_empty()).map(Into::into),
    }
}

/// Formatted metric cells of one stats row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cells {
    pub crap_s: Option<String>,
    pub crap_n: Option<f64>,
    pub cc_s: Option<String>,
    pub cov_s: Option<String>,
    pub coverage: Option<f64>,
    pub killed: Option<f64>,
    pub survived: Option<f64>,
    pub uncovered: Option<f64>,
    pub killed_s: Option<String>,
    pub survived_s: Option<String>,
    pub uncovered_s: Option<String>,
    pub mut_note: Option<String>,
}

/// One content-space row (y and h in points).
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub kind: &'static str,
    pub text: String,
    pub y: f64,
    pub h: f64,
    pub module: bool,
    pub op_name: Option<String>,
    pub private: bool,
    /// The related class of a `rel` row.
    pub id: Option<Value>,
    pub cells: Cells,
}

fn mu(c: Option<&Value>) -> Option<f64> {
    match c? {
        Value::Map(_) => get(c?, "mu").and_then(num),
        other => num(other),
    }
}

fn one(n: f64) -> String {
    format!("{n:.1}")
}

fn whole(n: f64) -> String {
    format!("{}", n.floor() as i64)
}

fn number_text(v: &Value) -> String {
    match num(v) {
        Some(n) if n.fract() == 0.0 => whole(n),
        Some(n) => n.to_string(),
        None => string(Some(v)).unwrap_or_default(),
    }
}

fn cells(c: &Value, class: bool, lang: Lang) -> Cells {
    let count = |k: &str| {
        let n = get(c, k).and_then(num);
        if n.is_some() || !class {
            return n;
        }
        seq(get(c, "ops"))
            .iter()
            .filter_map(|op| get(op, k).and_then(num))
            .fold(None, |acc, v| Some(acc.unwrap_or(0.0) + v))
    };
    let crap = mu(get(c, "crap"));
    let coverage = get(c, "coverage").and_then(num);
    let (killed, survived, uncovered, sites) = (
        count("killed"),
        count("survived"),
        count("uncovered"),
        count("sites"),
    );
    let mut r = Cells {
        crap_s: crap.map(|x| format!("{}{}", one(x), if class { "μ" } else { "" })),
        crap_n: crap,
        cc_s: if class {
            None
        } else {
            get(c, "cc").and_then(num).map(whole)
        },
        cov_s: coverage.map(|x| format!("{:.0}%", 100.0 * x)),
        coverage,
        killed,
        survived,
        uncovered,
        ..Cells::default()
    };
    let total = killed.unwrap_or(0.0) + survived.unwrap_or(0.0) + uncovered.unwrap_or(0.0);
    if total == 0.0 && sites.unwrap_or(0.0) == 0.0 {
        r.mut_note = Some(lang.text().no_mutation_sites.into());
    } else {
        r.killed_s = killed.map(whole);
        r.survived_s = survived.map(whole);
        r.uncovered_s = uncovered.map(whole);
    }
    r
}

fn row(kind: &'static str, text: impl Into<String>, y: f64, h: f64) -> Row {
    Row {
        kind,
        text: text.into(),
        y,
        h,
        module: false,
        op_name: None,
        private: false,
        id: None,
        cells: Cells::default(),
    }
}

/// The card rows in content space (20-point title, 18-point body rows),
/// worded in `lang`.
pub fn rows(model: &Model, lang: Lang) -> Vec<Row> {
    let text = lang.text();
    let c = &model.class;
    let mut rows: Vec<Row> = Vec::new();
    let mut y = 0.0;
    let emit = |rows: &mut Vec<Row>, y: &mut f64, mut r: Row| {
        r.y = *y;
        *y += r.h;
        rows.push(r);
    };
    emit(
        &mut rows,
        &mut y,
        row(
            "name",
            string(get(c, "name")).unwrap_or_default(),
            0.0,
            20.0,
        ),
    );
    let meta: Vec<String> = [
        get(c, "level")
            .filter(|v| **v != Value::Nil)
            .map(|level| format!("{}{}", text.level, number_text(level))),
        model
            .package_label()
            .map(|p| format!("{}{p}", text.package)),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !meta.is_empty() {
        emit(&mut rows, &mut y, row("muted", meta.join(" · "), 0.0, 18.0));
    }
    if let Some(stereo) = get(c, "stereotype") {
        emit(
            &mut rows,
            &mut y,
            row(
                "muted",
                format!("«{}»", local_name(stereo).unwrap_or_default().to_owned()),
                0.0,
                18.0,
            ),
        );
    }
    let base = model
        .file
        .as_ref()
        .and_then(|f| f.rsplit('/').next().map(String::from));
    let drill = edn::record::flag(c, "drill?");
    let module = match &base {
        Some(b) if !drill && b.to_lowercase().ends_with(".rs") => Some(b.clone()),
        _ => model.ns.clone(),
    };
    if let Some(module) = module {
        let mut r = row("module", module, 0.0, 18.0);
        r.module = true;
        emit(&mut rows, &mut y, r);
    }
    let crap = get(c, "crap").filter(|v| **v != Value::Nil);
    if let Some(crap) = crap {
        let line = match num(crap) {
            Some(n) => strings::CRAP_SINGLE.replace("%s", &one(n)),
            None => {
                let f = |k: &str| get(crap, k).and_then(num).map(one).unwrap_or_default();
                strings::CRAP_LINE
                    .replacen("%s", &f("mu"), 1)
                    .replacen("%s", &f("max"), 1)
                    .replacen("%s", &f("sigma"), 1)
            }
        };
        emit(&mut rows, &mut y, row("crap", line, 0.0, 18.0));
    }
    let ops = seq(get(c, "ops"));
    let has = |k: &str| get(c, k).is_some_and(|v| *v != Value::Nil);
    if !ops.is_empty()
        || mu(crap).is_some()
        || has("coverage")
        || has("cc")
        || has("killed")
        || has("survived")
    {
        y += 6.0;
        emit(&mut rows, &mut y, row("group-header", "", 0.0, 18.0));
        emit(&mut rows, &mut y, row("col-header", "", 0.0, 18.0));
        let mut stats = row(
            "stats",
            string(get(c, "name")).unwrap_or_default(),
            0.0,
            18.0,
        );
        stats.cells = cells(c, true, lang);
        emit(&mut rows, &mut y, stats);
        for op in ops {
            let private = edn::record::is_true(op, "private");
            let signature = format!(
                "{}{}",
                if private { "- " } else { "+ " },
                string(get(op, "text")).unwrap_or_default()
            );
            let mut r = row("stats", signature, 0.0, 18.0);
            r.cells = cells(op, false, lang);
            r.private = private;
            r.op_name = string(get(op, "name"));
            emit(&mut rows, &mut y, r);
        }
    }
    let fields = seq(get(c, "fields"));
    if !fields.is_empty() {
        y += 12.0;
        emit(&mut rows, &mut y, row("heading", text.fields, 0.0, 18.0));
        for f in fields {
            let field = match f {
                Value::String(s) => s.clone(),
                other => string(get(other, "text")).unwrap_or_default(),
            };
            emit(&mut rows, &mut y, row("field", field, 0.0, 18.0));
        }
    }
    if !model.rels.is_empty() {
        y += 12.0;
        emit(
            &mut rows,
            &mut y,
            row("heading", text.relationships, 0.0, 18.0),
        );
        for rel in &model.rels {
            let label = rel
                .label
                .as_ref()
                .map(|l| format!("  «{l}»"))
                .unwrap_or_default();
            let mut r = row(
                "rel",
                format!("{}  {}{label}", phrase(rel, lang), rel.name),
                0.0,
                18.0,
            );
            r.id = Some(rel.id.clone());
            emit(&mut rows, &mut y, r);
        }
    }
    rows
}

pub fn content_h(rows: &[Row]) -> f64 {
    rows.last().map_or(32.0, |r| r.y + r.h + 16.0)
}

pub fn max_scroll(rows: &[Row], view_h: f64) -> f64 {
    (content_h(rows) - view_h).max(0.0)
}

/// Clamped scroll offset after `delta` notches.
pub fn scroll(rows: &[Row], view_h: f64, offset: f64, delta: f64) -> f64 {
    (offset + delta * SCROLL_STEP).clamp(0.0, max_scroll(rows, view_h))
}

/// A wheel over the panel's class card: `delta` notches on the open card (or
/// the selected class's), clamped by its rows and the viewport height the
/// panel reported. Row heights do not depend on the language, only their text.
pub fn scroll_card(state: &ViewState, delta: f64, view_h: f64) -> ViewState {
    let id = state
        .detail_id
        .clone()
        .or_else(|| state.selected.as_ref().and_then(|s| get(s, "id").cloned()));
    let rows = id
        .and_then(|id| for_state(state, &id))
        .map(|m| rows(&m, Lang::default()))
        .unwrap_or_default();
    let mut next = state.clone();
    next.detail_scroll = scroll(&rows, view_h, state.detail_scroll, delta);
    next
}

/// The hovered card row: an operation, relationship, or the module row.
#[derive(Debug, Clone, PartialEq)]
pub enum Hover {
    Module,
    Op(String),
    Rel(Value),
}

pub fn highlight<'a>(rows: &'a [Row], hover: Option<&Hover>) -> Option<&'a Row> {
    let hover = hover?;
    rows.iter().find(|r| match hover {
        Hover::Module => r.module,
        Hover::Op(n) => r.op_name.as_deref() == Some(n.as_str()),
        Hover::Rel(id) => r.kind == "rel" && r.id.as_ref() == Some(id),
    })
}

/// The shared row marker for the currently hovered card row. Leaving the row
/// removes the description instead of retaining the last hover.
pub fn highlight_marker(rows: &[Row], hover: Option<&Hover>) -> Option<RowMarker> {
    highlight(rows, hover).map(|_| crate::theme::row_marker())
}

fn at(rows: &[Row], y: f64) -> impl Iterator<Item = &Row> {
    rows.iter()
        .filter(move |r| y >= r.y && y <= r.y + r.h - 1.0)
}

pub fn rel_at(rows: &[Row], y: f64) -> Option<Value> {
    at(rows, y)
        .find(|r| r.kind == "rel")
        .and_then(|r| r.id.clone())
}

pub fn member_at(rows: &[Row], y: f64) -> Option<String> {
    at(rows, y).find_map(|r| r.op_name.clone())
}

pub fn module_at(rows: &[Row], y: f64) -> bool {
    at(rows, y).any(|r| r.module)
}

/// A metric column in content space.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub id: &'static str,
    pub label: &'static str,
    pub left: f64,
    pub right: f64,
}

const COLUMN_WIDTHS: [(&str, f64); 6] = [
    ("crap", 56.0),
    ("cc", 32.0),
    ("cov", 44.0),
    ("killed", 52.0),
    ("survived", 60.0),
    ("uncovered", 78.0),
];

/// Right-anchored columns; `width` defaults to 624 and never goes below 560
/// (narrower windows scroll horizontally).
pub fn column_layout(width: Option<f64>) -> Vec<Column> {
    let mut x = width.unwrap_or(624.0).max(560.0);
    let mut out = Vec::new();
    for (i, (id, w)) in COLUMN_WIDTHS.iter().enumerate().rev() {
        out.push(Column {
            id,
            label: strings::COLUMNS[i].1,
            left: x - w,
            right: x,
        });
        x -= w + 10.0;
    }
    out.reverse();
    out
}

pub fn group_layout(width: Option<f64>) -> [Column; 2] {
    let cols = column_layout(width);
    [
        Column {
            id: "crap",
            label: strings::GROUP_CRAP,
            left: cols[0].left,
            right: cols[2].right,
        },
        Column {
            id: "mutation",
            label: strings::GROUP_MUTATION,
            left: cols[3].left,
            right: cols[5].right,
        },
    ]
}

/// A painted cell: right-aligned unless `center`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub id: &'static str,
    pub text: String,
    pub left: f64,
    pub right: f64,
    pub center: bool,
    pub color: Role,
    pub tip: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub id: &'static str,
    pub left: f64,
    pub right: f64,
    pub fraction: f64,
    pub color: Role,
}

/// One painted card row.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: &'static str,
    pub y: f64,
    pub h: f64,
    pub text: Option<String>,
    pub color: Role,
    pub title_font: bool,
    pub x: f64,
    pub clip_right: Option<f64>,
    pub cells: Vec<Cell>,
    pub bars: Vec<Bar>,
    pub stripe: bool,
    pub swatch: Option<Role>,
    pub clickable: bool,
    pub module: bool,
    pub op_name: Option<String>,
    pub rel: Option<Value>,
}

pub fn row_color(row: &Row) -> Role {
    if row.private {
        return Role::Muted;
    }
    match row.kind {
        "crap" | "heading" | "group-header" | "col-header" => Role::Accent,
        "name" | "module" | "field" | "rel" | "stats" => Role::Ink,
        _ if row.module => Role::Ink,
        _ => Role::Muted,
    }
}

fn rating(c: &Cells) -> scene::MetricRating {
    scene::metric_rating(c.crap_n, c.killed, c.survived, c.uncovered)
}

pub fn cell_color(c: &Cells, id: &str) -> Role {
    let positive = |v: Option<f64>| {
        if v.unwrap_or(0.0) > 0.0 {
            Role::Violation
        } else {
            Role::Good
        }
    };
    match id {
        "cov" => Role::Coverage(c.coverage),
        "crap" => Role::QualityStroke(Some(rating(c).crap_grade)),
        "killed" => Role::Marker,
        "survived" => positive(c.survived),
        "uncovered" => positive(c.uncovered),
        _ => Role::Muted,
    }
}

/// Averaged CRAP / mutation grade; a missing metric enters as grade 1.
pub fn swatch(c: &Cells) -> Role {
    Role::QualityStroke(rating(c).grade)
}

fn cell_text(c: &Cells, id: &str) -> Option<String> {
    match id {
        "crap" => c.crap_s.clone(),
        "cc" => c.cc_s.clone(),
        "cov" => c.cov_s.clone(),
        "killed" => c.killed_s.clone(),
        "survived" => c.survived_s.clone(),
        "uncovered" => c.uncovered_s.clone(),
        _ => None,
    }
}

fn tip(id: &str, lang: Lang) -> Option<&'static str> {
    lang.text()
        .column_tips
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, t)| *t)
}

/// The paint plan for `rows` at a content width (points), with the column
/// header and tips in `lang`.
pub fn view(rows: &[Row], width: Option<f64>, lang: Lang) -> Vec<Item> {
    let cols = column_layout(width);
    let groups = group_layout(width);
    let member_right = cols[0].left - 10.0;
    let mut stats = 0;
    rows.iter()
        .map(|r| {
            let mut it = Item {
                kind: r.kind,
                y: r.y,
                h: r.h,
                text: Some(r.text.clone()),
                color: row_color(r),
                title_font: r.kind == "name",
                x: PAD,
                clip_right: matches!(r.kind, "stats" | "col-header").then_some(member_right),
                cells: Vec::new(),
                bars: Vec::new(),
                stripe: false,
                swatch: None,
                clickable: r.module || r.op_name.is_some() || r.kind == "rel",
                module: r.module,
                op_name: r.op_name.clone(),
                rel: r.id.clone(),
            };
            match r.kind {
                "group-header" => {
                    it.text = None;
                    for g in &groups {
                        it.cells.push(Cell {
                            id: g.id,
                            text: g.label.into(),
                            left: g.left,
                            right: g.right,
                            center: true,
                            color: Role::Accent,
                            tip: None,
                        });
                    }
                }
                "col-header" => {
                    it.text = Some(lang.text().member.into());
                    for c in &cols {
                        it.cells.push(Cell {
                            id: c.id,
                            text: c.label.into(),
                            left: c.left,
                            right: c.right,
                            center: false,
                            color: Role::Accent,
                            tip: tip(c.id, lang),
                        });
                    }
                }
                "stats" => {
                    stats += 1;
                    it.stripe = stats % 2 == 0;
                    it.swatch = Some(swatch(&r.cells));
                    it.x = PAD + SWATCH_W + SWATCH_GAP;
                    for (i, c) in cols.iter().enumerate() {
                        if (i < 3 || r.cells.mut_note.is_none())
                            && let Some(text) = cell_text(&r.cells, c.id)
                        {
                            it.cells.push(Cell {
                                id: c.id,
                                text,
                                left: c.left,
                                right: c.right,
                                center: false,
                                color: cell_color(&r.cells, c.id),
                                tip: None,
                            });
                        }
                    }
                    if let Some(note) = &r.cells.mut_note {
                        it.cells.push(Cell {
                            id: "mut-note",
                            text: note.clone(),
                            left: groups[1].left,
                            right: groups[1].right,
                            center: true,
                            color: Role::Muted,
                            tip: None,
                        });
                    } else {
                        let m = rating(&r.cells);
                        if let Some(ratio) = m.kill_ratio {
                            it.bars.push(Bar {
                                id: "kill",
                                left: groups[1].left,
                                right: groups[1].right,
                                fraction: ratio,
                                color: Role::QualityStroke(Some(m.mutation_grade)),
                            });
                        }
                    }
                    if let Some(cov) = r.cells.coverage {
                        it.bars.push(Bar {
                            id: "cov",
                            left: cols[2].left,
                            right: cols[2].right,
                            fraction: cov,
                            color: Role::Coverage(Some(cov)),
                        });
                    }
                }
                _ => {}
            }
            it
        })
        .collect()
}
