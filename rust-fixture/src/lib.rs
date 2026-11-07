use tauri::Manager;
use serde::Serialize;
use std::fs;
mod proto;
mod rec;
#[cfg(windows)]
mod console;
#[cfg(unix)]
mod signals;
use self::proto::Q;
impl proto::Q for u8 {}
#[tauri::command]
fn read_text(path: String) -> String { path }
pub fn run() {}
fn hidden() {}
