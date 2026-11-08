#![cfg(test)]

fn explicit_left(value: i32) -> i32 {
    let adjusted = value + 3;
    if adjusted > 10 {
        adjusted * 2
    } else {
        adjusted - 1
    }
}

fn explicit_right(other: i32) -> i32 {
    let result = other + 8;
    if result > 20 {
        result * 4
    } else {
        result - 2
    }
}

#[test]
fn integration_test() {
    assert_eq!(explicit_left(1), 3);
    assert_eq!(explicit_right(1), 7);
}
