//! head and tail.
//!
//! head: -n [-]N, -c [-]N (a leading `-` means "all but the last N"), -q, -v, -z
//! tail: -n [+]N, -c [+]N (a leading `+` means "starting at N"), -q, -v, -z, -f
//! Both accept the old `-5` shorthand for `-n 5`, and size suffixes (K, M, kB…).

use crate::util::{describe, parse_size, try_help};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};

#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Lines,
    Bytes,
}

struct Args {
    unit: Unit,
    count: u64,
    /// head: "all but the last N"; tail: "start from N" (+N)
    flip: bool,
    quiet: bool,
    verbose: bool,
    zero: bool,
    follow: bool,
    files: Vec<OsString>,
}

fn parse(prog: &str, is_tail: bool, args: Vec<OsString>) -> Result<Args, u8> {
    let mut a = Args {
        unit: Unit::Lines,
        count: 10,
        flip: false,
        quiet: false,
        verbose: false,
        zero: false,
        follow: false,
        files: Vec::new(),
    };
    let mut only_files = false;
    let mut i = 0;

    let set_count = |a: &mut Args, unit: Unit, raw: &str| -> Result<(), u8> {
        let (flip, num) = match raw.as_bytes().first() {
            Some(b'-') if !is_tail => (true, &raw[1..]),
            Some(b'+') if is_tail => (true, &raw[1..]),
            Some(b'-') | Some(b'+') => (false, &raw[1..]),
            _ => (false, raw),
        };
        match parse_size(num) {
            Some(n) => {
                a.unit = unit;
                a.count = n;
                a.flip = flip;
                Ok(())
            }
            None => {
                let what = if unit == Unit::Lines {
                    "lines"
                } else {
                    "bytes"
                };
                eprintln!(
                    "{prog}: invalid number of {what}: {}",
                    crate::util::quote(raw)
                );
                Err(1)
            }
        }
    };

    while i < args.len() {
        let s = args[i].to_string_lossy().into_owned();
        i += 1;
        if only_files || s == "-" || !s.starts_with('-') {
            a.files.push(args[i - 1].clone());
            continue;
        }
        if s == "--" {
            only_files = true;
            continue;
        }
        if let Some(long) = s.strip_prefix("--") {
            let (key, val) = match long.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (long, None),
            };
            let mut need = |name: &str| -> Result<String, u8> {
                if let Some(v) = &val {
                    return Ok(v.clone());
                }
                match args.get(i) {
                    Some(v) => {
                        i += 1;
                        Ok(v.to_string_lossy().into_owned())
                    }
                    None => {
                        eprintln!("{prog}: option '--{name}' requires an argument");
                        try_help(prog);
                        Err(1)
                    }
                }
            };
            match key {
                "lines" => {
                    let v = need("lines")?;
                    set_count(&mut a, Unit::Lines, &v)?;
                }
                "bytes" => {
                    let v = need("bytes")?;
                    set_count(&mut a, Unit::Bytes, &v)?;
                }
                "quiet" | "silent" => {
                    a.quiet = true;
                    a.verbose = false;
                }
                "verbose" => {
                    a.verbose = true;
                    a.quiet = false;
                }
                "zero-terminated" => a.zero = true,
                "follow" if is_tail => a.follow = true,
                "help" => {
                    help(prog, is_tail);
                    return Err(0);
                }
                "version" => {
                    println!("{prog} (rng) {}", crate::VERSION);
                    return Err(0);
                }
                _ => {
                    eprintln!("{prog}: unrecognized option '{s}'");
                    try_help(prog);
                    return Err(1);
                }
            }
            continue;
        }
        // old-style -5
        if s[1..].bytes().all(|c| c.is_ascii_digit()) {
            set_count(&mut a, Unit::Lines, &s[1..])?;
            continue;
        }
        let chars: Vec<char> = s[1..].chars().collect();
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            match c {
                'n' | 'c' => {
                    let rest: String = chars[k + 1..].iter().collect();
                    let v = if !rest.is_empty() {
                        rest
                    } else if let Some(v) = args.get(i) {
                        i += 1;
                        v.to_string_lossy().into_owned()
                    } else {
                        eprintln!("{prog}: option requires an argument -- '{c}'");
                        try_help(prog);
                        return Err(1);
                    };
                    let unit = if c == 'n' { Unit::Lines } else { Unit::Bytes };
                    set_count(&mut a, unit, &v)?;
                    break;
                }
                'q' => {
                    a.quiet = true;
                    a.verbose = false;
                }
                'v' => {
                    a.verbose = true;
                    a.quiet = false;
                }
                'z' => a.zero = true,
                'f' if is_tail => a.follow = true,
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return Err(1);
                }
            }
            k += 1;
        }
    }
    if a.files.is_empty() {
        a.files.push("-".into());
    }
    Ok(a)
}

