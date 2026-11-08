#[test]
fn attributed_test() {
    let adjusted = 4 + 3;
    if adjusted > 10 {
        assert_eq!(adjusted * 2, 14);
    } else {
        assert_eq!(adjusted - 1, 6);
    }
}

#[cfg(test)]
pub fn cfg_test(input: i32) -> i32 {
    let adjusted = input + 3;
    if adjusted > 10 {
        adjusted * 2
    } else {
        adjusted - 1
    }
}

#[cfg(all(test, feature = "oracle"))]
pub fn cfg_all_test(input: i32) -> i32 {
    let adjusted = input + 3;
    if adjusted > 10 {
        adjusted * 2
    } else {
        adjusted - 1
    }
}

#[cfg(any(test, feature = "oracle"))]
pub fn cfg_any_test(input: i32) -> i32 {
    let adjusted = input + 3;
    if adjusted > 10 {
        adjusted * 2
    } else {
        adjusted - 1
    }
}

#[cfg(test)]
mod support {
    pub fn under_cfg_module(input: i32) -> i32 {
        let adjusted = input + 3;
        if adjusted > 10 {
            adjusted * 2
        } else {
            adjusted - 1
        }
    }
}

mod internal {
    #![cfg(test)]
    pub fn under_inner_cfg(input: i32) -> i32 {
        let adjusted = input + 3;
        if adjusted > 10 {
            adjusted * 2
        } else {
            adjusted - 1
        }
    }
}

// A named module nested inside a function is not an immediate block child.
pub fn container() {
    mod embedded {
        pub fn nested_module_function(input: i32) -> i32 {
            let adjusted = input + 3;
            if adjusted > 10 {
                adjusted * 2
            } else {
                adjusted - 1
            }
        }
    }
}
