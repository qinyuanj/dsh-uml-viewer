use crate::extract::Entry;
use quality_core::functions::{Function, functions_in_file};
use std::path::Path;

fn size(function: &Function) -> (usize, usize) {
    (function.end_line - function.start_line, function.start_line)
}

pub(crate) fn owner(functions: &[Function], start: usize, end: usize) -> Option<&Function> {
    functions
        .iter()
        .filter(|function| function.start_line == start)
        .min_by_key(|function| size(function))
        .or_else(|| {
            functions
                .iter()
                .filter(|function| function.start_line <= start && end <= function.end_line)
                .min_by_key(|function| size(function))
        })
}

pub(crate) fn named(
    entries: Vec<Entry>,
    language: quality_core::language::Language,
    source: &str,
    file: &str,
    root: &Path,
) -> std::io::Result<Vec<Entry>> {
    let functions = functions_in_file(language, source, file, root)?;
    Ok(entries
        .into_iter()
        .map(|mut entry| {
            if let Some(function) = owner(&functions, entry.start_line, entry.end_line) {
                entry.namespace = Some(function.namespace.clone());
                entry.name = Some(function.name.clone());
            }
            entry
        })
        .collect())
}
