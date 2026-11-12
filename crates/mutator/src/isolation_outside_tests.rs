//! `--copy` validation: a path outside the project root, at most one leading
//! `..`, and it must exist. The normalised relative path is the position a
//! Windows copy shows beside the project copy, so it may hold no `..` at all.
use super::resolve_outside;
use crate::scratch;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// A scratch root with one project directory: its parent is what `..` reaches.
fn temporary() -> (scratch::Root, PathBuf, PathBuf) {
    let temporary = scratch::Root::new(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let parent = temporary.path().to_path_buf();
    let project = parent.join("project");
    fs::create_dir(&project).unwrap();
    (temporary, project, parent)
}

fn request(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn an_outside_directory_is_resolved_to_the_position_the_copy_shows_it() {
    let (_temporary, project, parent) = temporary();
    fs::create_dir(parent.join("contracts")).unwrap();
    fs::write(parent.join("contracts/value.txt"), "copy-value").unwrap();
    let outside = resolve_outside(&project, &request(&["../contracts"])).unwrap();
    assert_eq!(outside.len(), 1);
    assert_eq!(outside[0].relative, PathBuf::from("contracts"));
    assert_eq!(
        outside[0].source,
        quality_core::paths::resolve(&parent.join("contracts"))
    );
}

#[test]
fn a_nested_outside_directory_keeps_its_relative_position() {
    let (_temporary, project, parent) = temporary();
    fs::create_dir_all(parent.join("shared/contracts")).unwrap();
    let outside = resolve_outside(&project, &request(&["../shared/contracts"])).unwrap();
    assert_eq!(outside[0].relative, PathBuf::from("shared/contracts"));
}

#[test]
fn an_outside_file_is_accepted_as_one_file() {
    let (_temporary, project, parent) = temporary();
    fs::write(parent.join("policy.json"), "{}").unwrap();
    let outside = resolve_outside(&project, &request(&["../policy.json"])).unwrap();
    assert_eq!(outside[0].relative, PathBuf::from("policy.json"));
    assert!(outside[0].source.is_file());
}

#[test]
fn one_position_declared_twice_is_one_outside_copy() {
    let (_temporary, project, parent) = temporary();
    fs::create_dir(parent.join("contracts")).unwrap();
    let outside = resolve_outside(&project, &request(&["../contracts", ".././contracts"])).unwrap();
    assert_eq!(outside.len(), 1, "{outside:?}");
}

#[test]
fn a_path_inside_the_project_root_is_a_usage_error() {
    let (_temporary, project, _parent) = temporary();
    fs::create_dir(project.join("contracts")).unwrap();
    // The copy already has these; accepting one would promise nothing.
    for value in ["contracts", "./contracts", "src/../contracts", "."] {
        let error = resolve_outside(&project, &request(&[value])).unwrap_err();
        assert!(
            error.contains("inside the project root"),
            "{value}: {error}"
        );
        assert!(error.contains(value), "{value}: {error}");
    }
}

#[test]
fn an_absolute_path_is_a_usage_error() {
    let (_temporary, project, parent) = temporary();
    fs::create_dir(parent.join("contracts")).unwrap();
    for value in [parent.join("contracts"), PathBuf::from("/")] {
        let text = value.display().to_string();
        let error = resolve_outside(&project, &request(&[text.as_str()])).unwrap_err();
        assert!(error.contains("not an absolute path"), "{text}: {error}");
        assert!(error.contains(&text), "{text}: {error}");
    }
}

#[test]
fn more_than_one_leading_parent_is_a_usage_error() {
    let (_temporary, project, _parent) = temporary();
    // Two leading `..` would reach outside this call's scratch slot.
    for value in ["../..", "../../elsewhere"] {
        let error = resolve_outside(&project, &request(&[value])).unwrap_err();
        assert!(
            error.contains("at most one leading '..'"),
            "{value}: {error}"
        );
    }
}

#[test]
fn the_parent_directory_itself_is_a_usage_error() {
    let (_temporary, project, _parent) = temporary();
    for value in ["..", "../."] {
        let error = resolve_outside(&project, &request(&[value])).unwrap_err();
        assert!(
            error.contains("parent directory itself"),
            "{value}: {error}"
        );
    }
}

#[test]
fn a_missing_outside_path_is_a_usage_error() {
    let (_temporary, project, _parent) = temporary();
    let error = resolve_outside(&project, &request(&["../missing"])).unwrap_err();
    assert!(error.contains("does not exist"), "{error}");
}

#[test]
fn a_path_that_resolves_back_inside_the_project_is_a_usage_error() {
    let (_temporary, project, _parent) = temporary();
    // The project's own parent entry for the project is the root again.
    let error = resolve_outside(&project, &request(&["../project"])).unwrap_err();
    assert!(
        error.contains("resolves back inside the project root"),
        "{error}"
    );
}

/// A directory link outside the project that points back in, as a Windows
/// junction or a Linux symlink would; the copy must not treat it as outside.
#[cfg(unix)]
#[test]
fn an_outside_link_into_the_project_is_a_usage_error() {
    let (_temporary, project, parent) = temporary();
    std::os::unix::fs::symlink(&project, parent.join("link")).unwrap();
    let error = resolve_outside(&project, &request(&["../link"])).unwrap_err();
    assert!(
        error.contains("resolves back inside the project root"),
        "{error}"
    );
}
