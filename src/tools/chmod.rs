//! chmod — change file mode bits.  -R -v -c -f --reference=RFILE
//! Modes are octal (755) or symbolic (u+x,go-w, a=rX), see src/mode.rs.

use crate::util::{describe, quote, try_help};
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
enum Verbose {
    Quiet,
    Changes,
    All,
}

struct Opts {
    recursive: bool,
    verbose: Verbose,
    silent: bool,
    mode: ModeSrc,
    umask: u32,
}

enum ModeSrc {
    Spec(String),
    Reference(u32),
}

/// rwxr-xr-x style string (9 chars, with s/S/t/T).
pub fn perm_string(mode: u32) -> String {
    let b = |bit: u32, c: char| if mode & bit != 0 { c } else { '-' };
    let sp = |x: u32, s: u32, on: char, off: char| match (mode & x != 0, mode & s != 0) {
        (true, true) => on,
        (false, true) => off,
        (true, false) => 'x',
        _ => '-',
    };
    [
        b(0o400, 'r'),
        b(0o200, 'w'),
        sp(0o100, 0o4000, 's', 'S'),
        b(0o040, 'r'),
        b(0o020, 'w'),
        sp(0o010, 0o2000, 's', 'S'),
        b(0o004, 'r'),
        b(0o002, 'w'),
        sp(0o001, 0o1000, 't', 'T'),
    ]
    .iter()
    .collect()
}

/// Looks like a mode rather than an option? (so `chmod -w file` works)
fn looks_like_mode(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() > 1
        && b[0] == b'-'
        && s[1..].bytes().all(|c| {
            matches!(
                c,
                b'r' | b'w'
                    | b'x'
                    | b'X'
                    | b's'
                    | b't'
                    | b'u'
                    | b'g'
                    | b'o'
                    | b'a'
                    | b','
                    | b'+'
                    | b'-'
                    | b'='
                    | b'0'..=b'7'
            )
        })
}

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut recursive = false;
    let mut verbose = Verbose::Quiet;
    let mut silent = false;
    let mut reference: Option<OsString> = None;
    let mut operands: Vec<OsString> = Vec::new();
    let mut only_ops = false;
    let mut mode_from_dash: Option<String> = None;

    for arg in &args {
        let s = arg.to_string_lossy().into_owned();
        if only_ops || !s.starts_with('-') || s == "-" {
            operands.push(arg.clone());
            continue;
        }
        if s == "--" {
            only_ops = true;
            continue;
        }
        if let Some(long) = s.strip_prefix("--") {
            match long {
                "recursive" => recursive = true,
                "verbose" => verbose = Verbose::All,
                "changes" => verbose = Verbose::Changes,
                "silent" | "quiet" => silent = true,
                "preserve-root" | "no-preserve-root" => {}
                "help" => {
                    println!("Usage: {prog} [OPTION]... MODE[,MODE]... FILE...\n  or:  {prog} [OPTION]... OCTAL-MODE FILE...\n  or:  {prog} [OPTION]... --reference=RFILE FILE...\nChange the mode of each FILE to MODE.\n\n  -c, --changes          like verbose but report only when a change is made\n  -f, --silent, --quiet  suppress most error messages\n  -v, --verbose          output a diagnostic for every file processed\n      --reference=RFILE  use RFILE's mode instead of MODE values\n  -R, --recursive        change files and directories recursively");
                    return 0;
                }
                "version" => {
                    println!("{prog} (rng) {}", crate::VERSION);
                    return 0;
                }
                _ if long.starts_with("reference=") => reference = Some(long[10..].into()),
                _ => {
                    eprintln!("{prog}: unrecognized option '{s}'");
                    try_help(prog);
                    return 1;
                }
            }
            continue;
        }
        if mode_from_dash.is_none()
            && looks_like_mode(&s)
            && !s[1..].chars().all(|c| matches!(c, 'R' | 'v' | 'c' | 'f'))
        {
            // `chmod -w file`: the "option" is really the mode
            mode_from_dash = Some(s.clone());
            operands.insert(0, arg.clone());
            continue;
        }
        for c in s[1..].chars() {
            match c {
                'R' => recursive = true,
                'v' => verbose = Verbose::All,
                'c' => verbose = Verbose::Changes,
                'f' => silent = true,
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return 1;
                }
            }
        }
    }

    let mode = if let Some(r) = reference {
        match fs::metadata(&r) {
            Ok(m) => ModeSrc::Reference(m.mode() & 0o7777),
            Err(e) => {
                eprintln!(
                    "{prog}: failed to get attributes of '{}': {}",
                    r.to_string_lossy(),
                    describe(&e)
                );
                return 1;
            }
        }
    } else {
        if operands.is_empty() {
            eprintln!("{prog}: missing operand");
            try_help(prog);
            return 1;
        }
        let m = operands.remove(0).to_string_lossy().into_owned();
        if operands.is_empty() {
            eprintln!("{prog}: missing operand after {}", quote(&m));
            try_help(prog);
            return 1;
        }
        if crate::mode::apply(&m, 0, false, 0).is_none() {
            eprintln!("{prog}: invalid mode: {}", quote(&m));
            try_help(prog);
            return 1;
        }
        ModeSrc::Spec(m)
    };
    if operands.is_empty() {
        eprintln!("{prog}: missing operand");
        try_help(prog);
        return 1;
    }

    let o = Opts {
        recursive,
        verbose,
        silent,
        mode,
        umask: crate::mode::umask(),
    };
    let mut ok = true;
    for f in &operands {
        if !change(prog, Path::new(f), &o, true) {
            ok = false;
        }
    }
    u8::from(!ok)
}

