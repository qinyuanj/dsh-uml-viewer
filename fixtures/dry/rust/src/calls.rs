fn identity<T>(value: T) -> T { value }

pub fn path_left(value: i32) -> i32 {
    let first = std::cmp::max(value, 3);
    let second = std::cmp::min(first, 10);
    second.abs()
}

pub fn path_right(other: i32) -> i32 {
    let lower = std::cmp::max(other, 7);
    let upper = std::cmp::min(lower, 20);
    upper.abs()
}

pub fn path_variant(value: i32) -> i32 {
    let first = core::cmp::max(value, 3);
    let second = core::cmp::min(first, 10);
    second.abs()
}

pub fn method_variant(value: i32) -> i32 {
    let first = std::cmp::max(value, 3);
    let second = std::cmp::min(first, 10);
    second.signum()
}

pub fn generic_left(value: i32) -> i32 {
    let first = identity::<i32>(value);
    let second = identity::<i32>(first + 3);
    second.abs()
}

pub fn generic_right(other: i64) -> i64 {
    let lower = identity::<i64>(other);
    let upper = identity::<i64>(lower + 8);
    upper.abs()
}

pub fn macro_left(value: i32) -> Vec<i32> {
    let values = vec![value, 3, 5];
    debug_assert!(value > 0);
    values
}

pub fn macro_right(other: i32) -> Vec<i32> {
    let result = vec![other, 7, 9];
    debug_assert!(other > 2);
    result
}

pub fn macro_variant(value: i32) -> Vec<i32> {
    let values = vec![value, 3, 5];
    assert!(value > 0);
    values
}

pub fn operators_left(mut value: i32, mask: i32) -> i32 {
    value += 3;
    value <<= 1;
    if value != 0 && mask > 0 {
        (value & mask) ^ !mask
    } else {
        value % 7
    }
}

pub fn operators_right(mut other: i32, bits: i32) -> i32 {
    other += 8;
    other <<= 2;
    if other != 1 && bits > 9 {
        (other & bits) ^ !bits
    } else {
        other % 4
    }
}

pub fn operators_variant(mut value: i32, mask: i32) -> i32 {
    value -= 3;
    value >>= 1;
    if value == 0 || mask < 0 {
        (value | mask) ^ !mask
    } else {
        value / 7
    }
}
