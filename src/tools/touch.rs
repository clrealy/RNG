//! touch — change file timestamps (and create files that don't exist).
//!
//! -a, -m, -c, -h, -r FILE, -t [[CC]YY]MMDDhhmm[.ss],
//! -d with: now, @SECONDS, YYYY-MM-DD, YYYY-MM-DD HH:MM[:SS], or ...THH:MM[:SS]

use crate::util::{describe, quote, try_help};
use std::ffi::{CString, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt;

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut only_atime = false;
    let mut only_mtime = false;
    let mut no_create = false;
    let mut no_deref = false;
    let mut when: Option<(i64, i64)> = None; // (seconds, nanos)
    let mut reference: Option<OsString> = None;
    let mut files: Vec<OsString> = Vec::new();
    let mut only_files = false;
    let mut i = 0;

    macro_rules! value {
        ($flag:expr, $inline:expr) => {{
            if let Some(v) = $inline {
                v
            } else if let Some(v) = args.get(i) {
                i += 1;
                v.to_string_lossy().into_owned()
            } else {
                eprintln!("{prog}: option requires an argument -- '{}'", $flag);
                try_help(prog);
                return 1;
            }
        }};
    }

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
            let (key, val) = match long.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (long, None),
            };
            match key {
                "no-create" => no_create = true,
                "no-dereference" => no_deref = true,
                "date" => {
                    let v = value!("d", val);
                    match parse_date(&v) {
                        Some(t) => when = Some(t),
                        None => {
                            eprintln!("{prog}: invalid date format {}", quote(&v));
                            return 1;
                        }
                    }
                }
                "reference" => reference = Some(value!("r", val).into()),
                "time" => match value!("time", val).as_str() {
                    "atime" | "access" | "use" => only_atime = true,
                    "mtime" | "modify" => only_mtime = true,
                    other => {
                        eprintln!("{prog}: invalid argument {} for '--time'", quote(other));
                        return 1;
                    }
                },
                "help" => {
                    println!("Usage: {prog} [OPTION]... FILE...\nUpdate the access and modification times of each FILE to the current time.\n\n  -a                     change only the access time\n  -c, --no-create        do not create any files\n  -d, --date=STRING      parse STRING and use it instead of current time\n  -h, --no-dereference   affect each symbolic link instead of any referenced file\n  -m                     change only the modification time\n  -r, --reference=FILE   use this file's times instead of current time\n  -t STAMP               use [[CC]YY]MMDDhhmm[.ss] instead of current time");
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
            let rest: String = chars[k + 1..].iter().collect();
            let inline = if rest.is_empty() { None } else { Some(rest) };
            match c {
                'a' => only_atime = true,
                'm' => only_mtime = true,
                'c' => no_create = true,
                'h' => no_deref = true,
                'f' => {}
                'd' => {
                    let v = value!(c, inline);
                    match parse_date(&v) {
                        Some(t) => when = Some(t),
                        None => {
                            eprintln!("{prog}: invalid date format {}", quote(&v));
                            return 1;
                        }
                    }
                    break;
                }
                't' => {
                    let v = value!(c, inline);
                    match parse_stamp(&v) {
                        Some(t) => when = Some(t),
                        None => {
                            eprintln!("{prog}: invalid date format {}", quote(&v));
                            return 1;
                        }
                    }
                    break;
                }
                'r' => {
                    reference = Some(value!(c, inline).into());
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

    if files.is_empty() {
        eprintln!("{prog}: missing file operand");
        try_help(prog);
        return 1;
    }

    // Work out the two timestamps.
    let now = libc::timespec {
        tv_sec: 0,
        tv_nsec: libc::UTIME_NOW,
    };
    let (mut atime, mut mtime) = (now, now);
    if let Some(r) = &reference {
        match std::fs::metadata(r) {
            Ok(m) => {
                use std::os::unix::fs::MetadataExt;
                atime = libc::timespec {
                    tv_sec: m.atime(),
                    tv_nsec: m.atime_nsec(),
                };
                mtime = libc::timespec {
                    tv_sec: m.mtime(),
                    tv_nsec: m.mtime_nsec(),
                };
            }
            Err(e) => {
                eprintln!(
                    "{prog}: failed to get attributes of '{}': {}",
                    r.to_string_lossy(),
                    describe(&e)
                );
                return 1;
            }
        }
    }
    if let Some((sec, nsec)) = when {
        atime = libc::timespec {
            tv_sec: sec,
            tv_nsec: nsec,
        };
        mtime = atime;
    }
    let omit = libc::timespec {
        tv_sec: 0,
        tv_nsec: libc::UTIME_OMIT,
    };
    if only_atime && !only_mtime {
        mtime = omit;
    }
    if only_mtime && !only_atime {
        atime = omit;
    }

    let mut code = 0;
    for f in &files {
        if f.to_string_lossy() == "-" {
            // GNU: "-" means standard output's file
            let times = [atime, mtime];
            // SAFETY: fd 1 and a valid 2-element array
            if unsafe { libc::futimens(1, times.as_ptr()) } != 0 {
                let e = io::Error::last_os_error();
                eprintln!(
                    "{prog}: setting times of 'standard output': {}",
                    describe(&e)
                );
                code = 1;
            }
            continue;
        }
        let name = f.to_string_lossy();
        let Ok(c) = CString::new(f.as_bytes()) else {
            continue;
        };
        let exists = std::fs::symlink_metadata(f).is_ok();
        if !exists {
            if no_create || no_deref {
                continue;
            }
            if let Err(e) = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(f)
            {
                eprintln!("{prog}: cannot touch '{name}': {}", describe(&e));
                code = 1;
                continue;
            }
        }
        let times = [atime, mtime];
        let flags = if no_deref {
            libc::AT_SYMLINK_NOFOLLOW
        } else {
            0
        };
        // SAFETY: valid C string and a 2-element timespec array
        let rc = unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), flags) };
        if rc != 0 {
            let e = io::Error::last_os_error();
            eprintln!("{prog}: setting times of '{name}': {}", describe(&e));
            code = 1;
        }
    }
    code
}

