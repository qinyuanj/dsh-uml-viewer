//! CLI behaviour of the `ir` role (#136): usage errors, missing policies and
//! language detection, mirroring the POSIX entry expectations. These run
//! `ir::run` in a process with a changed working directory, so they are
//! serial by file and restore the directory afterwards.
// Test code is the outside harness: it launches its own children.
#![allow(clippy::disallowed_methods)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("uml-ir-cli-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_in(dir: &Path, args: &[&str]) -> ExitCode {
    let args: Vec<OsString> = args.iter().map(OsString::from).collect();
    // --root keeps this process-global cwd untouched.
    let mut full: Vec<OsString> = vec![OsString::from("--root"), dir.as_os_str().to_owned()];
    full.extend(args);
    ir::run(&full)
}

#[test]
fn a_missing_policy_fails_and_preserves_previous_output() {
    let dir = tempdir("missing");
    let output = dir.join("result.edn");
    std::fs::write(&output, "previous").unwrap();
    let code = run_in(&dir, &["missing.edn", "result.edn"]);
    assert_eq!(ExitCode::from(1), code);
    assert_eq!("previous", std::fs::read_to_string(&output).unwrap());
}

#[test]
fn ambiguous_markers_require_an_explicit_language() {
    let dir = tempdir("ambiguous");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.lua"), "return {}\n").unwrap();
    std::fs::write(dir.join("Cargo.toml"), "").unwrap();
    std::fs::write(dir.join("ambiguous.edn"), "{:src \"src\" :prefix \"app\"}").unwrap();
    let code = run_in(&dir, &["ambiguous.edn", "ambiguous-result.edn"]);
    assert_eq!(ExitCode::from(1), code);
    assert!(!dir.join("ambiguous-result.edn").exists());
}

#[test]
fn detection_uses_the_single_marker_language() {
    let dir = tempdir("detect");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.py"), "def hello():\n    return True\n").unwrap();
    std::fs::write(dir.join("pyproject.toml"), "").unwrap();
    std::fs::write(dir.join("detect.edn"), "{:src \"src\" :prefix \"app\"}").unwrap();
    let code = run_in(&dir, &["detect.edn", "detected.edn"]);
    assert_eq!(ExitCode::SUCCESS, code);
    let document = std::fs::read_to_string(dir.join("detected.edn")).unwrap();
    assert!(document.contains(":lang :python"), "{document}");
    assert!(document.contains("hello"), "{document}");
}

#[test]
fn an_unknown_option_is_a_usage_failure() {
    let dir = tempdir("usage");
    assert_eq!(ExitCode::from(1), run_in(&dir, &["--bogus"]));
}

#[test]
fn duplicate_output_options_are_refused() {
    let dir = tempdir("duplicate");
    assert_eq!(
        ExitCode::from(1),
        run_in(&dir, &["--output", "a.edn", "--output", "b.edn"])
    );
}

fn assert_source_link_refused(dir: &Path) {
    std::fs::write(
        dir.join("policy.edn"),
        "{:lang :lua :src \"src\" :prefix \"app\"}",
    )
    .unwrap();
    let code = run_in(dir, &["policy.edn", "result.edn"]);
    assert_eq!(ExitCode::from(1), code);
    assert!(!dir.join("result.edn").exists());
}

#[cfg(unix)]
#[test]
fn source_symlinks_are_refused() {
    let dir = tempdir("symlink");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.lua"), "return {}\n").unwrap();
    std::os::unix::fs::symlink(dir.join("src"), dir.join("src/cycle")).unwrap();
    assert_source_link_refused(&dir);
}

/// A directory junction is Windows' unprivileged directory link (symlinks
/// need Developer Mode), so it is the link a Windows source tree really has.
#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let status = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "mklink /J failed");
}

#[cfg(windows)]
#[test]
fn source_junctions_are_refused() {
    let dir = tempdir("junction");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.lua"), "return {}\n").unwrap();
    junction(&dir.join("src").join("cycle"), &dir.join("src"));
    assert_source_link_refused(&dir);
}

#[cfg(windows)]
#[test]
fn a_junction_to_a_file_tree_outside_the_root_is_refused() {
    let outside = tempdir("junction-outside");
    std::fs::write(outside.join("b.lua"), "return {}\n").unwrap();
    let dir = tempdir("junction-out");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.lua"), "return {}\n").unwrap();
    junction(&dir.join("src").join("elsewhere"), &outside);
    assert_source_link_refused(&dir);
}

