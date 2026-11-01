//! egui adapter for the inspector sidebar, the class card window and the
//! source windows (#141). It paints the pure models (`inspector`, `detail`,
//! `source`) and returns [`Action`]s; the app applies them to the view
//! state, so every state change still goes through `scene::events`.
//!
//! Widget bounds are recorded per frame in view units (points from the
//! content origin) under the keys the LOVE drivers used (`proposal:<id>`,
//! `rename-menu`, `proposal-name`, `new-proposal`, `declutter`, `regen`,
//! `module`, `member:<op>`, ...), so a driver points at what was drawn.

use std::collections::BTreeMap;
use std::path::Path;

use edn::Value;
use edn::record::{get, local_name, num};
use egui::{
    Align2, Color32, CursorIcon, FontFamily, FontId, Id, Pos2, Rect as ERect, RichText, Sense, Vec2,
};
use scene::Rect;
use scene::document::ViewState;
use scene::events;

use crate::detail::{self, Hover};
use crate::inspector;
use crate::source::{self, Ident};
use crate::strings;
use crate::theme::{self, Rgb};

/// Height of the Cov / kill-rate bars, points.
const BAR: f32 = 3.0;
const SOURCE_SIZE: [f32; 2] = [900.0, 640.0];
const GUTTER: f32 = 44.0;
const INSET: f32 = 6.0;

/// What the user asked for this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Choose(Option<Value>),
    NewProposal,
    Declutter,
    Regen,
    Rename(Value, String),
    Delete(Value),
    CloseCard,
    Locate(Value),
    OpenSource(Ident),
    /// A right-click menu operation (`strings::MENU`): write the mail and
    /// wake the companion (#142).
    Mail(String),
}

/// The class card window as last drawn.
#[derive(Debug, Clone, Default)]
pub struct Card {
    pub open: bool,
    pub hovered: bool,
    pub view_h: f64,
    pub scroll: f64,
    pub hover: Option<Hover>,
    pub rect: Option<Rect>,
    pub h_max: f64,
    pub h_offset: f64,
    pub content_w: f64,
    pub row_heights: Vec<f64>,
    /// Forced window size and the frames it is still held.
    pub force: Option<(Vec2, u8)>,
}

/// One source window: its own text, scroll, selection and egui identity.
#[derive(Debug, Clone)]
pub struct SourceWindow {
    pub serial: u64,
    pub title: String,
    pub file: String,
    pub line: Option<usize>,
    pub lang: String,
    pub rows: Vec<source::Row>,
    displays: Vec<source::Display>,
    texts: Vec<String>,
    max_w: Option<f32>,
    pending: bool,
    anchor: Option<source::Pos>,
    caret: Option<source::Pos>,
    pub open: bool,
    pub scroll_y: f64,
    pub target_visible: bool,
    pub selection: String,
}

#[derive(Debug, Clone)]
struct Rename {
    id: Value,
    name: String,
    focus: bool,
}

type CardKey = (u64, u64, Value, Vec<Value>, Value);

#[derive(Default)]
pub struct Gui {
    /// Inspector width (points); clamped to the window every frame.
    pub width: Option<f64>,
    dragging_edge: bool,
    menu: Option<(Value, Pos2, u64)>,
    menu_open: bool,
    /// The element mail menu (right-click on the canvas).
    element_menu: Option<(Pos2, u64)>,
    element_menu_open: bool,
    rename: Option<Rename>,
    pub bounds: BTreeMap<String, Rect>,
    empty_help: bool,
    pub card: Card,
    cache: Option<(CardKey, detail::Model, Vec<detail::Row>)>,
    entries_key: Option<(u64, u64)>,
    entries_cache: Vec<inspector::Entry>,
    generation: u64,
    pub sources: Vec<SourceWindow>,
    serial: u64,
    focused_source: Option<u64>,
    pub copied: Option<String>,
    /// Floating window rects this frame (view units).
    areas: Vec<Rect>,
    window_w: f64,
}

fn rgb(c: Rgb) -> Color32 {
    c.color32()
}

fn paint_row_marker(painter: &egui::Painter, row: ERect, marker: theme::RowMarker) {
    painter.rect_filled(row, 0.0, marker.wash);
    painter.rect_filled(
        ERect::from_min_size(row.min, Vec2::new(marker.bar_width, row.height())),
        0.0,
        marker.bar,
    );
}

fn view_rect(r: ERect, origin: Pos2) -> Rect {
    Rect::new(
        (r.min.x - origin.x) as f64,
        (r.min.y - origin.y) as f64,
        r.width() as f64,
        r.height() as f64,
    )
}

fn contains(r: &Rect, x: f64, y: f64) -> bool {
    x >= r.x && x <= r.right() && y >= r.y && y <= r.bottom()
}

fn code_font() -> FontId {
    FontId::new(theme::fonts::CODE, FontFamily::Monospace)
}

fn body_font() -> FontId {
    FontId::new(theme::fonts::BODY, FontFamily::Monospace)
}

fn text_w(ctx: &egui::Context, font: &FontId, text: &str) -> f32 {
    ctx.fonts_mut(|f| {
        f.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
    })
}

fn one(n: f64) -> String {
    format!("{n:.1}")
}

fn themed_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let visuals = &mut ui.visuals_mut().widgets;
    visuals.hovered.weak_bg_fill = rgb(theme::BUTTON_HOVER);
    visuals.active.weak_bg_fill = rgb(theme::BUTTON_ACTIVE);
    let button = egui::Button::new(
        RichText::new(label)
            .color(rgb(theme::GOLD))
            .font(body_font()),
    )
    .fill(rgb(theme::LINE))
    .min_size(Vec2::new(ui.available_width(), 0.0));
    ui.add(button)
}

