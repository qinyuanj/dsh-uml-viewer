//! The panel overlays (#27): the inspector, the class card and the source
//! window as resident-protocol JSON. Every text, count, CRAP/mutation figure,
//! no-data line, card row, metric column, color role and source line comes
//! from the shared window-free models (`frame::inspector`, `frame::detail`,
//! `frame::source`) that the egui window paints. Nothing is recomputed here
//! beyond serializing those models, so the TypeScript panel only renders.
//!
//! Source text is read here, in the resident, from the session project; the
//! browser never touches the filesystem.

use edn::Value;
use edn::record::{get, local_name};
use serde_json::{Value as Json, json};

use frame::frame::role_json;
use frame::{detail, inspector, source, strings};

use crate::Session;

fn edn_text(value: &Value) -> String {
    edn::encode(value).unwrap_or_default()
}

fn optional_id(id: Option<&Value>) -> Json {
    id.map_or(Json::Null, |id| json!(edn_text(id)))
}

/// One `inspector::SummaryLine`: a label, or metric segments that wrap only
/// between whole statistics. Colors stay semantic roles.
fn summary_line(line: &inspector::SummaryLine) -> Json {
    match line {
        inspector::SummaryLine::Label { text, color } => {
            json!({"type": "label", "text": text, "color": role_json(*color)})
        }
        inspector::SummaryLine::Segments { parts, color } => {
            json!({"type": "segments", "parts": parts, "color": role_json(*color)})
        }
    }
}

fn summary(lines: Vec<inspector::SummaryLine>, kind: &str) -> Json {
    let lines: Vec<Json> = lines.iter().map(summary_line).collect();
    json!({"kind": kind, "lines": lines})
}

/// The inspector sidebar: the diagram/proposal list, the selected element's
/// summary and the labels of its controls.
pub fn inspector_json(session: &Session) -> Json {
    let state = session.state();
    let entries: Vec<Json> = inspector::entries(state)
        .iter()
        .map(|entry| {
            json!({"id": optional_id(entry.id.as_ref()), "label": entry.label,
                   "active": entry.active})
        })
        .collect();
    let selected = state.selected.as_ref();
    let id = selected.and_then(|s| get(s, "id"));
    let kind = selected
        .and_then(|s| get(s, "kind"))
        .and_then(local_name)
        .unwrap_or_default();
    // The window's own choice: a class with a card model summarizes the class,
    // a package summarizes the package, anything else shows the explanation.
    let summary = match id.and_then(|id| detail::for_state(state, id)) {
        Some(model) => summary(inspector::class_summary(&model), "class"),
        None if kind == "package" => match id {
            Some(id) => summary(inspector::package_summary(&state.scene, id), "package"),
            None => summary(Vec::new(), "package"),
        },
        None => {
            let message = inspector::empty_explanation(state).unwrap_or(strings::SELECT_CLASS);
            summary(
                vec![inspector::SummaryLine::Label {
                    text: message.to_owned(),
                    color: frame::theme::Role::Muted,
                }],
                "empty",
            )
        }
    };
    json!({
        "entries": entries,
        "summary": summary,
        "declutterLabel": inspector::declutter_label(state),
        "detail": optional_id(state.detail_id.as_ref()),
        "selected": optional_id(selected),
        "labels": {
            "inspector": strings::INSPECTOR,
            "proposals": strings::PROPOSALS_LABEL,
            "pHint": strings::P_HINT,
        },
    })
}

fn cell_json(cell: &detail::Cell) -> Json {
    json!({
        "id": cell.id, "text": cell.text, "left": cell.left, "right": cell.right,
        "center": cell.center, "color": role_json(cell.color), "tip": cell.tip,
    })
}

fn bar_json(bar: &detail::Bar) -> Json {
    json!({
        "id": bar.id, "left": bar.left, "right": bar.right,
        "fraction": bar.fraction, "color": role_json(bar.color),
    })
}

fn item_json(item: &detail::Item) -> Json {
    json!({
        "kind": item.kind, "y": item.y, "h": item.h, "text": item.text,
        "color": role_json(item.color), "titleFont": item.title_font, "x": item.x,
        "clipRight": item.clip_right,
        "cells": item.cells.iter().map(cell_json).collect::<Vec<_>>(),
        "bars": item.bars.iter().map(bar_json).collect::<Vec<_>>(),
        "stripe": item.stripe,
        "swatch": item.swatch.map(role_json),
        "clickable": item.clickable, "module": item.module,
        "opName": item.op_name, "rel": optional_id(item.rel.as_ref()),
    })
}

