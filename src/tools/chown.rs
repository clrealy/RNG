//! chown and chgrp.  -R -v -c -f -h --reference=RFILE
//! chown OWNER[:GROUP] FILE..., OWNER: (login group), :GROUP, numeric ids.

use crate::util::{describe, quote, try_help};
use std::ffi::{CStr, CString, OsString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
enum Verbose {
    Quiet,
    Changes,
    All,
}

struct Want {
    uid: Option<u32>,
    gid: Option<u32>,
    /// the names as the user wrote them, for messages
    user_str: Option<String>,
    group_str: Option<String>,
}

struct Opts {
    recursive: bool,
    verbose: Verbose,
    silent: bool,
    no_deref: bool,
    chgrp: bool,
}

fn uid_of(name: &str) -> Option<u32> {
    let c = CString::new(name).ok()?;
    // SAFETY: valid C string; result points to static storage or is null
    unsafe {
        let pw = libc::getpwnam(c.as_ptr());
        if !pw.is_null() {
            return Some((*pw).pw_uid);
        }
    }
    name.parse().ok()
}

fn gid_of(name: &str) -> Option<u32> {
    let c = CString::new(name).ok()?;
    // SAFETY: as above
    unsafe {
        let gr = libc::getgrnam(c.as_ptr());
        if !gr.is_null() {
            return Some((*gr).gr_gid);
        }
    }
    name.parse().ok()
}

fn user_name(uid: u32) -> String {
    // SAFETY: static storage or null
    unsafe {
        let pw = libc::getpwuid(uid);
        if !pw.is_null() {
            return CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned();
        }
    }
    uid.to_string()
}

fn group_name(gid: u32) -> String {
    // SAFETY: static storage or null
    unsafe {
        let gr = libc::getgrgid(gid);
        if !gr.is_null() {
            return CStr::from_ptr((*gr).gr_name).to_string_lossy().into_owned();
        }
    }
    gid.to_string()
}

fn login_group(name: &str) -> Option<u32> {
    let c = CString::new(name).ok()?;
    // SAFETY: as above
    unsafe {
        let pw = libc::getpwnam(c.as_ptr());
        if !pw.is_null() {
            return Some((*pw).pw_gid);
        }
    }
    None
}

fn parse_spec(prog: &str, spec: &str) -> Result<Want, ()> {
    let (u, g, colon) = match spec.split_once(':').or_else(|| {
        // old "user.group" form, only if the whole thing isn't a user name
        if uid_of(spec).is_none() {
            spec.split_once('.')
        } else {
            None
        }
    }) {
        Some((u, g)) => (u, g, true),
        None => (spec, "", false),
    };
    let mut want = Want {
        uid: None,
        gid: None,
        user_str: None,
        group_str: None,
    };
    if !u.is_empty() {
        match uid_of(u) {
            Some(id) => {
                want.uid = Some(id);
                want.user_str = Some(u.to_string());
            }
            None => {
                eprintln!("{prog}: invalid user: {}", quote(spec));
                return Err(());
            }
        }
    }
    if !g.is_empty() {
        match gid_of(g) {
            Some(id) => {
                want.gid = Some(id);
                want.group_str = Some(g.to_string());
            }
            None => {
                eprintln!("{prog}: invalid group: {}", quote(spec));
                return Err(());
            }
        }
    } else if colon && !u.is_empty() {
        // "user:" means user's login group
        match login_group(u) {
            Some(gid) => {
                want.gid = Some(gid);
                want.group_str = Some(group_name(gid));
            }
            None => {
                eprintln!("{prog}: invalid spec: {}", quote(spec));
                return Err(());
            }
        }
    }
    Ok(want)
}

fn run(prog: &str, args: Vec<OsString>, is_chgrp: bool) -> u8 {
    let mut o = Opts {
        recursive: false,
        verbose: Verbose::Quiet,
        silent: false,
        no_deref: false,
        chgrp: is_chgrp,
    };
    let mut reference: Option<OsString> = None;
    let mut operands: Vec<OsString> = Vec::new();
    let mut only_ops = false;

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
                "recursive" => o.recursive = true,
                "verbose" => o.verbose = Verbose::All,
                "changes" => o.verbose = Verbose::Changes,
                "silent" | "quiet" => o.silent = true,
                "no-dereference" => o.no_deref = true,
                "dereference" => o.no_deref = false,
                "preserve-root" | "no-preserve-root" => {}
                "help" => {
                    if is_chgrp {
                        println!("Usage: {prog} [OPTION]... GROUP FILE...\n  or:  {prog} [OPTION]... --reference=RFILE FILE...\nChange the group of each FILE to GROUP.\n\n  -c, --changes          like verbose but report only when a change is made\n  -f, --silent, --quiet  suppress most error messages\n  -v, --verbose          output a diagnostic for every file processed\n  -h, --no-dereference   affect symbolic links instead of any referenced file\n  -R, --recursive        operate on files and directories recursively");
                    } else {
                        println!("Usage: {prog} [OPTION]... [OWNER][:[GROUP]] FILE...\n  or:  {prog} [OPTION]... --reference=RFILE FILE...\nChange the owner and/or group of each FILE to OWNER and/or GROUP.\n\n  -c, --changes          like verbose but report only when a change is made\n  -f, --silent, --quiet  suppress most error messages\n  -v, --verbose          output a diagnostic for every file processed\n  -h, --no-dereference   affect symbolic links instead of any referenced file\n  -R, --recursive        operate on files and directories recursively");
                    }
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
        for c in s[1..].chars() {
            match c {
                'R' => o.recursive = true,
                'v' => o.verbose = Verbose::All,
                'c' => o.verbose = Verbose::Changes,
                'f' => o.silent = true,
                'h' => o.no_deref = true,
                'H' | 'L' | 'P' => {}
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return 1;
                }
            }
        }
    }

    let want = if let Some(r) = reference {
        match fs::metadata(&r) {
            Ok(m) => Want {
                uid: if is_chgrp { None } else { Some(m.uid()) },
                gid: Some(m.gid()),
                user_str: if is_chgrp {
                    None
                } else {
                    Some(user_name(m.uid()))
                },
                group_str: Some(group_name(m.gid())),
            },
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
        let spec = operands.remove(0).to_string_lossy().into_owned();
        if operands.is_empty() {
            eprintln!("{prog}: missing operand after {}", quote(&spec));
            try_help(prog);
            return 1;
        }
        if is_chgrp {
            match gid_of(&spec) {
                Some(g) => Want {
                    uid: None,
                    gid: Some(g),
                    user_str: None,
                    group_str: Some(spec),
                },
                None => {
                    eprintln!("{prog}: invalid group: {}", quote(&spec));
                    return 1;
                }
            }
        } else {
            match parse_spec(prog, &spec) {
                Ok(w) => w,
                Err(()) => return 1,
            }
        }
    };
    if operands.is_empty() {
        eprintln!("{prog}: missing operand");
        try_help(prog);
        return 1;
    }

    let mut ok = true;
    for f in &operands {
        if !change(prog, Path::new(f), &want, &o, true) {
            ok = false;
        }
    }
    u8::from(!ok)
}

fn change(prog: &str, path: &Path, want: &Want, o: &Opts, top: bool) -> bool {
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
    let is_link = lmeta.file_type().is_symlink();
    // like GNU -R (-P): links met while recursing are changed themselves, not followed
    let no_deref = o.no_deref || (is_link && !top);
    let meta = if no_deref {
        Ok(lmeta.clone())
    } else {
        fs::metadata(path)
    };
    let meta = match meta {
        Ok(m) => m,
        Err(e) => {
            if !o.silent {
                eprintln!("{prog}: cannot dereference '{show}': {}", describe(&e));
            }
            return false;
        }
    };
    let (old_u, old_g) = (meta.uid(), meta.gid());
    let new_u = want.uid.unwrap_or(old_u);
    let new_g = want.gid.unwrap_or(old_g);
    let requested = want.uid.is_some() || want.gid.is_some();
    let needs = new_u != old_u || new_g != old_g;

    let mut ok = true;
    if needs {
        let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        let u = want.uid.map(|x| x as libc::uid_t).unwrap_or(u32::MAX);
        let g = want.gid.map(|x| x as libc::gid_t).unwrap_or(u32::MAX);
        // SAFETY: valid C string
        let rc = unsafe {
            if no_deref {
                libc::lchown(c.as_ptr(), u, g)
            } else {
                libc::chown(c.as_ptr(), u, g)
            }
        };
        if rc != 0 {
            let e = std::io::Error::last_os_error();
            if !o.silent {
                let what = if want.uid.is_some() {
                    "ownership"
                } else {
                    "group"
                };
                eprintln!("{prog}: changing {what} of '{show}': {}", describe(&e));
            }
            ok = false;
        }
    }

    if o.verbose != Verbose::Quiet && requested {
        // GNU's wording: chgrp talks about "group", chown always about "ownership"
        let (what, old_spec, new_spec, kept_spec) = if o.chgrp {
            let g = want.group_str.clone().unwrap_or_default();
            ("group", group_name(old_g), g.clone(), g)
        } else {
            let full_old = format!("{}:{}", user_name(old_u), group_name(old_g));
            match (&want.user_str, &want.group_str) {
                (Some(u), Some(g)) => (
                    "ownership",
                    full_old,
                    format!("{u}:{g}"),
                    format!("{u}:{g}"),
                ),
                (Some(u), None) => ("ownership", user_name(old_u), u.clone(), u.clone()),
                (None, Some(g)) => ("ownership", full_old.clone(), format!(":{g}"), full_old),
                (None, None) => ("ownership", full_old.clone(), full_old.clone(), full_old),
            }
        };
        if needs && ok {
            println!("changed {what} of '{show}' from {old_spec} to {new_spec}");
        } else if !needs && o.verbose == Verbose::All {
            println!("{what} of '{show}' retained as {kept_spec}");
        } else if !ok && o.verbose == Verbose::All {
            println!("failed to change {what} of '{show}' from {old_spec} to {new_spec}");
        }
    }

    if o.recursive && lmeta.is_dir() {
        match fs::read_dir(path) {
            Ok(rd) => {
                for e in rd.flatten() {
                    if !change(prog, &path.join(e.file_name()), want, o, false) {
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

pub fn main_chown(prog: &str, args: Vec<OsString>) -> u8 {
    run(prog, args, false)
}

pub fn main_chgrp(prog: &str, args: Vec<OsString>) -> u8 {
    run(prog, args, true)
}
