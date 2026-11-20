//! mkdir — make directories.  -p, -m MODE, -v

use crate::util::{describe, try_help};
use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut parents = false;
    let mut verbose = false;
    let mut mode_str: Option<String> = None;
    let mut dirs: Vec<OsString> = Vec::new();
    let mut only_names = false;
    let mut i = 0;

    while i < args.len() {
        let s = args[i].to_string_lossy().into_owned();
        i += 1;
        if only_names || !s.starts_with('-') || s == "-" {
            dirs.push(args[i - 1].clone());
            continue;
        }
        if s == "--" {
            only_names = true;
            continue;
        }
        if let Some(long) = s.strip_prefix("--") {
            match long {
                "parents" => parents = true,
                "verbose" => verbose = true,
                "help" => {
                    println!("Usage: {prog} [OPTION]... DIRECTORY...\nCreate the DIRECTORY(ies), if they do not already exist.\n\n  -m, --mode=MODE   set file mode (as in chmod), not a=rwx - umask\n  -p, --parents     no error if existing, make parent directories as needed\n  -v, --verbose     print a message for each created directory");
                    return 0;
                }
                "version" => {
                    println!("{prog} (rng) {}", crate::VERSION);
                    return 0;
                }
                _ if long.starts_with("mode=") => mode_str = Some(long[5..].to_string()),
                "mode" => match args.get(i) {
                    Some(v) => {
                        mode_str = Some(v.to_string_lossy().into_owned());
                        i += 1;
                    }
                    None => {
                        eprintln!("{prog}: option '--mode' requires an argument");
                        try_help(prog);
                        return 1;
                    }
                },
                _ => {
                    eprintln!("{prog}: unrecognized option '{s}'");
                    try_help(prog);
                    return 1;
                }
            }
            continue;
        }
        let chars: Vec<char> = s[1..].chars().collect();
        for (k, &c) in chars.iter().enumerate() {
            match c {
                'p' => parents = true,
                'v' => verbose = true,
                'm' => {
                    let rest: String = chars[k + 1..].iter().collect();
                    if !rest.is_empty() {
                        mode_str = Some(rest);
                    } else if let Some(v) = args.get(i) {
                        mode_str = Some(v.to_string_lossy().into_owned());
                        i += 1;
                    } else {
                        eprintln!("{prog}: option requires an argument -- 'm'");
                        try_help(prog);
                        return 1;
                    }
                    break;
                }
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return 1;
                }
            }
        }
    }

    if dirs.is_empty() {
        eprintln!("{prog}: missing operand");
        try_help(prog);
        return 1;
    }

    let umask = crate::mode::umask();
    let final_mode = match &mode_str {
        Some(m) => match crate::mode::apply(m, 0o777, true, umask) {
            Some(v) => Some(v),
            None => {
                eprintln!("{prog}: invalid mode {}", crate::util::quote(m));
                return 1;
            }
        },
        None => None,
    };

    let mut code = 0;
    for d in &dirs {
        let path = Path::new(d);
        let res = if parents {
            make_parents(prog, path, final_mode, verbose)
        } else {
            make_one(prog, path, final_mode, verbose)
        };
        if let Err(e) = res {
            eprintln!(
                "{prog}: cannot create directory {}: {}",
                crate::util::quote(&path.display().to_string()),
                describe(&e)
            );
            code = 1;
        }
    }
    code
}

fn make_one(prog: &str, path: &Path, mode: Option<u32>, verbose: bool) -> io::Result<()> {
    DirBuilder::new().mode(0o777).create(path)?;
    if let Some(m) = mode {
        fs::set_permissions(path, fs::Permissions::from_mode(m))?;
    }
    if verbose {
        println!("{prog}: created directory '{}'", path.display());
    }
    Ok(())
}

/// -p: make every missing parent (umask'd, but always u+wx so we can keep
/// going), then the last one with the requested mode.
fn make_parents(prog: &str, path: &Path, mode: Option<u32>, verbose: bool) -> io::Result<()> {
    let mut built = PathBuf::new();
    let parts: Vec<_> = path.components().collect();
    let umask = crate::mode::umask();
    for (n, comp) in parts.iter().enumerate() {
        built.push(comp.as_os_str());
        let last = n + 1 == parts.len();
        if built.is_dir() {
            continue;
        }
        match DirBuilder::new().mode(0o777).create(&built) {
            Ok(()) => {
                if last {
                    if let Some(m) = mode {
                        fs::set_permissions(&built, fs::Permissions::from_mode(m))?;
                    }
                } else if umask & 0o300 != 0 {
                    let m = (0o777 & !umask) | 0o300;
                    fs::set_permissions(&built, fs::Permissions::from_mode(m))?;
                }
                if verbose {
                    println!("{prog}: created directory '{}'", built.display());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && built.is_dir() => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
