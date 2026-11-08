//! Worker slots: the private overlay views one call creates and reuses.
//!
//! A slot is a `0700` directory under a single scratch root for the whole call.
//! Every command still runs in a fresh overlay over the same slot path, so a
//! test's writes reach neither the project nor the next mutant, and the pool's
//! slots are what "one call, one coverage run, one baseline, reused workers"
//! means observably: `MUTATOR_VIEW` names the same few slot paths for every
//! mutant of every target file.
use crate::{
    interrupts,
    isolation::{Failure, View},
    scratch,
    sites::Site,
};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

/// Mutants to run at once: the original's
/// `min(site_count, processors, --max-workers)`, never below one.
pub fn worker_count(sites: usize, requested: Option<usize>) -> usize {
    let processors = std::thread::available_parallelism().map_or(1, |count| count.get());
    sites
        .min(processors)
        .min(requested.unwrap_or(processors))
        .max(1)
}

pub struct Workers {
    slots: Vec<PathBuf>,
    // The root owns the slots: dropping it removes the whole pool.
    _root: scratch::Root,
}

impl Workers {
    /// A pool of `count` reusable slots under one private scratch root.
    pub fn new(project: &Path, count: usize) -> io::Result<Self> {
        let root = scratch::Root::new(project)?;
        let count = count.max(1);
        let mut slots = Vec::with_capacity(count);
        for index in 0..count {
            let slot = root.path().join(format!("worker-{index}"));
            scratch::private_directory(&slot)?;
            slots.push(slot);
        }
        Ok(Self { slots, _root: root })
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// The view of one slot; the caller owns only the command it runs there.
    pub fn view(&self, index: usize) -> View {
        View::reused(&self.slots[index % self.slots.len()])
    }

    /// Run each site once across `count` workers and collect its outcome.
    ///
    /// Sites are taken from one queue, so a slow mutant never leaves a worker
    /// idle. The outcome map is keyed by mutation id, so the published bytes do
    /// not depend on which worker ran what, or in which order.
    pub fn run_selected(
        &self,
        count: usize,
        sites: &[Site],
        execute: impl Fn(&View, &Site) -> Result<i32, Failure> + Sync,
    ) -> Result<BTreeMap<String, String>, Failure> {
        let count = count.clamp(1, self.len());
        let queue = Mutex::new((0..sites.len()).collect::<VecDeque<usize>>());
        let outcomes = Mutex::new(BTreeMap::new());
        let failure = Mutex::new(None);
        std::thread::scope(|scope| {
            for index in 0..count {
                let view = self.view(index);
                let queue = &queue;
                let outcomes = &outcomes;
                let failure = &failure;
                let execute = &execute;
                scope.spawn(move || {
                    while locks(failure).is_none() && interrupts::pending().is_none() {
                        let Some(position) = locks(queue).pop_front() else {
                            break;
                        };
                        let site = &sites[position];
                        match execute(&view, site) {
                            Ok(code) => {
                                let status = if code == 0 { "survived" } else { "killed" };
                                locks(outcomes)
                                    .insert(site.mutation_id.clone(), status.to_string());
                            }
                            Err(error) => {
                                *locks(failure) = Some(error);
                                break;
                            }
                        }
                    }
                });
            }
        });
        let failure = failure
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match failure {
            Some(error) => Err(error),
            None => Ok(outcomes
                .into_inner()
                .unwrap_or_else(|poisoned| poisoned.into_inner())),
        }
    }
}

/// A worker panicking must not turn the others' results into a poisoned lock.
fn locks<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
