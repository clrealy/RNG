//! echo — print a line of text.
//!
//! Matches GNU coreutils echo: options are only recognised before the first
//! non-option word, and a word counts as an option only if it is `-` followed
//! entirely by `n`, `e` and `E` (so `-nx` is printed literally).

use std::ffi::OsString;
use std::io::{self, Write};

#[cfg(unix)]
fn bytes(s: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn bytes(s: &OsString) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    // A lone --help / --version is handled like GNU does.
    if args.len() == 1 {
        match args[0].to_str() {
            Some("--help") => {
                println!(
                    "Usage: {prog} [SHORT-OPTION]... [STRING]...\n\
                     Echo the STRING(s) to standard output.\n\n\
                     \x20 -n     do not output the trailing newline\n\
                     \x20 -e     enable interpretation of backslash escapes\n\
                     \x20 -E     disable interpretation of backslash escapes (default)"
                );
                return 0;
            }
            Some("--version") => {
                println!("{prog} (rng) {}", crate::VERSION);
                return 0;
            }
            _ => {}
        }
    }

    let mut newline = true;
    let mut escapes = false;
    let mut words = args.iter().map(bytes).peekable();

    while let Some(w) = words.peek() {
        let is_opt =
            w.len() > 1 && w[0] == b'-' && w[1..].iter().all(|c| matches!(c, b'n' | b'e' | b'E'));
        if !is_opt {
            break;
        }
        for c in &w[1..] {
            match c {
                b'n' => newline = false,
                b'e' => escapes = true,
                b'E' => escapes = false,
                _ => unreachable!(),
            }
        }
        words.next();
    }

    let mut out = Vec::new();
    let mut first = true;
    for w in words {
        if !first {
            out.push(b' ');
        }
        first = false;
        if escapes {
            if unescape(&w, &mut out) {
                // `\c` means: stop all output right here, no newline.
                return write_out(&out);
            }
        } else {
            out.extend_from_slice(&w);
        }
    }
    if newline {
        out.push(b'\n');
    }
    write_out(&out)
}

fn write_out(buf: &[u8]) -> u8 {
    let mut stdout = io::stdout().lock();
    match stdout.write_all(buf).and_then(|_| stdout.flush()) {
        Ok(()) => 0,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(e) => {
            eprintln!("echo: write error: {e}");
            1
        }
    }
}

/// Expand backslash escapes from `src` into `out`.
/// Returns true if `\c` was hit (caller must stop printing).
fn unescape(src: &[u8], out: &mut Vec<u8>) -> bool {
    let mut i = 0;
    while i < src.len() {
        let c = src[i];
        if c != b'\\' || i + 1 >= src.len() {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let e = src[i];
        i += 1;
        match e {
            b'\\' => out.push(b'\\'),
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'c' => return true,
            b'e' => out.push(0x1b),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'0' => {
                // \0NNN: up to 3 octal digits
                let mut v: u32 = 0;
                let mut n = 0;
                while n < 3 && i < src.len() && (b'0'..=b'7').contains(&src[i]) {
                    v = v * 8 + (src[i] - b'0') as u32;
                    i += 1;
                    n += 1;
                }
                out.push(v as u8);
            }
            b'x' => {
                // \xHH: 1 or 2 hex digits; with none, print it literally
                let mut v: u32 = 0;
                let mut n = 0;
                while n < 2 && i < src.len() && src[i].is_ascii_hexdigit() {
                    v = v * 16 + (src[i] as char).to_digit(16).unwrap();
                    i += 1;
                    n += 1;
                }
                if n == 0 {
                    out.extend_from_slice(b"\\x");
                } else {
                    out.push(v as u8);
                }
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::unescape;

    fn ue(s: &str) -> (Vec<u8>, bool) {
        let mut out = Vec::new();
        let stop = unescape(s.as_bytes(), &mut out);
        (out, stop)
    }

    #[test]
    fn basic_escapes() {
        assert_eq!(ue(r"a\tb\nc").0, b"a\tb\nc");
        assert_eq!(ue(r"\\").0, b"\\");
        assert_eq!(ue(r"\q").0, b"\\q");
    }

    #[test]
    fn octal_and_hex() {
        assert_eq!(ue(r"\0101").0, b"A");
        assert_eq!(ue(r"\x41\x4a").0, b"AJ");
        assert_eq!(ue(r"\xZ").0, b"\\xZ");
    }

    #[test]
    fn stop_at_c() {
        let (out, stop) = ue(r"hi\cbye");
        assert_eq!(out, b"hi");
        assert!(stop);
    }
}
