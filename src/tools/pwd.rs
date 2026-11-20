//! pwd — print the current directory.
//!
//! -P (default, like GNU coreutils): physical path, symlinks resolved.
//! -L: use $PWD if it really points at the current directory.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Component, Path};

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut logical = false;
    for a in &args {
        let s = a.to_string_lossy();
        match s.as_ref() {
            "-L" | "--logical" => logical = true,
            "-P" | "--physical" => logical = false,
            "--help" => {
                println!("Usage: {prog} [OPTION]...\nPrint the full filename of the current working directory.\n\n  -L, --logical   use PWD from environment, even if it contains symlinks\n  -P, --physical  avoid all symlinks (default)");
                return 0;
            }
            "--version" => {
                println!("{prog} (rng) {}", crate::VERSION);
                return 0;
            }
            s if s.starts_with("--") => {
                eprintln!("{prog}: unrecognized option '{s}'");
                crate::util::try_help(prog);
                return 1;
            }
            s if s.starts_with('-') && s.len() > 1 => {
                for c in s[1..].chars() {
                    match c {
                        'L' => logical = true,
                        'P' => logical = false,
                        _ => {
                            eprintln!("{prog}: invalid option -- '{c}'");
                            crate::util::try_help(prog);
                            return 1;
                        }
                    }
                }
            }
            _ => {
                eprintln!("{prog}: ignoring non-option arguments");
            }
        }
    }

    if logical {
        if let Some(p) = std::env::var_os("PWD") {
            if pwd_is_valid(Path::new(&p)) {
                return print_path(prog, Path::new(&p));
            }
        }
    }
    match std::env::current_dir() {
        Ok(p) => print_path(prog, &p),
        Err(e) => {
            eprintln!(
                "{prog}: couldn't find directory entry in '..' with matching i-node: {}",
                crate::util::describe(&e)
            );
            1
        }
    }
}

/// $PWD is usable if it's absolute, has no `.`/`..` parts, and is the same
/// directory as `.`.
fn pwd_is_valid(p: &Path) -> bool {
    if !p.is_absolute() {
        return false;
    }
    if p.components()
        .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
    {
        return false;
    }
    same_dir(p, Path::new("."))
}

#[cfg(unix)]
fn same_dir(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn print_path(prog: &str, p: &Path) -> u8 {
    let mut out = std::io::stdout().lock();
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        p.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes = p.to_string_lossy().into_owned().into_bytes();
    match out.write_all(&bytes).and_then(|_| out.write_all(b"\n")) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{prog}: write error: {}", crate::util::describe(&e));
            1
        }
    }
}
