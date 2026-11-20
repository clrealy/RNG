//! tr — translate, squeeze and delete characters (bytes, like GNU).
//! -c/-C -d -s -t; SETs understand \n \t \\ \NNN, a-z, [:class:], [=c=],
//! and in SET2 [c*N] / [c*].

use crate::util::{quote, try_help};
use std::ffi::OsString;
use std::io::{self, BufWriter, Read, Write};

#[derive(Debug)]
enum Item {
    Byte(u8),
    Range(u8, u8),
    Class(String),
    Repeat(u8, Option<usize>), // [c*n] ; None = fill
}

fn unescape(src: &[u8], i: &mut usize) -> u8 {
    // src[*i] == b'\\'
    *i += 1;
    if *i >= src.len() {
        return b'\\';
    }
    let c = src[*i];
    *i += 1;
    match c {
        b'a' => 7,
        b'b' => 8,
        b'f' => 12,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'v' => 11,
        b'0'..=b'7' => {
            let mut v = (c - b'0') as u32;
            let mut n = 1;
            while n < 3 && *i < src.len() && (b'0'..=b'7').contains(&src[*i]) {
                let nv = v * 8 + (src[*i] - b'0') as u32;
                if nv > 255 {
                    break;
                }
                v = nv;
                *i += 1;
                n += 1;
            }
            v as u8
        }
        other => other,
    }
}

fn parse(prog: &str, s: &[u8], allow_repeat: bool) -> Result<Vec<Item>, ()> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < s.len() {
        // [:class:]
        if s[i] == b'[' && s.get(i + 1) == Some(&b':') {
            if let Some(end) = s[i + 2..].windows(2).position(|w| w == b":]") {
                let name = String::from_utf8_lossy(&s[i + 2..i + 2 + end]).into_owned();
                let ok = [
                    "alnum", "alpha", "blank", "cntrl", "digit", "graph", "lower", "print",
                    "punct", "space", "upper", "xdigit",
                ];
                if !ok.contains(&name.as_str()) {
                    eprintln!("{prog}: invalid character class {}", quote(&name));
                    return Err(());
                }
                items.push(Item::Class(name));
                i += end + 4;
                continue;
            }
        }
        // [=c=]
        if s[i] == b'['
            && s.get(i + 1) == Some(&b'=')
            && s.get(i + 3) == Some(&b'=')
            && s.get(i + 4) == Some(&b']')
        {
            items.push(Item::Byte(s[i + 2]));
            i += 5;
            continue;
        }
        // [c*n]
        if allow_repeat && s[i] == b'[' && i + 2 < s.len() {
            let mut j = i + 1;
            let c = if s[j] == b'\\' {
                unescape(s, &mut j)
            } else {
                j += 1;
                s[j - 1]
            };
            if j < s.len() && s[j] == b'*' {
                if let Some(close) = s[j..].iter().position(|&x| x == b']') {
                    let num = std::str::from_utf8(&s[j + 1..j + close]).unwrap_or("x");
                    let count = if num.is_empty() {
                        None
                    } else {
                        let parsed = if num.starts_with('0') && num.len() > 1 {
                            usize::from_str_radix(num, 8).ok()
                        } else {
                            num.parse().ok()
                        };
                        match parsed {
                            Some(0) => None,
                            Some(n) => Some(n),
                            None => {
                                eprintln!(
                                    "{prog}: invalid repeat count {} in [c*n] construct",
                                    quote(num)
                                );
                                return Err(());
                            }
                        }
                    };
                    items.push(Item::Repeat(c, count));
                    i = j + close + 1;
                    continue;
                }
            }
        }
        let c = if s[i] == b'\\' {
            unescape(s, &mut i)
        } else {
            i += 1;
            s[i - 1]
        };
        if i + 1 < s.len() && s[i] == b'-' {
            let mut j = i + 1;
            let hi = if s[j] == b'\\' {
                unescape(s, &mut j)
            } else {
                j += 1;
                s[j - 1]
            };
            if hi < c {
                let show = |b: u8| -> String {
                    if b.is_ascii_graphic() {
                        (b as char).to_string()
                    } else {
                        format!("\\{b:03o}")
                    }
                };
                eprintln!(
                    "{prog}: range-endpoints of '{}-{}' are in reverse collating sequence order",
                    show(c),
                    show(hi)
                );
                return Err(());
            }
            items.push(Item::Range(c, hi));
            i = j;
            continue;
        }
        items.push(Item::Byte(c));
    }
    Ok(items)
}

fn class_bytes(name: &str) -> Vec<u8> {
    (0u8..=255)
        .filter(|&b| match name {
            "alnum" => b.is_ascii_alphanumeric(),
            "alpha" => b.is_ascii_alphabetic(),
            "blank" => b == b' ' || b == b'\t',
            "cntrl" => b.is_ascii_control(),
            "digit" => b.is_ascii_digit(),
            "graph" => b.is_ascii_graphic(),
            "lower" => b.is_ascii_lowercase(),
            "print" => b.is_ascii_graphic() || b == b' ',
            "punct" => b.is_ascii_punctuation(),
            "space" => matches!(b, b' ' | b'\t' | b'\n' | 11 | 12 | b'\r'),
            "upper" => b.is_ascii_uppercase(),
            "xdigit" => b.is_ascii_hexdigit(),
            _ => false,
        })
        .collect()
}

