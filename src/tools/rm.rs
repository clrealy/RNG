//! rm — remove files or directories.
//!
//! -f, -i, -I, -r/-R, -d, -v, --interactive[=WHEN]
//!
//! Easter egg / safety net: any attempt to remove `/` — `rm -rf /`,
//! `--no-preserve-root`, `//`, `/.`, a symlink to `/`, or a shell-expanded
//! `/*` — prints "Bye" and deletes nothing at all. There's no flag to turn
//! this off.

use crate::util::{describe, try_help};
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
enum Ask {
    Never,
    Once,
    Always,
}

struct Opts {
    force: bool,
    ask: Ask,
    recursive: bool,
    dir: bool,
    verbose: bool,
}

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let mut o = Opts {
        force: false,
        ask: Ask::Never,
        recursive: false,
        dir: false,
        verbose: false,
    };
    let mut files: Vec<OsString> = Vec::new();
    let mut only_files = false;

    for arg in &args {
        let s = arg.to_string_lossy().into_owned();
        if only_files || !s.starts_with('-') || s == "-" {
            files.push(arg.clone());
            continue;
        }
        if s == "--" {
            only_files = true;
            continue;
        }
        if let Some(long) = s.strip_prefix("--") {
            match long {
                "force" => {
                    o.force = true;
                    o.ask = Ask::Never;
                }
                "recursive" => o.recursive = true,
                "dir" => o.dir = true,
                "verbose" => o.verbose = true,
                "interactive" | "interactive=always" | "interactive=yes" => {
                    o.ask = Ask::Always;
                    o.force = false;
                }
                "interactive=once" => {
                    o.ask = Ask::Once;
                    o.force = false;
                }
                "interactive=never" | "interactive=no" | "interactive=none" => o.ask = Ask::Never,
                // there is no way to turn the safety net off: these are accepted and ignored
                "preserve-root" | "preserve-root=all" | "no-preserve-root" | "one-file-system" => {}
                "help" => {
                    println!("Usage: {prog} [OPTION]... [FILE]...\nRemove (unlink) the FILE(s).\n\n  -f, --force           ignore nonexistent files and arguments, never prompt\n  -i                    prompt before every removal\n  -I                    prompt once before removing more than three files, or\n                          when removing recursively\n  -r, -R, --recursive   remove directories and their contents recursively\n  -d, --dir             remove empty directories\n  -v, --verbose         explain what is being done\n\nrng's rm will never remove /. Try it and see.");
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
                'f' => {
                    o.force = true;
                    o.ask = Ask::Never;
                }
                'i' => {
                    o.ask = Ask::Always;
                    o.force = false;
                }
                'I' => {
                    o.ask = Ask::Once;
                    o.force = false;
                }
                'r' | 'R' => o.recursive = true,
                'd' => o.dir = true,
                'v' => o.verbose = true,
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return 1;
                }
            }
        }
    }

    if files.is_empty() {
        if o.force {
            return 0;
        }
        eprintln!("{prog}: missing operand");
        try_help(prog);
        return 1;
    }

    // 🎲 The safety net. Checked before anything is touched.
    if targets_root(&files) {
        println!("Bye");
        return 1;
    }

    if o.ask == Ask::Once && (files.len() > 3 || o.recursive) {
        let what = if files.len() == 1 {
            "argument"
        } else {
            "arguments"
        };
        let how = if o.recursive { " recursively" } else { "" };
        if !confirm(&format!("{prog}: remove {} {what}{how}? ", files.len())) {
            return 0;
        }
    }

    let mut code = 0;
    for f in &files {
        let path = PathBuf::from(f);
        if let Some(last) = path.file_name().or(Some(path.as_os_str())) {
            let l = last.to_string_lossy();
            let ends_dot = matches!(
                path.components().next_back(),
                Some(Component::CurDir | Component::ParentDir)
            ) || l == "."
                || l == "..";
            if ends_dot && o.recursive {
                eprintln!(
                    "{prog}: refusing to remove '.' or '..' directory: skipping '{}'",
                    path.display()
                );
                code = 1;
                continue;
            }
        }
        if !remove(prog, &path, &o) {
            code = 1;
        }
    }
    code
}

/// Lexically clean a path (no filesystem access): make it absolute and
/// fold away `.`, `..` and repeated slashes.
fn normalize(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(p)
    };
    let mut out = PathBuf::from("/");
    for c in abs.components() {
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

/// Does this command line try to wipe `/`?
fn targets_root(files: &[OsString]) -> bool {
    let root = Path::new("/");
    let mut top_level: Vec<OsString> = Vec::new();
    for f in files {
        let s = f.to_string_lossy();
        if s == "/*" || s == "/.*" {
            return true; // the glob wasn't expanded, but the intent is clear
        }
        let p = Path::new(f);
        let norm = normalize(p);
        if norm == root {
            return true;
        }
        // a symlink (or any path) that really points at /
        if let Ok(real) = fs::canonicalize(p) {
            if real == root {
                return true;
            }
        }
        if norm.parent() == Some(root) {
            if let Some(name) = norm.file_name() {
                top_level.push(name.to_os_string());
            }
        }
    }
    // `rm -rf /*` after the shell expanded it: every top-level entry is listed
    if top_level.len() >= 3 {
        if let Ok(entries) = fs::read_dir(root) {
            let all: Vec<OsString> = entries.flatten().map(|e| e.file_name()).collect();
            if !all.is_empty() && all.iter().all(|n| top_level.contains(n)) {
                return true;
            }
        }
    }
    false
}

fn confirm(question: &str) -> bool {
    eprint!("{question}");
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim_start().chars().next(), Some('y' | 'Y'))
}

