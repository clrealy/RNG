//! sort — sort lines of text files.
//!
//! -b -d -f -g -h -i -M -n -r -V (global or per key), -k POS1[,POS2], -t SEP,
//! -u -s -c -C -o FILE -z -m. Text comparison uses strcoll like GNU, so it
//! follows the locale.

use crate::util::{describe, quote, try_help};
use std::cmp::Ordering;
use std::ffi::{CString, OsString};
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};

#[derive(Clone, Copy, Default, PartialEq)]
struct Flags {
    blanks: bool,
    dict: bool,
    fold: bool,
    general: bool,
    human: bool,
    ignore_np: bool,
    month: bool,
    numeric: bool,
    reverse: bool,
    version: bool,
}

impl Flags {
    fn any_ordering(&self) -> bool {
        self.blanks
            || self.dict
            || self.fold
            || self.general
            || self.human
            || self.ignore_np
            || self.month
            || self.numeric
            || self.reverse
            || self.version
    }
    fn set(&mut self, c: char) -> bool {
        match c {
            'b' => self.blanks = true,
            'd' => self.dict = true,
            'f' => self.fold = true,
            'g' => self.general = true,
            'h' => self.human = true,
            'i' => self.ignore_np = true,
            'M' => self.month = true,
            'n' => self.numeric = true,
            'r' => self.reverse = true,
            'V' => self.version = true,
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Copy)]
struct Key {
    sword: usize,         // 0-based field
    schar: usize,         // 0-based char within field
    eword: Option<usize>, // 0-based end field; None = end of line
    echar: usize,         // 0 = end of field
    flags: Flags,
    skip_sblanks: bool,
    skip_eblanks: bool,
}

struct Cfg {
    keys: Vec<Key>,
    global: Flags,
    tab: Option<u8>,
    stable: bool,
    unique: bool,
}

fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn begfield(line: &[u8], k: &Key, tab: Option<u8>) -> usize {
    let lim = line.len();
    let mut p = 0;
    let mut sword = k.sword;
    match tab {
        Some(t) => {
            while p < lim && sword > 0 {
                while p < lim && line[p] != t {
                    p += 1;
                }
                if p < lim {
                    p += 1;
                }
                sword -= 1;
            }
        }
        None => {
            while p < lim && sword > 0 {
                while p < lim && is_blank(line[p]) {
                    p += 1;
                }
                while p < lim && !is_blank(line[p]) {
                    p += 1;
                }
                sword -= 1;
            }
        }
    }
    if k.skip_sblanks {
        while p < lim && is_blank(line[p]) {
            p += 1;
        }
    }
    (p + k.schar).min(lim)
}

fn limfield(line: &[u8], k: &Key, tab: Option<u8>) -> usize {
    let lim = line.len();
    let Some(ew) = k.eword else { return lim };
    let mut eword = if k.echar == 0 { ew + 1 } else { ew };
    let mut p = 0;
    match tab {
        Some(t) => {
            while p < lim && eword > 0 {
                eword -= 1;
                while p < lim && line[p] != t {
                    p += 1;
                }
                if p < lim && (eword > 0 || k.echar != 0) {
                    p += 1;
                }
            }
        }
        None => {
            while p < lim && eword > 0 {
                eword -= 1;
                while p < lim && is_blank(line[p]) {
                    p += 1;
                }
                while p < lim && !is_blank(line[p]) {
                    p += 1;
                }
            }
        }
    }
    if k.echar != 0 {
        if k.skip_eblanks {
            while p < lim && is_blank(line[p]) {
                p += 1;
            }
        }
        p = (p + k.echar).min(lim);
    }
    p
}

fn collate(a: &[u8], b: &[u8]) -> Ordering {
    // strcoll stops at NUL; fall back to bytes in that case
    if a.contains(&0) || b.contains(&0) {
        return a.cmp(b);
    }
    match (CString::new(a), CString::new(b)) {
        // SAFETY: two valid C strings
        (Ok(x), Ok(y)) => unsafe { libc::strcoll(x.as_ptr(), y.as_ptr()) }.cmp(&0),
        _ => a.cmp(b),
    }
}

/// (negative, integer digits without leading zeros, fraction digits without trailing zeros)
fn parse_num(s: &[u8]) -> (bool, &[u8], &[u8], usize) {
    let mut i = 0;
    while i < s.len() && is_blank(s[i]) {
        i += 1;
    }
    let neg = i < s.len() && s[i] == b'-';
    if neg {
        i += 1;
    }
    let int_start = i;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let mut int = &s[int_start..i];
    while int.first() == Some(&b'0') {
        int = &int[1..];
    }
    let mut frac: &[u8] = &[];
    if i < s.len() && s[i] == b'.' {
        let fs = i + 1;
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
        frac = &s[fs..i];
        while frac.last() == Some(&b'0') {
            frac = &frac[..frac.len() - 1];
        }
    }
    (neg, int, frac, i)
}

fn cmp_unsigned(ai: &[u8], af: &[u8], bi: &[u8], bf: &[u8]) -> Ordering {
    ai.len()
        .cmp(&bi.len())
        .then_with(|| ai.cmp(bi))
        .then_with(|| af.cmp(bf))
}

fn numcmp(a: &[u8], b: &[u8]) -> Ordering {
    let (an, ai, af, _) = parse_num(a);
    let (bn, bi, bf, _) = parse_num(b);
    let a_zero = ai.is_empty() && af.is_empty();
    let b_zero = bi.is_empty() && bf.is_empty();
    let an = an && !a_zero;
    let bn = bn && !b_zero;
    match (an, bn) {
        (false, true) => Ordering::Greater,
        (true, false) => Ordering::Less,
        (false, false) => cmp_unsigned(ai, af, bi, bf),
        (true, true) => cmp_unsigned(bi, bf, ai, af),
    }
}

fn human_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let suffix = |s: &[u8]| -> (bool, usize) {
        let (neg, int, frac, end) = parse_num(s);
        let zero = int.is_empty() && frac.is_empty();
        let mag = match s.get(end) {
            Some(b'K' | b'k') => 1,
            Some(b'M') => 2,
            Some(b'G') => 3,
            Some(b'T') => 4,
            Some(b'P') => 5,
            Some(b'E') => 6,
            Some(b'Z') => 7,
            Some(b'Y') => 8,
            Some(b'R') => 9,
            Some(b'Q') => 10,
            _ => 0,
        };
        (neg && !zero, if zero { 0 } else { mag })
    };
    let (an, am) = suffix(a);
    let (bn, bm) = suffix(b);
    let sign = |neg: bool| if neg { -1 } else { 1 };
    let ka = sign(an) * am as i64;
    let kb = sign(bn) * bm as i64;
    ka.cmp(&kb).then_with(|| numcmp(a, b))
}

