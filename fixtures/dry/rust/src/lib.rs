#![allow(dead_code, unexpected_cfgs)]

pub mod calls;
pub mod extraction;

pub fn duplicate_left(input: i32) -> i32 {
    let adjusted = input + 3;
    if adjusted > 10 {
        adjusted * 2
    } else {
        adjusted - 1
    }
}

// Local names, literals and comments normalize away.
pub fn duplicate_right(value: i32) -> i32 {
    let result = value + 9;
    if result > 25 {
        result * 4
    } else {
        result - 8
    }
}

pub fn operator_variant(value: i32) -> i32 {
    let result = value - 9;
    if result >= 25 {
        result / 4
    } else {
        result + 8
    }
}

pub fn parenthesized(value: i32) -> i32 {
    let result = (value + 9);
    if (result > 25) {
        (result * 4)
    } else {
        (result - 8)
    }
}

pub struct Counter(pub i32);
impl Counter {
    pub fn advance(&self, input: i32) -> i32 {
        let adjusted = input + 3;
        if adjusted > 10 {
            adjusted * 2
        } else {
            adjusted - 1
        }
    }

    pub fn retreat(&self, value: i32) -> i32 {
        let result = value + 9;
        if result > 25 {
            result * 4
        } else {
            result - 8
        }
    }
}

pub fn tiny_left(x: i32) -> i32 { x + 1 }
pub fn tiny_right(y: i32) -> i32 { y + 2 }

pub fn outer(value: i32) -> i32 {
    fn nested_direct(input: i32) -> i32 {
        let adjusted = input + 3;
        if adjusted > 10 {
            adjusted * 2
        } else {
            adjusted - 1
        }
    }
    nested_direct(value)
}

#[cfg(test)]
mod tests {
    fn excluded_by_module(input: i32) -> i32 {
        let adjusted = input + 3;
        if adjusted > 10 {
            adjusted * 2
        } else {
            adjusted - 1
        }
    }
    mod deeper {
        fn also_excluded(input: i32) -> i32 {
            let adjusted = input + 3;
            if adjusted > 10 {
                adjusted * 2
            } else {
                adjusted - 1
            }
        }
    }
}
