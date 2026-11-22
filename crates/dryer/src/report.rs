use crate::scan::{Candidate, Span};
use quality_core::snapshot::{decimal, string};
use std::io;
use std::path::{Path, PathBuf};

pub(crate) fn text(candidates: &[Candidate]) -> String {
    if candidates.is_empty() {
        return "No duplicate candidates found.\n".to_string();
    }
    candidates
        .iter()
        .map(|c| {
            format!(
                "DUPLICATE score={:.2}\n  {}:{}-{}\n  {}:{}-{}",
                c.score,
                c.left.file,
                c.left.start,
                c.left.end,
                c.right.file,
                c.right.start,
                c.right.end
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}
fn name(value: &Option<String>) -> String {
    value.as_deref().map_or_else(|| "nil".to_string(), string)
}
fn span(span: &Span) -> String {
    format!(
        "{{:file {}, :start-line {}, :end-line {}, :namespace {}, :name {}}}",
        string(&span.file),
        span.start,
        span.end,
        name(&span.namespace),
        name(&span.name)
    )
}
pub(crate) fn edn(candidates: &[Candidate]) -> String {
    if candidates.is_empty() {
        return "{:candidates []}\n".to_string();
    }
    let rows = candidates.iter().map(|c| format!(" {{:score {}\n  :language {}\n  :left {}\n  :right {}\n  :left-nodes {}\n  :right-nodes {}}}", decimal(c.score, 12), string(c.language.name()), span(&c.left), span(&c.right), c.left_nodes, c.right_nodes));
    format!(
        "{{:candidates [\n{}\n]}}\n",
        rows.collect::<Vec<_>>().join("\n")
    )
}

pub(crate) fn write_metrics(candidates: &[Candidate], root: &Path) -> io::Result<PathBuf> {
    let path = root.join(".metrics").join("dry.edn");
    quality_core::atomic::write_bytes(&path, edn(candidates).as_bytes())?;
    Ok(path)
}
