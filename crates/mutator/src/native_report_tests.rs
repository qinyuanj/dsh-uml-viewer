use super::*;
use quality_core::native_coverage::{NativeReports, jacoco_lines};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = crate::scratch::fixture().join(format!(
            "mutator-native-reports-{}-{stamp}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        Self(quality_core::paths::resolve(&directory))
    }
    fn root(&self) -> PathBuf {
        self.0.join("host")
    }
    fn view(&self, name: &str) -> View {
        View::reused(&self.0.join(name))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn put(path: impl AsRef<Path>, text: &str) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn go_profile(source: &str, hits: usize) -> String {
    format!("mode: set\n{source}:7.1,7.9 1 {hits}\n")
}
fn jacoco(hits: usize) -> String {
    format!(
        "<report><package name=\"p\"><sourcefile name=\"Foo.java\">\
         <line nr=\"7\" ci=\"{hits}\" cb=\"0\"/></sourcefile></package></report>"
    )
}
fn source(module: &Path, language: Language) -> PathBuf {
    match language {
        Language::Go => module.join("src/foo.go"),
        Language::Java => module.join("src/main/java/p/Foo.java"),
        _ => unreachable!(),
    }
}
fn module_report(base: &Path, module: &str, language: Language, hits: usize) {
    let module = base.join(module);
    match language {
        Language::Go => put(
            module.join("target/coverage/go/coverage.out"),
            &go_profile("src/foo.go", hits),
        ),
        Language::Java => put(module.join("target/site/jacoco/jacoco.xml"), &jacoco(hits)),
        _ => unreachable!(),
    }
}
fn exact_scope(root: &Path, view: &View, module: &Path, language: Language) -> ModuleCoverage {
    let mut coverage = Coverage::default();
    merge_module_native(&mut coverage, view, root, module, language).unwrap();
    coverage.modules.pop().unwrap()
}
fn contaminated_global(base: &Path, coverage: &mut Coverage, language: Language) {
    if language == Language::Go {
        coverage.native = Some(NativeReports::load(base).unwrap());
    } else {
        coverage.java = Some(jacoco_lines(base));
    }
}
fn selected_lines(coverage: &Coverage, root: &Path, file: &Path) -> Option<BTreeSet<usize>> {
    covered_lines(
        coverage,
        root,
        file,
        &quality_core::paths::posix(&relative(root, file)),
    )
}
fn with_marker(module: &Path, language: Language) {
    let marker = if language == Language::Go {
        "go.mod"
    } else {
        "pom.xml"
    };
    put(module.join(marker), "");
}

#[test]
fn generated_go_scope_ignores_a_copied_sibling_profile() {
    for (selected, sibling) in [("a", "b"), ("b", "a")] {
        let fixture = Fixture::new();
        let root = fixture.root();
        let selected_source = source(&root.join(selected), Language::Go);
        put(&selected_source, "source");
        let view = fixture.view(selected);
        let base = isolation::report_path(&view, Path::new(""));
        put(
            base.join(selected).join("target/coverage/go/coverage.out"),
            &go_profile("src/current.go", 0),
        );
        put(
            base.join(sibling).join("target/coverage/go/coverage.out"),
            &go_profile("src/foo.go", 1),
        );
        let mut coverage = Coverage::default();
        contaminated_global(&base, &mut coverage, Language::Go);
        coverage.modules.push(exact_scope(
            &root,
            &view,
            &root.join(selected),
            Language::Go,
        ));
        assert_eq!(
            selected_lines(&coverage, &root, &selected_source),
            None,
            "Go {selected} borrowed {sibling}'s copied profile"
        );
    }
}

#[test]
fn generated_java_scope_ignores_a_copied_sibling_report_with_the_same_key() {
    for (selected, sibling) in [("a", "b"), ("b", "a")] {
        let fixture = Fixture::new();
        let root = fixture.root();
        let selected_source = source(&root.join(selected), Language::Java);
        put(&selected_source, "source");
        let view = fixture.view(selected);
        let base = isolation::report_path(&view, Path::new(""));
        put(
            base.join(selected).join("target/site/jacoco/jacoco.xml"),
            &jacoco(0)
                .replace("Foo.java", "Current.java")
                .replace("nr=\"7\"", "nr=\"3\""),
        );
        module_report(&base, sibling, Language::Java, 1);
        let mut coverage = Coverage::default();
        contaminated_global(&base, &mut coverage, Language::Java);
        coverage.modules.push(exact_scope(
            &root,
            &view,
            &root.join(selected),
            Language::Java,
        ));
        assert_eq!(
            selected_lines(&coverage, &root, &selected_source),
            None,
            "Java {selected} borrowed {sibling}'s copied report"
        );
    }
}

#[test]
fn generated_scope_keeps_fresh_zero_hits_and_reports_absent_sources() {
    for language in [Language::Go, Language::Java] {
        let fixture = Fixture::new();
        let root = fixture.root();
        let view = fixture.view("copy");
        let base = isolation::report_path(&view, Path::new(""));
        module_report(&base, "", language, 0);
        let mut coverage = Coverage::default();
        coverage
            .modules
            .push(exact_scope(&root, &view, &root, language));
        put(source(&root, language), "source");
        assert_eq!(
            selected_lines(&coverage, &root, &source(&root, language)),
            Some(BTreeSet::new()),
            "{language:?} treated a fresh zero as absent"
        );
        let absent = if language == Language::Go {
            root.join("absent.go")
        } else {
            root.join("src/main/java/p/Absent.java")
        };
        assert_eq!(
            selected_lines(&coverage, &root, &absent),
            None,
            "{language:?} reported a source its report never named"
        );
    }
}

#[test]
fn two_full_copy_views_merge_only_the_module_each_command_refreshed() {
    for language in [Language::Go, Language::Java] {
        for order in [["a", "b"], ["b", "a"]] {
            let fixture = Fixture::new();
            let root = fixture.root();
            let mut coverage = Coverage::default();
            let mut selected = Vec::new();
            for module in ["a", "b"] {
                let file = if language == Language::Go {
                    root.join(module).join(format!("{module}.go"))
                } else {
                    root.join(module)
                        .join(format!("src/main/java/{module}/Foo.java"))
                };
                put(&file, "source");
                selected.push(file);
            }
            for fresh in order {
                let view = fixture.view(fresh);
                let base = isolation::report_path(&view, Path::new(""));
                for module in ["a", "b"] {
                    let hits = usize::from(module != fresh);
                    if language == Language::Go {
                        put(
                            base.join(module).join("target/coverage/go/coverage.out"),
                            &go_profile(&format!("{module}.go"), hits),
                        );
                    } else {
                        put(
                            base.join(module).join("target/site/jacoco/jacoco.xml"),
                            &jacoco(hits).replace("name=\"p\"", &format!("name=\"{module}\"")),
                        );
                    }
                }
                merge_module_native(&mut coverage, &view, &root, &root.join(fresh), language)
                    .unwrap();
            }
            for file in selected {
                assert_eq!(
                    selected_lines(&coverage, &root, &file),
                    Some(BTreeSet::new()),
                    "{language:?} {} retained the stale sibling's positive hit",
                    file.display()
                );
            }
        }
    }
}

#[test]
fn custom_native_reads_the_view_and_rebases_absolute_go_keys() {
    let fixture = Fixture::new();
    let root = fixture.root();
    put(root.join("go.mod"), "module example.com/app\n");
    put(root.join("pom.xml"), "<project/>");
    let go = root.join("src/foo.go");
    let java = source(&root, Language::Java);
    let clojure = root.join("src/demo/core.clj");
    for file in [&go, &java, &clojure] {
        put(file, "source");
    }
    module_report(&root, "", Language::Go, 0);
    module_report(&root, "", Language::Java, 0);
    let view = fixture.view("copy");
    let base = isolation::report_path(&view, Path::new(""));
    put(
        base.join("target/coverage/go/coverage.out"),
        &go_profile(&quality_core::paths::posix(&base.join("src/foo.go")), 1),
    );
    module_report(&base, "", Language::Java, 1);
    put(
        base.join("target/coverage/src/demo/core.clj.html"),
        "<span title=\"1 out of 1 forms covered\">7&nbsp;</span>",
    );
    let mut coverage = Coverage::default();
    collect_custom_native(
        &root,
        &[go.clone(), java.clone(), clojure.clone()],
        &mut coverage,
        &view,
    )
    .unwrap();
    for file in [&go, &java, &clojure] {
        assert_eq!(
            selected_lines(&coverage, &root, file),
            Some(BTreeSet::from([7])),
            "{}",
            file.display()
        );
    }
    assert_eq!(
        coverage
            .native
            .as_ref()
            .unwrap()
            .go_lines(&quality_core::paths::posix(&go)),
        Some(BTreeSet::from([7]))
    );
}

#[test]
fn custom_module_without_its_own_report_reads_the_root_globs() {
    for language in [Language::Go, Language::Java] {
        for (present, absent) in [("a", "b"), ("b", "a")] {
            let fixture = Fixture::new();
            let root = fixture.root();
            for module in [present, absent] {
                with_marker(&root.join(module), language);
                put(source(&root.join(module), language), "source");
            }
            let view = fixture.view("copy");
            let base = isolation::report_path(&view, Path::new(""));
            module_report(&base, present, language, 1);
            let files = [
                source(&root.join(present), language),
                source(&root.join(absent), language),
            ];
            let mut coverage = Coverage::default();
            assert_eq!(
                collect_custom_native(&root, &files, &mut coverage, &view),
                Ok(())
            );
            assert_eq!(
                coverage.modules.len(),
                1,
                "{language:?} pushed a scope for {absent}, which has no report"
            );
            for file in &files {
                assert_eq!(
                    selected_lines(&coverage, &root, file),
                    Some(BTreeSet::from([7])),
                    "{language:?} {}",
                    file.display()
                );
            }
        }
    }
}

#[test]
fn custom_cloverage_omitting_a_namespace_warns_instead_of_failing() {
    let fixture = Fixture::new();
    let root = fixture.root();
    put(root.join("deps.edn"), "{}");
    let measured = root.join("src/demo/core.clj");
    let omitted = root.join("src/demo/extra.clj");
    for file in [&measured, &omitted] {
        put(file, "source");
    }
    let view = fixture.view("copy");
    let base = isolation::report_path(&view, Path::new(""));
    put(base.join("coverage/lcov.info"), "");
    put(
        base.join("target/coverage/src/demo/core.clj.html"),
        "<span title=\"1 out of 1 forms covered\">7&nbsp;</span>",
    );
    let mut coverage = Coverage::default();
    assert_eq!(
        collect_custom_native(
            &root,
            &[measured.clone(), omitted.clone()],
            &mut coverage,
            &view
        ),
        Ok(()),
        "a namespace without data must not abort the run"
    );
    assert_eq!(
        selected_lines(&coverage, &root, &measured),
        Some(BTreeSet::from([7]))
    );
    assert_eq!(selected_lines(&coverage, &root, &omitted), None);
}

#[test]
fn custom_root_report_covers_a_nested_module_without_its_own_report() {
    for language in [Language::Java, Language::Go] {
        let fixture = Fixture::new();
        let root = fixture.root();
        with_marker(&root, language);
        let app = root.join("app");
        with_marker(&app, language);
        let file = if language == Language::Java {
            app.join("src/main/java/p/A.java")
        } else {
            app.join("foo.go")
        };
        put(&file, "source");
        let view = fixture.view("copy");
        let base = isolation::report_path(&view, Path::new(""));
        if language == Language::Java {
            put(
                base.join("target/site/jacoco/jacoco.xml"),
                &jacoco(1).replace("Foo.java", "A.java"),
            );
        } else {
            put(
                base.join("target/coverage/go/coverage.out"),
                &go_profile(&quality_core::paths::posix(&base.join("app/foo.go")), 1),
            );
        }
        let mut coverage = Coverage::default();
        assert_eq!(
            collect_custom_native(&root, std::slice::from_ref(&file), &mut coverage, &view),
            Ok(())
        );
        assert!(
            coverage.modules.is_empty(),
            "{language:?} app has no report, so it must not get a scope"
        );
        assert_eq!(
            selected_lines(&coverage, &root, &file),
            Some(BTreeSet::from([7])),
            "{language:?} ignored the root-level report"
        );
    }
}

#[test]
fn custom_reads_an_unchanged_project_report_through_the_view() {
    let fixture = Fixture::new();
    let root = fixture.root();
    put(root.join("go.mod"), "module example.com/app\n");
    let go = root.join("src/foo.go");
    put(&go, "source");
    let view = fixture.view("copy");
    let base = isolation::report_path(&view, Path::new(""));
    let unchanged = if cfg!(windows) {
        base.clone()
    } else {
        root.clone()
    };
    put(
        unchanged.join("target/coverage/go/coverage.out"),
        &go_profile(&quality_core::paths::posix(&go), 1),
    );
    put(base.join("coverage/lcov.info"), "");
    put(base.join("target/coverage/typescript/lcov.info"), "");
    let mut coverage = Coverage::default();
    assert_eq!(
        collect_custom_native(&root, std::slice::from_ref(&go), &mut coverage, &view),
        Ok(())
    );
    assert_eq!(
        selected_lines(&coverage, &root, &go),
        Some(BTreeSet::from([7])),
        "the unchanged project Go profile was not read"
    );
}

#[cfg(target_os = "linux")]
fn whiteout(path: &Path) {
    use quality_core::libc;
    use std::os::unix::ffi::OsStrExt;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `name` is NUL-terminated and outlives the call. Linux 5.8+
    // lets an unprivileged user create the 0/0 whiteout device.
    let made = unsafe { libc::mknod(name.as_ptr(), libc::S_IFCHR | 0o600, 0) };
    assert_eq!(
        made,
        0,
        "mknod whiteout: {}",
        std::io::Error::last_os_error()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn custom_whiteout_in_the_view_keeps_a_deleted_project_report_absent() {
    let fixture = Fixture::new();
    let root = fixture.root();
    put(root.join("go.mod"), "module example.com/app\n");
    let go = root.join("src/foo.go");
    put(&go, "source");
    let report = Path::new("target/coverage/go/coverage.out");
    put(
        root.join(report),
        &go_profile(&quality_core::paths::posix(&go), 1),
    );
    let view = fixture.view("copy");
    let base = isolation::report_path(&view, Path::new(""));
    whiteout(&base.join(report));
    put(base.join("coverage/lcov.info"), "");
    assert_eq!(isolation::report_tree(&base, &root).file(report), None);
    let mut coverage = Coverage::default();
    assert_eq!(
        collect_custom_native(&root, std::slice::from_ref(&go), &mut coverage, &view),
        Ok(())
    );
    assert!(coverage.modules.is_empty(), "a deleted report got a scope");
    assert_eq!(
        selected_lines(&coverage, &root, &go),
        None,
        "the whiteout resurrected the project's deleted profile"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn custom_opaque_view_directory_hides_the_project_reports_below_it() {
    use quality_core::libc;
    use std::os::unix::ffi::OsStrExt;
    let fixture = Fixture::new();
    let root = fixture.root();
    let report = Path::new("target/coverage/go/coverage.out");
    put(root.join(report), &go_profile("src/foo.go", 1));
    let view = fixture.view("copy");
    let base = isolation::report_path(&view, Path::new(""));
    let directory = base.join("target/coverage/go");
    fs::create_dir_all(&directory).unwrap();
    let name = std::ffi::CString::new(directory.as_os_str().as_bytes()).unwrap();
    // SAFETY: both strings are NUL-terminated; the value is one byte long.
    let set = unsafe {
        libc::lsetxattr(
            name.as_ptr(),
            c"user.overlay.opaque".as_ptr(),
            b"y".as_ptr().cast(),
            1,
            0,
        )
    };
    if set != 0 {
        let reason = format!(
            "custom_opaque_view_directory_hides_the_project_reports_below_it needs user xattrs; \
             {} has none: {}",
            directory.display(),
            std::io::Error::last_os_error()
        );
        assert!(
            !std::env::var_os("MUTATOR_REQUIRE_TOOLCHAINS").is_some_and(|value| value == "1"),
            "MUTATOR_REQUIRE_TOOLCHAINS=1; {reason}"
        );
        use std::io::Write;
        let _ = std::io::stderr().write_all(format!("SKIP: {reason}\n").as_bytes());
        return;
    }
    let tree = isolation::report_tree(&base, &root);
    assert_eq!(tree.file(report), None);
    put(root.join("coverage.out"), &go_profile("src/foo.go", 1));
    assert_eq!(
        tree.file(Path::new("coverage.out")),
        Some(root.join("coverage.out"))
    );
}

#[test]
fn go_root_source_does_not_borrow_a_subpackage_with_the_same_basename() {
    let fixture = Fixture::new();
    let root = fixture.root();
    let root_source = root.join("foo.go");
    put(&root_source, "source");
    let view = fixture.view("copy");
    let base = isolation::report_path(&view, Path::new(""));
    put(
        base.join("target/coverage/go/coverage.out"),
        &format!(
            "{}{}",
            go_profile("example.com/app/sub/foo.go", 0),
            go_profile(&quality_core::paths::posix(&base.join("foo.go")), 1)
        ),
    );
    let mut coverage = Coverage::default();
    coverage
        .modules
        .push(exact_scope(&root, &view, &root, Language::Go));
    assert_eq!(
        selected_lines(&coverage, &root, &root_source),
        Some(BTreeSet::from([7]))
    );
    assert_eq!(
        selected_lines(&coverage, &root, &root.join("other/foo.go")),
        None
    );
}
