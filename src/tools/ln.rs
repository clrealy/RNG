//! ln — make links.  -s -f -n -r -v -t DIR -T -L -P
//! ln TARGET [LINK], ln TARGET... DIR, ln -t DIR TARGET...

use crate::util::{describe, try_help};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::{Component, Path, PathBuf};

struct Opts {
    symbolic: bool,
    force: bool,
    no_deref: bool,
    relative: bool,
    verbose: bool,
    logical: bool,
}

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut o = Opts {
        symbolic: false,
        force: false,
        no_deref: false,
        relative: false,
        verbose: false,
        logical: false,
    };
    let mut target_dir: Option<PathBuf> = None;
    let mut no_target_dir = false;
    let mut ops: Vec<OsString> = Vec::new();
    let mut only_ops = false;
    let mut i = 0;

    while i < args.len() {
        let s = args[i].to_string_lossy().into_owned();
        i += 1;
        if only_ops || !s.starts_with('-') || s == "-" {
            ops.push(args[i - 1].clone());
            continue;
        }
        if s == "--" {
            only_ops = true;
            continue;
        }
        if let Some(long) = s.strip_prefix("--") {
            match long {
                "symbolic" => o.symbolic = true,
                "force" => o.force = true,
                "no-dereference" => o.no_deref = true,
                "relative" => o.relative = true,
                "verbose" => o.verbose = true,
                "logical" => o.logical = true,
                "physical" => o.logical = false,
                "no-target-directory" => no_target_dir = true,
                "help" => {
                    println!("Usage: {prog} [OPTION]... [-T] TARGET LINK_NAME\n  or:  {prog} [OPTION]... TARGET\n  or:  {prog} [OPTION]... TARGET... DIRECTORY\n  or:  {prog} [OPTION]... -t DIRECTORY TARGET...\nCreate hard links by default, symbolic links with --symbolic.\n\n  -f, --force                 remove existing destination files\n  -n, --no-dereference        treat LINK_NAME as a normal file if it is a symlink to a directory\n  -r, --relative              with -s, create links relative to link location\n  -s, --symbolic              make symbolic links instead of hard links\n  -t, --target-directory=DIR  specify the DIRECTORY in which to create the links\n  -T, --no-target-directory   treat LINK_NAME as a normal file always\n  -v, --verbose               print name of each linked file");
                    return 0;
                }
                "version" => {
                    println!("{prog} (rng) {}", crate::VERSION);
                    return 0;
                }
                _ if long.starts_with("target-directory=") => {
                    target_dir = Some(PathBuf::from(&long[17..]))
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
        for (k, &c) in chars.iter().enumerate() {
            match c {
                's' => o.symbolic = true,
                'f' => o.force = true,
                'n' => o.no_deref = true,
                'r' => o.relative = true,
                'v' => o.verbose = true,
                'L' => o.logical = true,
                'P' => o.logical = false,
                'T' => no_target_dir = true,
                'i' | 'b' => {}
                't' => {
                    let rest: String = chars[k + 1..].iter().collect();
                    let v = if !rest.is_empty() {
                        rest.into()
                    } else if let Some(v) = args.get(i) {
                        i += 1;
                        v.clone()
                    } else {
                        eprintln!("{prog}: option requires an argument -- 't'");
                        try_help(prog);
                        return 1;
                    };
                    target_dir = Some(PathBuf::from(v));
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

    if o.relative && !o.symbolic {
        eprintln!("{prog}: cannot do --relative without --symbolic");
        return 1;
    }
    if ops.is_empty() {
        eprintln!("{prog}: missing file operand");
        try_help(prog);
        return 1;
    }

    // Work out (target, link) pairs.
    let pairs: Vec<(PathBuf, PathBuf)> = if let Some(dir) = &target_dir {
        ops.iter()
            .map(|t| (PathBuf::from(t), dir.join(base(t))))
            .collect()
    } else if ops.len() == 1 {
        vec![(PathBuf::from(&ops[0]), PathBuf::from(base(&ops[0])))]
    } else {
        let last = PathBuf::from(&ops[ops.len() - 1]);
        let is_dir = if o.no_deref {
            fs::symlink_metadata(&last)
                .map(|m| m.is_dir())
                .unwrap_or(false)
        } else {
            last.is_dir()
        };
        if !no_target_dir && is_dir {
            ops[..ops.len() - 1]
                .iter()
                .map(|t| (PathBuf::from(t), last.join(base(t))))
                .collect()
        } else if ops.len() > 2 {
            if no_target_dir {
                eprintln!("{prog}: extra operand '{}'", ops[2].to_string_lossy());
                try_help(prog);
            } else {
                eprintln!("{prog}: target '{}' is not a directory", last.display());
            }
            return 1;
        } else {
            vec![(PathBuf::from(&ops[0]), last)]
        }
    };

    let mut code = 0;
    for (target, link) in &pairs {
        if !make(prog, target, link, &o) {
            code = 1;
        }
    }
    code
}

fn base(p: &OsStr) -> &OsStr {
    OsStr::from_bytes(crate::tools::basename::base(p.as_bytes()))
}

/// Lexically clean an absolute path.
fn clean(p: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(n) => out.push(n),
            _ => {}
        }
    }
    out
}

/// Resolve as much of a path as exists (like `realpath -m`).
fn resolve(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let abs = clean(&abs);
    if let Ok(r) = fs::canonicalize(&abs) {
        return r;
    }
    match (abs.parent(), abs.file_name()) {
        (Some(parent), Some(name)) => resolve(parent).join(name),
        _ => abs,
    }
}

/// Path to `target` as seen from the directory holding `link`.
fn relative(target: &Path, link: &Path) -> PathBuf {
    let t = resolve(target);
    let link_dir = resolve(
        link.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    );
    let tc: Vec<_> = t.components().collect();
    let lc: Vec<_> = link_dir.components().collect();
    let common = tc.iter().zip(&lc).take_while(|(a, b)| a == b).count();
    let mut r = PathBuf::new();
    for _ in common..lc.len() {
        r.push("..");
    }
    for c in &tc[common..] {
        r.push(c.as_os_str());
    }
    if r.as_os_str().is_empty() {
        r.push(".");
    }
    r
}

fn make(prog: &str, target: &Path, link: &Path, o: &Opts) -> bool {
    let tshow = target.display();
    let lshow = link.display();
    if !o.symbolic {
        match fs::symlink_metadata(target) {
            Ok(m) if m.is_dir() => {
                eprintln!("{prog}: {tshow}: hard link not allowed for directory");
                return false;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("{prog}: failed to access '{tshow}': {}", describe(&e));
                return false;
            }
        }
    }
    let link_target = if o.relative {
        relative(target, link)
    } else {
        target.to_path_buf()
    };

    let attempt = || -> io::Result<()> {
        if o.symbolic {
            symlink(&link_target, link)
        } else if o.logical {
            fs::hard_link(fs::canonicalize(target)?, link)
        } else {
            fs::hard_link(target, link)
        }
    };
    let mut res = attempt();
    if let Err(e) = &res {
        if e.kind() == io::ErrorKind::AlreadyExists && o.force {
            // same file? then refuse like GNU
            if let (Ok(a), Ok(b)) = (fs::metadata(target), fs::symlink_metadata(link)) {
                use std::os::unix::fs::MetadataExt;
                if !o.symbolic && a.dev() == b.dev() && a.ino() == b.ino() {
                    eprintln!("{prog}: '{tshow}' and '{lshow}' are the same file");
                    return false;
                }
            }
            let is_real_dir = fs::symlink_metadata(link)
                .map(|m| m.is_dir())
                .unwrap_or(false);
            if is_real_dir {
                eprintln!("{prog}: cannot overwrite directory '{lshow}'");
                return false;
            }
            let _ = fs::remove_file(link);
            res = attempt();
        }
    }
    match res {
        Ok(()) => {
            if o.verbose {
                let arrow = if o.symbolic { "->" } else { "=>" };
                println!("'{lshow}' {arrow} '{}'", link_target.display());
            }
            true
        }
        Err(e) => {
            if o.symbolic {
                eprintln!(
                    "{prog}: failed to create symbolic link '{lshow}': {}",
                    describe(&e)
                );
            } else {
                eprintln!(
                    "{prog}: failed to create hard link '{lshow}': {}",
                    describe(&e)
                );
            }
            false
        }
    }
}
