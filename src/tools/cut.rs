//! cut — print selected parts of lines.
//! -b LIST, -c LIST (bytes, like GNU), -f LIST, -d DELIM, -s, -z,
//! --complement, --output-delimiter=STR

use crate::util::{describe, quote, try_help};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Bytes,
    Fields,
}

/// Inclusive 1-based ranges, sorted and merged. `usize::MAX` = to the end.
type Ranges = Vec<(usize, usize)>;

fn parse_list(prog: &str, list: &str, fields: bool) -> Result<Ranges, ()> {
    let mut v: Ranges = Vec::new();
    for part in list.split([',', ' ']) {
        if part.is_empty() {
            continue;
        }
        let bad = |what: &str| {
            eprintln!("{prog}: {what}");
            try_help(prog);
        };
        let num = |s: &str| -> Result<usize, ()> {
            match s.parse::<usize>() {
                Ok(0) => {
                    bad(if fields {
                        "fields are numbered from 1"
                    } else {
                        "byte/character positions are numbered from 1"
                    });
                    Err(())
                }
                Ok(n) => Ok(n),
                Err(_) => {
                    let kind = if fields {
                        "field value"
                    } else {
                        "byte/character position"
                    };
                    eprintln!("{prog}: invalid {kind} {}", quote(s));
                    try_help(prog);
                    Err(())
                }
            }
        };
        let (a, b) = match part.split_once('-') {
            None => {
                let n = num(part)?;
                (n, n)
            }
            Some(("", "")) => {
                bad("invalid range with no endpoint: -");
                return Err(());
            }
            Some(("", hi)) => (1, num(hi)?),
            Some((lo, "")) => (num(lo)?, usize::MAX),
            Some((lo, hi)) => {
                let (l, h) = (num(lo)?, num(hi)?);
                if l > h {
                    bad("invalid decreasing range");
                    return Err(());
                }
                (l, h)
            }
        };
        v.push((a, b));
    }
    if v.is_empty() {
        eprintln!(
            "{prog}: {}",
            if fields {
                "fields are numbered from 1"
            } else {
                "byte/character positions are numbered from 1"
            }
        );
        try_help(prog);
        return Err(());
    }
    v.sort();
    let mut merged: Ranges = Vec::new();
    for (a, b) in v {
        if let Some(last) = merged.last_mut() {
            if a <= last.1.saturating_add(1) {
                last.1 = last.1.max(b);
                continue;
            }
        }
        merged.push((a, b));
    }
    Ok(merged)
}

fn complement(r: &Ranges) -> Ranges {
    let mut out = Vec::new();
    let mut next = 1usize;
    for &(a, b) in r {
        if a > next {
            out.push((next, a - 1));
        }
        if b == usize::MAX {
            return out;
        }
        next = b + 1;
    }
    out.push((next, usize::MAX));
    out
}