/// Local time → seconds since the epoch.
fn local_to_epoch(y: i32, mo: i32, d: i32, h: i32, mi: i32, s: i32) -> Option<i64> {
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    // SAFETY: tm is plain data; mktime only reads/normalises it
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_year = y - 1900;
        tm.tm_mon = mo - 1;
        tm.tm_mday = d;
        tm.tm_hour = h;
        tm.tm_min = mi;
        tm.tm_sec = s;
        tm.tm_isdst = -1;
        let t = libc::mktime(&mut tm);
        if t == -1 {
            None
        } else {
            Some(t)
        }
    }
}

fn num(s: &str) -> Option<i32> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// -t [[CC]YY]MMDDhhmm[.ss]
fn parse_stamp(s: &str) -> Option<(i64, i64)> {
    let (main, secs) = match s.split_once('.') {
        Some((m, ss)) if ss.len() == 2 => (m, num(ss)?),
        Some(_) => return None,
        None => (s, 0),
    };
    if !main.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (year, rest) = match main.len() {
        8 => {
            // SAFETY: time(NULL) and localtime_r with valid pointers
            let y = unsafe {
                let t = libc::time(std::ptr::null_mut());
                let mut tm: libc::tm = std::mem::zeroed();
                libc::localtime_r(&t, &mut tm);
                tm.tm_year + 1900
            };
            (y, main)
        }
        10 => {
            let yy = num(&main[..2])?;
            (if yy < 69 { 2000 + yy } else { 1900 + yy }, &main[2..])
        }
        12 => (num(&main[..4])?, &main[4..]),
        _ => return None,
    };
    let t = local_to_epoch(
        year,
        num(&rest[0..2])?,
        num(&rest[2..4])?,
        num(&rest[4..6])?,
        num(&rest[6..8])?,
        secs,
    )?;
    Some((t, 0))
}

/// -d: a practical subset of GNU's date syntax.
fn parse_date(s: &str) -> Option<(i64, i64)> {
    let s = s.trim();
    if s == "now" {
        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        return Some((d.as_secs() as i64, d.subsec_nanos() as i64));
    }
    if let Some(n) = s.strip_prefix('@') {
        let (whole, frac) = n.split_once('.').unwrap_or((n, ""));
        let secs: i64 = whole.parse().ok()?;
        let mut nanos = 0i64;
        if !frac.is_empty() {
            let f = format!("{:0<9}", &frac[..frac.len().min(9)]);
            nanos = f.parse().ok()?;
        }
        return Some((secs, nanos));
    }
    let (date, time) = match s.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t.trim())),
        None => (s, None),
    };
    let mut dp = date.split('-');
    let y = num(dp.next()?)?;
    let mo = num(dp.next()?)?;
    let d = num(dp.next()?)?;
    if dp.next().is_some() {
        return None;
    }
    let (h, mi, sec) = match time {
        None => (0, 0, 0),
        Some(t) => {
            let mut tp = t.split(':');
            let h = num(tp.next()?)?;
            let mi = num(tp.next()?)?;
            let sec = match tp.next() {
                Some(x) => num(x)?,
                None => 0,
            };
            if tp.next().is_some() {
                return None;
            }
            (h, mi, sec)
        }
    };
    Some((local_to_epoch(y, mo, d, h, mi, sec)?, 0))
}
