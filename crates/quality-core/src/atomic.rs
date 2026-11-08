//! Replace a file in one step so readers never see a partial write.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Windows refuses to replace a file another process holds open without
/// FILE_SHARE_DELETE: access denied (5) or sharing violation (32). Those
/// are retried for a few seconds; anything else is not.
const REPLACE_DELAYS_MS: &[u64] = &[50, 100, 200, 400, 800, 1000, 1000];

fn staged_path(path: &Path) -> PathBuf {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or(0);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(
        ".{name}.{}{nanos:08x}{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ))
}

fn replace(staged: &Path, destination: &Path) -> io::Result<()> {
    let delays: &[u64] = if cfg!(windows) {
        REPLACE_DELAYS_MS
    } else {
        &[]
    };
    let mut attempt = 0;
    loop {
        match fs::rename(staged, destination) {
            Ok(()) => return Ok(()),
            Err(error)
                if attempt < delays.len() && matches!(error.raw_os_error(), Some(5 | 32)) =>
            {
                std::thread::sleep(Duration::from_millis(delays[attempt]));
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Stage `payload` beside `path`, then rename it into place. Bytes are
/// written as given, so LF stays LF on every host. A failure at any step
/// leaves the previous file untouched and removes the staged copy; the
/// staged file is world-readable, as the Python tools leave it.
pub fn write_bytes(path: &Path, payload: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let staged = staged_path(path);
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)?;
        file.write_all(payload)?;
        file.sync_all()?;
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o644))?;
        }
        replace(&staged, path)
    })();
    let _ = fs::remove_file(&staged);
    result
}
