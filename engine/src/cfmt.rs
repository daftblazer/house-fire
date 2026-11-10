//! The C library's number formats the tools' output uses (`%#x`, `%g`), so that their lines
//! read as the C++ tools' did, and its fixed buffers (`snprintf` into n bytes).

/// `%#x`: hexadecimal with 0x, but a plain 0 for zero.
pub fn alt_hex(v: u64) -> String {
    if v == 0 { "0".into() } else { format!("{v:#x}") }
}

/// `%.{prec}g` (`%g` is 6).
pub fn g(v: f64, prec: usize) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan" } else { "nan" }.into();
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf" } else { "inf" }.into();
    }
    let p = prec.max(1);
    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.into();
    }
    // The exponent after rounding to p significant digits.
    let e = format!("{:.*e}", p - 1, v);
    let (mantissa, exp) = e.split_once('e').unwrap();
    let x: i32 = exp.parse().unwrap();
    let strip = |s: &str| -> String {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s.to_string() }
    };
    if x < -4 || x >= p as i32 {
        let m = strip(mantissa);
        format!("{m}e{}{:02}", if x < 0 { '-' } else { '+' }, x.abs())
    } else {
        strip(&format!("{:.*}", (p as i32 - 1 - x) as usize, v))
    }
}

/// What `snprintf` into a buffer of `n` bytes keeps of `s`: its first n - 1 bytes.
pub fn buf(mut s: String, n: usize) -> String {
    if s.len() >= n {
        let mut k = n - 1;
        while !s.is_char_boundary(k) {
            k -= 1;
        }
        s.truncate(k);
    }
    s
}