fn help(prog: &str, is_tail: bool) {
    if is_tail {
        println!("Usage: {prog} [OPTION]... [FILE]...\nPrint the last 10 lines of each FILE to standard output.\n\n  -c, --bytes=[+]NUM       output the last NUM bytes; or use -c +NUM to\n                           output starting with byte NUM of each file\n  -f, --follow             output appended data as the file grows\n  -n, --lines=[+]NUM       output the last NUM lines, instead of the last 10;\n                           or use -n +NUM to skip NUM-1 lines at the start\n  -q, --quiet, --silent    never output headers giving file names\n  -v, --verbose            always output headers giving file names\n  -z, --zero-terminated    line delimiter is NUL, not newline");
    } else {
        println!("Usage: {prog} [OPTION]... [FILE]...\nPrint the first 10 lines of each FILE to standard output.\n\n  -c, --bytes=[-]NUM       print the first NUM bytes of each file;\n                           with the leading '-', print all but the last NUM bytes\n  -n, --lines=[-]NUM       print the first NUM lines instead of the first 10;\n                           with the leading '-', print all but the last NUM lines\n  -q, --quiet, --silent    never print headers giving file names\n  -v, --verbose            always print headers giving file names\n  -z, --zero-terminated    line delimiter is NUL, not newline");
    }
}

fn display_name(f: &OsString) -> String {
    let s = f.to_string_lossy();
    if s == "-" {
        "standard input".into()
    } else {
        s.into_owned()
    }
}

fn open(f: &OsString) -> io::Result<Box<dyn Read>> {
    if f.to_string_lossy() == "-" {
        Ok(Box::new(io::stdin()))
    } else {
        let file = File::open(f)?;
        if file.metadata()?.is_dir() {
            return Err(io::Error::other("Is a directory"));
        }
        Ok(Box::new(file))
    }
}

fn read_all(f: &OsString) -> io::Result<Vec<u8>> {
    let mut r = open(f)?;
    let mut buf = Vec::new();
    r.read_to_end(&mut buf)?;
    Ok(buf)
}

/// Position just past the `n`th delimiter (or the end if there aren't that many).
fn after_nth(buf: &[u8], delim: u8, n: u64) -> usize {
    if n == 0 {
        return 0;
    }
    let mut seen = 0;
    for (i, &b) in buf.iter().enumerate() {
        if b == delim {
            seen += 1;
            if seen == n {
                return i + 1;
            }
        }
    }
    buf.len()
}

/// Start of the last `n` lines.
fn last_n_lines_start(buf: &[u8], delim: u8, n: u64) -> usize {
    if n == 0 {
        return buf.len();
    }
    // a final line without a delimiter still counts as a line
    let mut end = buf.len();
    if end > 0 && buf[end - 1] == delim {
        end -= 1;
    }
    let mut seen = 0;
    for i in (0..end).rev() {
        if buf[i] == delim {
            seen += 1;
            if seen == n {
                return i + 1;
            }
        }
    }
    0
}

