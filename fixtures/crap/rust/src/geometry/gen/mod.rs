//! Generated code, excluded in some golden scenarios.

pub mod table;

pub fn lookup(key: u8) -> u8 {
    match key {
        0 => 10,
        1 => 11,
        2 => 12,
        _ => table::fallback(key),
    }
}
