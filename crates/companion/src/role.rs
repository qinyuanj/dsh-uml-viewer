//! The four headless companion roles, dispatched by the single binary (#134).
//! Argument shapes for the companion roles: `[exe, role, manifest]`.
//! `companion-preflight <root> [args...]`, `companion-worker <request.edn>`,
//! `companion-runner <manifest.edn>`, `companion-controller <manifest.edn>`.

use crate::herdr::{Config, ControllerSpec, Herdr};
use crate::host::Host;
use crate::record;
use crate::vocab::{Failure, Result};
use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

fn text(args: &[OsString], index: usize) -> String {
    args.get(index)
        .map(|arg| arg.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `companion-runner <manifest.edn>`: run the companion command attached,
/// publish started/exited records and exit with the verified child code.
pub fn runner(args: &[OsString]) -> ExitCode {
    let manifest = text(args, 0);
    let outcome = (|| -> Result<i32> {
        let identity = read_manifest(&manifest)?;
        let exit = crate::recovery::run_child(&identity, &Host::posix())?;
        Ok(exit.unwrap_or(1))
    })();
    match outcome {
        Ok(code) => ExitCode::from(u8::try_from(code.clamp(0, 255)).unwrap_or(1)),
        Err(error) => {
            eprintln!("UML recovery runner failed: {}", error.message);
            ExitCode::FAILURE
        }
    }
}

fn read_manifest(path: &str) -> Result<edn::Value> {
    if path.is_empty() {
        return Err(Failure::new("Missing recovery manifest path"));
    }
    let text = std::fs::read_to_string(Path::new(path)).map_err(|error| {
        Failure::new(format!("Could not read recovery manifest {path}: {error}"))
    })?;
    edn::parse(&text)
        .map_err(|error| Failure::new(format!("Malformed recovery manifest {path}: {error}")))
}

/// `companion-controller <manifest.edn>`: the detached recovery controller.
/// It reselects Herdr only when it must relaunch, and relaunches with its own
/// executable so a mixed-runtime restart can never silently switch runtimes.
pub fn controller(args: &[OsString]) -> ExitCode {
    let manifest = text(args, 0);
    let outcome = (|| -> Result<String> {
        let identity = read_manifest(&manifest)?;
        let host = Host::posix();
        let root = record::get_name(&identity, "cwd").unwrap_or_default();
        let selected = std::cell::RefCell::new(None);
        let backend = |submission: &edn::Value| -> Result<Herdr> {
            if selected.borrow().is_none() {
                *selected.borrow_mut() = Some(crate::preflight::select(
                    &root,
                    &["--backend".to_owned(), "herdr".to_owned()],
                    false,
                )?);
            }
            let config = Config::controller_relaunch(
                selected.borrow().clone(),
                crate::lifecycle::runner_launcher()?,
            );
            Ok(Herdr::new(Some(submission.clone()), host.clone(), config))
        };
        let launch = |submission: &edn::Value| backend(submission)?.relaunch(submission);
        let verify = |record: &edn::Value| backend(record)?.resumed_session(record);
        crate::recovery::controller_loop(&identity, &host, &launch, &verify)
    })();
    match outcome {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("UML recovery controller failed: {}", error.message);
            ExitCode::FAILURE
        }
    }
}

/// `companion-worker <request.edn>`: one isolated helper per project.
pub fn worker(args: &[OsString]) -> ExitCode {
    let request = text(args, 0);
    let host = Host::posix();
    let attach = (host.variable)("UML_COMPANION_NO_ATTACH").as_deref() != Some("1");
    match crate::worker::run(&request, attach, &host) {
        Ok(code) => ExitCode::from(u8::try_from(code.clamp(0, 255)).unwrap_or(1)),
        Err(error) => {
            eprintln!("{}", error.message);
            ExitCode::FAILURE
        }
    }
}

/// `companion-preflight <root> [args...]`.
pub fn preflight(args: &[OsString]) -> ExitCode {
    crate::preflight::run(args)
}

/// `companion-smoke <root> <fake-cmd>`: an isolated native acceptance harness
/// against real Herdr. Never runs against the examined project.
pub fn smoke(args: &[OsString]) -> ExitCode {
    let root = text(args, 0);
    let fake = text(args, 1);
    let outcome = (|| -> Result<String> {
        if root.is_empty() || fake.is_empty() {
            return Err(Failure::new("usage: companion-smoke <root> <fake-cmd>"));
        }
        let host = Host::posix();
        println!("SMOKE STEP paths");
        let selected =
            crate::preflight::select(&root, &["--backend".to_owned(), "herdr".to_owned()], false)?;
        println!("SMOKE STEP preflight");
        let config = Config::smoke(
            selected,
            crate::lifecycle::runner_launcher()?,
            ControllerSpec {
                executable: crate::lifecycle::current_exe()?,
                argv: vec!["companion-controller".to_owned()],
            },
        );
        println!("SMOKE STEP backend");
        let session = crate::lifecycle::session_name(
            &root,
            &(host.token)(),
            &crate::lifecycle::config_root(&host),
        )?;
        println!("SMOKE STEP session");
        let canonical = (host.canonical)(&root);
        let mut backend = Herdr::new(None, host.clone(), config);
        println!("SMOKE STEP create");
        let owned = backend.create(
            &session,
            &canonical,
            &[
                fake,
                "-n".to_owned(),
                "1".to_owned(),
                "127.0.0.1".to_owned(),
            ],
        )?;
        println!("SMOKE STEP record");
        let recorded = crate::ownership::read_in(&host, &root)?
            .ok_or_else(|| Failure::new("ownership record missing"))?;
        if record::get_name(&owned, "session").as_deref() != Some(session.as_str()) {
            return Err(Failure::new("session identity changed"));
        }
        if record::get_name(&recorded, "session").is_none() {
            return Err(Failure::new("ownership record missing"));
        }
        println!("SMOKE STEP cleanup");
        let cleaner = Herdr::new(Some(owned.clone()), host.clone(), backend_config(&root)?);
        cleaner.cleanup(&session)?;
        crate::ownership::write(&host, &root, &edn::Value::Map(vec![]))?;
        Ok(session)
    })();
    match outcome {
        Ok(session) => {
            println!("SMOKE PASS session={session}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("SMOKE FAIL: {}", error.message);
            ExitCode::FAILURE
        }
    }
}

fn backend_config(root: &str) -> Result<Config> {
    let selected =
        crate::preflight::select(root, &["--backend".to_owned(), "herdr".to_owned()], false)?;
    Ok(Config::backend(selected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_manifest_is_reported_by_role() {
        let code = runner(&[OsString::from("/definitely/missing.edn")]);
        assert_eq!(ExitCode::FAILURE, code);
        let code = controller(&[OsString::from("/definitely/missing.edn")]);
        assert_eq!(ExitCode::FAILURE, code);
    }

    #[test]
    fn a_manifest_without_a_command_fails_the_runner_without_guessing() {
        let directory = std::env::temp_dir().join(format!(
            "uml role {} {}",
            std::process::id(),
            crate::posix::monotonic_ms()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let manifest = directory.join("manifest.edn");
        std::fs::write(
            &manifest,
            format!(
                "{{:cwd \"{}\" :runner-state \"{}/runner.edn\" :runner-token \"t\"}}",
                directory.display(),
                directory.display()
            ),
        )
        .unwrap();
        let code = runner(&[manifest.into_os_string()]);
        assert_eq!(ExitCode::FAILURE, code);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_smoke_harness_requires_both_arguments() {
        assert_eq!(ExitCode::FAILURE, smoke(&[]));
        assert_eq!(ExitCode::FAILURE, smoke(&[OsString::from("/tmp")]));
    }

    #[test]
    fn the_worker_role_reports_a_missing_request() {
        let code = worker(&[OsString::from("/definitely/missing-request.edn")]);
        assert_eq!(ExitCode::FAILURE, code);
    }
}
