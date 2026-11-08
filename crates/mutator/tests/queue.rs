//! The shared worker queue, ported from the pinned original's
//! `tests/test_lua_queue.py` (f772a83).
//!
//! The original unit-tests `run_queue` with a fake runner: the scheduling
//! contract is about which mutant a free worker takes next, when a file's
//! snapshot is published and what a failure leaves behind, not about Busted.
//! This port keeps the same seam, so these cases need no Lua toolchain and no
//! overlay mount; the real Busted queue is exercised in `tests/lua.rs`.
use mutator::queue::run_queue;
use mutator::sites::Site;
use std::collections::BTreeMap;
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};

/// A site whose mutation identity names its file and value, as the original's
/// fake `Site(source.name + str(value))` does.
fn site(file: &str, value: usize) -> Site {
    Site {
        file: file.to_string(),
        namespace: file.trim_end_matches(".lua").to_string(),
        form_id: format!("defn/{value}"),
        name: format!("value{value}"),
        line: 1,
        start: 0,
        end: 1,
        original: "1".to_string(),
        mutant: value.to_string(),
        category: "number".to_string(),
        mutation_id: format!("{file}{value}"),
    }
}

fn sites(file: &str, values: &[usize]) -> Vec<Site> {
    values.iter().map(|value| site(file, *value)).collect()
}

/// What the fake executor and publisher saw, in order.
#[derive(Default)]
struct Record {
    started: Vec<String>,
    published: Vec<(usize, BTreeMap<String, String>)>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A value that a mutant has to wait for: the original's `Event.wait`.
#[derive(Default)]
struct Signal {
    set: Mutex<bool>,
    changed: Condvar,
}

impl Signal {
    fn set(&self) {
        *lock(&self.set) = true;
        self.changed.notify_all();
    }

    /// True when the signal was set before the five-second deadline.
    fn wait(&self) -> bool {
        let deadline = std::time::Duration::from_secs(5);
        let mut set = lock(&self.set);
        let started = std::time::Instant::now();
        while !*set {
            let elapsed = started.elapsed();
            if elapsed >= deadline {
                return false;
            }
            set = self
                .changed
                .wait_timeout(set, deadline - elapsed)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
        true
    }
}

/// Ported from `test_next_file_starts_while_previous_file_runs`: a worker that
/// finishes a mutant takes the next one from any file, so one file's slowest
/// mutant never idles the rest. Value 3 kills; value 9 waits until b.lua is
/// published.
#[test]
fn the_next_file_starts_while_the_previous_file_runs() {
    let a = sites("a.lua", &[9, 3]);
    let b = sites("b.lua", &[2, 3]);
    let jobs: Vec<&[Site]> = vec![&a, &b];
    let record = Mutex::new(Record::default());
    let b_published = Signal::default();
    let overlapped = AtomicBool::new(false);
    let result = run_queue(
        2,
        &jobs,
        |_slot, site| -> Result<bool, &'static str> {
            lock(&record).started.push(site.mutation_id.clone());
            if site.mutation_id == "a.lua9" {
                overlapped.store(b_published.wait(), Ordering::Relaxed);
            }
            Ok(site.mutant == "3")
        },
        |job, outcomes| -> Result<(), &'static str> {
            lock(&record).published.push((job, outcomes.clone()));
            if job == 1 {
                b_published.set();
            }
            Ok(())
        },
    );
    assert_eq!(result, Ok(()));
    assert!(
        overlapped.load(Ordering::Relaxed),
        "a.lua's slow mutant was still running when b.lua was published"
    );
    let record = record.into_inner().unwrap();
    // b.lua finished first: its mutants need no wait.
    assert_eq!(
        record
            .published
            .iter()
            .map(|(job, _)| *job)
            .collect::<Vec<_>>(),
        vec![1, 0]
    );
    assert_eq!(
        record.published[0].1,
        BTreeMap::from([
            ("b.lua2".to_string(), "survived".to_string()),
            ("b.lua3".to_string(), "killed".to_string()),
        ])
    );
    assert_eq!(
        record.published[1].1,
        BTreeMap::from([
            ("a.lua9".to_string(), "survived".to_string()),
            ("a.lua3".to_string(), "killed".to_string()),
        ])
    );
}

/// Ported from `test_queue_preserves_each_files_survivor_first_order`: the
/// sites of one file keep the order the engine scheduled them in, and files
/// enter the queue in order.
#[test]
fn the_queue_preserves_each_files_order() {
    let a = sites("a.lua", &[2, 3]);
    let b = sites("b.lua", &[4, 3]);
    let jobs: Vec<&[Site]> = vec![&a, &b];
    let record = Mutex::new(Record::default());
    let result = run_queue(
        1,
        &jobs,
        |_slot, site| -> Result<bool, &'static str> {
            lock(&record).started.push(site.mutation_id.clone());
            Ok(true)
        },
        |job, outcomes| -> Result<(), &'static str> {
            lock(&record).published.push((job, outcomes.clone()));
            Ok(())
        },
    );
    assert_eq!(result, Ok(()));
    let record = record.into_inner().unwrap();
    assert_eq!(
        record.started,
        ["a.lua2", "a.lua3", "b.lua4", "b.lua3"],
        "each file's scheduled order is kept"
    );
    assert_eq!(
        record
            .published
            .iter()
            .map(|(job, _)| *job)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
}