fn kind(meta: &fs::Metadata) -> &'static str {
    let ft = meta.file_type();
    if ft.is_symlink() {
        "symbolic link"
    } else if ft.is_dir() {
        "directory"
    } else if ft.is_file() {
        if meta.len() == 0 {
            "regular empty file"
        } else {
            "regular file"
        }
    } else {
        use std::os::unix::fs::FileTypeExt;
        if ft.is_fifo() {
            "fifo"
        } else if ft.is_socket() {
            "socket"
        } else if ft.is_block_device() {
            "block special file"
        } else {
            "character special file"
        }
    }
}

fn write_protected(meta: &fs::Metadata, path: &Path) -> bool {
    if meta.file_type().is_symlink() {
        return false;
    }
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: valid C string
    unsafe {
        libc::access(c.as_ptr(), libc::W_OK) != 0 && meta.permissions().mode() & 0o222 != 0o222
    }
}

/// Remove one path. Returns false if something went wrong.
fn remove(prog: &str, path: &Path, o: &Opts) -> bool {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            if o.force && e.kind() == io::ErrorKind::NotFound {
                return true;
            }
            eprintln!(
                "{prog}: cannot remove '{}': {}",
                path.display(),
                describe(&e)
            );
            return false;
        }
    };
    let show = path.display();

    if meta.is_dir() {
        if !o.recursive {
            if o.dir {
                if o.ask == Ask::Always && !confirm(&format!("{prog}: remove directory '{show}'? "))
                {
                    return true;
                }
                return match fs::remove_dir(path) {
                    Ok(()) => {
                        if o.verbose {
                            println!("removed directory '{show}'");
                        }
                        true
                    }
                    Err(e) => {
                        eprintln!("{prog}: cannot remove '{show}': {}", describe(&e));
                        false
                    }
                };
            }
            eprintln!("{prog}: cannot remove '{show}': Is a directory");
            return false;
        }
        let empty = fs::read_dir(path)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if o.ask == Ask::Always
            && !empty
            && !confirm(&format!("{prog}: descend into directory '{show}'? "))
        {
            return true;
        }
        let mut ok = true;
        match fs::read_dir(path) {
            Ok(entries) => {
                // directory order, like GNU rm (fts) does
                let names: Vec<_> = entries.flatten().map(|e| e.file_name()).collect();
                for n in names {
                    if !remove(prog, &path.join(n), o) {
                        ok = false;
                    }
                }
            }
            Err(e) => {
                eprintln!("{prog}: cannot remove '{show}': {}", describe(&e));
                return false;
            }
        }
        if !ok {
            return false;
        }
        if o.ask == Ask::Always && !confirm(&format!("{prog}: remove directory '{show}'? ")) {
            return true;
        }
        return match fs::remove_dir(path) {
            Ok(()) => {
                if o.verbose {
                    println!("removed directory '{show}'");
                }
                true
            }
            Err(e) => {
                eprintln!("{prog}: cannot remove '{show}': {}", describe(&e));
                false
            }
        };
    }

    let protected = !o.force && io::stdin().is_terminal() && write_protected(&meta, path);
    if o.ask == Ask::Always || protected {
        let wp = if protected { "write-protected " } else { "" };
        if !confirm(&format!("{prog}: remove {wp}{} '{show}'? ", kind(&meta))) {
            return true;
        }
    }
    match fs::remove_file(path) {
        Ok(()) => {
            if o.verbose {
                println!("removed '{show}'");
            }
            true
        }
        Err(e) => {
            eprintln!("{prog}: cannot remove '{show}': {}", describe(&e));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize, targets_root};
    use std::ffi::OsString;
    use std::path::Path;

    fn t(args: &[&str]) -> bool {
        let v: Vec<OsString> = args.iter().map(OsString::from).collect();
        targets_root(&v)
    }

    #[test]
    fn bye_cases() {
        assert!(t(&["/"]));
        assert!(t(&["//"]));
        assert!(t(&["/."]));
        assert!(t(&["/../.."]));
        assert!(t(&["/usr/.."]));
        assert!(t(&["/*"]));
        assert!(t(&["safe-file", "/"]));
        assert!(!t(&["/tmp/definitely-not-root"]));
        assert!(!t(&["some/relative/file"]));
    }

    #[test]
    fn expanded_glob_is_bye() {
        let all: Vec<String> = std::fs::read_dir("/")
            .unwrap()
            .flatten()
            .map(|e| format!("/{}", e.file_name().to_string_lossy()))
            .collect();
        let refs: Vec<&str> = all.iter().map(|s| s.as_str()).collect();
        if refs.len() >= 3 {
            assert!(t(&refs));
            // leaving one out is just a (very bad) normal rm
            assert!(!t(&refs[1..]));
        }
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize(Path::new("/a/./b/../c//")), Path::new("/a/c"));
    }
}
