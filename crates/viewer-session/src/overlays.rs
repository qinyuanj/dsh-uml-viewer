//! The panel overlays (#27): the inspector, the class card and the source
//! window as resident-protocol JSON. Every text, count, CRAP/mutation figure,
//! no-data line, card row, metric column, color role and source line comes
//! from the shared window-free models (`frame::inspector`, `frame::detail`,
//! `frame::source`). Nothing is recomputed here
//! beyond serializing those models, so the TypeScript panel only renders.
//!
//! Source text is read here, in the resident, from the session project; the
//! browser never touches the filesystem. A source window is bounded by the
//! `SOURCE_*` limits below, checked before the body is read past
//! [`SOURCE_MAX_FILE_BYTES`], before any declaration lookup or tokenization
//! and before the event is serialized. A source over a limit is an explicit
//! `error`; it is never truncated. `packages/contracts/src/overlays.ts`
//! (`SOURCE_LIMITS`) carries the same numbers and must stay equal.

use edn::Value;
use edn::record::{get, local_name};
use serde_json::{Value as Json, json};

use frame::frame::role_json;
use frame::{detail, inspector, source, strings};

use crate::Session;

/// Whole source file, in bytes; the read stops one byte past it.
pub const SOURCE_MAX_FILE_BYTES: u64 = 256 * 1024;
/// Rows in one source (a trailing newline counts its empty last row).
pub const SOURCE_MAX_ROWS: usize = 4000;
/// One source row in UTF-8 bytes, CR trimmed.
pub const SOURCE_MAX_LINE_BYTES: usize = 4096;
/// One tab-expanded row in UTF-8 bytes (a tab paints two spaces).
pub const SOURCE_MAX_DISPLAY_BYTES: usize = 8192;
/// Token runs in one row.
pub const SOURCE_MAX_RUNS_PER_ROW: usize = 512;
/// Token runs in the whole source.
pub const SOURCE_MAX_RUNS: usize = 20_000;
/// One serialized `source` event line, newline excluded.
pub const SOURCE_MAX_EVENT_BYTES: usize = 2 * 1024 * 1024;
/// `file` and `title`, in UTF-8 bytes.
pub const SOURCE_MAX_PATH_BYTES: usize = 4096;
/// `lang`, in UTF-8 bytes.
pub const SOURCE_MAX_LANG_BYTES: usize = 64;
/// Room left in [`SOURCE_MAX_EVENT_BYTES`] for the resident's
/// `{"version":1,"seq":N,"event":"source","source":…}` envelope.
const SOURCE_ENVELOPE_BYTES: usize = 256;

fn too_large(what: &str, detail: String) -> String {
    format!("source too large to open: {what} {detail}; it is not truncated")
}

/// Read at most `max` bytes of `path`: a larger file is refused from its
/// metadata, and a file that grows while it is read is refused at `max + 1`
/// bytes, so the resident never holds more than one byte past the limit.
/// `Ok(None)` is a file that is not UTF-8, which is not openable source.
fn read_bounded(path: &std::path::Path, max: u64) -> Result<Option<String>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read source: {e}"))?;
    let size = file
        .metadata()
        .map_err(|e| format!("cannot read source: {e}"))?
        .len();
    let refuse = |size: u64| too_large("file", format!("has {size} bytes; the limit is {max}"));
    if size > max {
        return Err(refuse(size));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read source: {e}"))?;
    if bytes.len() as u64 > max {
        return Err(refuse(bytes.len() as u64));
    }
    Ok(String::from_utf8(bytes).ok())
}

/// The row, line and painted-line bounds of a body, checked on its bytes
/// before the declaration lookup splits, scans or tokenizes it.
fn check_body(body: &str) -> Result<(), String> {
    let rows = body.bytes().filter(|b| *b == b'\n').count() + 1;
    if rows > SOURCE_MAX_ROWS {
        return Err(too_large(
            "file",
            format!("has {rows} rows; the limit is {SOURCE_MAX_ROWS}"),
        ));
    }
    for (index, line) in body.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let number = index + 1;
        if line.len() > SOURCE_MAX_LINE_BYTES {
            return Err(too_large(
                &format!("line {number}"),
                format!(
                    "has {} bytes; the limit is {SOURCE_MAX_LINE_BYTES}",
                    line.len()
                ),
            ));
        }
        let painted = line.len() + line.bytes().filter(|b| *b == b'\t').count();
        if painted > SOURCE_MAX_DISPLAY_BYTES {
            return Err(too_large(
                &format!("line {number}"),
                format!("paints {painted} bytes; the limit is {SOURCE_MAX_DISPLAY_BYTES}"),
            ));
        }
    }
    Ok(())
}