#[cfg(windows)]
#[test]
fn a_windows_root_yields_slash_separated_relative_files() {
    // The root arrives as a native `C:\…` path; IR text must carry only
    // project-relative `/` paths: no drive letter, backslash or `\\?\`.
    let dir = tempdir("native-root");
    std::fs::create_dir_all(dir.join("src").join("pkg")).unwrap();
    std::fs::write(
        dir.join("src").join("pkg").join("mod.lua"),
        "local M = {}\nreturn M\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("policy.edn"),
        "{:lang :lua :src \"src\" :prefix \"app\"}",
    )
    .unwrap();
    let output = dir.join("result.edn");
    let code = run_in(&dir, &["policy.edn", output.to_str().unwrap()]);
    assert_eq!(ExitCode::SUCCESS, code);
    let document = std::fs::read_to_string(&output).unwrap();
    assert!(
        document.contains(r#":file "src/pkg/mod.lua""#),
        "{document}"
    );
    assert!(!document.contains('\\'), "{document}");
    assert!(!document.contains("uml-ir-cli-native-root"), "{document}");
    assert!(!document.contains('\r'), "{document}");
}

// A backslash is a separator in a Windows file name, so this name only
// exists on POSIX file systems.
#[cfg(unix)]
#[test]
fn a_backslash_in_a_posix_file_name_is_kept() {
    let dir = tempdir("backslash");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/odd\\name.lua"), "local M = {}\nreturn M\n").unwrap();
    std::fs::write(
        dir.join("policy.edn"),
        "{:lang :lua :src \"src\" :prefix \"app\"}",
    )
    .unwrap();
    let code = run_in(&dir, &["policy.edn", "result.edn"]);
    assert_eq!(ExitCode::SUCCESS, code);
    let document = std::fs::read_to_string(dir.join("result.edn")).unwrap();
    assert!(
        document.contains(r#":file "src/odd\\name.lua""#),
        "{document}"
    );
}

#[test]
fn an_ir_document_given_as_the_policy_is_refused_without_writing() {
    let dir = tempdir("ir-as-policy");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.lua"), "return {}\n").unwrap();
    std::fs::write(
        dir.join("previous.edn"),
        ";; Generated by uml ir from the policy file. Do not edit.\n\
         {:hierarchical true :title \"UML\" :prefix \"app\" :lang :lua :levels [] :order [] \
         :omit [] :policy-file \"policy.edn\" :classes [] :edges []}\n",
    )
    .unwrap();
    assert_eq!(ExitCode::from(1), run_in(&dir, &["previous.edn"]));
    assert!(!dir.join("diagram.edn").exists());
    assert!(!dir.join("examples").exists());
}

#[test]
fn an_ir_document_given_as_policy_and_output_keeps_its_bytes() {
    let dir = tempdir("ir-as-both");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.lua"), "return {}\n").unwrap();
    let ir = ";; Generated by uml ir from the policy file. Do not edit.\n\
              {:hierarchical true :title \"UML\" :prefix \"app\" :lang :lua :levels [] :order [] \
              :omit [] :policy-file \"policy.edn\" :classes [] :edges []}\n";
    std::fs::write(dir.join("previous.edn"), ir).unwrap();
    assert_eq!(
        ExitCode::from(1),
        run_in(&dir, &["previous.edn", "previous.edn"])
    );
    assert_eq!(
        ir,
        std::fs::read_to_string(dir.join("previous.edn")).unwrap()
    );
    assert!(!dir.join("diagram.edn").exists());
}

#[test]
fn a_tsconfig_beside_the_source_root_resolves_path_aliases() {
    let dir = tempdir("tsconfig");
    std::fs::create_dir_all(dir.join("src/lib")).unwrap();
    std::fs::write(
        dir.join("tsconfig.json"),
        "{ \"compilerOptions\": { \"paths\": { \"@/*\": [\"./src/*\"] } } }\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/lib/util.ts"), "export const util = 1;\n").unwrap();
    std::fs::write(
        dir.join("src/page.ts"),
        "import { util } from \"@/lib/util\";\nexport const page = util;\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("policy.edn"),
        "{:lang :typescript :src \"src\" :prefix \"app\"}",
    )
    .unwrap();
    assert_eq!(
        ExitCode::SUCCESS,
        run_in(&dir, &["policy.edn", "result.edn"])
    );
    let document = std::fs::read_to_string(dir.join("result.edn")).unwrap();
    assert!(
        document.contains("{:from :page :kind :dependency :to :lib.util}"),
        "{document}"
    );
}

#[test]
fn a_policy_without_a_source_declaration_still_detects_its_language() {
    // #166 out of scope: :src and :lang stay optional (src default + detection).
    let dir = tempdir("no-source");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.py"), "def hello():\n    return True\n").unwrap();
    std::fs::write(dir.join("pyproject.toml"), "").unwrap();
    std::fs::write(dir.join("minimal.edn"), "{:prefix \"app\"}").unwrap();
    assert_eq!(
        ExitCode::SUCCESS,
        run_in(&dir, &["minimal.edn", "result.edn"])
    );
    let document = std::fs::read_to_string(dir.join("result.edn")).unwrap();
    assert!(document.contains(":lang :python"), "{document}");
    assert!(document.contains("hello"), "{document}");
}
