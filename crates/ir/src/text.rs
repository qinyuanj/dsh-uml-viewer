pub fn sub(s: &[u8], i: i64, j: i64) -> &[u8] {
    let n = s.len() as i64;
    let start = if i < 0 {
        (n + i + 1).max(1)
    } else {
        i.min(n + 1)
    };
    let finish = if j < 0 { (n + j + 1).max(0) } else { j.min(n) };
    if start > finish {
        &[]
    } else {
        &s[(start - 1) as usize..finish as usize]
    }
}

pub fn at(s: &[u8], i: usize) -> Option<u8> {
    if i == 0 { None } else { s.get(i - 1).copied() }
}

pub fn find_plain(s: &[u8], needle: &[u8], i: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(i.max(1).min(s.len() + 1));
    }
    let start = i.saturating_sub(1);
    s.get(start..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| start + p + 1)
}

pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

pub fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

pub fn is_alpha_(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

pub fn is_ts_ident(c: u8) -> bool {
    is_word(c) || c == b'$'
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub fn trim(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|&c| !is_space(c)).unwrap_or(s.len());
    let end = s.iter().rposition(|&c| !is_space(c)).map_or(0, |p| p + 1);
    &s[start..end.max(start)]
}

pub fn scan_quoted(source: &[u8], i: usize, quote: u8, n: Option<usize>) -> usize {
    let n = n.unwrap_or(source.len());
    let mut j = i + 1;
    let mut escaped = false;
    while j <= n {
        let c = source[j - 1];
        if escaped {
            escaped = false;
        } else if c == b'\\' {
            escaped = true;
        } else if c == quote {
            return j + 1;
        }
        j += 1;
    }
    n + 1
}

pub fn line_comment(c: Option<u8>, nxt: Option<u8>) -> bool {
    c == Some(b'/') && nxt == Some(b'/')
}

pub fn block_comment(c: Option<u8>, nxt: Option<u8>) -> bool {
    c == Some(b'/') && nxt == Some(b'*')
}

pub fn mask_line_comment(source: &[u8], out: &mut Vec<u8>, i: usize, n: Option<usize>) -> usize {
    let finish = find_plain(source, b"\n", i).unwrap_or(n.unwrap_or(source.len()) + 1);
    out.extend(std::iter::repeat_n(b' ', finish - i));
    finish
}

pub fn mask_block_comment(source: &[u8], out: &mut Vec<u8>, i: usize, n: Option<usize>) -> usize {
    let limit = n.unwrap_or(source.len()) + 1;
    let close = find_plain(source, b"*/", i + 2);
    let finish = close.map_or(limit, |c| limit.min(c + 2));
    out.extend(
        source[i - 1..finish - 1]
            .iter()
            .map(|&c| if c == b'\n' { c } else { b' ' }),
    );
    finish
}
