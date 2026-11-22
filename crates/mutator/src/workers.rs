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

pub fn worker_count(sites: usize, requested: Option<usize>) -> usize {
    let processors = std::thread::available_parallelism().map_or(1, |count| count.get());
    sites
        .min(processors)
        .min(requested.unwrap_or(processors))
        .max(1)
}

pub struct Workers {
    slots: Vec<PathBuf>,
    #[cfg(windows)]
    caches: Vec<crate::build_cache::BuildCache>,
    _root: scratch::Root,
}

impl Workers {
    pub fn new(project: &Path, count: usize) -> io::Result<Self> {
        let root = scratch::Root::new(project)?;
        let count = count.max(1);
        let mut slots = Vec::with_capacity(count);
        #[cfg(windows)]
        let mut caches = Vec::with_capacity(count);
        #[cfg(windows)]
        let caches_root = root
            .path()
            .join(crate::build_cache::DIRECTORY)
            .to_path_buf();
        #[cfg(windows)]
        scratch::private_directory(&caches_root)?;
        for index in 0..count {
            let slot = root.path().join(format!("worker-{index}"));
            scratch::private_directory(&slot)?;
            slots.push(slot);
            #[cfg(windows)]
            {
                let directory = caches_root.join(format!("worker-{index}"));
                scratch::private_directory(&directory)?;
                caches.push(crate::build_cache::BuildCache::new(directory));
            }
        }
        Ok(Self {
            slots,
            #[cfg(windows)]
            caches,
            _root: root,
        })
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn view(&self, index: usize) -> View {
        let index = index % self.slots.len();
        let view = View::reused(&self.slots[index]);
        #[cfg(windows)]
        let view = match self.caches.get(index) {
            Some(build) => view.with_build(build.clone()),
            None => view,
        };
        view
    }

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

fn locks<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
