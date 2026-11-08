//! Byte-exact oracle snapshots produced by pinned Python dryer (see fixture README).
use super::crap::Project;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}
fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());
}
fn args(project: &Project, scenario: &Path) -> Vec<String> {
    fs::read_to_string(scenario)
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (word, rest) = line.split_once(' ').unwrap();
            match word {
                "project" => {
                    copy_tree(&repo().join(format!("fixtures/dry/{rest}")), &project.root);
                    None
                }
                "arg" => Some(rest.to_string()),
                "git" if rest == "init" => {
                    git(&project.root, &["init", "--quiet"]);
                    None
                }
                "git" if rest == "add" => {
                    git(&project.root, &["add", "."]);
                    None
                }
                "commit" => {
                    let paths =
                        [&["add", "--"][..], &rest.split(' ').collect::<Vec<_>>()[..]].concat();
                    git(&project.root, &paths);
                    git(
                        &project.root,
                        &[
                            "-c",
                            "user.name=oracle",
                            "-c",
                            "user.email=<REDACTED_EMAIL>",
                            "commit",
                            "--quiet",
                            "-m",
                            "base",
                        ],
                    );
                    None
                }
                "modify" => {
                    let path = project.root.join(rest);
                    fs::write(
                        &path,
                        fs::read_to_string(&path).unwrap()
                            + "// Modified for a git status selection.\n",
                    )
                    .unwrap();
                    None
                }
                "rename" => {
                    let (from, to) = rest.split_once(' ').unwrap();
                    fs::rename(project.root.join(from), project.root.join(to)).unwrap();
                    None
                }
                _ => panic!("unknown scenario line {line}"),
            }
        })
        .collect()
}
fn run(project: &Project, args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_uml-viewer"))
        .arg("dry")
        .arg("--root")
        .arg(&project.root)
        .args(args)
        .current_dir(&project.root)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .unwrap()
}
fn normalize(stderr: &[u8], root: &Path) -> String {
    let text = String::from_utf8(stderr.to_vec()).unwrap();
    text.replace(root.to_string_lossy().as_ref(), "<ROOT>")
        .replace(root.to_string_lossy().replace('\\', "/").as_str(), "<ROOT>")
        .replace('\\', "/")
        .replace("\r\n", "\n")
}
#[test]
fn every_scenario_matches_the_pinned_python_dryer_byte_for_byte() {
    let goldens = repo().join("fixtures/dry/goldens");
    let mut scenarios: Vec<_> = fs::read_dir(&goldens)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("scenario").is_file())
        .collect();
    scenarios.sort();
    for scenario in scenarios {
        let project = Project::new();
        let argv = args(&project, &scenario.join("scenario"));
        let actual = run(&project, &argv);
        let name = scenario.file_name().unwrap().to_string_lossy();
        let read = |file: &str| fs::read(scenario.join(file)).unwrap();
        assert_eq!(
            actual.status.code().unwrap() as u8,
            read("exit")[0] - b'0',
            "{name}"
        );
        assert_eq!(actual.stdout, read("stdout"), "{name}");
        assert_eq!(
            normalize(&actual.stderr, &project.root).as_bytes(),
            read("stderr"),
            "{name}"
        );
        let expected = scenario.join("dry.edn");
        let actual_path = project.root.join(".metrics/dry.edn");
        assert_eq!(actual_path.exists(), expected.exists(), "{name}");
        if expected.exists() {
            assert_eq!(
                fs::read(actual_path).unwrap(),
                fs::read(expected).unwrap(),
                "{name}"
            );
        }
    }
}