/// Expand items to a byte list. `fill_to` is SET1's length (for [c*]).
fn expand(items: &[Item], fill_to: Option<usize>) -> Vec<u8> {
    let fixed: usize = items
        .iter()
        .map(|it| match it {
            Item::Byte(_) => 1,
            Item::Range(a, b) => (*b - *a) as usize + 1,
            Item::Class(n) => class_bytes(n).len(),
            Item::Repeat(_, Some(n)) => *n,
            Item::Repeat(_, None) => 0,
        })
        .sum();
    let mut out = Vec::new();
    for it in items {
        match it {
            Item::Byte(b) => out.push(*b),
            Item::Range(a, b) => out.extend(*a..=*b),
            Item::Class(n) => out.extend(class_bytes(n)),
            Item::Repeat(c, Some(n)) => out.extend(std::iter::repeat_n(*c, *n)),
            Item::Repeat(c, None) => {
                let n = fill_to.map(|t| t.saturating_sub(fixed)).unwrap_or(0);
                out.extend(std::iter::repeat_n(*c, n));
            }
        }
    }
    out
}

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut complement = false;
    let mut delete = false;
    let mut squeeze = false;
    let mut truncate = false;
    let mut sets: Vec<Vec<u8>> = Vec::new();
    let mut only_sets = false;

    for a in &args {
        let s = a.to_string_lossy();
        if only_sets || !s.starts_with('-') || s == "-" {
            sets.push(crate::util::bytes(a));
            continue;
        }
        if s == "--" {
            only_sets = true;
            continue;
        }
        if let Some(long) = s.strip_prefix("--") {
            match long {
                "complement" => complement = true,
                "delete" => delete = true,
                "squeeze-repeats" => squeeze = true,
                "truncate-set1" => truncate = true,
                "help" => {
                    println!("Usage: {prog} [OPTION]... STRING1 [STRING2]\nTranslate, squeeze, and/or delete characters from standard input,\nwriting to standard output.\n\n  -c, -C, --complement    use the complement of ARRAY1\n  -d, --delete            delete characters in ARRAY1, do not translate\n  -s, --squeeze-repeats   replace each sequence of a repeated character\n                          that is listed in the last specified ARRAY\n  -t, --truncate-set1     first truncate ARRAY1 to length of ARRAY2");
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
                'c' | 'C' => complement = true,
                'd' => delete = true,
                's' => squeeze = true,
                't' => truncate = true,
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return 1;
                }
            }
        }
    }

    let needed_max = if delete && squeeze {
        2
    } else if delete || (squeeze && sets.len() < 2) {
        1
    } else {
        2
    };
    if sets.is_empty() {
        eprintln!("{prog}: missing operand");
        try_help(prog);
        return 1;
    }
    if sets.len() == 1 && !delete && !squeeze {
        eprintln!(
            "{prog}: missing operand after {}",
            quote(&String::from_utf8_lossy(&sets[0]))
        );
        eprintln!("Two strings must be given when translating.");
        try_help(prog);
        return 1;
    }
    if delete && squeeze && sets.len() == 1 {
        eprintln!(
            "{prog}: missing operand after {}",
            quote(&String::from_utf8_lossy(&sets[0]))
        );
        eprintln!("Two strings must be given when both deleting and squeezing repeats.");
        try_help(prog);
        return 1;
    }
    if sets.len() > needed_max {
        eprintln!(
            "{prog}: extra operand {}",
            quote(&String::from_utf8_lossy(&sets[needed_max]))
        );
        if delete && !squeeze && sets.len() == 2 {
            eprintln!("Only one string may be given when deleting without squeezing repeats.");
        }
        try_help(prog);
        return 1;
    }

    let Ok(items1) = parse(prog, &sets[0], false) else {
        return 1;
    };
    let mut set1 = expand(&items1, None);
    if complement {
        let mut seen = [false; 256];
        for &b in &set1 {
            seen[b as usize] = true;
        }
        set1 = (0u8..=255).filter(|b| !seen[*b as usize]).collect();
    }
    let set2: Option<Vec<u8>> = if sets.len() > 1 {
        let Ok(items2) = parse(prog, &sets[1], true) else {
            return 1;
        };
        Some(expand(&items2, Some(set1.len())))
    } else {
        None
    };

    // Build the translation table.
    let mut map: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut in_set1 = [false; 256];
    for &b in &set1 {
        in_set1[b as usize] = true;
    }
    let translating = !delete && set2.is_some();
    if translating {
        let mut s2 = set2.clone().unwrap_or_default();
        if s2.is_empty() && !set1.is_empty() {
            eprintln!("{prog}: when not truncating set1, string2 must be non-empty");
            return 1;
        }
        if truncate {
            set1.truncate(s2.len());
        } else if s2.len() < set1.len() {
            let last = *s2.last().unwrap_or(&0);
            s2.resize(set1.len(), last);
        }
        for (i, &b) in set1.iter().enumerate() {
            map[b as usize] = s2[i];
        }
    }
    // the set squeezing looks at
    let mut squeeze_set = [false; 256];
    if squeeze {
        let src: &[u8] = if delete || translating {
            set2.as_deref().unwrap_or(&set1)
        } else {
            &set1
        };
        for &b in src {
            squeeze_set[b as usize] = true;
        }
    }

    let mut input = Vec::new();
    let mut stdin = io::stdin().lock();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut buf = vec![0u8; 64 * 1024];
    let mut last: Option<u8> = None;
    loop {
        let n = match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                eprintln!("{prog}: read error: {}", crate::util::describe(&e));
                return 1;
            }
        };
        input.clear();
        for &b in &buf[..n] {
            if delete && in_set1[b as usize] {
                continue;
            }
            let c = if translating { map[b as usize] } else { b };
            if squeeze && squeeze_set[c as usize] && last == Some(c) {
                continue;
            }
            input.push(c);
            last = Some(c);
        }
        if out.write_all(&input).is_err() {
            return 1;
        }
    }
    let _ = out.flush();
    0
}