/// Popup menu items as whole rows: each spans the menu width and lights up
/// on hover, so the hit and highlight area is the row, not the label box.
fn menu_rows<'a>(
    ui: &mut egui::Ui,
    labels: impl IntoIterator<Item = &'a str>,
) -> Vec<egui::Response> {
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
        labels
            .into_iter()
            .map(|label| ui.add(egui::Button::new(label).frame_when_inactive(false)))
            .collect()
    })
    .inner
}

impl Gui {
    pub fn inspector_width(&self) -> f64 {
        inspector::clamp_width(self.width, self.window_w)
    }

    pub fn canvas_width(&self, window_w: f64) -> f64 {
        inspector::canvas_width(window_w, inspector::clamp_width(self.width, window_w))
    }

    /// Does the GUI own this point (sidebar, its edge grip, a window, menu)?
    /// Record a widget drawn outside the GUI (the window controls) under
    /// `key`, so drivers can point at it. `draw` clears bounds each frame.
    pub fn record_bound(&mut self, key: &str, rect: ERect, origin: Pos2) {
        self.bounds.insert(key.to_string(), view_rect(rect, origin));
    }

    pub fn captures(&self, x: f64, y: f64, window_w: f64) -> bool {
        let width = inspector::clamp_width(self.width, window_w);
        x >= inspector::canvas_width(window_w, width) - inspector::EDGE_GRIP
            || self.areas.iter().any(|r| contains(r, x, y))
    }

    pub fn card_hovered(&self) -> bool {
        self.card.open && self.card.hovered
    }

    /// Forget cached card models (the document changed in place).
    pub fn invalidate(&mut self) {
        self.generation += 1;
    }

    pub fn menu_open(&self) -> bool {
        self.menu_open
    }

    /// Open the source window for a file member (the driver's
    /// `:open-source`): `name` is the member, `None` the whole file.
    pub fn open_source_file(
        &mut self,
        lang: &str,
        file: &str,
        name: Option<&str>,
        root: &Path,
    ) -> Result<(), String> {
        let ident = Ident {
            lang: Some(lang.into()),
            file: Some(file.into()),
            ns: None,
            name: name.map(Into::into),
        };
        self.open_source(&ident, root)
    }

    /// Open a source window; `Err` carries the reason for the status line.
    pub fn open_source(&mut self, ident: &Ident, root: &Path) -> Result<(), String> {
        let files = source::Files {
            root: Some(root.to_string_lossy().into_owned()),
            read: &|p| std::fs::read_to_string(p).ok(),
            is_file: &|p| Path::new(p).is_file(),
        };
        let opened = source::member_source(ident, &files).ok_or("Source not found")?;
        let rows = source::rows(&opened.body, opened.line);
        let displays: Vec<_> = rows.iter().map(|r| source::display(&r.text)).collect();
        self.serial += 1;
        self.sources.push(SourceWindow {
            serial: self.serial,
            title: opened.title,
            file: opened.file,
            line: opened.line,
            lang: opened.lang.unwrap_or_default(),
            texts: rows.iter().map(|r| r.text.clone()).collect(),
            rows,
            displays,
            max_w: None,
            pending: true,
            anchor: None,
            caret: None,
            open: true,
            scroll_y: 0.0,
            target_visible: false,
            selection: String::new(),
        });
        self.focused_source = Some(self.serial);
        Ok(())
    }

    /// Close the newest source window (the driver's `ui.source_window.close`).
    pub fn close_newest_source(&mut self) {
        self.sources.pop();
    }

    /// The diagram list, cached on the state revision: recomputing it walks
    /// and clones the whole document (10 ms on 2000 classes, every frame).
    fn entries(&mut self, state: &ViewState) -> Vec<inspector::Entry> {
        let key = (state.revision, self.generation);
        if self.entries_key != Some(key) {
            self.entries_cache = inspector::entries(state);
            self.entries_key = Some(key);
        }
        self.entries_cache.clone()
    }

    /// The diagram list plus whether this call had to rebuild it.
    pub fn entries_for(&mut self, state: &ViewState) -> (Vec<inspector::Entry>, bool) {
        let key = (state.revision, self.generation);
        let rebuilt = self.entries_key != Some(key);
        (self.entries(state), rebuilt)
    }

    /// `entries` for the read-only record.
    fn cached_entries(&self, state: &ViewState) -> Vec<inspector::Entry> {
        let key = (state.revision, self.generation);
        if self.entries_key == Some(key) {
            self.entries_cache.clone()
        } else {
            inspector::entries(state)
        }
    }

    /// The rows of the class card open (or selected) in this view.
    pub(crate) fn card_rows(&mut self, state: &ViewState) -> Vec<detail::Row> {
        inspector::card_id(state)
            .and_then(|id| self.card_for(state, &id))
            .map(|(_, rows)| rows)
            .unwrap_or_default()
    }

    /// The cached card model and rows of class `id` in this view.
    pub(crate) fn card_for(
        &mut self,
        state: &ViewState,
        id: &Value,
    ) -> Option<(detail::Model, Vec<detail::Row>)> {
        let key: CardKey = (
            state.revision,
            self.generation,
            id.clone(),
            state.focus.clone(),
            state.extra.clone(),
        );
        if let Some((k, m, r)) = &self.cache
            && *k == key
        {
            return Some((m.clone(), r.clone()));
        }
        let model = detail::for_state(state, id)?;
        let rows = detail::rows(&model);
        self.cache = Some((key, model.clone(), rows.clone()));
        Some((model, rows))
    }

    /// Draw everything for this frame; returns the requested actions.
    pub fn draw(&mut self, ctx: &egui::Context, state: &ViewState, frame: u64) -> Vec<Action> {
        let screen = ctx.content_rect();
        let origin = screen.min;
        self.window_w = screen.width() as f64;
        self.bounds.clear();
        self.areas.clear();
        let mut actions = Vec::new();
        if state.document.is_some() && !state.waiting {
            self.edge(ctx, screen);
            self.sidebar(ctx, state, screen, frame, &mut actions);
            let canvas = ERect::from_min_size(
                origin,
                Vec2::new(self.canvas_width(self.window_w) as f32, screen.height()),
            );
            self.card_window(ctx, state, canvas, &mut actions);
            self.source_windows(ctx, canvas);
        } else {
            self.card.open = false;
        }
        self.element_mail_menu(ctx, state, origin, &mut actions);
        actions
    }

