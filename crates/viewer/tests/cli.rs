//! Argument parsing and process identity (ports of the LOVE `parse_args`
//! and the POSIX identity contract in `docs/herdr-backend-contract.md` §9.1).

use std::ffi::OsString;
use std::path::PathBuf;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn parses_backend_restart_and_one_positional_path() {
    let options = viewer::cli::parse(&args(&["--backend", "herdr", "--restart", "d.edn"])).unwrap();
    assert!(options.backend);
    assert!(options.restart);
    assert_eq!(Some(PathBuf::from("d.edn")), options.path);
}

#[test]
fn a_bare_diagram_path_is_the_common_case() {
    let options = viewer::cli::parse(&args(&["diagram.edn"])).unwrap();
    assert_eq!(Some(PathBuf::from("diagram.edn")), options.path);
    assert!(!options.backend && !options.restart);
}

#[test]
fn extra_positionals_are_ignored_like_the_lua_parser() {
    let options = viewer::cli::parse(&args(&["a.edn", "b.edn"])).unwrap();
    assert_eq!(Some(PathBuf::from("a.edn")), options.path);
}

#[test]
fn rejects_unknown_options_and_non_herdr_backends() {
    assert!(viewer::cli::parse(&args(&["-x"])).is_err());
    let error = viewer::cli::parse(&args(&["--backend", "tmux"])).unwrap_err();
    assert_eq!(
        "Unknown or duplicate companion backend; use --backend herdr",
        error
    );
    assert!(viewer::cli::parse(&args(&["--backend"])).is_err());
    assert!(viewer::cli::parse(&args(&["--backend", "herdr", "--backend", "herdr"])).is_err());
}

#[test]
fn identity_json_keeps_the_fixed_key_order_and_shape() {
    let text = viewer::identity::encode(1234, "5381724", "/abs/root");
    assert_eq!(
        "{\"pid\":1234,\"started\":\"5381724\",\"runtime\":\"rust\",\"cwd\":\"/abs/root\"}",
        text
    );
}

#[test]
fn starttime_reads_proc_stat_field_22() {
    // This very process exists in /proc; its starttime is all digits.
    let started = viewer::identity::starttime(std::process::id()).unwrap();
    assert!(!started.is_empty() && started.chars().all(|c| c.is_ascii_digit()));
    assert!(viewer::identity::starttime(u32::MAX).is_none());
}

#[test]
fn record_writes_the_identity_document_atomically() {
    let dir = std::env::temp_dir().join(format!("viewer-identity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let target = viewer::identity::record(&dir).unwrap();
    let text = std::fs::read_to_string(&target).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(std::process::id() as u64, value["pid"].as_u64().unwrap());
    assert_eq!("rust", value["runtime"].as_str().unwrap());
    assert_eq!(dir.to_string_lossy(), value["cwd"].as_str().unwrap());
    assert!(
        value["started"]
            .as_str()
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_digit())
    );
    assert!(!text.ends_with('\n'));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn watch_stamp_has_nanoseconds_and_catches_equal_length_edits() {
    let dir = std::env::temp_dir().join(format!("viewer-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("crap.edn");
    std::fs::write(&file, "{:crap 1}").unwrap();
    let before = viewer::watch::mtime(&file).unwrap();
    let parts: Vec<&str> = before.split(':').collect();
    assert_eq!(3, parts.len(), "sec:nsec:size");
    assert_eq!("9", parts[2]);
    // Equal-byte-length edit: only the nanosecond mtime may change.
    for _ in 0..100 {
        std::fs::write(&file, "{:crap 9}").unwrap();
        let after = viewer::watch::mtime(&file).unwrap();
        if after != before {
            assert_ne!(parts[1], after.split(':').nth(1).unwrap());
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(&file, "{:crap 1}").unwrap();
        let _ = viewer::watch::mtime(&file).unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
    panic!("nanosecond mtime never changed across equal-length edits");
}

#[test]
fn a_coding_agent_choice_is_carried_and_anything_else_is_refused() {
    assert_eq!(None, viewer::cli::parse(&args(&["d.edn"])).unwrap().agent);
    for (name, agent) in [
        ("claude", companion::CodingAgent::Claude),
        ("codex", companion::CodingAgent::Codex),
    ] {
        let options = viewer::cli::parse(&args(&[
            "--backend",
            "herdr",
            "--coding-agent",
            name,
            "d.edn",
        ]))
        .unwrap();
        assert_eq!(Some(agent), options.agent);
        assert_eq!(Some(PathBuf::from("d.edn")), options.path);
    }
    for values in [
        vec!["--coding-agent"],
        vec!["--coding-agent", "gemini"],
        vec!["--coding-agent", "codex", "--coding-agent", "codex"],
    ] {
        assert_eq!(
            "Unknown or duplicate coding-agent provider; use --coding-agent claude|codex",
            viewer::cli::parse(&args(&values)).unwrap_err()
        );
    }
}