/// Ported from `test_worker_failure_publishes_only_completed_files`: a worker
/// error stops the queue, keeps the snapshots of files that had finished, and
/// never publishes one that had not.
#[test]
fn a_failing_site_publishes_only_completed_files() {
    let a = sites("a.lua", &[2]);
    let b = sites("b.lua", &[2, 3]);
    let jobs: Vec<&[Site]> = vec![&a, &b];
    let record = Mutex::new(Record::default());
    let result = run_queue(
        1,
        &jobs,
        |_slot, site| -> Result<bool, &'static str> {
            lock(&record).started.push(site.mutation_id.clone());
            if site.file == "b.lua" {
                return Err("runner broke");
            }
            Ok(true)
        },
        |job, outcomes| -> Result<(), &'static str> {
            lock(&record).published.push((job, outcomes.clone()));
            Ok(())
        },
    );
    assert_eq!(result, Err("runner broke"));
    let record = record.into_inner().unwrap();
    assert_eq!(record.published.len(), 1, "only a.lua completed");
    assert_eq!(record.published[0].0, 0);
    assert!(
        !record.started.iter().any(|id| id == "b.lua3"),
        "no further mutant starts after a failure: {:?}",
        record.started
    );
}

/// Ported from `test_failed_publication_stops_the_queue`: a publication that
/// fails stops the queue before another file's mutants can run.
#[test]
fn a_failed_publication_stops_the_queue() {
    let a = sites("a.lua", &[2]);
    let b = sites("b.lua", &[2, 3]);
    let jobs: Vec<&[Site]> = vec![&a, &b];
    let record = Mutex::new(Record::default());
    let result = run_queue(
        1,
        &jobs,
        |_slot, site| -> Result<bool, &'static str> {
            lock(&record).started.push(site.mutation_id.clone());
            Ok(true)
        },
        |job, _outcomes| -> Result<(), &'static str> {
            lock(&record).published.push((job, BTreeMap::new()));
            Err("publication failed")
        },
    );
    assert_eq!(result, Err("publication failed"));
    let record = record.into_inner().unwrap();
    assert_eq!(record.published.len(), 1);
    assert!(
        !record.started.iter().any(|id| id.starts_with("b.lua")),
        "b.lua must not start after a failed publication: {:?}",
        record.started
    );
}

/// Ported from the original's rule "after any failure no further file
/// completes": a file whose last mutant lands after the stop is not published,
/// even though its own mutant succeeded. The stop is raised inside the
/// publication lock, so by the time the failing publication returns the queue's
/// failure is stored; a short grace after the signal keeps the assertion about
/// that ordering rather than about a scheduler race.
#[test]
fn a_stop_after_the_last_mutant_of_a_file_publishes_nothing() {
    let a = sites("a.lua", &[2]);
    let b = sites("b.lua", &[2]);
    let jobs: Vec<&[Site]> = vec![&a, &b];
    let record = Mutex::new(Record::default());
    let failed = Signal::default();
    let result = run_queue(
        2,
        &jobs,
        |_slot, site| -> Result<bool, &'static str> {
            lock(&record).started.push(site.mutation_id.clone());
            if site.file == "a.lua" {
                assert!(failed.wait(), "the failure arrives first");
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Ok(true)
        },
        |job, outcomes| -> Result<(), &'static str> {
            lock(&record).published.push((job, outcomes.clone()));
            if job == 1 {
                failed.set();
                return Err("publication failed");
            }
            Ok(())
        },
    );
    assert_eq!(result, Err("publication failed"));
    let record = record.into_inner().unwrap();
    assert_eq!(
        record
            .published
            .iter()
            .map(|(job, _)| *job)
            .collect::<Vec<_>>(),
        vec![1],
        "a.lua completed after the stop and must not publish"
    );
}

/// No jobs is a no-op: nothing runs and nothing publishes.
#[test]
fn an_empty_queue_runs_nothing() {
    let jobs: Vec<&[Site]> = Vec::new();
    let result = run_queue(
        2,
        &jobs,
        |_slot, _site| -> Result<bool, &'static str> { panic!("no mutant may run without a job") },
        |_job, _outcomes| -> Result<(), &'static str> {
            panic!("nothing may publish without a job")
        },
    );
    assert_eq!(result, Ok(()));
}

/// Every site of every file runs exactly once, whatever the worker count: the
/// outcome map is keyed by mutation id, so no site is dropped or doubled.
#[test]
fn every_site_runs_once_across_workers() {
    let a = sites("a.lua", &[2, 3, 4]);
    let b = sites("b.lua", &[5, 6, 7]);
    let jobs: Vec<&[Site]> = vec![&a, &b];
    let seen = Mutex::new(BTreeMap::<String, usize>::new());
    let published = Mutex::new(BTreeMap::<usize, BTreeMap<String, String>>::new());
    let result = run_queue(
        3,
        &jobs,
        |_slot, site| -> Result<bool, &'static str> {
            *lock(&seen).entry(site.mutation_id.clone()).or_default() += 1;
            Ok(site.mutant == "5")
        },
        |job, outcomes| -> Result<(), &'static str> {
            lock(&published).insert(job, outcomes.clone());
            Ok(())
        },
    );
    assert_eq!(result, Ok(()));
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.len(), 6);
    assert!(seen.values().all(|count| *count == 1), "{seen:?}");
    let published = published.into_inner().unwrap();
    assert_eq!(published.len(), 2);
    assert_eq!(published[&0]["a.lua2"], "survived");
    assert_eq!(published[&1]["b.lua5"], "killed");
    assert_eq!(published[&1].len(), 3);
}