    /// Right-click on a class, child row or proposal package opens the element
    /// menu (`mail_menu.entries`): refresh CRAP, refresh mutation, full
    /// mutation, hide. Each item writes a `to-agent.edn` command and wakes the
    /// companion for that delivery.
    fn element_mail_menu(
        &mut self,
        ctx: &egui::Context,
        state: &ViewState,
        origin: Pos2,
        actions: &mut Vec<Action>,
    ) {
        // A real right click on the canvas opens the menu; a replayed one is
        // opened by the app at the press (`open_element_menu`).
        let (secondary, pos) = ctx.input(|i| {
            (
                i.pointer.secondary_clicked(),
                i.pointer.interact_pos().or(i.pointer.hover_pos()),
            )
        });
        if secondary
            && let Some(at) = pos
            && at.x < origin.x + self.canvas_width(self.window_w) as f32
            && events::element_menu(state.selected.as_ref())
        {
            self.element_menu = Some((at, 0));
        }
        self.element_menu_open = false;
        let Some((at, _)) = self.element_menu else {
            return;
        };
        if state.waiting || state.document.is_none() {
            self.element_menu = None;
            return;
        }
        let response = egui::Area::new(Id::unique("uml-element-mail"))
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .show(ui, |ui| {
                        let labels = strings::MENU.iter().map(|(_, label)| *label);
                        menu_rows(ui, labels)
                            .into_iter()
                            .zip(strings::MENU)
                            .map(|(row, (op, _))| (row, format!("menu-{op}")))
                            .collect::<Vec<_>>()
                    })
                    .inner
            });
        let mut bounds = Vec::new();
        for (button, key) in &response.inner {
            bounds.push((key.clone(), button.rect));
            if button.clicked() {
                let op = key.trim_start_matches("menu-").to_string();
                actions.push(Action::Mail(op));
                self.element_menu = None;
            }
        }
        self.areas.push(view_rect(response.response.rect, origin));
        for (key, r) in bounds {
            self.bounds.insert(key, view_rect(r, origin));
        }
        self.element_menu_open = self.element_menu.is_some();
        let rect = response.response.rect;
        if ctx.input(|i| i.pointer.any_pressed())
            && !ctx
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| rect.contains(p))
            && !secondary
        {
            self.element_menu = None;
            self.element_menu_open = false;
        }
    }

    /// Open the element mail menu at a screen point (the app's right click).
    pub fn open_element_menu(&mut self, at: Pos2) {
        self.element_menu = Some((at, 0));
    }

    /// Is the element mail menu open (the `:element-menu` GUI record)?
    pub fn element_menu_open(&self) -> bool {
        self.element_menu_open
    }

    /// The draggable left edge of the sidebar.
    fn edge(&mut self, ctx: &egui::Context, screen: ERect) {
        let window_w = screen.width() as f64;
        let width = inspector::clamp_width(self.width, window_w);
        let (pos, pressed, down) = ctx.input(|i| {
            (
                i.pointer.hover_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
            )
        });
        let Some(pos) = pos else {
            self.dragging_edge &= down;
            return;
        };
        let x = (pos.x - screen.min.x) as f64;
        let over = inspector::on_edge(x, window_w, width);
        if pressed && over && self.menu.is_none() {
            self.dragging_edge = true;
        }
        if self.dragging_edge {
            self.width = Some(inspector::drag_width(x, window_w));
            if !down {
                self.dragging_edge = false;
            }
        }
        if over || self.dragging_edge {
            ctx.set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
    }

    fn sidebar(
        &mut self,
        ctx: &egui::Context,
        state: &ViewState,
        screen: ERect,
        frame: u64,
        actions: &mut Vec<Action>,
    ) {
        let origin = screen.min;
        let width = inspector::clamp_width(self.width, screen.width() as f64) as f32;
        let left = screen.max.x - width;
        let mut bounds: Vec<(String, ERect)> = Vec::new();
        let entries = self.entries(state);
        let mut summary_id = state.selected.as_ref().and_then(|s| get(s, "id").cloned());
        let summary = summary_id.take().and_then(|id| {
            let kind = state
                .selected
                .as_ref()
                .and_then(|s| get(s, "kind"))
                .map(|v| local_name(v).unwrap_or_default().to_owned());
            self.card_for(state, &id)
                .map(|c| (id.clone(), Some(c), kind.clone()))
                .or(Some((id, None, kind)))
        });
        let mut menu_request = None;
        let mut empty_help = false;
        egui::Area::new(Id::unique("uml-inspector"))
            .fixed_pos(Pos2::new(left, screen.min.y))
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(rgb(theme::SIDEBAR))
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.set_width(width - 32.0);
                        ui.set_height(screen.height() - 32.0);
                        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                        ui.label(
                            RichText::new(strings::INSPECTOR)
                                .color(rgb(theme::GOLD))
                                .font(FontId::new(theme::fonts::TITLE, FontFamily::Monospace)),
                        );
                        for (n, entry) in entries.iter().enumerate() {
                            if n == 1 {
                                ui.label(
                                    RichText::new(strings::PROPOSALS_LABEL)
                                        .color(rgb(theme::MUTED)),
                                );
                            }
                            let text = RichText::new(&entry.label)
                                .font(body_font())
                                .color(rgb(theme::INK));
                            let row = ui.add(
                                egui::Button::selectable(entry.active, text)
                                    .min_size(Vec2::new(ui.available_width(), 0.0)),
                            );
                            if row.clicked() {
                                actions.push(Action::Choose(entry.id.clone()));
                            }
                            if let Some(id) = &entry.id
                                && row.secondary_clicked()
                            {
                                let at = ctx
                                    .input(|i| i.pointer.interact_pos())
                                    .unwrap_or(row.rect.center());
                                menu_request = Some((id.clone(), at));
                            }
                            let key = entry.id.as_ref().map_or("real".to_string(), |id| {
                                format!(
                                    "proposal:{}",
                                    local_name(id).unwrap_or_default().to_owned()
                                )
                            });
                            bounds.push((key, row.rect));
                        }
                        let new = themed_button(ui, strings::NEW_PROPOSAL);
                        if new.clicked() {
                            actions.push(Action::NewProposal);
                        }
                        bounds.push(("new-proposal".into(), new.rect));
                        let declutter = themed_button(ui, inspector::declutter_label(state));
                        if declutter.clicked() {
                            actions.push(Action::Declutter);
                        }
                        bounds.push(("declutter".into(), declutter.rect));
                        if let Some(rename) = &mut self.rename {
                            ui.separator();
                            ui.label(
                                RichText::new(strings::PROPOSAL_NAME).color(rgb(theme::MUTED)),
                            );
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut rename.name)
                                    .id(Id::unique("uml-proposal-name"))
                                    .font(body_font())
                                    .desired_width(f32::INFINITY),
                            );
                            if rename.focus {
                                field.request_focus();
                                rename.focus = false;
                            }
                            bounds.push(("proposal-name".into(), field.rect));
                            let ok = ui.button(strings::RENAME_PROPOSAL);
                            bounds.push(("rename-proposal".into(), ok.rect));
                            let cancel = ui.button(strings::CANCEL);
                            bounds.push(("cancel-rename".into(), cancel.rect));
                            if ok.clicked() {
                                actions
                                    .push(Action::Rename(rename.id.clone(), rename.name.clone()));
                            }
                            if ok.clicked() || cancel.clicked() {
                                self.rename = None;
                            }
                        }
                        ui.label(RichText::new(strings::P_HINT).color(rgb(theme::MUTED)));
                        ui.separator();
                        let summary_h = (ui.available_height() - 44.0).max(40.0);
                        let card = egui::ScrollArea::vertical()
                            .id_salt("uml-summary")
                            .max_height(summary_h)
                            .auto_shrink([false, true])
                            .show(ui, |ui| match &summary {
                                Some((_, Some((model, _)), _)) => class_summary(ui, model),
                                Some((id, None, Some(kind))) if kind == "package" => {
                                    package_summary(ui, state, id)
                                }
                                _ => {
                                    let message = inspector::empty_explanation(state)
                                        .unwrap_or(strings::SELECT_CLASS);
                                    ui.label(RichText::new(message).color(rgb(theme::MUTED)));
                                    empty_help = true;
                                }
                            });
                        bounds.push(("summary-card".into(), card.inner_rect));
                        ui.add_space((ui.available_height() - 34.0).max(0.0));
                        let regen = themed_button(ui, strings::REGEN);
                        if regen.clicked() {
                            actions.push(Action::Regen);
                        }
                        bounds.push(("regen".into(), regen.rect));
                    });
            });
        bounds.push((
            "inspector-window".into(),
            ERect::from_min_max(Pos2::new(left, screen.min.y), screen.max),
        ));
        self.empty_help = empty_help;
        if let Some((id, at)) = menu_request {
            self.menu = Some((id, at, frame));
        }
        // The proposal menu: Rename / Delete.
        self.menu_open = false;
        if let Some((id, at, opened)) = self.menu.clone() {
            let response = egui::Area::new(Id::unique("uml-proposal-menu"))
                .fixed_pos(at)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style())
                        .show(ui, |ui| {
                            let mut rows = menu_rows(ui, [strings::RENAME, strings::DELETE]);
                            let delete = rows.pop().expect("two rows");
                            let rename = rows.pop().expect("two rows");
                            (rename, delete)
                        })
                        .inner
                });
            let (rename, delete) = response.inner;
            bounds.push(("rename-menu".into(), rename.rect));
            bounds.push(("delete-menu".into(), delete.rect));
            self.areas.push(view_rect(response.response.rect, origin));
            self.menu_open = true;
            if rename.clicked() {
                let label = entries
                    .iter()
                    .find(|e| e.id.as_ref() == Some(&id))
                    .map(|e| e.label.clone())
                    .unwrap_or_default();
                self.rename = Some(Rename {
                    id: id.clone(),
                    name: label,
                    focus: true,
                });
                self.menu = None;
            } else if delete.clicked() {
                actions.push(Action::Delete(id));
                self.menu = None;
            } else if frame > opened
                && ctx.input(|i| i.pointer.any_pressed())
                && !ctx
                    .input(|i| i.pointer.interact_pos())
                    .is_some_and(|p| response.response.rect.contains(p))
            {
                self.menu = None;
            }
        }
        for (key, r) in bounds {
            self.bounds.insert(key, view_rect(r, origin));
        }
    }

    fn card_window(
        &mut self,
        ctx: &egui::Context,
        state: &ViewState,
        canvas: ERect,
        actions: &mut Vec<Action>,
    ) {
        let origin = canvas.min;
        let Some(id) = state.detail_id.clone() else {
            self.card = Card {
                force: self.card.force,
                ..Card::default()
            };
            return;
        };
        let Some((model, rows)) = self.card_for(state, &id) else {
            self.card.open = false;
            return;
        };
        let title = get(&model.class, "name")
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => local_name(other).unwrap_or_default().to_owned(),
            })
            .or(model.ns.clone())
            .unwrap_or_default();
        let mut open = true;
        let mut window = egui::Window::new(title)
            .id(Id::unique("uml-detail"))
            .open(&mut open)
            .default_pos(origin + Vec2::new(48.0, 80.0))
            .default_size([detail::WIDTH as f32, detail::HEIGHT as f32])
            .collapsible(false)
            .constrain_to(canvas)
            .max_size(canvas.size())
            .frame(egui::Frame::window(&ctx.global_style()).fill(rgb(theme::CANVAS_BG)));
        if let Some((size, frames)) = self.card.force {
            window = window.min_size(size).max_size(size);
            self.card.force = (frames > 1).then_some((size, frames - 1));
        }
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let scroll = state.detail_scroll;
        let mut bounds: Vec<(String, ERect)> = Vec::new();
        let mut card = Card {
            open: true,
            force: self.card.force,
            ..Card::default()
        };
        let shown = window.show(ctx, |ui| {
            let plan_w = (ui.available_width() as f64).max(560.0);
            let view_h = ui.available_height().max(1.0);
            let items = detail::view(&rows, Some(plan_w));
            let out = egui::ScrollArea::horizontal()
                .id_salt("uml-card-h")
                .auto_shrink([false, false])
                .scroll_source(egui::containers::scroll_area::ScrollSource {
                    mouse_wheel: false,
                    ..Default::default()
                })
                .show(ui, |ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(plan_w as f32, view_h), Sense::hover());
                    let clip = ui.clip_rect().intersect(rect);
                    let painter = ui.painter_at(clip);
                    let scroll = scroll.clamp(0.0, detail::max_scroll(&rows, rect.height() as f64));
                    let mut hover = None;
                    let px = |x: f64| rect.min.x + x as f32;
                    for (n, it) in items.iter().enumerate() {
                        let top = rect.min.y + (it.y - scroll) as f32;
                        let h = it.h as f32;
                        let row = ERect::from_min_size(
                            Pos2::new(rect.min.x, top),
                            Vec2::new(plan_w as f32, h),
                        );
                        if it.stripe {
                            painter.rect_filled(row, 0.0, rgb(theme::PANEL));
                        }
                        if it.clickable && row.intersects(clip) {
                            let response = ui.interact(
                                row.intersect(clip),
                                ui.scope_id().with(("card-row", n)),
                                Sense::click(),
                            );
                            if response.hovered() {
                                let current = if it.module {
                                    Some(Hover::Module)
                                } else if let Some(op) = &it.op_name {
                                    Some(Hover::Op(op.clone()))
                                } else {
                                    it.rel.clone().map(Hover::Rel)
                                };
                                if let Some(marker) =
                                    detail::highlight_marker(&rows, current.as_ref())
                                {
                                    paint_row_marker(&painter, row, marker);
                                }
                                hover = current;
                            }
                            if response.clicked() {
                                actions.push(match &it.rel {
                                    Some(rel) if it.kind == "rel" => Action::Locate(rel.clone()),
                                    _ => Action::OpenSource(detail::member_ident(
                                        &model,
                                        it.op_name.as_deref(),
                                    )),
                                });
                            }
                            let key = if it.module {
                                "module".to_string()
                            } else if let Some(op) = &it.op_name {
                                format!("member:{op}")
                            } else {
                                format!(
                                    "rel:{}",
                                    it.rel
                                        .as_ref()
                                        .and_then(local_name)
                                        .unwrap_or_default()
                                        .to_owned()
                                )
                            };
                            bounds.push((key, row.intersect(clip)));
                        }
                        if let Some(swatch) = it.swatch {
                            let s = ERect::from_min_max(
                                Pos2::new(px(detail::PAD), top + 4.0),
                                Pos2::new(px(detail::PAD + detail::SWATCH_W), top + h - 4.0),
                            );
                            painter.rect_filled(s, 2.0, rgb(swatch));
                        }
                        let font = if it.title_font {
                            FontId::new(theme::fonts::TITLE, FontFamily::Monospace)
                        } else {
                            code_font()
                        };
                        if let Some(text) = &it.text {
                            let p = match it.clip_right {
                                Some(right) => painter.with_clip_rect(
                                    ERect::from_min_max(
                                        Pos2::new(rect.min.x, top),
                                        Pos2::new(px(right), top + h),
                                    )
                                    .intersect(clip),
                                ),
                                None => painter.clone(),
                            };
                            p.text(
                                Pos2::new(px(it.x), top + h / 2.0),
                                Align2::LEFT_CENTER,
                                text,
                                font.clone(),
                                rgb(it.color),
                            );
                        }
                        for c in &it.cells {
                            let (x, align) = if c.center {
                                (px((c.left + c.right) / 2.0), Align2::CENTER_CENTER)
                            } else {
                                (px(c.right), Align2::RIGHT_CENTER)
                            };
                            painter.text(
                                Pos2::new(x, top + h / 2.0),
                                align,
                                &c.text,
                                code_font(),
                                rgb(c.color),
                            );
                            if let (Some(tip), Some(p)) = (c.tip, pointer)
                                && ERect::from_min_max(
                                    Pos2::new(px(c.left), top),
                                    Pos2::new(px(c.right), top + h),
                                )
                                .contains(p)
                                && clip.contains(p)
                            {
                                egui::Tooltip::always_open(
                                    ctx.clone(),
                                    ui.layer_id(),
                                    Id::unique("uml-col-tip"),
                                    egui::PopupAnchor::Pointer,
                                )
                                .show(|ui| ui.label(tip));
                            }
                        }
                        for b in &it.bars {
                            let y = top + h - BAR;
                            let track = ERect::from_min_max(
                                Pos2::new(px(b.left), y),
                                Pos2::new(px(b.right), y + BAR),
                            );
                            painter.rect_filled(track, BAR / 2.0, rgb(theme::LINE));
                            let fill = ERect::from_min_max(
                                track.min,
                                Pos2::new(
                                    px(b.left + (b.right - b.left) * b.fraction.clamp(0.0, 1.0)),
                                    y + BAR,
                                ),
                            );
                            painter.rect_filled(fill, BAR / 2.0, rgb(b.color));
                        }
                    }
                    (scroll, hover, rect.height() as f64)
                });
            let (scroll, hover, view_h) = out.inner;
            card.scroll = scroll;
            card.hover = hover;
            card.view_h = view_h;
            card.hovered = pointer.is_some_and(|p| out.inner_rect.contains(p));
            card.h_max = (out.content_size.x - out.inner_rect.width()).max(0.0) as f64;
            card.h_offset = out.state.clamped_offset().x as f64;
            card.content_w = plan_w;
            card.row_heights = items.iter().take(2).map(|it| it.h).collect();
            bounds.push(("detail-card".into(), out.inner_rect));
        });
        if let Some(shown) = shown {
            let r = view_rect(shown.response.rect, origin);
            self.areas.push(r);
            card.rect = Some(r);
        }
        if !open {
            actions.push(Action::CloseCard);
        }
        self.card = card;
        for (key, r) in bounds {
            self.bounds.insert(key, view_rect(r, origin));
        }
    }

    fn source_windows(&mut self, ctx: &egui::Context, canvas: ERect) {
        let origin = canvas.min;
        let font = code_font();
        let line_h = ctx.fonts_mut(|f| f.row_height(&font));
        let copy = ctx.input(|i| {
            i.events.iter().any(|e| matches!(e, egui::Event::Copy))
                || (i.modifiers.command && i.key_pressed(egui::Key::C))
        });
        let editing = ctx.text_edit_focused();
        let newest = self.sources.last().map(|w| w.serial);
        let mut bounds: Vec<(String, ERect)> = Vec::new();
        let mut focus = self.focused_source;
        for w in &mut self.sources {
            let mut open = true;
            let shown = egui::Window::new(&w.title)
                .id(Id::unique(("uml-source", w.serial)))
                .open(&mut open)
                .default_size(SOURCE_SIZE)
                .default_pos(
                    origin
                        + Vec2::new(
                            80.0 + 24.0 * (w.serial % 8) as f32,
                            60.0 + 24.0 * (w.serial % 8) as f32,
                        ),
                )
                .collapsible(false)
                .constrain_to(canvas)
                .max_size(canvas.size())
                .frame(egui::Frame::window(&ctx.global_style()).fill(rgb(theme::SOURCE_BG)))
                .show(ctx, |ui| {
                    let max_w = *w.max_w.get_or_insert_with(|| {
                        w.displays
                            .iter()
                            .map(|d| text_w(ctx, &font, &d.text))
                            .fold(0.0, f32::max)
                    });
                    let avail_h = ui.available_height();
                    let mut area = egui::ScrollArea::both()
                        .id_salt(("uml-source-lines", w.serial))
                        .auto_shrink([false, false]);
                    if w.pending {
                        let target = w
                            .line
                            .map_or(0.0, |l| ((l - 1) as f32 * line_h - 0.25 * avail_h).max(0.0));
                        area = area.vertical_scroll_offset(target);
                    }
                    let total =
                        Vec2::new(GUTTER + INSET + max_w + 16.0, w.rows.len() as f32 * line_h);
                    let out = area.show_viewport(ui, |ui, viewport| {
                        let (rect, response) = ui.allocate_exact_size(
                            total.max(Vec2::new(ui.available_width(), 0.0)),
                            Sense::click_and_drag(),
                        );
                        let painter = ui.painter_at(ui.clip_rect());
                        let first = (viewport.min.y / line_h).floor().max(0.0) as usize;
                        let last =
                            ((viewport.max.y / line_h).ceil() as usize + 1).min(w.rows.len());
                        let text_x = rect.min.x + GUTTER + INSET;
                        let position = |p: Pos2| -> Option<source::Pos> {
                            let row = ((p.y - rect.min.y) / line_h).floor();
                            if row < 0.0 || row as usize >= w.rows.len() {
                                return None;
                            }
                            let row = row as usize;
                            let d = &w.displays[row];
                            let mut col = 0;
                            for (i, c) in d.text.char_indices() {
                                let l = text_w(ctx, &font, &d.text[..i]);
                                let r = text_w(ctx, &font, &d.text[..i + c.len_utf8()]);
                                if p.x < text_x + (l + r) / 2.0 {
                                    col = i;
                                    break;
                                }
                                col = i + c.len_utf8();
                            }
                            Some(source::Pos {
                                row,
                                col: d.to_source[col],
                            })
                        };
                        // egui reports drag_started only after the pointer moved
                        // past the threshold: anchor at where the press began.
                        if (response.drag_started() || response.clicked())
                            && let Some(p) = ctx.input(|i| i.pointer.press_origin())
                        {
                            w.anchor = position(p);
                            w.caret = w.anchor;
                        }
                        if response.dragged()
                            && let Some(p) = response.interact_pointer_pos()
                            && let Some(pos) = position(p)
                        {
                            w.caret = Some(pos);
                        }
                        let selection = match (w.anchor, w.caret) {
                            (Some(a), Some(b)) => Some(source::ordered(a, b)),
                            _ => None,
                        };
                        let mut target_rect = None;
                        let mut lines = Vec::new();
                        for i in first..last {
                            let row = &w.rows[i];
                            let d = &w.displays[i];
                            let top = rect.min.y + i as f32 * line_h;
                            let line = ERect::from_min_size(
                                Pos2::new(rect.min.x, top),
                                Vec2::new(rect.width(), line_h),
                            );
                            if row.marker.is_some() {
                                target_rect = Some(line);
                            }
                            painter.rect_filled(
                                ERect::from_min_size(line.min, Vec2::new(GUTTER, line_h)),
                                0.0,
                                rgb(theme::SOURCE_GUTTER_BG),
                            );
                            if let Some(marker) = row.marker {
                                paint_row_marker(
                                    &painter,
                                    ERect::from_min_max(
                                        Pos2::new(text_x, top),
                                        Pos2::new(rect.max.x, top + line_h),
                                    ),
                                    marker,
                                );
                            }
                            painter.text(
                                Pos2::new(rect.min.x + GUTTER - INSET, top),
                                Align2::RIGHT_TOP,
                                row.number.to_string(),
                                font.clone(),
                                rgb(theme::SOURCE_GUTTER),
                            );
                            if let Some((a, b)) = selection
                                && i >= a.row
                                && i <= b.row
                            {
                                let left = if i == a.row {
                                    d.to_display[a.col.min(d.to_display.len() - 1)]
                                } else {
                                    0
                                };
                                let right = if i == b.row {
                                    d.to_display[b.col.min(d.to_display.len() - 1)]
                                } else {
                                    d.text.len()
                                };
                                let lx = text_w(ctx, &font, &d.text[..left]);
                                let rx = text_w(ctx, &font, &d.text[..right.max(left)]);
                                painter.rect_filled(
                                    ERect::from_min_max(
                                        Pos2::new(text_x + lx, top),
                                        Pos2::new(text_x + rx, top + line_h),
                                    ),
                                    0.0,
                                    rgb(theme::SOURCE_SELECTION),
                                );
                            }
                            let mut x = text_x;
                            for run in source::tokens(&d.text, &w.lang) {
                                let color = match run.kind {
                                    "keyword" => theme::SOURCE_KEYWORD,
                                    "string" => theme::SOURCE_STRING,
                                    "comment" => theme::SOURCE_COMMENT,
                                    _ => theme::SOURCE_INK,
                                };
                                let galley = ctx.fonts_mut(|f| {
                                    f.layout_no_wrap(run.text.clone(), font.clone(), rgb(color))
                                });
                                let advance = galley.size().x;
                                painter.galley(Pos2::new(x, top), galley, rgb(color));
                                x += advance;
                            }
                            lines.push((
                                row.number,
                                ERect::from_min_size(
                                    Pos2::new(text_x, top),
                                    Vec2::new(rect.max.x - text_x, line_h),
                                ),
                            ));
                        }
                        (
                            target_rect,
                            lines,
                            response.is_pointer_button_down_on() || response.clicked(),
                        )
                    });
                    let (target_rect, lines, touched) = out.inner;
                    w.scroll_y = out.state.clamped_offset().y as f64;
                    w.target_visible = target_rect.is_some_and(|r| out.inner_rect.contains_rect(r));
                    if Some(w.serial) == newest {
                        for (number, r) in lines {
                            bounds.push((format!("source-line:{number}"), r));
                        }
                    }
                    touched
                });
            w.pending = false;
            if let Some(shown) = shown {
                self.areas.push(view_rect(shown.response.rect, origin));
                if shown.inner == Some(true) || shown.response.clicked() {
                    focus = Some(w.serial);
                }
            }
            w.selection = source::selected(&w.texts, w.anchor, w.caret);
            if copy && !editing && focus == Some(w.serial) && !w.selection.is_empty() {
                ctx.copy_text(w.selection.clone());
                self.copied = Some(w.selection.clone());
            }
            if !open {
                w.open = false;
            }
        }
        self.focused_source = focus;
        self.sources.retain(|w| w.open);
        for (key, r) in bounds {
            self.bounds.insert(key, view_rect(r, origin));
        }
    }

    /// The `:dump-gui` evidence record.
    pub fn record(&self, state: &ViewState) -> Value {
        let kw = Value::keyword;
        let rect = |r: &Rect| {
            Value::Map(vec![
                (kw("x"), Value::Float(r.x)),
                (kw("y"), Value::Float(r.y)),
                (kw("w"), Value::Float(r.w)),
                (kw("h"), Value::Float(r.h)),
            ])
        };
        let bounds = Value::Map(
            self.bounds
                .iter()
                .map(|(k, r)| (Value::String(k.clone()), rect(r)))
                .collect(),
        );
        let entries = Value::Vector(
            self.cached_entries(state)
                .iter()
                .map(|e| {
                    Value::Map(vec![
                        (kw("label"), Value::String(e.label.clone())),
                        (kw("active"), Value::Bool(e.active)),
                    ])
                })
                .collect(),
        );
        let hover = match &self.card.hover {
            Some(Hover::Module) => kw("module"),
            Some(Hover::Op(op)) => Value::String(op.clone()),
            Some(Hover::Rel(id)) => id.clone(),
            None => Value::Nil,
        };
        let card = Value::Map(vec![
            (kw("open"), Value::Bool(self.card.open)),
            (kw("hovered"), Value::Bool(self.card.hovered)),
            (kw("scroll"), Value::Float(state.detail_scroll)),
            (kw("drawn-scroll"), Value::Float(self.card.scroll)),
            (kw("view-h"), Value::Float(self.card.view_h)),
            (kw("hover"), hover),
            (kw("rect"), self.card.rect.as_ref().map_or(Value::Nil, rect)),
            (
                kw("horizontal"),
                Value::Map(vec![
                    (kw("maximum"), Value::Float(self.card.h_max)),
                    (kw("offset"), Value::Float(self.card.h_offset)),
                    (kw("content-width"), Value::Float(self.card.content_w)),
                ]),
            ),
            (
                kw("row-heights"),
                Value::Vector(
                    self.card
                        .row_heights
                        .iter()
                        .map(|h| Value::Float(*h))
                        .collect(),
                ),
            ),
        ]);
        let sources = Value::Vector(
            self.sources
                .iter()
                .map(|w| {
                    Value::Map(vec![
                        (kw("title"), Value::String(w.title.clone())),
                        (kw("file"), Value::String(w.file.clone())),
                        (
                            kw("line"),
                            w.line.map_or(Value::Nil, |l| Value::Integer(l as i64)),
                        ),
                        (kw("rows"), Value::Integer(w.rows.len() as i64)),
                        (
                            kw("highlighted"),
                            Value::Vector(
                                w.rows
                                    .iter()
                                    .filter(|r| r.marker.is_some())
                                    .map(|r| Value::Integer(r.number as i64))
                                    .collect(),
                            ),
                        ),
                        (kw("scroll-y"), Value::Float(w.scroll_y)),
                        (kw("target-visible"), Value::Bool(w.target_visible)),
                        (kw("selection"), Value::String(w.selection.clone())),
                    ])
                })
                .collect(),
        );
        let rename = self.rename.as_ref().map_or(Value::Nil, |r| {
            Value::Map(vec![
                (kw("id"), r.id.clone()),
                (kw("name"), Value::String(r.name.clone())),
            ])
        });
        Value::Map(vec![
            (
                kw("inspector"),
                Value::Map(vec![
                    (kw("width"), Value::Float(self.inspector_width())),
                    (kw("entries"), entries),
                    (kw("menu-open"), Value::Bool(self.menu_open)),
                    (kw("element-menu"), Value::Bool(self.element_menu_open)),
                    (kw("rename"), rename),
                    (kw("empty-help"), Value::Bool(self.empty_help)),
                ]),
            ),
            (kw("bounds"), bounds),
            (kw("card"), card),
            (kw("sources"), sources),
            (
                kw("copied"),
                self.copied.clone().map_or(Value::Nil, Value::String),
            ),
            (kw("focus"), Value::Vector(state.focus.clone())),
            (
                kw("proposal-id"),
                get(&state.extra, "proposal-id")
                    .cloned()
                    .unwrap_or(Value::Nil),
            ),
            (
                kw("proposal"),
                Value::Bool(edn::record::flag(&state.extra, "proposal")),
            ),
        ])
    }
}

