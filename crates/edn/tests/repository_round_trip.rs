//! Acceptance (#135): every EDN file in the repository (IR, policy, JVM
//! goldens, `.metrics`, mail samples, evidence) round-trips.
//! Round-trip means parse -> encode -> parse yields the same value, and the
//! encoded bytes are a fixed point of encode(parse(_)).

use std::path::{Path, PathBuf};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), ".git" | "target" | ".uml-viewer" | ".claude") {
                collect(&path, out);
            }
        } else if name.ends_with(".edn") {
            out.push(path);
        }
    }
}

fn repository_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(&repository(), &mut files);
    files.sort();
    files
}

/// Writes every non-ASCII character as `\uXXXX` (UTF-16, so astral
/// characters become surrogate pairs). Non-ASCII only occurs inside strings.
fn ascii_escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04X}"));
            }
        }
    }
    out
}

#[test]
fn repository_files_read_back_identically_when_written_with_unicode_escapes() {
    // No checked-in file uses `\u` yet, so derive escaped twins of every file.
    let mut escaped_files = 0;
    let mut failures = Vec::new();
    for file in repository_files() {
        let value = edn::parse(&std::fs::read_to_string(&file).unwrap()).unwrap();
        let encoded = edn::encode(&value).unwrap();
        if encoded.is_ascii() {
            continue;
        }
        escaped_files += 1;
        match edn::parse(&ascii_escaped(&encoded)) {
            Ok(again) if again == value => {}
            other => failures.push(format!("{}: {:?}", file.display(), other.err())),
        }
    }
    assert!(
        escaped_files > 0,
        "no repository file has non-ASCII text to escape"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_repository_edn_file_round_trips_byte_stably() {
    let files = repository_files();
    // 169 tracked EDN files when this test was written; a walk that finds far
    // fewer has silently lost a directory.
    assert!(files.len() >= 169, "found only {} EDN files", files.len());
    let mut failures = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        let result = edn::parse(&text).and_then(|value| {
            let encoded = edn::encode(&value)?;
            let again = edn::parse(&encoded)?;
            if again != value {
                return Err(edn::Error {
                    offset: 0,
                    message: "value changed".into(),
                });
            }
            if edn::encode(&again)? != encoded {
                return Err(edn::Error {
                    offset: 0,
                    message: "bytes not stable".into(),
                });
            }
            Ok(())
        });
        if let Err(error) = result {
            failures.push(format!("{}: {:?}", file.display(), error));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} failed:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
