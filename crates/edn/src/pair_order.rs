//! Clojure 1.12.0 `group-by` key enumeration order for keyword-pair keys
//! (port of `love/uml/domain/pair_order.lua`): map iteration identity up to
//! eight distinct keys, then HAMT slot order. Both the IR generator and the
//! scene core merge parallel edges through this one implementation.
//!
//! Source: clojure/clojure tag clojure-1.12.0, `Murmur3.java`
//! (`hashUnencodedChars`/`hashOrdered`), `Symbol.java`, `Keyword.java`,
//! `Util.java` (`hashCombine`), `PersistentArrayMap.java` (8 entries) and
//! `PersistentHashMap.java` (mask/`NodeSeq`/`ArrayNode.Seq`/
//! `HashCollisionNode`). Only distinct directed keyword pairs are accepted,
//! in first encounter order.

const U: i64 = 4_294_967_296;

fn mixk(k: u32) -> u32 {
    k.wrapping_mul(3_432_918_353)
        .rotate_left(15)
        .wrapping_mul(461_845_907)
}

fn mixh(h: u32, k: u32) -> u32 {
    (h ^ k)
        .rotate_left(13)
        .wrapping_mul(5)
        .wrapping_add(3_864_292_196)
}

fn fmix(mut h: u32, n: u32) -> u32 {
    h ^= n;
    h ^= h >> 16;
    h = h.wrapping_mul(2_246_822_507);
    h ^= h >> 13;
    h = h.wrapping_mul(3_266_489_909);
    h ^ (h >> 16)
}

fn string_hash(s: &str) -> u32 {
    let units: Vec<u32> = s.encode_utf16().map(u32::from).collect();
    let mut h = 0u32;
    let pairs = units.len() / 2;
    for i in 0..pairs {
        h = mixh(h, mixk(units[2 * i] + units[2 * i + 1] * 65536));
    }
    if units.len() % 2 == 1 {
        h ^= mixk(units[units.len() - 1]);
    }
    fmix(h, units.len() as u32 * 2)
}

fn keyword_hash(name: &str) -> u32 {
    // `^(.-)/(.+)$`: the namespace ends at the first '/' and must be followed
    // by at least one character; otherwise the whole name is the name.
    let (ns, bare) = match name.split_once('/') {
        Some((ns, rest)) if !rest.is_empty() => (Some(ns), rest),
        _ => (None, name),
    };
    let h = string_hash(bare);
    let nh = ns.unwrap_or("").encode_utf16().fold(0u32, |acc, unit| {
        acc.wrapping_mul(31).wrapping_add(u32::from(unit))
    });
    // Java's signed >> in Util.hashCombine, unlike Murmur's unsigned >>>.
    let signed = (h as i32) >> 2;
    let inner = (i64::from(nh) + 2_654_435_769 + (u64::from(h) * 64) as i64 % U + i64::from(signed))
        .rem_euclid(U) as u32;
    (h ^ inner).wrapping_add(2_654_435_769)
}

fn pair_hash(from: &str, to: &str) -> u32 {
    let h = 31u32.wrapping_add(keyword_hash(from));
    let h = h.wrapping_mul(31).wrapping_add(keyword_hash(to));
    fmix(mixh(0, mixk(h)), 2)
}

/// Enumeration order of the pair keys: identity up to eight distinct pairs
/// (PersistentArrayMap), then ascending five-bit hash fragments, low bits
/// first, with full collisions keeping insertion order.
pub fn indices(pairs: &[(&str, &str)]) -> Vec<usize> {
    let mut out: Vec<usize> = (0..pairs.len()).collect();
    if pairs.len() <= 8 {
        return out;
    }
    let hashes: Vec<u32> = pairs.iter().map(|(f, t)| pair_hash(f, t)).collect();
    out.sort_by(|&a, &b| {
        let (mut x, mut y) = (hashes[a], hashes[b]);
        for _ in 0..7 {
            if x % 32 != y % 32 {
                return (x % 32).cmp(&(y % 32));
            }
            x /= 32;
            y /= 32;
        }
        a.cmp(&b)
    });
    out
}