fn label(ui: &mut egui::Ui, text: impl Into<String>, color: Rgb) {
    ui.label(
        RichText::new(text.into())
            .color(rgb(color))
            .font(body_font()),
    );
}

/// A metric line that wraps only between whole segments, never inside one.
fn segments(ui: &mut egui::Ui, parts: Vec<String>, color: Rgb) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for part in parts {
            ui.add(
                egui::Label::new(RichText::new(part).color(rgb(color)).font(body_font()))
                    .wrap_mode(egui::TextWrapMode::Extend),
            );
        }
    });
}

fn mutation_line(c: &Value) -> Option<Vec<String>> {
    let (k, s) = (
        get(c, "killed").and_then(num),
        get(c, "survived").and_then(num),
    );
    if k.is_none() && s.is_none() {
        return None;
    }
    let mut parts = vec![format!(
        "{} killed / {} survived",
        k.unwrap_or(0.0) as i64,
        s.unwrap_or(0.0) as i64
    )];
    if let Some(u) = get(c, "uncovered").and_then(num).filter(|u| *u > 0.0) {
        parts.push(format!("{} uncovered", u as i64));
    }
    Some(parts)
}

/// `Crap μ 44.0`, `max 870.0`, `σ 115.0`: one segment per statistic.
fn crap_summary(crap: &Value) -> Option<Vec<String>> {
    let mu = get(crap, "mu").and_then(num)?;
    let max = get(crap, "max").and_then(num).unwrap_or(mu);
    let sigma = get(crap, "sigma").and_then(num).unwrap_or(0.0);
    Some(
        strings::CRAP_LINE
            .split_inclusive("%s")
            .zip([mu, max, sigma])
            .map(|(part, x)| part.trim().replace("%s", &one(x)))
            .collect(),
    )
}