fn general_value(s: &[u8]) -> Option<f64> {
    let t = std::str::from_utf8(s).ok()?.trim_start();
    // longest prefix that parses as a float
    let mut best = None;
    for end in (1..=t.len()).rev() {
        if !t.is_char_boundary(end) {
            continue;
        }
        if let Ok(v) = t[..end].parse::<f64>() {
            let lower = t[..end].to_ascii_lowercase();
            if !(lower.ends_with("inf") || lower.ends_with("infinity") || lower.contains("nan"))
                || lower.trim_start_matches(['-', '+']).len() >= 3
            {
                best = Some(v);
                break;
            }
        }
    }
    best
}

fn general_cmp(a: &[u8], b: &[u8]) -> Ordering {
    match (general_value(a), general_value(b)) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(x), Some(y)) => match (x.is_nan(), y.is_nan()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        },
    }
}

fn month_of(s: &[u8]) -> usize {
    let mut i = 0;
    while i < s.len() && is_blank(s[i]) {
        i += 1;
    }
    const M: [&[u8]; 12] = [
        b"JAN", b"FEB", b"MAR", b"APR", b"MAY", b"JUN", b"JUL", b"AUG", b"SEP", b"OCT", b"NOV",
        b"DEC",
    ];
    if s.len() >= i + 3 {
        let up: Vec<u8> = s[i..i + 3].iter().map(|c| c.to_ascii_uppercase()).collect();
        if let Some(p) = M.iter().position(|m| *m == up.as_slice()) {
            return p + 1;
        }
    }
    0
}