fn selected(r: &Ranges, i: usize) -> bool {
    r.iter().any(|&(a, b)| i >= a && i <= b)
}

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut mode: Option<Mode> = None;
    let mut list: Option<String> = None;
    let mut delim: Option<u8> = None;
    let mut out_delim: Option<Vec<u8>> = None;
    let mut only_delimited = false;
    let mut compl = false;
    let mut zero = false;
    let mut files: Vec<OsString> = Vec::new();
    let mut only_files = false;
    let mut i = 0;
    let mut types = 0;

    while i < args.len() {
        let s = args[i].to_string_lossy().into_owned();
        i += 1;
        if only_files || !s.starts_with('-') || s == "-" {
            files.push(args[i - 1].clone());
            continue;
        }
        if s == "--" {
            only_files = true;
            continue;
        }
        let mut set_delim = |d: &str| -> Result<(), u8> {
            let b = d.as_bytes();
            if b.len() > 1 {
                eprintln!("{prog}: the delimiter must be a single character");
                try_help(prog);
                return Err(1);
            }
            delim = Some(*b.first().unwrap_or(&b'\0'));
            Ok(())
        };
        if let Some(long) = s.strip_prefix("--") {
            let (key, val) = match long.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (long, None),
            };
            let mut value = || -> Option<String> {
                val.clone().or_else(|| {
                    let v = args.get(i).map(|v| v.to_string_lossy().into_owned());
                    if v.is_some() {
                        i += 1;
                    }
                    v
                })
            };
            match key {
                "bytes" | "characters" | "fields" => {
                    types += 1;
                    mode = Some(if key == "fields" {
                        Mode::Fields
                    } else {
                        Mode::Bytes
                    });
                    list = value();
                }
                "delimiter" => {
                    if let Err(c) = set_delim(&value().unwrap_or_default()) {
                        return c;
                    }
                }
                "output-delimiter" => out_delim = Some(value().unwrap_or_default().into_bytes()),
                "only-delimited" => only_delimited = true,
                "complement" => compl = true,
                "zero-terminated" => zero = true,
                "help" => {
                    println!("Usage: {prog} OPTION... [FILE]...\nPrint selected parts of lines from each FILE to standard output.\n\n  -b, --bytes=LIST        select only these bytes\n  -c, --characters=LIST   select only these characters\n  -d, --delimiter=DELIM   use DELIM instead of TAB for field delimiter\n  -f, --fields=LIST       select only these fields\n      --complement        complement the set of selected bytes, characters or fields\n  -s, --only-delimited    do not print lines not containing delimiters\n      --output-delimiter=STRING  use STRING as the output delimiter\n  -z, --zero-terminated   line delimiter is NUL, not newline");
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
        let chars: Vec<char> = s[1..].chars().collect();
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            let take = |i: &mut usize| -> Option<String> {
                let rest: String = chars[k + 1..].iter().collect();
                if !rest.is_empty() {
                    Some(rest)
                } else if let Some(v) = args.get(*i) {
                    *i += 1;
                    Some(v.to_string_lossy().into_owned())
                } else {
                    None
                }
            };
            match c {
                'b' | 'c' | 'f' | 'd' => {
                    let Some(v) = take(&mut i) else {
                        eprintln!("{prog}: option requires an argument -- '{c}'");
                        try_help(prog);
                        return 1;
                    };
                    if c == 'd' {
                        if let Err(code) = set_delim(&v) {
                            return code;
                        }
                    } else {
                        types += 1;
                        mode = Some(if c == 'f' { Mode::Fields } else { Mode::Bytes });
                        list = Some(v);
                    }
                    break;
                }
                's' => only_delimited = true,
                'z' => zero = true,
                'n' => {}
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return 1;
                }
            }
            k += 1;
        }
    }

    if types > 1 {
        eprintln!("{prog}: only one list may be specified");
        try_help(prog);
        return 1;
    }
    let (Some(mode), Some(list)) = (mode, list) else {
        eprintln!("{prog}: you must specify a list of bytes, characters, or fields");
        try_help(prog);
        return 1;
    };
    if mode != Mode::Fields && delim.is_some() {
        eprintln!("{prog}: an input delimiter may be specified only when operating on fields");
        try_help(prog);
        return 1;
    }
    if mode != Mode::Fields && only_delimited {
        eprintln!(
            "{prog}: suppressing non-delimited lines makes sense\n\tonly when operating on fields"
        );
        try_help(prog);
        return 1;
    }
    let Ok(mut ranges) = parse_list(prog, &list, mode == Mode::Fields) else {
        return 1;
    };
    if compl {
        ranges = complement(&ranges);
    }
    let delim = delim.unwrap_or(b'\t');
    let eol = if zero { b'\0' } else { b'\n' };
    if files.is_empty() {
        files.push("-".into());
    }

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut code = 0;
    for f in &files {
        let name = f.to_string_lossy();
        let reader: Box<dyn Read> = if name == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(f) {
                Ok(fh) => Box::new(fh),
                Err(e) => {
                    let _ = out.flush();
                    eprintln!("{prog}: {name}: {}", describe(&e));
                    code = 1;
                    continue;
                }
            }
        };
        let mut r = BufReader::new(reader);
        let mut line = Vec::new();
        loop {
            line.clear();
            match r.read_until(eol, &mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    let _ = out.flush();
                    eprintln!("{prog}: {name}: {}", describe(&e));
                    code = 1;
                    break;
                }
            }
            if line.last() == Some(&eol) {
                line.pop();
            }
            let res = match mode {
                Mode::Bytes => cut_bytes(&mut out, &line, &ranges, out_delim.as_deref()),
                Mode::Fields => cut_fields(
                    &mut out,
                    &line,
                    &ranges,
                    delim,
                    out_delim.as_deref(),
                    only_delimited,
                ),
            };
            match res {
                Ok(true) => {
                    let _ = out.write_all(&[eol]);
                }
                Ok(false) => {}
                Err(_) => return code,
            }
        }
    }
    let _ = out.flush();
    code
}

fn cut_bytes<W: Write>(
    out: &mut W,
    line: &[u8],
    r: &Ranges,
    od: Option<&[u8]>,
) -> io::Result<bool> {
    let mut prev: Option<usize> = None;
    for (idx, &b) in line.iter().enumerate() {
        let pos = idx + 1;
        if selected(r, pos) {
            if let (Some(d), Some(p)) = (od, prev) {
                if p + 1 != pos {
                    out.write_all(d)?;
                }
            }
            out.write_all(&[b])?;
            prev = Some(pos);
        }
    }
    Ok(true)
}

fn cut_fields<W: Write>(
    out: &mut W,
    line: &[u8],
    r: &Ranges,
    d: u8,
    od: Option<&[u8]>,
    only: bool,
) -> io::Result<bool> {
    if !line.contains(&d) {
        if only {
            return Ok(false);
        }
        out.write_all(line)?;
        return Ok(true);
    }
    let od = od.map(|x| x.to_vec()).unwrap_or_else(|| vec![d]);
    let mut first = true;
    for (idx, field) in line.split(|&c| c == d).enumerate() {
        if selected(r, idx + 1) {
            if !first {
                out.write_all(&od)?;
            }
            out.write_all(field)?;
            first = false;
        }
    }
    Ok(true)
}
