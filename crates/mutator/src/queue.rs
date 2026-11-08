//! One FIFO queue shared by every worker slot of a call.
//!
//! The pinned original runs all full-suite Lua files through `run_queue`: a
//! worker that finishes a mutant takes the next one from *any* file, a file's
//! snapshot is published as soon as its own last mutant finishes, and after
//! any failure no further file completes. This module keeps that contract and
//! nothing else, so it is the same for every caller and testable without an
//! overlay mount or a Lua toolchain.
use crate::{interrupts, sites::Site};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

/// Run `jobs` on one queue with up to `slots` workers.
///
/// `jobs` lists each file's sites in the order they must run (the engine puts
/// each file's previous survivors first). `execute` runs one site in worker
/// slot `slot` and reports whether the mutant was killed; `publish` is called
/// once per file, serialized, when that file's last mutant has finished, and
/// its error stops the queue. The first error — a failed command or a failed
/// publication — stops every worker from taking more sites and is returned;
/// the files that had already been published stay published. A file whose last
/// mutant lands after the stop is never published.
///
/// A job with no sites is ignored (the caller publishes such files itself).
/// An arriving `SIGINT`/`SIGTERM` stops the queue the same way as a failure,
/// without an error: the caller reports the signal and keeps the finished
/// files' snapshots.
///
/// The outcome map handed to `publish` is keyed by mutation id, so the bytes a
/// caller writes never depend on worker count, worker identity or the order
/// sites were taken in.
pub fn run_queue<E: Send>(
    slots: usize,
    jobs: &[&[Site]],
    execute: impl Fn(usize, &Site) -> Result<bool, E> + Sync,
    publish: impl FnMut(usize, &BTreeMap<String, String>) -> Result<(), E> + Send,
) -> Result<(), E> {
    let order: VecDeque<(usize, usize)> = jobs
        .iter()
        .enumerate()
        .flat_map(|(job, sites)| (0..sites.len()).map(move |site| (job, site)))
        .collect();
    if order.is_empty() {
        return Ok(());
    }
    let queue = Mutex::new(order);
    let outcomes: Vec<Mutex<BTreeMap<String, String>>> =
        jobs.iter().map(|_| Mutex::new(BTreeMap::new())).collect();
    let remaining: Vec<AtomicUsize> = jobs
        .iter()
        .map(|sites| AtomicUsize::new(sites.len()))
        .collect();
    let failure: Mutex<Option<E>> = Mutex::new(None);
    let stopped = AtomicBool::new(false);
    let publish = Mutex::new(publish);
    std::thread::scope(|scope| {
        for slot in 0..slots.max(1) {
            let queue = &queue;
            let outcomes = &outcomes;
            let remaining = &remaining;
            let failure = &failure;
            let stopped = &stopped;
            let publish = &publish;
            let execute = &execute;
            scope.spawn(move || {
                loop {
                    if stop_requested(stopped) {
                        break;
                    }
                    let Some((job, index)) = locks(queue).pop_front() else {
                        break;
                    };
                    let site = &jobs[job][index];
                    match execute(slot, site) {
                        Ok(killed) => {
                            let outcome = if killed { "killed" } else { "survived" };
                            locks(&outcomes[job])
                                .insert(site.mutation_id.clone(), outcome.to_string());
                            if remaining[job].fetch_sub(1, Ordering::AcqRel) == 1 {
                                // Publication is the file's own completion point:
                                // serialize it, and never publish after a stop.
                                let mut publish = locks(publish);
                                if stopped.load(Ordering::Acquire) {
                                    break;
                                }
                                let snapshot = locks(&outcomes[job]).clone();
                                if let Err(error) = publish(job, &snapshot) {
                                    *locks(failure) = Some(error);
                                    stopped.store(true, Ordering::Release);
                                    break;
                                }
                            }
                        }
                        Err(error) => {
                            *locks(failure) = Some(error);
                            stopped.store(true, Ordering::Release);
                            break;
                        }
                    }
                }
            });
        }
    });
    match std::mem::take(&mut *locks(&failure)) {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// A stop signal arrived, or the queue was stopped by a failure.
fn stop_requested(stopped: &AtomicBool) -> bool {
    stopped.load(Ordering::Acquire) || interrupts::pending().is_some()
}

/// A worker panicking must not turn the others' results into a poisoned lock.
fn locks<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
