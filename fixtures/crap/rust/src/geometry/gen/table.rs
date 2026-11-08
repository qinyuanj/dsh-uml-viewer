pub fn fallback(key: u8) -> u8 {
    key.wrapping_mul(3)
}
