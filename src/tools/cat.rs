//! cat — concatenate files and print them on standard output.
//!
//! Supports the GNU flags: -A -b -e -E -n -s -t -T -u -v, their long forms,
//! combined short flags (`-nE`), `-` for stdin, and `--` to end options.
//! Line numbering carries on across files, like GNU.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};

#[derive(Default, Clone, Copy)]
struct Opts {
    number: bool,           // -n
    number_nonblank: bool,  // -b (wins over -n)
    squeeze: bool,          // -s
    show_ends: bool,        // -E
    show_tabs: bool,        // -T
    show_nonprinting: bool, // -v
}

impl Opts {
    fn plain(&self) -> bool {
        !(self.number
            || self.number_nonblank
            || self.squeeze
            || self.show_ends
            || self.show_tabs
            || self.show_nonprinting)
    }
}

/// State that has to survive from one file to the next.
struct State {
    line_no: u64,
    at_line_start: bool,
    prev_blank: bool,
}

const HELP: &str = "Usage: cat [OPTION]... [FILE]...
Concatenate FILE(s) to standard output.

With no FILE, or when FILE is -, read standard input.

  -A, --show-all           equivalent to -vET
  -b, --number-nonblank    number nonempty output lines, overrides -n
  -e                       equivalent to -vE
  -E, --show-ends          display $ at end of each line
  -n, --number             number all output lines
  -s, --squeeze-blank      suppress repeated empty output lines
  -t                       equivalent to -vT
  -T, --show-tabs          display TAB characters as ^I
  -u                       (ignored)
  -v, --show-nonprinting   use ^ and M- notation, except for LFD and TAB
      --help               display this help and exit
      --version            output version information and exit";

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut o = Opts::default();
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
                "show-all" => {
                    o.show_nonprinting = true;
                    o.show_ends = true;
                    o.show_tabs = true;
                }
                "number-nonblank" => o.number_nonblank = true,
                "show-ends" => o.show_ends = true,
                "number" => o.number = true,
                "squeeze-blank" => o.squeeze = true,
                "show-tabs" => o.show_tabs = true,
                "show-nonprinting" => o.show_nonprinting = true,
                "help" => {
                    println!("{HELP}");
                    return 0;
                }
                "version" => {
                    println!("{prog} (rng) {}", crate::VERSION);
                    return 0;
                }
                _ => {
                    eprintln!("{prog}: unrecognized option '{s}'");
                    eprintln!("Try '{prog} --help' for more information.");
                    return 1;
                }
            }
            continue;
        }
        for c in s[1..].chars() {
            match c {
                'A' => {
                    o.show_nonprinting = true;
                    o.show_ends = true;
                    o.show_tabs = true;
                }
                'b' => o.number_nonblank = true,
                'e' => {
                    o.show_nonprinting = true;
                    o.show_ends = true;
                }
                'E' => o.show_ends = true,
                'n' => o.number = true,
                's' => o.squeeze = true,
                't' => {
                    o.show_nonprinting = true;
                    o.show_tabs = true;
                }
                'T' => o.show_tabs = true,
                'u' => {}
                'v' => o.show_nonprinting = true,
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    eprintln!("Try '{prog} --help' for more information.");
                    return 1;
                }
            }
        }
    }
    if o.number_nonblank {
        o.number = false;
    }
    if files.is_empty() {
        files.push("-".into());
    }

    let stdout = io::stdout();
    let mut out = BufWriter::with_capacity(64 * 1024, stdout.lock());
    let mut st = State {
        line_no: 0,
        at_line_start: true,
        prev_blank: false,
    };
    let mut code = 0;

    for f in &files {
        let name = f.to_string_lossy();
        let res = if name == "-" {
            let stdin = io::stdin();
            let lock = stdin.lock();
            copy(lock, &mut out, o, &mut st)
        } else {
            match File::open(f) {
                Ok(file) => {
                    if file.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                        eprintln!("{prog}: {name}: Is a directory");
                        code = 1;
                        continue;
                    }
                    copy(file, &mut out, o, &mut st)
                }
                Err(e) => Err(e),
            }
        };
        if let Err(e) = res {
            if e.kind() == io::ErrorKind::BrokenPipe {
                return code;
            }
            eprintln!("{prog}: {name}: {}", describe(&e));
            code = 1;
        }
    }
    if let Err(e) = out.flush() {
        if e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("{prog}: write error: {}", describe(&e));
            code = 1;
        }
    }
    code
}

/// GNU-style error text ("No such file or directory", not "... (os error 2)").
fn describe(e: &io::Error) -> String {
    let s = e.to_string();
    match s.find(" (os error") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

fn copy<R: Read, W: Write>(src: R, out: &mut W, o: Opts, st: &mut State) -> io::Result<()> {
    if o.plain() {
        let mut src = src;
        io::copy(&mut src, out)?;
        return Ok(());
    }

    let mut reader = BufReader::with_capacity(64 * 1024, src);
    let mut line = Vec::with_capacity(256);
    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(());
        }
        let has_nl = line.last() == Some(&b'\n');
        let body = if has_nl {
            &line[..line.len() - 1]
        } else {
            &line[..]
        };
        // A "blank" line is one that is just "\n" and starts at a line start.
        let blank = st.at_line_start && has_nl && body.is_empty();

        if o.squeeze && blank && st.prev_blank {
            continue;
        }

        if st.at_line_start && (o.number || (o.number_nonblank && !blank)) {
            st.line_no += 1;
            write!(out, "{:>6}\t", st.line_no)?;
        }

        for &b in body {
            put_byte(out, b, o)?;
        }
        if has_nl {
            if o.show_ends {
                out.write_all(b"$")?;
            }
            out.write_all(b"\n")?;
            st.prev_blank = blank;
            st.at_line_start = true;
        } else {
            // file ended mid-line; the next file continues this line
            st.at_line_start = false;
            st.prev_blank = false;
        }
    }
}

fn put_byte<W: Write>(out: &mut W, b: u8, o: Opts) -> io::Result<()> {
    if b == b'\t' {
        return if o.show_tabs {
            out.write_all(b"^I")
        } else {
            out.write_all(b"\t")
        };
    }
    if !o.show_nonprinting {
        return out.write_all(&[b]);
    }
    let mut c = b;
    if c >= 128 {
        out.write_all(b"M-")?;
        c -= 128;
    }
    match c {
        0..=31 => out.write_all(&[b'^', c + 64]),
        127 => out.write_all(b"^?"),
        _ => out.write_all(&[c]),
    }
}
