//! uniq — report or omit repeated lines.
//! -c -d -D -u -i -f N -s N -w N -z, uniq [INPUT [OUTPUT]]

use crate::util::{describe, quote, try_help};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};

struct Opts {
    count: bool,
    only_dups: bool,
    all_dups: bool,
    only_unique: bool,
    ignore_case: bool,
    skip_fields: usize,
    skip_chars: usize,
    check_chars: Option<usize>,
    eol: u8,
}

fn key<'a>(line: &'a [u8], o: &Opts) -> &'a [u8] {
    let mut i = 0;
    for _ in 0..o.skip_fields {
        while i < line.len() && (line[i] == b' ' || line[i] == b'\t') {
            i += 1;
        }
        while i < line.len() && line[i] != b' ' && line[i] != b'\t' {
            i += 1;
        }
    }
    i = (i + o.skip_chars).min(line.len());
    let end = match o.check_chars {
        Some(n) => (i + n).min(line.len()),
        None => line.len(),
    };
    &line[i..end]
}

fn same(a: &[u8], b: &[u8], o: &Opts) -> bool {
    let (ka, kb) = (key(a, o), key(b, o));
    if o.ignore_case {
        ka.eq_ignore_ascii_case(kb)
    } else {
        ka == kb
    }
}

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut o = Opts {
        count: false,
        only_dups: false,
        all_dups: false,
        only_unique: false,
        ignore_case: false,
        skip_fields: 0,
        skip_chars: 0,
        check_chars: None,
        eol: b'\n',
    };
    let mut files: Vec<OsString> = Vec::new();
    let mut only_files = false;
    let mut i = 0;

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
        let parse_n = |v: &str, what: &str| -> Result<usize, u8> {
            v.parse::<usize>().map_err(|_| {
                eprintln!("{prog}: {v}: invalid number of {what} to skip");
                1
            })
        };
        if let Some(long) = s.strip_prefix("--") {
            let (k, val) = match long.split_once('=') {
                Some((a, b)) => (a, Some(b.to_string())),
                None => (long, None),
            };
            let mut value = || {
                val.clone().or_else(|| {
                    let v = args.get(i).map(|v| v.to_string_lossy().into_owned());
                    if v.is_some() {
                        i += 1;
                    }
                    v
                })
            };
            match k {
                "count" => o.count = true,
                "repeated" => o.only_dups = true,
                "all-repeated" => o.all_dups = true,
                "unique" => o.only_unique = true,
                "ignore-case" => o.ignore_case = true,
                "zero-terminated" => o.eol = b'\0',
                "skip-fields" => match parse_n(&value().unwrap_or_default(), "fields") {
                    Ok(n) => o.skip_fields = n,
                    Err(c) => return c,
                },
                "skip-chars" => match parse_n(&value().unwrap_or_default(), "bytes") {
                    Ok(n) => o.skip_chars = n,
                    Err(c) => return c,
                },
                "check-chars" => {
                    let v = value().unwrap_or_default();
                    match v.parse() {
                        Ok(n) => o.check_chars = Some(n),
                        Err(_) => {
                            eprintln!("{prog}: {v}: invalid number of bytes to compare");
                            return 1;
                        }
                    }
                }
                "help" => {
                    println!("Usage: {prog} [OPTION]... [INPUT [OUTPUT]]\nFilter adjacent matching lines from INPUT (or standard input),\nwriting to OUTPUT (or standard output).\n\n  -c, --count           prefix lines by the number of occurrences\n  -d, --repeated        only print duplicate lines, one for each group\n  -D                    print all duplicate lines\n  -f, --skip-fields=N   avoid comparing the first N fields\n  -i, --ignore-case     ignore differences in case when comparing\n  -s, --skip-chars=N    avoid comparing the first N characters\n  -u, --unique          only print unique lines\n  -z, --zero-terminated line delimiter is NUL, not newline\n  -w, --check-chars=N   compare no more than N characters in lines");
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
        // old style -N means -f N
        if chars.iter().all(|c| c.is_ascii_digit()) {
            match parse_n(&s[1..], "fields") {
                Ok(n) => o.skip_fields = n,
                Err(c) => return c,
            }
            continue;
        }
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            match c {
                'c' => o.count = true,
                'd' => o.only_dups = true,
                'D' => o.all_dups = true,
                'u' => o.only_unique = true,
                'i' => o.ignore_case = true,
                'z' => o.eol = b'\0',
                'f' | 's' | 'w' => {
                    let rest: String = chars[k + 1..].iter().collect();
                    let v = if !rest.is_empty() {
                        rest
                    } else if let Some(v) = args.get(i) {
                        i += 1;
                        v.to_string_lossy().into_owned()
                    } else {
                        eprintln!("{prog}: option requires an argument -- '{c}'");
                        try_help(prog);
                        return 1;
                    };
                    match c {
                        'f' => match parse_n(&v, "fields") {
                            Ok(n) => o.skip_fields = n,
                            Err(code) => return code,
                        },
                        's' => match parse_n(&v, "bytes") {
                            Ok(n) => o.skip_chars = n,
                            Err(code) => return code,
                        },
                        _ => match v.parse() {
                            Ok(n) => o.check_chars = Some(n),
                            Err(_) => {
                                eprintln!("{prog}: {v}: invalid number of bytes to compare");
                                return 1;
                            }
                        },
                    }
                    break;
                }
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return 1;
                }
            }
            k += 1;
        }
    }
    if files.len() > 2 {
        eprintln!(
            "{prog}: extra operand {}",
            quote(&files[2].to_string_lossy())
        );
        try_help(prog);
        return 1;
    }
    if o.all_dups && o.count {
        eprintln!("{prog}: printing all duplicated lines and repeat counts is meaningless");
        try_help(prog);
        return 1;
    }

    let input_name = files
        .first()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or("-".into());
    let reader: Box<dyn Read> = if input_name == "-" {
        Box::new(io::stdin())
    } else {
        match File::open(&files[0]) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("{prog}: {input_name}: {}", describe(&e));
                return 1;
            }
        }
    };
    let writer: Box<dyn Write> = match files.get(1).map(|f| f.to_string_lossy().into_owned()) {
        Some(n) if n != "-" => match File::create(&files[1]) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("{prog}: {n}: {}", describe(&e));
                return 1;
            }
        },
        _ => Box::new(io::stdout()),
    };
    let mut out = BufWriter::new(writer);
    let mut r = BufReader::new(reader);

    // read the group's lines, then decide what to print
    let mut group: Vec<Vec<u8>> = Vec::new();
    let mut line = Vec::new();
    let mut first_group = true;
    loop {
        line.clear();
        let n = r.read_until(o.eol, &mut line).unwrap_or(0);
        if n > 0 && line.last() == Some(&o.eol) {
            line.pop();
        }
        let done = n == 0;
        if !done && group.first().map(|g| same(g, &line, &o)).unwrap_or(true) {
            group.push(line.clone());
            continue;
        }
        if !group.is_empty() {
            flush(&mut out, &group, &o, &mut first_group);
        }
        if done {
            break;
        }
        group.clear();
        group.push(line.clone());
    }
    let _ = out.flush();
    0
}

fn flush<W: Write>(out: &mut W, group: &[Vec<u8>], o: &Opts, first_group: &mut bool) {
    let n = group.len();
    let dup = n > 1;
    if o.all_dups {
        if dup && !o.only_unique {
            for l in group {
                let _ = out.write_all(l);
                let _ = out.write_all(&[o.eol]);
            }
        }
        *first_group = false;
        return;
    }
    let show = if o.only_dups && o.only_unique {
        false
    } else if o.only_dups {
        dup
    } else if o.only_unique {
        !dup
    } else {
        true
    };
    if show {
        if o.count {
            let _ = write!(out, "{n:>7} ");
        }
        let _ = out.write_all(&group[0]);
        let _ = out.write_all(&[o.eol]);
    }
}
