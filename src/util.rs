//! Bits shared by several tools.

use std::ffi::OsString;
use std::io;

/// GNU-style error text: "No such file or directory", not "... (os error 2)".
pub fn describe(e: &io::Error) -> String {
    let s = e.to_string();
    match s.find(" (os error") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

/// Raw bytes of an argument (paths can be any bytes on Unix).
#[cfg(unix)]
pub fn bytes(s: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

#[cfg(not(unix))]
pub fn bytes(s: &OsString) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

/// Parse a GNU size like `10`, `2K`, `1MB`, `3b`. Returns None if invalid.
pub fn parse_size(s: &str) -> Option<u64> {
    let digits_end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if digits_end == 0 {
        return None;
    }
    let n: u64 = s[..digits_end].parse().ok()?;
    let mult: u64 = match &s[digits_end..] {
        "" => 1,
        "b" => 512,
        "kB" | "KB" => 1000,
        "K" | "KiB" => 1024,
        "MB" => 1000u64.pow(2),
        "M" | "MiB" => 1024u64.pow(2),
        "GB" => 1000u64.pow(3),
        "G" | "GiB" => 1024u64.pow(3),
        "TB" => 1000u64.pow(4),
        "T" | "TiB" => 1024u64.pow(4),
        _ => return None,
    };
    n.checked_mul(mult)
}

/// Print a "Try --help" hint the GNU way.
pub fn try_help(prog: &str) {
    eprintln!("Try '{prog} --help' for more information.");
}

/// Quote a value the way GNU's quote() does: ‘x’ in a UTF-8 locale, 'x' otherwise.
pub fn quote(s: &str) -> String {
    if utf8_locale() {
        format!("\u{2018}{s}\u{2019}")
    } else {
        format!("'{s}'")
    }
}

/// True if the locale says UTF-8 (LC_ALL > LC_CTYPE > LANG).
pub fn utf8_locale() -> bool {
    for var in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                let v = v.to_ascii_lowercase();
                return v.contains("utf-8") || v.contains("utf8");
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::parse_size;

    #[test]
    fn sizes() {
        assert_eq!(parse_size("10"), Some(10));
        assert_eq!(parse_size("2K"), Some(2048));
        assert_eq!(parse_size("1kB"), Some(1000));
        assert_eq!(parse_size("1b"), Some(512));
        assert_eq!(parse_size("x"), None);
        assert_eq!(parse_size("5Q"), None);
    }
}
