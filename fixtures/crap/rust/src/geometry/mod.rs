//! `mod.rs` is named after its directory; inline modules extend the path.

pub mod gen;

pub fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

pub mod inner {
    pub fn clamp(value: i32, low: i32, high: i32) -> i32 {
        if value < low {
            low
        } else if value > high {
            high
        } else {
            value
        }
    }

    pub struct Grid;

    impl Grid {
        pub fn cells(&self, n: usize) -> usize {
            (0..n).filter(|i| i % 3 == 0).count()
        }
    }

    #[cfg(test)]
    mod checks {
        #![cfg(test)]
        fn hidden() {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn inline_tests_are_not_scored() {
        if 1 + 1 == 2 {}
    }
}
