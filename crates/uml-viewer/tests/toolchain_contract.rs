#![allow(clippy::disallowed_methods)]

mod support;

mod crap {
    pub use crate::support::{Project, stderr};
}

mod crap_languages;
mod crap_runner_smoke;
mod mutate_lua;

pub use support::{binary, binary_env};