/// The open class card as the native paint plan: rows with their cells, bars,
/// swatches and roles in content space, plus the scroll model. `width` is the
/// card's content width in points (the native default is 624, floor 560) and
/// `viewHeight` the card viewport the scroll clamps against.
pub fn card_json(
    session: &Session,
    width: Option<f64>,
    view_height: Option<f64>,
) -> Result<Json, String> {
    let state = session.state();
    let id = state
        .detail_id
        .clone()
        .ok_or_else(|| "no class card is open".to_owned())?;
    let model = detail::for_state(state, &id)
        .ok_or_else(|| format!("no class card for {}", edn_text(&id)))?;
    let rows = detail::rows(&model);
    let items = detail::view(&rows, width);
    let view_h = view_height.filter(|h| *h > 0.0).unwrap_or(detail::HEIGHT);
    let max_scroll = detail::max_scroll(&rows, view_h);
    let title = get(&model.class, "name")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => local_name(other).unwrap_or_default().to_owned(),
        })
        .or(model.ns.clone())
        .unwrap_or_default();
    Ok(json!({
        "id": edn_text(&id),
        "title": title,
        "contentHeight": detail::content_h(&rows),
        "viewHeight": view_h,
        "scroll": state.detail_scroll.clamp(0.0, max_scroll),
        "maxScroll": max_scroll,
        "items": items.iter().map(item_json).collect::<Vec<_>>(),
    }))
}

/// Open a source window on the class's module (`member` None) or on one
/// member's declaration line. The file is read from the session project by
/// `frame::source`, which owns the per-language declaration patterns.
pub fn source_json(session: &Session, id: &Value, member: Option<&str>) -> Result<Json, String> {
    let state = session.state();
    let model = detail::for_state(state, id).ok_or_else(|| format!("no class {}", edn_text(id)))?;
    let ident = detail::member_ident(&model, member);
    let project = std::fs::canonicalize(session.project_root())
        .map_err(|e| format!("cannot resolve source project: {e}"))?;
    let file = model.file.as_deref().ok_or("class has no source file")?;
    let candidate = session.project_root().join(file);
    project_source(&project, &candidate)?;
    let root = session.project_root().to_string_lossy().into_owned();
    let files = source::Files {
        root: Some(root),
        // Recheck each actual access, including link/junction resolution. No
        // source body is ever read before the project containment check.
        read: &|path| {
            let path = project_source(&project, std::path::Path::new(path)).ok()?;
            std::fs::read_to_string(path).ok()
        },
        is_file: &|path| {
            project_source(&project, std::path::Path::new(path)).is_ok_and(|p| p.is_file())
        },
    };
    let opened = source::member_source(&ident, &files)
        .ok_or_else(|| strings::SOURCE_NOT_OPENED.replace("%s", &opened_title(member, &model)))?;
    let lang = opened.lang.clone().unwrap_or_default();
    let rows: Vec<Json> = source::rows(&opened.body, opened.line)
        .iter()
        .map(|row| {
            let display = source::display(&row.text);
            let tokens: Vec<Json> = source::tokens(&display.text, &lang)
                .iter()
                .map(|run| json!({"text": run.text, "kind": run.kind}))
                .collect();
            json!({
                "number": row.number,
                "text": row.text,
                "display": display.text,
                "marked": row.marker.is_some(),
                "tokens": tokens,
            })
        })
        .collect();
    Ok(json!({
        "file": opened.file,
        "title": opened.title,
        "line": opened.line,
        "lang": lang,
        "rows": rows,
    }))
}

/// The source boundary resolves links/junctions before checking containment.
/// Reject parent traversal explicitly, even when a later component returns to
/// the project, so source resolution cannot depend on an escaping prefix.
fn project_source(
    project: &std::path::Path,
    candidate: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    if candidate
        .components()
        .any(|part| part == std::path::Component::ParentDir)
    {
        return Err("source path must stay inside the session project".into());
    }
    let resolved = std::fs::canonicalize(candidate)
        .map_err(|e| format!("cannot resolve source {}: {e}", candidate.display()))?;
    if !resolved.starts_with(project) {
        return Err("source path must stay inside the session project".into());
    }
    Ok(resolved)
}

fn opened_title(member: Option<&str>, model: &detail::Model) -> String {
    let base = model.file.clone().or(model.ns.clone()).unwrap_or_default();
    match member {
        Some(name) => format!("{base}/{name}"),
        None => base,
    }
}