fn change(prog: &str, path: &Path, o: &Opts, top: bool) -> bool {
    let show = path.display();
    let lmeta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            if !o.silent {
                eprintln!("{prog}: cannot access '{show}': {}", describe(&e));
            }
            return false;
        }
    };
    // symlinks found while recursing are left alone (they have no mode of their own)
    if lmeta.file_type().is_symlink() && !top {
        if o.verbose == Verbose::All {
            println!("neither symbolic link '{show}' nor referent has been changed");
        }
        return true;
    }
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            if !o.silent {
                eprintln!("{prog}: cannot operate on dangling symlink '{show}'");
                let _ = e;
            }
            return false;
        }
    };
    let old = meta.mode() & 0o7777;
    let is_dir = meta.is_dir();
    let (new, wanted) = match &o.mode {
        ModeSrc::Reference(m) => (*m, *m),
        ModeSrc::Spec(s) => (
            crate::mode::apply(s, old, is_dir, o.umask).unwrap_or(old),
            crate::mode::apply(s, old, is_dir, 0).unwrap_or(old),
        ),
    };
    let mut ok = true;
    if new != old {
        if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(new)) {
            if !o.silent {
                eprintln!("{prog}: changing permissions of '{show}': {}", describe(&e));
            }
            ok = false;
        }
    }
    if ok {
        match o.verbose {
            Verbose::All | Verbose::Changes if new != old => println!(
                "mode of '{show}' changed from {old:04o} ({}) to {new:04o} ({})",
                perm_string(old),
                perm_string(new)
            ),
            Verbose::All => println!(
                "mode of '{show}' retained as {old:04o} ({})",
                perm_string(old)
            ),
            _ => {}
        }
    }
    // umask stopped something the user asked for (only possible without "who")
    if ok && new != wanted {
        if !o.silent {
            eprintln!(
                "{prog}: {show}: new permissions are {}, not {}",
                perm_string(new),
                perm_string(wanted)
            );
        }
        ok = false;
    }

    if o.recursive && is_dir && !lmeta.file_type().is_symlink() {
        match fs::read_dir(path) {
            Ok(rd) => {
                for e in rd.flatten() {
                    if !change(prog, &path.join(e.file_name()), o, false) {
                        ok = false;
                    }
                }
            }
            Err(e) => {
                if !o.silent {
                    eprintln!("{prog}: cannot read directory '{show}': {}", describe(&e));
                }
                ok = false;
            }
        }
    }
    ok
}
