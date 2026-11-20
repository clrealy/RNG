//! yes — print a line over and over until killed.

use std::ffi::OsString;
use std::io::{self, Write};

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    if args.len() == 1 {
        match args[0].to_str() {
            Some("--help") => {
                println!("Usage: {prog} [STRING]...\nRepeatedly output a line with all specified STRING(s), or 'y'.");
                return 0;
            }
            Some("--version") => {
                println!("{prog} (rng) {}", crate::VERSION);
                return 0;
            }
            _ => {}
        }
    }
    let mut words = args.as_slice();
    if words.first().and_then(|w| w.to_str()) == Some("--") {
        words = &words[1..];
    }

    let mut line: Vec<u8> = if words.is_empty() {
        b"y".to_vec()
    } else {
        words
            .iter()
            .map(crate::util::bytes)
            .collect::<Vec<_>>()
            .join(&b' ')
    };
    line.push(b'\n');

    // Fill an 8 KiB buffer with copies of the line. 8 KiB writes are the
    // sweet spot for pipes (bigger was ~2x slower in testing).
    let mut buf = Vec::with_capacity(8 * 1024);
    while buf.len() + line.len() <= 8 * 1024 || buf.is_empty() {
        buf.extend_from_slice(&line);
    }

    // Write straight to fd 1: Rust's stdout is line-buffered, which would
    // chop every big write into tiny pieces.
    let mut out = raw_stdout();
    loop {
        if let Err(e) = out.write_all(&buf) {
            if e.kind() == io::ErrorKind::BrokenPipe {
                // GNU yes dies from SIGPIPE; a shell reports that as 141.
                return 141;
            }
            eprintln!("{prog}: standard output: {}", crate::util::describe(&e));
            return 1;
        }
    }
}

#[cfg(unix)]
fn raw_stdout() -> std::mem::ManuallyDrop<std::fs::File> {
    use std::os::fd::FromRawFd;
    // SAFETY: fd 1 stays open for the whole process; ManuallyDrop keeps us
    // from closing it.
    std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(1) })
}

#[cfg(not(unix))]
fn raw_stdout() -> io::Stdout {
    io::stdout()
}
