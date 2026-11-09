//! Quality refresh targets (#28) as the resident answers them: the elements
//! `scene::targets` resolved, turned into bounded, deterministic, canonical
//! project-relative source files. The grouping rules (namespace subtree,
//! proposal layers and nested groups, `:omit`, `:proposal.unassigned`) stay
//! in the scene; this module owns only the file boundary, which is the
//! resident's, exactly as for the source window ([`crate::overlays`]).
//!
//! The protocol is `docs/resident-protocol.md`. A class with no `:file`, a
//! path outside the session project, a missing file or an id the shown tree
//! does not have is an explicit `error`; nothing is ever delivered with an
//! empty or partial file list.

use std::collections::BTreeSet;
use std::path::{Component, Path};

use edn::Value;
use serde_json::{Value as Json, json};

use crate::Session;

/// At most this many files in one answer.
pub const QUALITY_TARGET_MAX_FILES: usize = 4096;
/// One project-relative file path, in UTF-8 bytes.
pub const QUALITY_TARGET_MAX_PATH_BYTES: usize = 4096;
/// One serialized `quality-target` payload, newline and envelope excluded.
pub const QUALITY_TARGET_MAX_EVENT_BYTES: usize = 1024 * 1024;
/// Room left in [`QUALITY_TARGET_MAX_EVENT_BYTES`] for the resident's
/// `{"version":1,"seq":N,"event":"quality-target","target":…}` envelope.
const QUALITY_TARGET_ENVELOPE_BYTES: usize = 256;

fn edn_text(value: &Value) -> String {
    edn::encode(value).unwrap_or_default()
}

/// The `quality-target` payload for `id`: its kind and files, in a fixed
/// order, each file once.
///
/// # Errors
///
/// The session shows no diagram, the id is not in the shown tree, a member
/// class has no source file, a path leaves the session project or is missing,
/// or the answer would exceed a bound.
pub fn quality_target_json(session: &Session, id: &Value) -> Result<Json, String> {
    let state = session.state();
    let doc = state.document.as_ref().ok_or_else(|| {
        "no diagram: the panel shows nothing yet; run uml_ir and uml_display first".to_owned()
    })?;
    quality_target_for(doc, &state.focus, &state.extra, session.project_root(), id)
}

/// The `quality-target` payload for `id` in one loaded model: the grouping
/// comes from `scene::targets` (which follows the same view selection as
/// `scene::document::compile_view`), the file answer from the session's
/// project.
///
/// # Errors
///
/// As [`quality_target_json`], without the no-diagram case.
pub fn quality_target_for(
    doc: &Value,
    focus: &[Value],
    extra: &Value,
    project: &Path,
    id: &Value,
) -> Result<Json, String> {
    if !matches!(id, Value::Keyword(_)) || edn_text(id).len() > QUALITY_TARGET_MAX_PATH_BYTES {
        return Err("quality target id must be a bounded EDN keyword".into());
    }
    let target = scene::targets::resolve(doc, focus, extra, id)?;
    let files = target_files(project, &target.members)?;
    let payload = json!({
        "id": edn_text(id),
        "kind": target.kind.as_str(),
        "files": files,
    });
    let budget = QUALITY_TARGET_MAX_EVENT_BYTES - QUALITY_TARGET_ENVELOPE_BYTES;
    if !crate::overlays::serializes_within(&payload, budget) {
        return Err(format!(
            "quality target serializes over {budget} bytes; narrow the selection"
        ));
    }
    Ok(payload)
}

/// The canonical project-relative source files of one target, sorted and
/// deduplicated: two classes in one file refresh that file once.
fn target_files(project: &Path, members: &[scene::targets::Member]) -> Result<Vec<String>, String> {
    let project = std::fs::canonicalize(project)
        .map_err(|e| format!("cannot resolve the session project: {e}"))?;
    let mut files: BTreeSet<String> = BTreeSet::new();
    for member in members {
        let file = member.file.as_deref().ok_or_else(|| {
            format!(
                "class {} has no source file; it cannot be refreshed",
                member.id
            )
        })?;
        let resolved = canonical_source(&project, file)?;
        if files.len() == QUALITY_TARGET_MAX_FILES && !files.contains(&resolved) {
            return Err(format!(
                "over {QUALITY_TARGET_MAX_FILES} source files to refresh; narrow the selection"
            ));
        }
        files.insert(resolved);
    }
    if files.is_empty() {
        return Err("no source files to refresh".to_owned());
    }
    Ok(files.into_iter().collect())
}

/// One IR `:file` as a canonical project-relative path: normalized forward
/// slashes, no `.`/`..` segment, inside the session project, an existing
/// file. The containment check is the source window's own
/// ([`crate::overlays::project_source`]), so links and junctions resolve the
/// same way, and the canonical spelling collapses two paths to one file.
fn canonical_source(project: &Path, file: &str) -> Result<String, String> {
    let relative = file.replace('\\', "/");
    if relative.len() > QUALITY_TARGET_MAX_PATH_BYTES {
        return Err(format!(
            "source path is over {QUALITY_TARGET_MAX_PATH_BYTES} bytes; it cannot be refreshed"
        ));
    }
    if relative.is_empty()
        || relative.starts_with('-')
        || relative.chars().any(|ch| ch.is_control() || ch == ':')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!(
            "source file {relative:?} must be a normalized project-relative path"
        ));
    }
    if !Path::new(&relative)
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(format!(
            "source file {relative:?} must be a normalized project-relative path inside the session project"
        ));
    }
    let candidate = project.join(Path::new(&relative));
    let resolved = crate::overlays::project_source(project, &candidate)
        .map_err(|e| format!("source file {relative:?}: {e}"))?;
    if !resolved.is_file() {
        return Err(format!(
            "source file {relative:?} is not a file in the session project"
        ));
    }
    let inside = resolved
        .strip_prefix(project)
        .map_err(|_| format!("source file {relative:?} must stay inside the session project"))?;
    let mut parts: Vec<String> = Vec::new();
    for part in inside.components() {
        match part {
            Component::Normal(name) => parts.push(name.to_string_lossy().into_owned()),
            _ => {
                return Err(format!(
                    "source file {relative:?} must stay inside the session project"
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(format!(
            "source file {relative:?} must stay inside the session project"
        ));
    }
    let result = parts.join("/");
    if result.len() > QUALITY_TARGET_MAX_PATH_BYTES
        || result.starts_with('-')
        || result.chars().any(|ch| ch.is_control() || ch == ':')
    {
        return Err("canonical source path exceeds the quality target path bounds".into());
    }
    Ok(result)
}
