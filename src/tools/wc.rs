//! wc — count lines, words, bytes, characters, and the longest line.
//!
//! Column widths follow GNU: wide enough for the total size of all regular
//! files, and at least 7 when reading from a pipe or terminal.

use crate::util::{describe, try_help, utf8_locale};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};

#[derive(Default, Clone, Copy)]
struct Counts {
    lines: u64,
    words: u64,
    chars: u64,
    bytes: u64,
    max_line: u64,
}

#[derive(Clone, Copy)]
struct Show {
    lines: bool,
    words: bool,
    chars: bool,
    bytes: bool,
    max_line: bool,
}

fn count<R: Read>(mut r: R, utf8: bool) -> io::Result<Counts> {
    let mut c = Counts::default();
    let mut buf = vec![0u8; 64 * 1024];
    let mut in_word = false;
    let mut line_pos: u64 = 0;
    loop {
        let n = match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        c.bytes += n as u64;
        for &b in &buf[..n] {
            // a UTF-8 continuation byte isn't a new character
            let new_char = !utf8 || (b & 0xC0) != 0x80;
            if new_char {
                c.chars += 1;
            }
            match b {
                b'\n' => {
                    c.lines += 1;
                    c.max_line = c.max_line.max(line_pos);
                    line_pos = 0;
                    in_word = false;
                }
                b'\r' | b'\x0c' => {
                    c.max_line = c.max_line.max(line_pos);
                    line_pos = 0;
                    in_word = false;
                }
                b'\t' => {
                    line_pos += 8 - (line_pos % 8);
                    in_word = false;
                }
                b' ' | b'\x0b' => {
                    line_pos += 1;
                    in_word = false;
                }
                _ => {
                    let printable = (0x20..0x7f).contains(&b) || (utf8 && b >= 0x80 && new_char);
                    if printable {
                        line_pos += 1;
                    }
                    if !in_word && printable {
                        in_word = true;
                        c.words += 1;
                    }
                }
            }
        }
    }
    c.max_line = c.max_line.max(line_pos);
    Ok(c)
}

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut show = Show {
        lines: false,
        words: false,
        chars: false,
        bytes: false,
        max_line: false,
    };
    let mut files: Vec<OsString> = Vec::new();
    let mut only_files = false;

    for arg in args {
        let s = arg.to_string_lossy().into_owned();
        if only_files || s == "-" || !s.starts_with('-') {
            files.push(arg);
            continue;
        }
        if s == "--" {
            only_files = true;
            continue;
        }
        if let Some(long) = s.strip_prefix("--") {
            match long {
                "lines" => show.lines = true,
                "words" => show.words = true,
                "chars" => show.chars = true,
                "bytes" => show.bytes = true,
                "max-line-length" => show.max_line = true,
                "help" => {
                    println!("Usage: {prog} [OPTION]... [FILE]...\nPrint newline, word, and byte counts for each FILE, and a total line if\nmore than one FILE is specified.\n\n  -c, --bytes            print the byte counts\n  -m, --chars            print the character counts\n  -l, --lines            print the newline counts\n  -L, --max-line-length  print the maximum display width\n  -w, --words            print the word counts");
                    return 0;
                }
                "version" => {
                    println!("{prog} (rng) {}", crate::VERSION);
                    return 0;
                }
                _ => {
                    eprintln!("{prog}: unrecognized option '{s}'");
                    try_help(prog);
                    return 1;
                }
            }
            continue;
        }
        for c in s[1..].chars() {
            match c {
                'l' => show.lines = true,
                'w' => show.words = true,
                'm' => show.chars = true,
                'c' => show.bytes = true,
                'L' => show.max_line = true,
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return 1;
                }
            }
        }
    }
    if !(show.lines || show.words || show.chars || show.bytes || show.max_line) {
        show.lines = true;
        show.words = true;
        show.bytes = true;
    }
    let no_names = files.is_empty();
    if no_names {
        files.push("-".into());
    }
    let utf8 = utf8_locale();

    // Width: GNU sums the sizes of regular files; anything else forces >= 7.
    let mut regular_total: u64 = 0;
    let mut min_width = 1;
    let mut any_ok = false;
    for f in &files {
        let meta = if f.to_string_lossy() == "-" {
            stdin_meta()
        } else {
            std::fs::metadata(f).ok()
        };
        if let Some(m) = meta {
            any_ok = true;
            if m.is_file() {
                regular_total += m.len();
            } else {
                min_width = 7;
            }
        }
    }
    let one_column = [
        show.lines,
        show.words,
        show.chars,
        show.bytes,
        show.max_line,
    ]
    .iter()
    .filter(|&&x| x)
    .count()
        == 1;
    let mut width = regular_total.max(1).to_string().len().max(min_width);
    if !any_ok || (one_column && files.len() == 1) {
        width = 1;
    }

    let mut out = io::stdout().lock();
    let mut total = Counts::default();
    let mut code = 0;
    for f in &files {
        let name = f.to_string_lossy();
        let res = if name == "-" {
            count(io::stdin().lock(), utf8)
        } else {
            match File::open(f) {
                Ok(file) if file.metadata().map(|m| m.is_dir()).unwrap_or(false) => {
                    Err(io::Error::other("Is a directory"))
                }
                Ok(file) => count(file, utf8),
                Err(e) => Err(e),
            }
        };
        match res {
            Ok(c) => {
                total.lines += c.lines;
                total.words += c.words;
                total.chars += c.chars;
                total.bytes += c.bytes;
                total.max_line = total.max_line.max(c.max_line);
                let label = if no_names { None } else { Some(name.as_ref()) };
                let _ = print_row(&mut out, &c, show, width, label);
            }
            Err(e) => {
                let _ = out.flush();
                if e.to_string() == "Is a directory" {
                    eprintln!("{prog}: {name}: Is a directory");
                    let _ = print_row(&mut out, &Counts::default(), show, width, Some(&name));
                } else {
                    eprintln!("{prog}: {name}: {}", describe(&e));
                }
                code = 1;
            }
        }
    }
    if files.len() > 1 {
        let _ = print_row(&mut out, &total, show, width, Some("total"));
    }
    code
}

#[cfg(unix)]
fn stdin_meta() -> Option<std::fs::Metadata> {
    use std::os::fd::AsFd;
    let fd = io::stdin().as_fd().try_clone_to_owned().ok()?;
    File::from(fd).metadata().ok()
}

#[cfg(not(unix))]
fn stdin_meta() -> Option<std::fs::Metadata> {
    None
}

fn print_row<W: Write>(
    out: &mut W,
    c: &Counts,
    s: Show,
    w: usize,
    name: Option<&str>,
) -> io::Result<()> {
    let mut parts = Vec::new();
    if s.lines {
        parts.push(c.lines);
    }
    if s.words {
        parts.push(c.words);
    }
    if s.chars {
        parts.push(c.chars);
    }
    if s.bytes {
        parts.push(c.bytes);
    }
    if s.max_line {
        parts.push(c.max_line);
    }
    let line: Vec<String> = parts.iter().map(|n| format!("{n:>w$}")).collect();
    let mut row = line.join(" ");
    if let Some(n) = name {
        row.push(' ');
        row.push_str(n);
    }
    writeln!(out, "{row}")
}