fn run(prog: &str, is_tail: bool, args: Vec<OsString>) -> u8 {
    let a = match parse(prog, is_tail, args) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let delim = if a.zero { b'\0' } else { b'\n' };
    let headers = a.verbose || (a.files.len() > 1 && !a.quiet);
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut code = 0;
    let mut first = true;

    for f in &a.files {
        let name = display_name(f);
        let data = match read_all(f) {
            Ok(d) => d,
            Err(e) => {
                if e.to_string() == "Is a directory" {
                    if headers {
                        let _ = writeln!(out, "{}==> {name} <==", if first { "" } else { "\n" });
                        first = false;
                    }
                    let _ = out.flush();
                    eprintln!("{prog}: error reading '{name}': Is a directory");
                } else {
                    let _ = out.flush();
                    eprintln!("{prog}: cannot open '{name}' for reading: {}", describe(&e));
                }
                code = 1;
                continue;
            }
        };
        if headers {
            let _ = writeln!(out, "{}==> {name} <==", if first { "" } else { "\n" });
        }
        first = false;

        let slice: &[u8] = match (is_tail, a.unit, a.flip) {
            // head
            (false, Unit::Lines, false) => &data[..after_nth(&data, delim, a.count)],
            (false, Unit::Lines, true) => &data[..last_n_lines_start(&data, delim, a.count)],
            (false, Unit::Bytes, false) => &data[..data.len().min(a.count as usize)],
            (false, Unit::Bytes, true) => &data[..data.len().saturating_sub(a.count as usize)],
            // tail
            (true, Unit::Lines, false) => &data[last_n_lines_start(&data, delim, a.count)..],
            (true, Unit::Lines, true) => {
                &data[after_nth(&data, delim, a.count.saturating_sub(1))..]
            }
            (true, Unit::Bytes, false) => &data[data.len().saturating_sub(a.count as usize)..],
            (true, Unit::Bytes, true) => {
                &data[(a.count.saturating_sub(1) as usize).min(data.len())..]
            }
        };
        if let Err(e) = out.write_all(slice) {
            if e.kind() == io::ErrorKind::BrokenPipe {
                return code;
            }
            eprintln!("{prog}: write error: {}", describe(&e));
            return 1;
        }
    }
    if out.flush().is_err() {
        return code;
    }
    drop(out);

    if is_tail && a.follow {
        follow(prog, &a.files, headers);
    }
    code
}

/// tail -f: keep printing whatever gets appended (polls once a second).
fn follow(prog: &str, files: &[OsString], headers: bool) -> ! {
    let mut watched: Vec<(OsString, Option<File>, u64)> = files
        .iter()
        .filter(|f| f.to_string_lossy() != "-")
        .map(|f| {
            let file = File::open(f).ok();
            let len = file
                .as_ref()
                .and_then(|x| x.metadata().ok())
                .map(|m| m.len())
                .unwrap_or(0);
            (f.clone(), file, len)
        })
        .collect();
    let mut last_shown: Option<usize> = watched.len().checked_sub(1);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        for (idx, (name, file, pos)) in watched.iter_mut().enumerate() {
            let Some(fh) = file else { continue };
            let Ok(meta) = fh.metadata() else { continue };
            let len = meta.len();
            if len < *pos {
                eprintln!("{prog}: {}: file truncated", name.to_string_lossy());
                *pos = 0;
            }
            if len == *pos {
                continue;
            }
            let mut buf = Vec::new();
            if fh.seek(SeekFrom::Start(*pos)).is_ok() && fh.read_to_end(&mut buf).is_ok() {
                *pos += buf.len() as u64;
                let mut out = io::stdout().lock();
                if headers && last_shown != Some(idx) {
                    let _ = write!(out, "\n==> {} <==\n", name.to_string_lossy());
                    last_shown = Some(idx);
                }
                if out.write_all(&buf).and_then(|_| out.flush()).is_err() {
                    std::process::exit(1);
                }
            }
        }
    }
}

pub fn main_head(prog: &str, args: Vec<OsString>) -> u8 {
    run(prog, false, args)
}

pub fn main_tail(prog: &str, args: Vec<OsString>) -> u8 {
    run(prog, true, args)
}

#[cfg(test)]
mod tests {
    use super::{after_nth, last_n_lines_start};

    #[test]
    fn line_math() {
        let d = b"a\nb\nc\n";
        assert_eq!(&d[..after_nth(d, b'\n', 2)], b"a\nb\n");
        assert_eq!(&d[last_n_lines_start(d, b'\n', 2)..], b"b\nc\n");
        let no_nl = b"a\nb\nc";
        assert_eq!(&no_nl[last_n_lines_start(no_nl, b'\n', 1)..], b"c");
        assert_eq!(&no_nl[..last_n_lines_start(no_nl, b'\n', 1)], b"a\nb\n");
    }
}