/// Debian-style version comparison (digits compared as numbers, '~' sorts first).
fn version_cmp(a: &[u8], b: &[u8]) -> Ordering {
    fn order(c: Option<&u8>) -> i32 {
        match c {
            None => 0,
            Some(b'~') => -1,
            Some(c) if c.is_ascii_digit() => 0,
            Some(c) if c.is_ascii_alphabetic() => *c as i32,
            Some(c) => *c as i32 + 256,
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        // non-digit part
        while (i < a.len() && !a[i].is_ascii_digit()) || (j < b.len() && !b[j].is_ascii_digit()) {
            let ac = order(a.get(i).filter(|c| !c.is_ascii_digit()));
            let bc = order(b.get(j).filter(|c| !c.is_ascii_digit()));
            if ac != bc {
                return ac.cmp(&bc);
            }
            if i < a.len() && !a[i].is_ascii_digit() {
                i += 1;
            }
            if j < b.len() && !b[j].is_ascii_digit() {
                j += 1;
            }
        }
        // digit part
        while i < a.len() && a[i] == b'0' {
            i += 1;
        }
        while j < b.len() && b[j] == b'0' {
            j += 1;
        }
        let (si, sj) = (i, j);
        while i < a.len() && a[i].is_ascii_digit() {
            i += 1;
        }
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        let (da, db) = (&a[si..i], &b[sj..j]);
        let c = da.len().cmp(&db.len()).then_with(|| da.cmp(db));
        if c != Ordering::Equal {
            return c;
        }
    }
    Ordering::Equal
}

fn transform(s: &[u8], f: &Flags) -> Vec<u8> {
    s.iter()
        .filter(|&&c| {
            (!f.dict || c.is_ascii_alphanumeric() || is_blank(c) || c >= 0x80)
                && (!f.ignore_np || (0x20..0x7f).contains(&c) || c >= 0x80)
        })
        .map(|&c| if f.fold { c.to_ascii_uppercase() } else { c })
        .collect()
}

fn compare_key(a: &[u8], b: &[u8], f: &Flags) -> Ordering {
    let (mut a, mut b) = (a, b);
    if f.blanks && !(f.numeric || f.general || f.human || f.month) {
        while a.first().is_some_and(|c| is_blank(*c)) {
            a = &a[1..];
        }
        while b.first().is_some_and(|c| is_blank(*c)) {
            b = &b[1..];
        }
    }
    let ord = if f.numeric {
        numcmp(a, b)
    } else if f.human {
        human_cmp(a, b)
    } else if f.general {
        general_cmp(a, b)
    } else if f.month {
        month_of(a).cmp(&month_of(b))
    } else if f.version {
        version_cmp(a, b)
    } else if f.fold || f.dict || f.ignore_np {
        collate(&transform(a, f), &transform(b, f))
    } else {
        collate(a, b)
    };
    if f.reverse {
        ord.reverse()
    } else {
        ord
    }
}

fn compare(a: &[u8], b: &[u8], c: &Cfg, last_resort: bool) -> Ordering {
    for k in &c.keys {
        let (sa, ea) = (begfield(a, k, c.tab), limfield(a, k, c.tab));
        let (sb, eb) = (begfield(b, k, c.tab), limfield(b, k, c.tab));
        let ka = &a[sa..ea.max(sa)];
        let kb = &b[sb..eb.max(sb)];
        let o = compare_key(ka, kb, &k.flags);
        if o != Ordering::Equal {
            return o;
        }
    }
    if !last_resort || c.stable || c.unique {
        return Ordering::Equal;
    }
    let o = collate(a, b);
    if c.global.reverse {
        o.reverse()
    } else {
        o
    }
}

fn bad_key(prog: &str, msg: &str, spec: &str) -> u8 {
    eprintln!("{prog}: {msg}: invalid field specification {}", quote(spec));
    2
}

fn parse_key(prog: &str, spec: &str, global: &Flags) -> Result<Key, u8> {
    let (p1, p2) = match spec.split_once(',') {
        Some((a, b)) => (a, Some(b)),
        None => (spec, None),
    };
    // POS = F[.C][OPTS]
    let split = |p: &str| -> (String, String, String) {
        let digits_end = p.find(|c: char| !c.is_ascii_digit()).unwrap_or(p.len());
        let field = p[..digits_end].to_string();
        let rest = &p[digits_end..];
        if let Some(r) = rest.strip_prefix('.') {
            let ce = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
            (field, r[..ce].to_string(), r[ce..].to_string())
        } else {
            (field, String::new(), rest.to_string())
        }
    };
    let mut k = Key {
        sword: 0,
        schar: 0,
        eword: None,
        echar: 0,
        flags: Flags::default(),
        skip_sblanks: false,
        skip_eblanks: false,
    };
    let (f1, c1, o1) = split(p1);
    let Ok(sw) = f1.parse::<usize>() else {
        eprintln!(
            "{prog}: invalid number at field start: invalid count at start of {}",
            quote(p1)
        );
        return Err(2);
    };
    if sw == 0 {
        return Err(bad_key(prog, "field number is zero", spec));
    }
    k.sword = sw - 1;
    if !c1.is_empty() {
        match c1.parse::<usize>() {
            Ok(0) => return Err(bad_key(prog, "character offset is zero", spec)),
            Ok(n) => k.schar = n - 1,
            Err(_) => return Err(bad_key(prog, "invalid number after '.'", spec)),
        }
    }
    for c in o1.chars() {
        if c == 'b' {
            k.skip_sblanks = true;
        } else if !k.flags.set(c) {
            return Err(bad_key(prog, "stray character in field spec", spec));
        }
    }
    if let Some(p2) = p2 {
        let (f2, c2, o2) = split(p2);
        let Ok(ew) = f2.parse::<usize>() else {
            eprintln!(
                "{prog}: invalid number after ',': invalid count at start of {}",
                quote(p2)
            );
            return Err(2);
        };
        if ew == 0 {
            return Err(bad_key(prog, "field number is zero", spec));
        }
        k.eword = Some(ew - 1);
        if !c2.is_empty() {
            k.echar = c2.parse().unwrap_or(0);
        }
        for c in o2.chars() {
            if c == 'b' {
                k.skip_eblanks = true;
            } else if !k.flags.set(c) {
                return Err(bad_key(prog, "stray character in field spec", spec));
            }
        }
    }
    // a key with no options of its own inherits the global ones
    if !k.flags.any_ordering() && !k.skip_sblanks && !k.skip_eblanks {
        k.flags = *global;
        k.skip_sblanks = global.blanks;
        k.skip_eblanks = global.blanks;
    }
    k.flags.blanks = false; // blanks are handled by the field functions for keys
    Ok(k)
}

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut global = Flags::default();
    let mut key_specs: Vec<String> = Vec::new();
    let mut tab: Option<u8> = None;
    let mut unique = false;
    let mut stable = false;
    let mut check: Option<bool> = None; // Some(quiet)
    let mut output: Option<OsString> = None;
    let mut zero = false;
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
                "ignore-leading-blanks" => global.blanks = true,
                "dictionary-order" => global.dict = true,
                "ignore-case" => global.fold = true,
                "general-numeric-sort" => global.general = true,
                "human-numeric-sort" => global.human = true,
                "ignore-nonprinting" => global.ignore_np = true,
                "month-sort" => global.month = true,
                "numeric-sort" => global.numeric = true,
                "reverse" => global.reverse = true,
                "version-sort" => global.version = true,
                "unique" => unique = true,
                "stable" => stable = true,
                "zero-terminated" => zero = true,
                "merge" => {}
                "check" => check = Some(matches!(val.as_deref(), Some("quiet" | "silent"))),
                "key" => key_specs.push(value().unwrap_or_default()),
                "field-separator" => {
                    let v = value().unwrap_or_default();
                    if v.len() != 1 {
                        eprintln!("{prog}: multi-character tab {}", quote(&v));
                        return 2;
                    }
                    tab = Some(v.as_bytes()[0]);
                }
                "output" => output = value().map(OsString::from),
                "buffer-size"
                | "temporary-directory"
                | "parallel"
                | "compress-program"
                | "random-source" => {
                    let _ = value();
                }
                "help" => {
                    println!("Usage: {prog} [OPTION]... [FILE]...\nWrite sorted concatenation of all FILE(s) to standard output.\n\n  -b, --ignore-leading-blanks  ignore leading blanks\n  -d, --dictionary-order      consider only blanks and alphanumeric characters\n  -f, --ignore-case           fold lower case to upper case characters\n  -g, --general-numeric-sort  compare according to general numerical value\n  -i, --ignore-nonprinting    consider only printable characters\n  -M, --month-sort            compare (unknown) < 'JAN' < ... < 'DEC'\n  -h, --human-numeric-sort    compare human readable numbers (e.g., 2K 1G)\n  -n, --numeric-sort          compare according to string numerical value\n  -r, --reverse               reverse the result of comparisons\n  -V, --version-sort          natural sort of (version) numbers within text\n  -c, --check                 check for sorted input; do not sort\n  -C                          like -c, but do not report first bad line\n  -k, --key=KEYDEF            sort via a key; KEYDEF gives location and type\n  -m, --merge                 merge already sorted files; do not sort\n  -o, --output=FILE           write result to FILE instead of standard output\n  -s, --stable                stabilize sort by disabling last-resort comparison\n  -t, --field-separator=SEP   use SEP instead of non-blank to blank transition\n  -u, --unique                output only the first of an equal run\n  -z, --zero-terminated       line delimiter is NUL, not newline");
                    return 0;
                }
                "version" => {
                    println!("{prog} (rng) {}", crate::VERSION);
                    return 0;
                }
                _ => {
                    eprintln!("{prog}: unrecognized option '{s}'");
                    try_help(prog);
                    return 2;
                }
            }
            continue;
        }
        let chars: Vec<char> = s[1..].chars().collect();
        let mut k = 0;
        while k < chars.len() {
            let c = chars[k];
            if global.set(c) {
                k += 1;
                continue;
            }
            match c {
                'u' => unique = true,
                's' => stable = true,
                'z' => zero = true,
                'm' => {}
                'c' => check = Some(false),
                'C' => check = Some(true),
                'k' | 't' | 'o' | 'S' | 'T' => {
                    let rest: String = chars[k + 1..].iter().collect();
                    let v = if !rest.is_empty() {
                        rest
                    } else if let Some(v) = args.get(i) {
                        i += 1;
                        v.to_string_lossy().into_owned()
                    } else {
                        eprintln!("{prog}: option requires an argument -- '{c}'");
                        try_help(prog);
                        return 2;
                    };
                    match c {
                        'k' => key_specs.push(v),
                        't' => {
                            let b = v.as_bytes();
                            if b.len() != 1 && v != "\\0" {
                                eprintln!("{prog}: multi-character tab {}", quote(&v));
                                return 2;
                            }
                            tab = Some(if v == "\\0" { 0 } else { b[0] });
                        }
                        'o' => output = Some(v.into()),
                        _ => {}
                    }
                    break;
                }
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return 2;
                }
            }
            k += 1;
        }
    }

    let mut keys = Vec::new();
    for spec in &key_specs {
        match parse_key(prog, spec, &global) {
            Ok(k) => keys.push(k),
            Err(c) => return c,
        }
    }
    if keys.is_empty() {
        let mut f = global;
        f.blanks = false;
        keys.push(Key {
            sword: 0,
            schar: 0,
            eword: None,
            echar: 0,
            flags: f,
            skip_sblanks: global.blanks,
            skip_eblanks: global.blanks,
        });
    }
    let cfg = Cfg {
        keys,
        global,
        tab,
        stable,
        unique,
    };
    let eol = if zero { b'\0' } else { b'\n' };
    if files.is_empty() {
        files.push("-".into());
    }

    // read everything
    let mut lines: Vec<Vec<u8>> = Vec::new();
    let mut origin: Vec<String> = Vec::new();
    for f in &files {
        let name = f.to_string_lossy().into_owned();
        let mut data = Vec::new();
        let res = if name == "-" {
            io::stdin().read_to_end(&mut data)
        } else {
            File::open(f).and_then(|mut fh| {
                if fh.metadata()?.is_dir() {
                    return Err(io::Error::other("Is a directory"));
                }
                fh.read_to_end(&mut data)
            })
        };
        if let Err(e) = res {
            let what = if e.to_string() == "Is a directory" {
                "read failed"
            } else {
                "cannot read"
            };
            eprintln!("{prog}: {what}: {name}: {}", describe(&e));
            return 2;
        }
        if data.is_empty() {
            continue;
        }
        if data.last() == Some(&eol) {
            data.pop();
        }
        for l in data.split(|&c| c == eol) {
            lines.push(l.to_vec());
            origin.push(name.clone());
        }
    }

    if let Some(quiet) = check {
        for idx in 1..lines.len() {
            let o = compare(&lines[idx - 1], &lines[idx], &cfg, true);
            let bad = if unique {
                o != Ordering::Less
            } else {
                o == Ordering::Greater
            };
            if bad {
                if !quiet {
                    let shown = if origin[idx] == "-" {
                        "-".to_string()
                    } else {
                        origin[idx].clone()
                    };
                    eprintln!(
                        "{prog}: {shown}:{}: disorder: {}",
                        idx + 1,
                        String::from_utf8_lossy(&lines[idx])
                    );
                }
                return 1;
            }
        }
        return 0;
    }

    lines.sort_by(|a, b| compare(a, b, &cfg, true));
    if unique {
        lines.dedup_by(|b, a| compare(a, b, &cfg, false) == Ordering::Equal);
    }

    let writer: Box<dyn Write> = match &output {
        Some(o) if o.to_string_lossy() != "-" => match File::create(o) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!(
                    "{prog}: open failed: {}: {}",
                    o.to_string_lossy(),
                    describe(&e)
                );
                return 2;
            }
        },
        _ => Box::new(io::stdout()),
    };
    let mut out = BufWriter::new(writer);
    for l in &lines {
        if out
            .write_all(l)
            .and_then(|_| out.write_all(&[eol]))
            .is_err()
        {
            return 2;
        }
    }
    let _ = out.flush();
    0
}

#[cfg(test)]
mod tests {
    use super::{numcmp, version_cmp};
    use std::cmp::Ordering::*;

    #[test]
    fn numbers() {
        assert_eq!(numcmp(b"10", b"9"), Greater);
        assert_eq!(numcmp(b"-1", b"0"), Less);
        assert_eq!(numcmp(b"-0", b"0"), Equal);
        assert_eq!(numcmp(b"1.5", b"1.25"), Greater);
        assert_eq!(numcmp(b"abc", b"0"), Equal);
        assert_eq!(numcmp(b"-10", b"-9"), Less);
    }

    #[test]
    fn versions() {
        assert_eq!(version_cmp(b"file2", b"file10"), Less);
        assert_eq!(version_cmp(b"1.0~rc1", b"1.0"), Less);
        assert_eq!(version_cmp(b"v1.10", b"v1.9"), Greater);
    }
}