/// The window's rows with their token runs, refusing a row over
/// [`SOURCE_MAX_RUNS_PER_ROW`] or a source over [`SOURCE_MAX_RUNS`] before
/// allocating the run that crosses it.
fn source_rows(body: &str, line: Option<usize>, lang: &str) -> Result<Vec<Json>, String> {
    let mut runs = 0usize;
    source::rows(body, line)
        .iter()
        .map(|row| {
            let display = source::display(&row.text);
            let budget = SOURCE_MAX_RUNS_PER_ROW.min(SOURCE_MAX_RUNS - runs);
            let tokens = source::tokens_bounded(&display.text, lang, budget).ok_or_else(|| {
                if budget < SOURCE_MAX_RUNS_PER_ROW {
                    too_large("file", format!("has over {SOURCE_MAX_RUNS} token runs"))
                } else {
                    too_large(
                        &format!("line {}", row.number),
                        format!("has over {SOURCE_MAX_RUNS_PER_ROW} token runs"),
                    )
                }
            })?;
            runs += tokens.len();
            let tokens: Vec<Json> = tokens
                .iter()
                .map(|run| json!({"text": run.text, "kind": run.kind}))
                .collect();
            Ok(json!({
                "number": row.number,
                "text": row.text,
                "display": display.text,
                "marked": row.marker.is_some(),
                "tokens": tokens,
            }))
        })
        .collect()
}

/// A writer that stops at `left` bytes, so measuring a payload never builds
/// more than the limit allows.
struct Budget {
    left: usize,
}

impl std::io::Write for Budget {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.len() > self.left {
            self.left = 0;
            return Err(std::io::Error::other("over budget"));
        }
        self.left -= buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// True when `value` serializes to at most `max` bytes. The measurement
/// stops at the limit instead of allocating the oversized text. The quality
/// target answer (#28, [`crate::targets`]) is bounded the same way.
pub(crate) fn serializes_within(value: &Json, max: usize) -> bool {
    serde_json::to_writer(Budget { left: max }, value).is_ok()
}

fn check_text(what: &str, text: &str, max: usize) -> Result<(), String> {
    if text.len() > max {
        return Err(too_large(
            what,
            format!("has {} bytes; the limit is {max}", text.len()),
        ));
    }
    Ok(())
}

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
    // `Files::read` answers only Some/None; a size refusal is kept here so it
    // reaches the caller as itself, not as "could not open".
    let refused: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
    let files = source::Files {
        root: Some(root),
        // Recheck each actual access, including link/junction resolution. No
        // source body is ever read before the project containment check, and
        // none is kept past the size, row and line bounds.
        read: &|path| {
            let path = project_source(&project, std::path::Path::new(path)).ok()?;
            let checked = read_bounded(&path, SOURCE_MAX_FILE_BYTES).and_then(|body| match body {
                Some(body) => check_body(&body).map(|()| Some(body)),
                None => Ok(None),
            });
            match checked {
                Ok(body) => body,
                Err(reason) => {
                    refused.borrow_mut().get_or_insert(reason);
                    None
                }
            }
        },
        is_file: &|path| {
            project_source(&project, std::path::Path::new(path)).is_ok_and(|p| p.is_file())
        },
    };
    let opened = source::member_source(&ident, &files);
    if let Some(reason) = refused.borrow_mut().take() {
        return Err(reason);
    }
    let opened = opened
        .ok_or_else(|| strings::SOURCE_NOT_OPENED.replace("%s", &opened_title(member, &model)))?;
    let lang = opened.lang.clone().unwrap_or_default();
    check_text("file path", &opened.file, SOURCE_MAX_PATH_BYTES)?;
    check_text("title", &opened.title, SOURCE_MAX_PATH_BYTES)?;
    check_text("language", &lang, SOURCE_MAX_LANG_BYTES)?;
    let rows = source_rows(&opened.body, opened.line, &lang)?;
    let payload = json!({
        "file": opened.file,
        "title": opened.title,
        "line": opened.line,
        "lang": lang,
        "rows": rows,
    });
    let budget = SOURCE_MAX_EVENT_BYTES - SOURCE_ENVELOPE_BYTES;
    if !serializes_within(&payload, budget) {
        return Err(too_large(
            "event",
            format!("serializes over {budget} bytes"),
        ));
    }
    Ok(payload)
}

