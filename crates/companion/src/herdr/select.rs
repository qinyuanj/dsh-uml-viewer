//! Pure Herdr executable selection (port of `love/uml/domain/companion.lua`):
//! the supported version floor, PATH candidates, the protocol probe and the
//! choice of the selected Herdr. The adapter supplies PATH, canonical paths
//! and command execution; nothing here touches a process or a file.

pub use crate::search::Search;
use crate::vocab::{Failure, Result, RunOut};

pub const HERDR_GUIDANCE: &str = "Herdr 0.9.3 or newer (protocol 22) is required. Install a release from https://github.com/herdrdev/herdr/releases (https://herdr.dev). No automatic download, upgrade or fallback. WSL2 + WSLg, macOS and native Windows 10 22H2 (build 19045) or later are the delivered platforms (ADR 0007, ADR 0010); on Windows only a native herdr.exe on PATH counts. On WSL and native Windows, terminal attach opens Windows Terminal; on macOS it opens Terminal.app.";

pub const HERDR_INSTALL: &str = "Install a release from https://github.com/herdrdev/herdr/releases (https://herdr.dev). No automatic download, upgrade or fallback.";

/// Only dotted numeric X.Y.Z at or above 0.9.3 (ADR 0003).
pub fn herdr_version_supported(version: &str) -> bool {
    release_at_least(version, [0, 9, 3])
}

/// Whether `version` is a dotted numeric X.Y.Z at or above `floor`.
pub fn release_at_least(version: &str, floor: [u64; 3]) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    let mut numbers = [0u64; 3];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        numbers[index] = match part.parse() {
            Ok(number) => number,
            Err(_) => return false,
        };
    }
    for index in 0..3 {
        if numbers[index] != floor[index] {
            return numbers[index] > floor[index];
        }
    }
    true
}

pub struct Probe {
    pub executable: bool,
    pub canonical: String,
}

/// Distinct herdr executables in PATH order, deduplicated by canonical path.
/// Quoted or empty PATH entries are ignored. Per directory the first file the
/// platform's search names (`herdr`; `herdr.com`/`herdr.exe` on Windows)
/// that probes as present is the candidate.
pub fn herdr_candidates(
    search: &Search,
    path: &str,
    probe: &dyn Fn(&str) -> Option<Probe>,
) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen = Vec::new();
    for files in search.candidates(path, "herdr") {
        let Some(observed) = files.iter().find_map(|file| probe(file)) else {
            continue;
        };
        if observed.executable && !seen.contains(&observed.canonical) {
            seen.push(observed.canonical.clone());
            result.push(observed.canonical);
        }
    }
    result
}

/// The bundled schema text must open with `"$schema"` and declare
/// `"protocol": 22` before any later field (the same text pattern the Lua
/// preflight matched, not a full JSON parse).
fn schema_is_protocol_22(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut position = 0;
    let skip = |position: &mut usize| {
        while *position < bytes.len() && matches!(bytes[*position], b' ' | b'\t' | b'\r' | b'\n') {
            *position += 1;
        }
    };
    let expect = |position: &mut usize, literal: &str| -> bool {
        skip(position);
        if bytes[*position..].starts_with(literal.as_bytes()) {
            *position += literal.len();
            true
        } else {
            false
        }
    };
    let string = |position: &mut usize| -> bool {
        skip(position);
        if *position >= bytes.len() || bytes[*position] != b'"' {
            return false;
        }
        *position += 1;
        while *position < bytes.len() && bytes[*position] != b'"' {
            *position += 1;
        }
        if *position >= bytes.len() {
            return false;
        }
        *position += 1;
        true
    };
    expect(&mut position, "{")
        && expect(&mut position, "\"$schema\"")
        && expect(&mut position, ":")
        && string(&mut position)
        && expect(&mut position, ",")
        && expect(&mut position, "\"protocol\"")
        && expect(&mut position, ":")
        && expect(&mut position, "22")
        && expect(&mut position, ",")
}

pub struct ProbeResult {
    pub path: String,
    pub release: Option<String>,
    pub reason: Option<String>,
}