/// Selected class summary: identity, quality lines, member lines.
fn class_summary(ui: &mut egui::Ui, model: &detail::Model) {
    let c = &model.class;
    let text = |v: Option<&Value>| match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Integer(n)) => n.to_string(),
        Some(Value::Float(x)) => x.to_string(),
        Some(other) => local_name(other).unwrap_or_default().to_owned(),
        None => String::new(),
    };
    label(ui, text(get(c, "name")), theme::INK);
    if let Some(package) = model.package_label() {
        label(ui, format!("{}{package}", strings::PACKAGE), theme::MUTED);
    }
    if let Some(level) = get(c, "level").filter(|v| **v != Value::Nil) {
        label(
            ui,
            format!("{}{}", strings::LEVEL, text(Some(level))),
            theme::MUTED,
        );
    }
    if let Some(stereo) = get(c, "stereotype") {
        label(
            ui,
            format!("«{}»", local_name(stereo).unwrap_or_default().to_owned()),
            theme::MUTED,
        );
    }
    let crap = get(c, "crap");
    let mut measured = false;
    if let Some(n) = crap.and_then(num) {
        label(ui, strings::CRAP_SINGLE.replace("%s", &one(n)), theme::GOLD);
        measured = true;
    } else if let Some(parts) = crap.and_then(crap_summary) {
        segments(ui, parts, theme::GOLD);
        measured = true;
    }
    if let Some(parts) = mutation_line(c) {
        segments(ui, parts, theme::MUTED);
    }
    for line in edn::record::seq(get(c, "lines")) {
        let kind = get(line, "kind")
            .and_then(local_name)
            .unwrap_or_default()
            .to_owned();
        if kind == "field" || kind == "op" {
            label(ui, text(get(line, "text")), theme::INK);
        }
    }
    if !measured {
        label(ui, strings::NO_DATA, theme::BAND_RED);
    }
}

fn package_summary(ui: &mut egui::Ui, state: &ViewState, id: &Value) {
    let Some(p) = scene::hit::package_by_id(&state.scene, id) else {
        return;
    };
    let title = match get(p, "label") {
        Some(Value::String(s)) => s.clone(),
        _ => local_name(id).unwrap_or_default().to_owned(),
    };
    label(ui, title, theme::INK);
    let crap = get(p, "crap");
    if let Some(parts) = crap.and_then(crap_summary) {
        segments(ui, parts, theme::GOLD);
    }
    if let Some(parts) = mutation_line(p) {
        segments(ui, parts, theme::MUTED);
    }
    let count = edn::record::seq(get(&state.scene, "classes"))
        .iter()
        .filter(|c| get(c, "package") == Some(id))
        .count();
    label(ui, format!("{}{count}", strings::CLASS_COUNT), theme::MUTED);
    if crap.is_none() {
        label(ui, strings::NO_DATA, theme::BAND_RED);
    }
}