/// The source boundary resolves links/junctions before checking containment.
/// Reject parent traversal explicitly, even when a later component returns to
/// the project, so source resolution cannot depend on an escaping prefix.
/// The quality target answer (#28) uses the same boundary for its files.
pub(crate) fn project_source(
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

#[cfg(test)]
mod source_bound_tests {
    use super::*;

    fn scratch(label: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "uml-viewer-source-bounds-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("source.rs");
        std::fs::write(&file, bytes).unwrap();
        file
    }

    fn cleanup(file: &std::path::Path) {
        let _ = std::fs::remove_dir_all(file.parent().unwrap());
    }

    #[test]
    fn limits_match_the_contracts_source_limits() {
        // packages/contracts/src/overlays.ts SOURCE_LIMITS.
        assert_eq!(SOURCE_MAX_FILE_BYTES, 262_144);
        assert_eq!(SOURCE_MAX_ROWS, 4000);
        assert_eq!(SOURCE_MAX_LINE_BYTES, 4096);
        assert_eq!(SOURCE_MAX_DISPLAY_BYTES, 8192);
        assert_eq!(SOURCE_MAX_RUNS_PER_ROW, 512);
        assert_eq!(SOURCE_MAX_RUNS, 20_000);
        assert_eq!(SOURCE_MAX_EVENT_BYTES, 2_097_152);
        assert_eq!(SOURCE_MAX_PATH_BYTES, 4096);
        assert_eq!(SOURCE_MAX_LANG_BYTES, 64);
    }

    #[test]
    fn reads_a_file_at_the_byte_limit_and_refuses_one_byte_more() {
        let at = scratch("at", &[b'a'; 64]);
        assert_eq!(read_bounded(&at, 64).unwrap().unwrap().len(), 64);
        let error = read_bounded(&at, 63).unwrap_err();
        assert!(error.contains("has 64 bytes; the limit is 63"), "{error}");
        cleanup(&at);
    }

    #[test]
    fn a_non_utf8_file_is_not_openable_source_rather_than_an_error() {
        let file = scratch("latin1", &[b'a', 0xff, b'b']);
        assert_eq!(read_bounded(&file, 64).unwrap(), None);
        cleanup(&file);
    }

    #[test]
    fn row_bound_counts_the_empty_row_after_a_trailing_newline() {
        let at = "\n".repeat(SOURCE_MAX_ROWS - 1);
        assert!(check_body(&at).is_ok());
        let over = "\n".repeat(SOURCE_MAX_ROWS);
        let error = check_body(&over).unwrap_err();
        assert!(
            error.contains("has 4001 rows; the limit is 4000"),
            "{error}"
        );
    }

    #[test]
    fn line_bound_counts_utf8_bytes_and_ignores_the_crlf_terminator() {
        let at = "é".repeat(SOURCE_MAX_LINE_BYTES / 2);
        assert!(check_body(&format!("{at}\r\nnext")).is_ok());
        let error = check_body(&format!("ok\n{at}x")).unwrap_err();
        assert!(error.contains("line 2 has 4097 bytes"), "{error}");
    }

    #[test]
    fn display_bound_counts_each_tab_as_two_painted_bytes() {
        let tabs = "\t".repeat(SOURCE_MAX_LINE_BYTES);
        assert!(check_body(&tabs).is_ok());
        let wider = format!("{}x", "\t".repeat(SOURCE_MAX_LINE_BYTES - 1));
        assert!(check_body(&wider).is_ok());
        // 4096 raw bytes is the line limit; the display limit is exactly twice it,
        // so a line within the raw limit always paints within the display limit.
        assert_eq!(SOURCE_MAX_DISPLAY_BYTES, 2 * SOURCE_MAX_LINE_BYTES);
    }

    #[test]
    fn rows_keep_native_tokens_and_mark_the_declaration_line() {
        let rows = source_rows("fn a() {}\n\tpub fn b() {}\n", Some(2), "rust").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1]["display"], "  pub fn b() {}");
        assert_eq!(rows[1]["marked"], true);
        assert_eq!(rows[0]["marked"], false);
        let joined: String = rows[1]["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .map(|run| run["text"].as_str().unwrap())
            .collect();
        assert_eq!(joined, "  pub fn b() {}");
    }

    #[test]
    fn a_punctuation_line_over_the_row_run_limit_is_refused() {
        let at = ";".repeat(SOURCE_MAX_RUNS_PER_ROW);
        assert!(source_rows(&at, None, "rust").is_ok());
        let over = ";".repeat(SOURCE_MAX_RUNS_PER_ROW + 1);
        let error = source_rows(&over, None, "rust").unwrap_err();
        assert!(error.contains("line 1 has over 512 token runs"), "{error}");
    }

    #[test]
    fn a_source_over_the_total_run_limit_is_refused() {
        let line = ";".repeat(SOURCE_MAX_RUNS_PER_ROW);
        let rows = SOURCE_MAX_RUNS / SOURCE_MAX_RUNS_PER_ROW; // 39 full rows = 19968 runs
        let at = format!("{}{}", format!("{line}\n").repeat(rows), ";".repeat(32));
        assert!(source_rows(&at, None, "rust").is_ok());
        let over = format!("{}{}", format!("{line}\n").repeat(rows), ";".repeat(33));
        let error = source_rows(&over, None, "rust").unwrap_err();
        assert!(error.contains("file has over 20000 token runs"), "{error}");
    }

    #[test]
    fn serialized_size_is_measured_without_building_an_oversized_text() {
        let value = json!({"text": "x".repeat(100)});
        let exact = serde_json::to_vec(&value).unwrap().len();
        assert!(serializes_within(&value, exact));
        assert!(!serializes_within(&value, exact - 1));
    }

    #[test]
    fn metadata_over_its_bound_is_refused() {
        let max = SOURCE_MAX_PATH_BYTES;
        assert!(check_text("title", &"t".repeat(max), max).is_ok());
        let error = check_text("title", &"t".repeat(max + 1), max).unwrap_err();
        assert!(error.contains("title has 4097 bytes"), "{error}");
    }
}
