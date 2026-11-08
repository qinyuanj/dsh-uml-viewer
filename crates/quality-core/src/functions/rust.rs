//! Rust functions and cyclomatic complexity.
//!
//! Decision points are `if` / `if let`, `for`, `while` / `while let`, `loop`,
//! each `match` arm, `?`, and `&&` / `||`. Free functions are namespaced by
//! the module path (`crate::foo::bar`), methods by the self type
//! (`crate::foo::Widget`). Test code is not scored: a `tests` module (inline
//! or a `tests.rs` file), a file or module opening with `#![cfg(test)]`, an
//! item marked `#[test]`, `#[bench]`, `#[<runner>::test]`, or a `cfg` that
//! holds only under `test` (`cfg(test)`, `cfg(all(test, ...))`; not
//! `cfg(any(test, ...))`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::Node;

use super::Function;
use crate::grammar::{
    Grammar, binary_logic, child_of_kind, children, complexity, end_line, first_of_kind, preorder,
    start_line, text, universal_newlines,
};
use crate::language::Language;
use crate::paths::{relative_posix, resolve};

const DECISIONS: &[&str] = &[
    "if_expression",
    "for_expression",
    "while_expression",
    "loop_expression",
    "match_arm",
    "try_expression",
];
const SELF_TYPES: &[&str] = &["type_identifier", "generic_type", "scoped_type_identifier"];

fn is_decision(node: Node) -> bool {
    DECISIONS.contains(&node.kind()) || binary_logic(node, &["&&", "||"])
}

/// The comma-separated predicates inside a cfg `token_tree`.
fn cfg_items(tree: Node) -> Vec<Vec<Node>> {
    let kids = children(tree);
    let inner = if kids.len() >= 2 {
        &kids[1..kids.len() - 1]
    } else {
        &[][..]
    };
    let mut items: Vec<Vec<Node>> = vec![Vec::new()];
    for child in inner {
        if child.kind() == "," {
            items.push(Vec::new());
        } else {
            items.last_mut().unwrap().push(*child);
        }
    }
    items.into_iter().filter(|item| !item.is_empty()).collect()
}

/// True when this cfg predicate can only hold in a `cfg(test)` build.
fn requires_test(source: &str, item: &[Node]) -> bool {
    let name = text(source, item[0]);
    if item.len() == 1 {
        return name == "test";
    }
    if item[1].kind() != "token_tree" {
        return false;
    }
    let args = cfg_items(item[1]);
    match name {
        "all" => args.iter().any(|arg| requires_test(source, arg)),
        "any" => !args.is_empty() && args.iter().all(|arg| requires_test(source, arg)),
        _ => false,
    }
}

fn test_attribute(source: &str, item: Node) -> bool {
    let Some(attribute) = child_of_kind(item, &["attribute"]) else {
        return false;
    };
    let kids = children(attribute);
    let Some(first) = kids.first() else {
        return false;
    };
    let path = text(source, *first);
    let Some(arguments) = child_of_kind(attribute, &["token_tree"]) else {
        let last = path.rsplit("::").next().unwrap_or(path);
        return last == "test" || last == "bench";
    };
    if path != "cfg" {
        return false;
    }
    let predicates = cfg_items(arguments);
    predicates.len() == 1 && requires_test(source, &predicates[0])
}

/// Per-file lookups that the per-function walk up the tree would otherwise
/// redo: each node's previous named sibling, and which containers are test
/// modules. A generated file with thousands of items stays linear.
struct Context<'t> {
    previous_named: HashMap<usize, Node<'t>>,
    test_modules: RefCell<HashMap<usize, bool>>,
}

