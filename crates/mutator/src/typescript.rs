//! TypeScript mutation: sites, privacy, the per-file test plan and coverage.
use crate::operators;
use quality_core::{
    grammar::{Grammar, Node},
    language::{Language, is_test_file, language_of},
    paths::{relative_posix, resolve},
    process::shlex_quote,
    runner::{modules_with, nearest},
    serde_json::{self, Value},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

/// `_grammar`: `.tsx` parses with the TSX grammar, every other suffix with
/// TypeScript's.
pub fn grammar(path: &Path) -> Grammar {
    if path.to_string_lossy().ends_with(".tsx") {
        Grammar::Tsx
    } else {
        Grammar::TypeScript
    }
}

/// Python `re.search(r"\bprivate\b", header)` with Unicode word characters.
pub(crate) fn mentions_private(header: &str) -> bool {
    let word = |ch: char| ch.is_alphanumeric() || ch == '_';
    header.match_indices("private").any(|(at, _)| {
        let before = header[..at].chars().next_back();
        let after = header[at + "private".len()..].chars().next();
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

/// `_typescript_private`: an ECMAScript `#name` or the `private` keyword in
/// the declaration's first lines.
pub fn private(name: &str, header: &str) -> bool {
    name.starts_with('#') || mentions_private(header)
}

/// The leaf's site under the original's TypeScript rules: `>>>` has one
/// mutant, `<<`.
pub fn site(node: Node, token: &str) -> Option<(String, &'static str)> {
    operators::leaf(node, token, |token| {
        if token == ">>>" {
            return Some("<<");
        }
        operators::bitwise(token).or_else(|| operators::binary(token))
    })
}

/// `test_plan(root, source, "typescript", None)`: `npm test` at the package.
pub fn test_plan(root: &Path, source: &Path) -> (Vec<String>, PathBuf) {
    let root = resolve(root);
    let directory = nearest(source, "package.json", &root).unwrap_or(root);
    (vec!["npm".into(), "test".into()], directory)
}

const VITEST_CONFIGS: &[&str] = &[
    "vitest.config.ts",
    "vitest.config.mts",
    "vitest.config.cts",
    "vitest.config.js",
    "vitest.config.mjs",
    "vitest.config.cjs",
];

fn json(path: &Path) -> Option<Value> {
    let value: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    value.is_object().then_some(value)
}

/// One package's coverage: a single view command, so the install, the tests
/// and the report share one upper layer.
pub struct Coverage {
    pub script: String,
    /// Project-relative report the caller reads from the view's upper layer.
    pub report: PathBuf,
    /// `npm run coverage` owns its report location; the others are anchored.
    pub custom: bool,
}

/// `runners._typescript_command`: the project's script, Vitest, or c8.
pub fn coverage(root: &Path, package: &Path, files: &[PathBuf]) -> Option<Coverage> {
    let data = json(&package.join("package.json"))?;
    let scripts = data.get("scripts").and_then(Value::as_object);
    let report = quality_core::runner::coverage_report(root, package, "typescript");
    let directory = report.parent().expect("coverage report directory");
    let quoted = |path: &Path| shlex_quote(&path.to_string_lossy());
    let custom = scripts.is_some_and(|scripts| scripts.contains_key("coverage"));
    let mut script = format!("rm -rf -- {0} && mkdir -p {0}\n", quoted(directory));
    let command = if custom {
        vec!["npm".into(), "run".into(), "coverage".into()]
    } else {
        let uses_vitest = scripts
            .and_then(|scripts| scripts.get("test"))
            .is_some_and(|test| {
                test.as_str().map_or_else(
                    || test.to_string().contains("vitest"),
                    |s| s.contains("vitest"),
                )
            })
            || VITEST_CONFIGS
                .iter()
                .any(|name| package.join(name).is_file());
        if uses_vitest {
            if !package.join("node_modules/@vitest/coverage-v8").is_dir() {
                let installed = json(&package.join("node_modules/vitest/package.json"));
                let version = installed
                    .as_ref()
                    .and_then(|data| data.get("version"))
                    .and_then(Value::as_str);
                let spec = version.map_or_else(
                    || "@vitest/coverage-v8".into(),
                    |version| format!("@vitest/coverage-v8@{version}"),
                );
                script.push_str(&format!(
                    "echo 'Installing {spec} so Vitest can write LCOV. The install happens in the view; package.json and the lockfile are never changed in the project.' >&2\n\
                     npm install --no-save --no-package-lock {spec} || exit 0\n"
                ));
            }
            let binary = package.join("node_modules/.bin/vitest");
            let mut command = if binary.is_file() {
                vec![binary.to_string_lossy().into_owned()]
            } else {
                vec!["npx".into(), "vitest".into()]
            };
            command.extend([
                "run".into(),
                "--coverage".into(),
                "--coverage.reporter=lcov".into(),
                format!("--coverage.reportsDirectory={}", directory.display()),
                "--coverage.reportOnFailure=true".into(),
            ]);
            let package = resolve(package);
            for file in files {
                if language_of(file) != Some(Language::TypeScript) || is_test_file(file) {
                    continue;
                }
                if let Some(relative) = relative_posix(&resolve(file), &package) {
                    command.push(format!("--coverage.include={relative}"));
                }
            }
            command
        } else {
            if !scripts.is_some_and(|scripts| scripts.contains_key("test")) {
                return None;
            }
            ["npx", "--yes", "c8", "--reporter=lcov", "--reports-dir"]
                .into_iter()
                .map(str::to_string)
                .chain([
                    directory.to_string_lossy().into_owned(),
                    "npm".into(),
                    "test".into(),
                ])
                .collect()
        }
    };
    let rendered = command
        .iter()
        .map(|arg| shlex_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    script.push_str(&format!(
        "{rendered}\ncode=$?\n\
         if [ \"$code\" != 0 ]; then echo \"TypeScript coverage exited $code in {}. TypeScript coverage will be N/A.\" >&2; exit 0; fi\n",
        package.display()
    ));
    if !custom {
        script.push_str(&format!(
            "if [ ! -f {} ]; then echo 'TypeScript coverage wrote no {}. TypeScript coverage will be N/A.' >&2; fi\n",
            quoted(&report),
            report.display()
        ));
    }
    Some(Coverage {
        script,
        // `npm run coverage` writes the project's own report; the original
        // reads coverage/lcov.info from the root without anchoring it.
        report: if custom {
            PathBuf::from("coverage/lcov.info")
        } else {
            report
                .strip_prefix(resolve(root))
                .unwrap_or(&report)
                .to_path_buf()
        },
        custom,
    })
}

/// Every TypeScript package among the selected files.
pub fn coverage_roots(root: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    modules_with(files, Language::TypeScript, "package.json", root)
}

#[cfg(test)]
mod tests {
    //! Ported from the original `tests/test_runner.py` TypeScript plan cases.
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    #[test]
    fn typescript_plan_is_npm_test_at_the_nearest_package() {
        let root = std::env::temp_dir().join(format!(
            "mutator-typescript-plan-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.ts"), "export const a = 1;\n").unwrap();
        assert_eq!(
            test_plan(&root, &root.join("src/a.ts")),
            (vec!["npm".into(), "test".into()], resolve(&root))
        );
        fs::write(root.join("package.json"), "{}\n").unwrap();
        fs::create_dir_all(root.join("nested/src")).unwrap();
        fs::write(root.join("nested/src/a.ts"), "export const a = 1;\n").unwrap();
        fs::write(root.join("nested/package.json"), "{}\n").unwrap();
        assert_eq!(
            test_plan(&root, &root.join("nested/src/a.ts")).1,
            root.join("nested")
        );
        let _ = fs::remove_dir_all(&root);
    }
}
