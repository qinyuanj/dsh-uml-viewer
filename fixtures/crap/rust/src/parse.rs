//! `?`, `loop`, `while let`, closures and nested functions.

use std::num::ParseIntError;

pub fn parse_pairs(text: &str) -> Result<Vec<(i32, i32)>, ParseIntError> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut parts = line.split(',');
        let (Some(a), Some(b)) = (parts.next(), parts.next()) else {
            continue;
        };
        out.push((a.trim().parse()?, b.trim().parse()?));
    }
    Ok(out)
}

pub fn drain(mut stack: Vec<i32>) -> i32 {
    let mut seen = 0;
    while let Some(top) = stack.pop() {
        seen += top;
    }
    loop {
        if seen > 10 {
            seen -= 10;
        } else {
            break;
        }
    }
    seen
}

pub fn keep_even(values: &[i32]) -> Vec<i32> {
    values.iter().copied().filter(|v| v % 2 == 0 || *v == 1).collect()
}

pub fn outer(x: i32) -> i32 {
    fn inner(y: i32) -> i32 {
        if y > 0 {
            y
        } else {
            -y
        }
    }
    inner(x) + 1
}

pub fn r#match(word: &str) -> bool {
    word == "match"
}

pub fn grüße(name: &str) -> String {
    if name.is_empty() {
        String::from("hallo")
    } else {
        format!("hallo {name}")
    }
}