pub fn probe_herdr(run: &dyn Fn(&[String], Option<i64>) -> RunOut, exe: &str) -> ProbeResult {
    let argv = [exe.to_owned(), "--version".to_owned()];
    let version = run(&argv, None);
    let release = if version.exit == Some(0) {
        version
            .out
            .strip_prefix("herdr ")
            .filter(|rest| !rest.is_empty() && !rest.contains(char::is_whitespace))
            .map(str::to_owned)
    } else {
        None
    };
    if !release.as_deref().is_some_and(herdr_version_supported) {
        // An unsupported version is reported as unknown, matching the Lua
        // probe which only keeps the release once the floor check passes.
        return ProbeResult {
            path: exe.to_owned(),
            release: None,
            reason: Some("not a herdr X.Y.Z >= 0.9.3".to_owned()),
        };
    }
    let argv = [
        exe.to_owned(),
        "api".to_owned(),
        "schema".to_owned(),
        "--json".to_owned(),
    ];
    let schema = run(&argv, None);
    if schema.exit == Some(0) && schema_is_protocol_22(&schema.out) {
        return ProbeResult {
            path: exe.to_owned(),
            release,
            reason: None,
        };
    }
    ProbeResult {
        path: exe.to_owned(),
        release,
        reason: Some("bundled schema is not protocol 22".to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub path: String,
    pub release: String,
}

fn rank(release: &str) -> [u64; 3] {
    let mut out = [0u64; 3];
    for (index, part) in release.split('.').take(3).enumerate() {
        out[index] = part.parse().unwrap_or(0);
    }
    out
}

/// Selected Herdr: the compatible candidate with the highest version; equal
/// versions keep PATH order. When none qualifies, every candidate's path and
/// refusal reason are reported.
pub fn select_herdr(
    candidates: &[String],
    run: &dyn Fn(&[String], Option<i64>) -> RunOut,
) -> Result<Selected> {
    let probes: Vec<ProbeResult> = candidates.iter().map(|exe| probe_herdr(run, exe)).collect();
    let mut best: Option<&ProbeResult> = None;
    for candidate in &probes {
        if candidate.reason.is_none() {
            let release = candidate.release.as_deref().unwrap_or("");
            let better = match best {
                None => true,
                Some(current) => rank(release) > rank(current.release.as_deref().unwrap_or("")),
            };
            if better {
                best = Some(candidate);
            }
        }
    }
    if let Some(best) = best {
        return Ok(Selected {
            path: best.path.clone(),
            release: best.release.clone().unwrap_or_default(),
        });
    }
    let rejected: Vec<String> = probes
        .iter()
        .map(|candidate| {
            format!(
                "{} ({}: {})",
                candidate.path,
                candidate.release.as_deref().unwrap_or("unknown"),
                candidate.reason.as_deref().unwrap_or("unknown")
            )
        })
        .collect();
    let detail = if rejected.is_empty() {
        "none".to_owned()
    } else {
        rejected.join("; ")
    };
    Err(Failure::new(format!(
        "{HERDR_GUIDANCE} Candidates on PATH: {detail}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(out: &'static str) -> impl Fn(&[String], Option<i64>) -> RunOut {
        move |_, _| RunOut {
            exit: Some(0),
            out: out.to_owned(),
            err: String::new(),
            timeout: false,
        }
    }

    #[test]
    fn herdr_guidance_is_ported_verbatim() {
        assert!(HERDR_GUIDANCE.contains("Herdr 0.9.3 or newer (protocol 22) is required."));
    }

    #[test]
    fn herdr_version_floor_accepts_only_dotted_numeric_at_or_above_093() {
        for version in ["0.9.3", "0.9.4", "0.10.0", "1.0.0", "10.0.0"] {
            assert!(herdr_version_supported(version), "{version}");
        }
        for version in [
            "0.9.2",
            "0.8.9",
            "abc",
            "0.9.4-rc.1",
            "v0.9.4",
            "0.9",
            "0.9.3.1",
            "",
        ] {
            assert!(!herdr_version_supported(version), "{version}");
        }
    }

    #[test]
    fn candidates_follow_path_order_and_dedupe_canonical() {
        let probe = |path: &str| {
            if path.starts_with("/a/") || path.starts_with("/b/") {
                Some(Probe {
                    executable: true,
                    canonical: path.to_owned(),
                })
            } else {
                None
            }
        };
        let posix = Search::posix();
        assert_eq!(
            vec!["/b/herdr".to_owned(), "/a/herdr".to_owned()],
            herdr_candidates(&posix, "/b:/none::/a:/b", &probe)
        );
        assert!(herdr_candidates(&posix, "/a:/b", &(|_| None)).is_empty());
    }

    /// A declared Windows PATH: `;` lists, `herdr.exe` found by PATHEXT, a
    /// `herdr.cmd` wrapper never probed.
    #[test]
    fn windows_candidates_take_the_native_pathext_name_per_directory() {
        let probed = std::cell::RefCell::new(Vec::new());
        let probe = |path: &str| {
            probed.borrow_mut().push(path.to_owned());
            (path == "C:\\b\\herdr.exe" || path == "D:\\a b\\herdr.com").then(|| Probe {
                executable: true,
                canonical: path.to_owned(),
            })
        };
        assert_eq!(
            vec![
                "C:\\b\\herdr.exe".to_owned(),
                "D:\\a b\\herdr.com".to_owned()
            ],
            herdr_candidates(
                &Search::windows(".COM;.EXE;.BAT;.CMD"),
                "C:\\b\\;C:\\none;;D:\\a b;C:\\b",
                &probe
            )
        );
        assert!(probed.borrow().iter().all(|path| !path.ends_with(".cmd")));
    }

    fn installed<'a>(
        releases: &'static [(&'static str, &'static str, u32)],
        calls: &'a std::cell::RefCell<Vec<Vec<String>>>,
    ) -> impl Fn(&[String], Option<i64>) -> RunOut + 'a {
        move |argv, _| {
            calls.borrow_mut().push(argv.to_vec());
            let entry = releases.iter().find(|(path, _, _)| *path == argv[0]);
            match entry {
                None => RunOut {
                    exit: Some(1),
                    out: String::new(),
                    err: "not found".to_owned(),
                    timeout: false,
                },
                Some((_, release, protocol)) => {
                    if argv[1] == "--version" {
                        RunOut {
                            exit: Some(0),
                            out: format!("herdr {release}"),
                            err: String::new(),
                            timeout: false,
                        }
                    } else {
                        RunOut {
                            exit: Some(0),
                            out: format!(
                                "{{\"$schema\":\"fixture\",\"protocol\":{protocol},\"schema_version\":1}}"
                            ),
                            err: String::new(),
                            timeout: false,
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn selects_the_highest_compatible_release_regardless_of_path_order() {
        let calls = std::cell::RefCell::new(Vec::new());
        let run = installed(
            &[
                ("/a/herdr", "0.9.3", 22),
                ("/b/herdr", "0.10.0", 22),
                ("/c/herdr", "0.9.4", 22),
            ],
            &calls,
        );
        let selected = select_herdr(
            &["/a/herdr".into(), "/b/herdr".into(), "/c/herdr".into()],
            &run,
        )
        .unwrap();
        assert_eq!("/b/herdr", selected.path);
        assert_eq!("0.10.0", selected.release);
    }

    #[test]
    fn falls_back_to_the_highest_protocol_22_release() {
        let calls = std::cell::RefCell::new(Vec::new());
        let run = installed(
            &[
                ("/a/herdr", "0.9.3", 22),
                ("/b/herdr", "0.10.0", 23),
                ("/c/herdr", "0.9.4", 22),
            ],
            &calls,
        );
        let selected = select_herdr(
            &["/a/herdr".into(), "/b/herdr".into(), "/c/herdr".into()],
            &run,
        )
        .unwrap();
        assert_eq!("/c/herdr", selected.path);
        assert!(!calls.borrow().is_empty());
    }

    #[test]
    fn keeps_path_order_between_equal_releases() {
        let calls = std::cell::RefCell::new(Vec::new());
        let run = installed(
            &[("/a/herdr", "0.9.4", 22), ("/b/herdr", "0.9.4", 22)],
            &calls,
        );
        let selected = select_herdr(&["/a/herdr".into(), "/b/herdr".into()], &run).unwrap();
        assert_eq!("/a/herdr", selected.path);
    }

    #[test]
    fn reports_every_rejected_candidate_when_none_is_compatible() {
        let calls = std::cell::RefCell::new(Vec::new());
        let run = installed(
            &[("/a/herdr", "0.9.2", 22), ("/b/herdr", "0.10.0", 23)],
            &calls,
        );
        let error = select_herdr(
            &[
                "/a/herdr".into(),
                "/b/herdr".into(),
                "/missing/herdr".into(),
            ],
            &run,
        )
        .unwrap_err();
        assert!(
            error
                .message
                .contains("/a/herdr (unknown: not a herdr X.Y.Z >= 0.9.3)")
        );
        assert!(
            error
                .message
                .contains("/b/herdr (0.10.0: bundled schema is not protocol 22)")
        );
        assert!(
            error
                .message
                .contains("/missing/herdr (unknown: not a herdr X.Y.Z >= 0.9.3)")
        );
        let empty = select_herdr(&[], &run).unwrap_err();
        assert!(empty.message.contains("Candidates on PATH: none"));
    }

    #[test]
    fn probes_version_before_the_bundled_schema_without_creating_resources() {
        let calls = std::cell::RefCell::new(Vec::new());
        let run = |argv: &[String], _: Option<i64>| {
            calls.borrow_mut().push(argv.to_vec());
            RunOut {
                exit: Some(0),
                out: if argv[1] == "--version" {
                    "herdr 0.9.3".to_owned()
                } else {
                    "{\"$schema\":\"fixture\",\"protocol\":22,\"schema_version\":1}".to_owned()
                },
                err: String::new(),
                timeout: false,
            }
        };
        let selected = select_herdr(&["/x/herdr".into()], &run).unwrap();
        assert_eq!("0.9.3", selected.release);
        assert_eq!(
            vec![
                vec!["/x/herdr".to_owned(), "--version".to_owned()],
                vec![
                    "/x/herdr".to_owned(),
                    "api".to_owned(),
                    "schema".to_owned(),
                    "--json".to_owned()
                ],
            ],
            calls.into_inner()
        );
    }

    #[test]
    fn rejects_missing_malformed_or_failed_schemas_without_further_commands() {
        let schemas: Vec<RunOut> = vec![
            RunOut {
                exit: None,
                out: String::new(),
                err: String::new(),
                timeout: false,
            },
            RunOut {
                exit: Some(1),
                out: "{\"$schema\":\"fixture\",\"protocol\":22,".to_owned(),
                err: String::new(),
                timeout: false,
            },
            RunOut {
                exit: Some(0),
                out: "not json".to_owned(),
                err: String::new(),
                timeout: false,
            },
            RunOut {
                exit: Some(0),
                out: "{\"$schema\":\"fixture\",\"protocol\":21,\"schema_version\":1}".to_owned(),
                err: String::new(),
                timeout: false,
            },
        ];
        for schema in schemas {
            let calls = std::cell::RefCell::new(Vec::new());
            let run = |argv: &[String], _: Option<i64>| {
                calls.borrow_mut().push(argv.to_vec());
                if argv[1] == "--version" {
                    RunOut {
                        exit: Some(0),
                        out: "herdr 0.9.3".to_owned(),
                        err: String::new(),
                        timeout: false,
                    }
                } else {
                    schema.clone()
                }
            };
            assert!(select_herdr(&["/x/herdr".into()], &run).is_err());
            assert_eq!(2, calls.borrow().len());
        }
    }

    #[test]
    fn schema_pattern_requires_protocol_22_after_the_schema_key() {
        assert!(schema_is_protocol_22(
            "{\"$schema\":\"https://x\",\"protocol\":22,\"schema_version\":1}"
        ));
        assert!(schema_is_protocol_22(
            "  { \"$schema\" : \"x\" , \"protocol\" : 22 , }"
        ));
        assert!(!schema_is_protocol_22(
            "{\"$schema\":\"x\",\"protocol\":21,\"v\":1}"
        ));
        assert!(!schema_is_protocol_22(
            "{\"protocol\":22,\"$schema\":\"x\"}"
        ));
        assert!(!schema_is_protocol_22(
            "{\"$schema\":\"x\",\"protocol\":22}"
        ));
        assert!(!schema_is_protocol_22("not json"));
    }

    #[test]
    fn probe_passes_literal_executable_paths() {
        let run = run_with("herdr 0.9.5");
        let probe = probe_herdr(&run, "/opt/herdr 0.9/herdr");
        // Version output matches but the schema probe fails: the stub always
        // answers the version text, so the schema is not protocol 22.
        assert_eq!(
            Some("0.9.5"),
            probe.release.as_deref().map(str::to_owned).as_deref()
        );
        assert!(probe.reason.is_some());
    }
}
