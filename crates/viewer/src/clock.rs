//! The host clock for proposal writeback (#157). The scene core takes `now`
//! and the proposal name as inputs; the front end reads the clock and the
//! local time zone here. [`Now`] is the instant the viewer session takes
//! as its clock input (#161).

use std::time::Instant;

/// The current moment as the viewer session sees it: a monotonic instant
/// for poll cadences and the wall-clock seconds proposal writeback stores.
#[derive(Debug, Clone, Copy)]
pub struct Now {
    pub at: Instant,
    pub epoch: i64,
}

impl Now {
    pub fn read() -> Now {
        Now {
            at: Instant::now(),
            epoch: now(),
        }
    }
}

/// Seconds since the Unix epoch.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The default proposal name: `%Y-%m-%d %H:%M:%S` in local time.
pub fn proposal_name(epoch: i64) -> String {
    // SAFETY: `tm` is plain data that `localtime_r`/`localtime_s` fills;
    // the reentrant variants share no static buffer between threads.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let time: libc::time_t = epoch;
    #[cfg(unix)]
    unsafe {
        libc::localtime_r(&time, &mut tm)
    };
    // MSVC's reentrant form takes its arguments the other way round.
    #[cfg(windows)]
    unsafe {
        libc::localtime_s(&mut tm, &time)
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_proposal_name_is_a_local_timestamp_to_the_second() {
        // The machine's time zone is not fixed, so only the shape is checked.
        let name = proposal_name(1_700_000_000);
        let digits =
            |range: std::ops::Range<usize>| name[range].bytes().all(|b| b.is_ascii_digit());
        assert_eq!(19, name.len(), "{name}");
        assert_eq!("-- ::", [4, 7, 10, 13, 16].map(|i| &name[i..=i]).concat());
        for range in [0..4, 5..7, 8..10, 11..13, 14..16, 17..19] {
            assert!(digits(range), "{name}");
        }
    }

    #[test]
    fn the_clock_reads_seconds_after_the_epoch() {
        assert!(now() > 1_700_000_000);
    }
}
