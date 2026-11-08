//! Library root: free functions are namespaced `crap_demo`.

pub mod geometry;
mod parse;
pub mod shapes;

#[cfg(test)]
mod tests;

pub use parse::parse_pairs;

/// Classifies a number: `if`, `else if` and `&&` / `||` decisions.
pub fn classify(n: i32) -> &'static str {
    if n < 0 && n % 2 == 0 {
        "negative even"
    } else if n < 0 || n > 1000 {
        "out of range"
    } else if n == 0 {
        "zero"
    } else {
        "positive"
    }
}

/// `for` and `while` loops with a nested `if`.
pub fn sum_until(values: &[i32], limit: i32) -> i32 {
    let mut total = 0;
    for value in values {
        if total + value > limit {
            break;
        }
        total += value;
    }
    let mut extra = 0;
    while extra < 3 && total < limit {
        extra += 1;
        total += 1;
    }
    total
}

/// Never called by the tests.
pub fn unused(flag: bool) -> u8 {
    if flag {
        1
    } else {
        0
    }
}

#[cfg(any(test, feature = "fixtures"))]
pub fn fixture_helper() -> u8 {
    7
}

#[cfg(test)]
pub fn test_only_helper() -> u8 {
    if true {
        1
    } else {
        2
    }
}

#[cfg(all(test, unix))]
fn unix_test_helper() {}
