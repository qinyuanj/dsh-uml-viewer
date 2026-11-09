//! The crate's one integration test binary (see `autotests` in
//! Cargo.toml): each file below is a module of it.
// Test code is the outside harness: it launches the binary and children itself.
#![allow(clippy::disallowed_methods)]

mod class_card;
mod cli;
mod commands;
mod common;
mod driver;
mod driver_gui;
mod fonts;
mod idle_wake;
mod inspector;
mod lod;
mod metrics;
mod paint;
mod real_input;
mod real_input_keyboard;
mod real_input_pointer;
mod real_input_wheel;
mod render_cache;
mod search_box;
mod session;
mod source_jump;
mod text_cache;
mod text_sizes;
mod theme;
mod window_mode;