impl<'t> Context<'t> {
    fn new(root: Node<'t>) -> Context<'t> {
        let mut previous_named = HashMap::new();
        for node in preorder(root) {
            let mut last: Option<Node> = None;
            for child in children(node) {
                if let Some(previous) = last {
                    previous_named.insert(child.id(), previous);
                }
                if child.is_named() {
                    last = Some(child);
                }
            }
        }
        Context {
            previous_named,
            test_modules: RefCell::new(HashMap::new()),
        }
    }

    /// An outer attribute right before `node` marks it as test code.
    fn marked_test(&self, source: &str, node: Node<'t>) -> bool {
        let mut sibling = self.previous_named.get(&node.id()).copied();
        while let Some(current) = sibling {
            if !["attribute_item", "line_comment", "block_comment"].contains(&current.kind()) {
                break;
            }
            if current.kind() == "attribute_item" && test_attribute(source, current) {
                return true;
            }
            sibling = self.previous_named.get(&current.id()).copied();
        }
        false
    }

    fn test_module(&self, source: &str, node: Node<'t>) -> bool {
        if let Some(known) = self.test_modules.borrow().get(&node.id()) {
            return *known;
        }
        let found = test_module(source, node);
        self.test_modules.borrow_mut().insert(node.id(), found);
        found
    }

    fn in_test_code(&self, source: &str, node: Node<'t>) -> bool {
        let mut current = Some(node);
        while let Some(item) = current {
            if self.marked_test(source, item) || self.test_module(source, item) {
                return true;
            }
            current = item.parent();
        }
        false
    }
}

fn inner_test(source: &str, container: Node) -> bool {
    children(container)
        .into_iter()
        .any(|child| child.kind() == "inner_attribute_item" && test_attribute(source, child))
}

fn test_module(source: &str, node: Node) -> bool {
    if node.kind() == "source_file" {
        return inner_test(source, node);
    }
    if node.kind() != "mod_item" {
        return false;
    }
    if let Some(ident) = child_of_kind(node, &["identifier"])
        && text(source, ident) == "tests"
    {
        return true;
    }
    child_of_kind(node, &["declaration_list"]).is_some_and(|body| inner_test(source, body))
}

/// The self type of `impl Type` or `impl Trait for Type`, by its last path
/// segment.
fn impl_type(source: &str, implementation: Node) -> Option<String> {
    let node = implementation.child_by_field_name("type")?;
    if !SELF_TYPES.contains(&node.kind()) {
        return None;
    }
    let found = child_of_kind(node, &["type_identifier"])
        .map(|child| text(source, child))
        .filter(|name| !name.is_empty())
        .or_else(|| first_of_kind(node, "type_identifier").map(|child| text(source, child)))?;
    Some(found.to_string())
}

fn impl_owner(source: &str, node: Node) -> Option<String> {
    let parent = node.parent()?;
    let owner = parent.parent()?;
    if parent.kind() != "declaration_list" || owner.kind() != "impl_item" {
        return None;
    }
    impl_type(source, owner)
}

/// The start of the first line beginning with `[package]`, and the end of
/// its table (the next line beginning with `[`, or the end).
fn package_block(manifest: &str) -> Option<&str> {
    let bytes = manifest.as_bytes();
    let line_start = |at: usize| at == 0 || bytes[at - 1] == b'\n';
    let start =
        (0..bytes.len()).find(|&at| line_start(at) && bytes[at..].starts_with(b"[package]"))?;
    let from = start + "[package]".len();
    let end = (from..manifest.len())
        .find(|&at| line_start(at) && bytes[at] == b'[')
        .unwrap_or(manifest.len());
    Some(&manifest[start..end])
}

/// The first `name = "..."` at the start of a line in the package table.
fn package_name(block: &str) -> Option<&str> {
    let bytes = block.as_bytes();
    for at in 0..block.len() {
        if !(at == 0 || bytes[at - 1] == b'\n') || !bytes[at..].starts_with(b"name") {
            continue;
        }
        let rest = block[at + 4..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('"') else {
            continue;
        };
        if let Some(close) = rest.find('"')
            && close > 0
        {
            return Some(&rest[..close]);
        }
    }
    None
}

/// The crate of `file`: the nearest ancestor `Cargo.toml` with a
/// `[package]` table names it (`-` as `_`); without one it is `crate`.
fn crate_name(file: &Path) -> (String, Option<PathBuf>) {
    for directory in file.ancestors().skip(1) {
        let cargo = directory.join("Cargo.toml");
        if !cargo.is_file() {
            continue;
        }
        let manifest = fs::read(&cargo)
            .map(|bytes| universal_newlines(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default();
        if let Some(block) = package_block(&manifest) {
            let name = match package_name(block) {
                Some(name) => name.to_string(),
                None => directory
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            };
            return (name.replace('-', "_"), Some(directory.to_path_buf()));
        }
    }
    ("crate".to_string(), None)
}

fn stem(file: &Path) -> String {
    file.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Module segments of a file below its crate root; `lib.rs`, `main.rs` and
/// `mod.rs` add none.
fn crate_relative(file: &Path, crate_root: &Path) -> Vec<String> {
    let relative = relative_posix(file, crate_root).unwrap_or_default();
    let relative = relative.strip_prefix("src/").unwrap_or(&relative);
    if relative == "lib.rs" || relative == "main.rs" {
        return Vec::new();
    }
    let mut parts: Vec<String> = relative.split('/').map(str::to_string).collect();
    let name = parts.pop().unwrap_or_default();
    if name != "mod.rs" {
        parts.push(name.strip_suffix(".rs").unwrap_or(&name).to_string());
    }
    parts
}

fn file_modules(file: &Path, crate_name: &str, crate_root: Option<&Path>) -> Vec<String> {
    let Some(crate_root) = crate_root else {
        let stem = stem(file);
        return if stem == "lib" || stem == "main" {
            vec![crate_name.to_string()]
        } else {
            vec![crate_name.to_string(), stem]
        };
    };
    let parts = crate_relative(file, crate_root);
    if parts.first().is_some_and(|part| part == "bin") && parts.len() >= 2 {
        return parts[1..].to_vec();
    }
    let mut modules = vec![crate_name.to_string()];
    modules.extend(parts);
    modules
}

/// Names of the enclosing inline `mod` items, outermost first.
fn enclosing_modules(source: &str, node: Node) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = node.parent();
    while let Some(item) = current {
        if item.kind() == "mod_item"
            && let Some(ident) = child_of_kind(item, &["identifier"])
        {
            names.push(text(source, ident).to_string());
        }
        current = item.parent();
    }
    names.reverse();
    names
}

fn record<'t>(
    context: &Context<'t>,
    source: &str,
    node: Node<'t>,
    modules: &[String],
    path: &str,
) -> Option<Function> {
    if node.kind() != "function_item"
        || child_of_kind(node, &["block"]).is_none()
        || node.parent().is_some_and(|parent| parent.kind() == "block")
    {
        return None;
    }
    let ident = child_of_kind(node, &["identifier"])?;
    if context.in_test_code(source, node) {
        return None;
    }
    let mut parts: Vec<String> = modules.to_vec();
    parts.extend(enclosing_modules(source, node));
    parts.extend(impl_owner(source, node));
    let namespace = parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("::");
    Some(Function {
        name: text(source, ident).to_string(),
        namespace,
        complexity: complexity(node, &is_decision),
        start_line: start_line(node),
        end_line: end_line(node),
        path: path.to_string(),
        language: Language::Rust,
    })
}

pub(super) fn functions(source: &str, path: &str, project_root: &Path) -> Vec<Function> {
    let tree = Grammar::Rust.parse(source);
    let located = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        resolve(&project_root.join(path))
    };
    let located = resolve(&located);
    let (crate_name, crate_root) = crate_name(&located);
    let modules = file_modules(&located, &crate_name, crate_root.as_deref());
    if modules.iter().skip(1).any(|module| module == "tests") {
        return Vec::new();
    }
    let context = Context::new(tree.root_node());
    preorder(tree.root_node())
        .into_iter()
        .filter_map(|node| record(&context, source, node, &modules, path))
        .collect()
}
