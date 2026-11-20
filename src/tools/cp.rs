//! cp and mv. They share the copy engine (mv falls back to copy + delete
//! when a rename crosses filesystems).
//!
//! cp: -a -d -f -i -l -L -n -p -P -r/-R -s -t -T -u -v, --preserve[=LIST]
//! mv: -f -i -n -t -T -u -v

use crate::util::{describe, try_help};
use std::collections::HashMap;
use std::ffi::{CString, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{symlink, DirEntryExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
enum Clobber {
    Yes,
    Ask,
    Never,
}

#[derive(Clone)]
struct Opts {
    recursive: bool,
    deref: Option<bool>, // None = cp's default (follow unless -R)
    clobber: Clobber,
    force: bool,
    update: bool,
    verbose: bool,
    hard_link: bool,
    sym_link: bool,
    keep_mode: bool,
    keep_owner: bool,
    keep_times: bool,
    keep_links: bool,
    target_dir: Option<PathBuf>,
    no_target_dir: bool,
}

impl Opts {
    fn new() -> Self {
        Opts {
            recursive: false,
            deref: None,
            clobber: Clobber::Yes,
            force: false,
            update: false,
            verbose: false,
            hard_link: false,
            sym_link: false,
            keep_mode: false,
            keep_owner: false,
            keep_times: false,
            keep_links: false,
            target_dir: None,
            no_target_dir: false,
        }
    }
}

fn q(p: &Path) -> String {
    format!("'{}'", p.display())
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

/// Parse args shared by cp/mv. Returns (operands, opts) or an exit code.
fn parse(prog: &str, is_mv: bool, args: Vec<OsString>) -> Result<(Vec<OsString>, Opts), u8> {
    let mut o = Opts::new();
    let mut ops = Vec::new();
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
            let (key, val) = match long.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (long, None),
            };
            match (key, is_mv) {
                ("force", _) => {
                    o.clobber = Clobber::Yes;
                    o.force = true;
                }
                ("interactive", _) => o.clobber = Clobber::Ask,
                ("no-clobber", _) => o.clobber = Clobber::Never,
                ("update", _) => o.update = true,
                ("verbose", _) => o.verbose = true,
                ("no-target-directory", _) => o.no_target_dir = true,
                ("target-directory", _) => {
                    let v = match val {
                        Some(v) => v,
                        None => match args.get(i) {
                            Some(v) => {
                                i += 1;
                                v.to_string_lossy().into_owned()
                            }
                            None => {
                                eprintln!(
                                    "{prog}: option '--target-directory' requires an argument"
                                );
                                try_help(prog);
                                return Err(1);
                            }
                        },
                    };
                    o.target_dir = Some(PathBuf::from(v));
                }
                ("recursive", false) => o.recursive = true,
                ("archive", false) => archive(&mut o),
                ("link", false) => o.hard_link = true,
                ("symbolic-link", false) => o.sym_link = true,
                ("dereference", false) => o.deref = Some(true),
                ("no-dereference", false) => {
                    o.deref = Some(false);
                    o.keep_links = true;
                }
                ("preserve", false) => {
                    let list = val.unwrap_or_else(|| "mode,ownership,timestamps".into());
                    for item in list.split(',') {
                        match item {
                            "mode" => o.keep_mode = true,
                            "ownership" => o.keep_owner = true,
                            "timestamps" => o.keep_times = true,
                            "links" => o.keep_links = true,
                            "all" => {
                                o.keep_mode = true;
                                o.keep_owner = true;
                                o.keep_times = true;
                                o.keep_links = true;
                            }
                            "context" | "xattr" => {}
                            other => {
                                eprintln!("{prog}: invalid argument '{other}' for '--preserve'");
                                return Err(1);
                            }
                        }
                    }
                }
                ("help", _) => {
                    help(prog, is_mv);
                    return Err(0);
                }
                ("version", _) => {
                    println!("{prog} (rng) {}", crate::VERSION);
                    return Err(0);
                }
                _ => {
                    eprintln!("{prog}: unrecognized option '{s}'");
                    try_help(prog);
                    return Err(1);
                }
            }
            continue;
        }
        let chars: Vec<char> = s[1..].chars().collect();
        for (k, &c) in chars.iter().enumerate() {
            match (c, is_mv) {
                ('f', _) => {
                    o.clobber = Clobber::Yes;
                    o.force = true;
                }
                ('i', _) => o.clobber = Clobber::Ask,
                ('n', _) => o.clobber = Clobber::Never,
                ('u', _) => o.update = true,
                ('v', _) => o.verbose = true,
                ('T', _) => o.no_target_dir = true,
                ('t', _) => {
                    let rest: String = chars[k + 1..].iter().collect();
                    let v = if !rest.is_empty() {
                        rest
                    } else if let Some(v) = args.get(i) {
                        i += 1;
                        v.to_string_lossy().into_owned()
                    } else {
                        eprintln!("{prog}: option requires an argument -- 't'");
                        try_help(prog);
                        return Err(1);
                    };
                    o.target_dir = Some(PathBuf::from(v));
                    break;
                }
                ('r' | 'R', false) => o.recursive = true,
                ('a', false) => archive(&mut o),
                ('p', false) => {
                    o.keep_mode = true;
                    o.keep_owner = true;
                    o.keep_times = true;
                }
                ('d', false) => {
                    o.deref = Some(false);
                    o.keep_links = true;
                }
                ('P', false) => o.deref = Some(false),
                ('L', false) => o.deref = Some(true),
                ('l', false) => o.hard_link = true,
                ('s', false) => o.sym_link = true,
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return Err(1);
                }
            }
        }
    }
    Ok((ops, o))
}

fn archive(o: &mut Opts) {
    o.recursive = true;
    o.deref = Some(false);
    o.keep_mode = true;
    o.keep_owner = true;
    o.keep_times = true;
    o.keep_links = true;
}

fn help(prog: &str, is_mv: bool) {
    if is_mv {
        println!("Usage: {prog} [OPTION]... [-T] SOURCE DEST\n  or:  {prog} [OPTION]... SOURCE... DIRECTORY\n  or:  {prog} [OPTION]... -t DIRECTORY SOURCE...\nRename SOURCE to DEST, or move SOURCE(s) to DIRECTORY.\n\n  -f, --force                  do not prompt before overwriting\n  -i, --interactive            prompt before overwrite\n  -n, --no-clobber             do not overwrite an existing file\n  -t, --target-directory=DIR   move all SOURCE arguments into DIRECTORY\n  -T, --no-target-directory    treat DEST as a normal file\n  -u, --update                 move only when the SOURCE file is newer\n  -v, --verbose                explain what is being done");
    } else {
        println!("Usage: {prog} [OPTION]... [-T] SOURCE DEST\n  or:  {prog} [OPTION]... SOURCE... DIRECTORY\n  or:  {prog} [OPTION]... -t DIRECTORY SOURCE...\nCopy SOURCE to DEST, or multiple SOURCE(s) to DIRECTORY.\n\n  -a, --archive                same as -dR --preserve=all\n  -d                           same as --no-dereference --preserve=links\n  -f, --force                  if a destination can't be opened, remove it and try again\n  -i, --interactive            prompt before overwrite\n  -l, --link                   hard link files instead of copying\n  -L, --dereference            always follow symbolic links in SOURCE\n  -n, --no-clobber             do not overwrite an existing file\n  -P, --no-dereference         never follow symbolic links in SOURCE\n  -p                           same as --preserve=mode,ownership,timestamps\n      --preserve[=ATTR_LIST]   preserve mode, ownership, timestamps, links, all\n  -R, -r, --recursive          copy directories recursively\n  -s, --symbolic-link          make symbolic links instead of copying\n  -t, --target-directory=DIR   copy all SOURCE arguments into DIRECTORY\n  -T, --no-target-directory    treat DEST as a normal file\n  -u, --update                 copy only when the SOURCE file is newer\n  -v, --verbose                explain what is being done");
    }
}

/// Turn the operand list into (source, destination) pairs, GNU style.
fn plan(prog: &str, ops: &[OsString], o: &Opts) -> Result<Vec<(PathBuf, PathBuf)>, u8> {
    if ops.is_empty() {
        eprintln!("{prog}: missing file operand");
        try_help(prog);
        return Err(1);
    }
    if let Some(dir) = &o.target_dir {
        if !dir.is_dir() {
            eprintln!(
                "{prog}: target directory {}: {}",
                q(dir),
                if dir.exists() {
                    "Not a directory"
                } else {
                    "No such file or directory"
                }
            );
            return Err(1);
        }
        return Ok(ops
            .iter()
            .map(|s| (PathBuf::from(s), into_dir(dir, Path::new(s))))
            .collect());
    }
    if ops.len() == 1 {
        eprintln!(
            "{prog}: missing destination file operand after {}",
            q(Path::new(&ops[0]))
        );
        try_help(prog);
        return Err(1);
    }
    let dest = PathBuf::from(&ops[ops.len() - 1]);
    let srcs = &ops[..ops.len() - 1];
    if o.no_target_dir {
        if srcs.len() > 1 {
            eprintln!("{prog}: extra operand {}", q(Path::new(&ops[2])));
            try_help(prog);
            return Err(1);
        }
        return Ok(vec![(PathBuf::from(&srcs[0]), dest)]);
    }
    if dest.is_dir() {
        return Ok(srcs
            .iter()
            .map(|s| (PathBuf::from(s), into_dir(&dest, Path::new(s))))
            .collect());
    }
    if srcs.len() > 1 {
        if dest.exists() {
            eprintln!("{prog}: target {} is not a directory", q(&dest));
        } else {
            eprintln!("{prog}: target {}: No such file or directory", q(&dest));
        }
        return Err(1);
    }
    Ok(vec![(PathBuf::from(&srcs[0]), dest)])
}

fn into_dir(dir: &Path, src: &Path) -> PathBuf {
    let base = crate::tools::basename::base(src.as_os_str().as_bytes());
    dir.join(std::ffi::OsStr::from_bytes(base))
}

fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// Is `inner` the same as, or somewhere inside, `outer`?
fn is_inside(outer: &Path, inner: &Path) -> bool {
    let (Ok(o), Some(parent)) = (fs::canonicalize(outer), inner.parent()) else {
        return false;
    };
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let Ok(p) = fs::canonicalize(parent) else {
        return false;
    };
    let full = match inner.file_name() {
        Some(n) => p.join(n),
        None => p,
    };
    full.starts_with(&o)
}

struct Copier<'a> {
    prog: &'a str,
    o: &'a Opts,
    umask: u32,
    /// (dev, ino) of a source file → where we copied it, for --preserve=links
    links: HashMap<(u64, u64), PathBuf>,
}

impl Copier<'_> {
    fn err(&self, msg: String) -> bool {
        eprintln!("{}: {msg}", self.prog);
        false
    }

    /// Copy `src` to `dst`. `top` is true for command-line operands.
    fn copy(&mut self, src: &Path, dst: &Path, top: bool) -> bool {
        let follow = match self.o.deref {
            Some(d) => d,
            None => !self.o.recursive,
        };
        let meta = if follow {
            fs::metadata(src)
        } else {
            fs::symlink_metadata(src)
        };
        let meta = match meta {
            Ok(m) => m,
            Err(e) => return self.err(format!("cannot stat {}: {}", q(src), describe(&e))),
        };
        let dmeta = fs::symlink_metadata(dst).ok();

        if let Some(dm) = &dmeta {
            let dm_follow = fs::metadata(dst).ok();
            if dm_follow
                .as_ref()
                .map(|d| same_file(&meta, d))
                .unwrap_or(false)
                || same_file(&meta, dm)
            {
                return self.err(format!("{} and {} are the same file", q(src), q(dst)));
            }
        }

        if meta.is_dir() {
            if !self.o.recursive {
                return self.err(format!("-r not specified; omitting directory {}", q(src)));
            }
            if top && is_inside(src, dst) {
                return self.err(format!(
                    "cannot copy a directory, {}, into itself, {}",
                    q(src),
                    q(dst)
                ));
            }
            match &dmeta {
                Some(dm) if !dm.is_dir() => {
                    return self.err(format!(
                        "cannot overwrite non-directory {} with directory {}",
                        q(dst),
                        q(src)
                    ));
                }
                Some(_) => {}
                None => {
                    if let Err(e) = fs::create_dir(dst) {
                        return self.err(format!(
                            "cannot create directory {}: {}",
                            q(dst),
                            describe(&e)
                        ));
                    }
                    if self.o.verbose {
                        println!("{} -> {}", q(src), q(dst));
                    }
                }
            }
            let mut ok = true;
            match fs::read_dir(src) {
                Ok(rd) => {
                    // GNU reads directories in inode order (fastest on disk)
                    let mut ents: Vec<_> = rd.flatten().collect();
                    ents.sort_by_key(|e| e.ino());
                    let names: Vec<_> = ents.iter().map(|e| e.file_name()).collect();
                    for n in names {
                        if !self.copy(&src.join(&n), &dst.join(&n), false) {
                            ok = false;
                        }
                    }
                }
                Err(e) => return self.err(format!("cannot access {}: {}", q(src), describe(&e))),
            }
            let mode = if self.o.keep_mode {
                meta.mode() & 0o7777
            } else {
                meta.mode() & 0o777 & !self.umask
            };
            if dmeta.is_none() || self.o.keep_mode {
                let _ = fs::set_permissions(dst, fs::Permissions::from_mode(mode));
            }
            self.finish(&meta, dst);
            return ok;
        }

        // not a directory
        if let Some(dm) = &dmeta {
            if dm.is_dir() {
                return self.err(format!(
                    "cannot overwrite directory {} with non-directory",
                    q(dst)
                ));
            }
            if self.o.update
                && (dm.mtime() > meta.mtime()
                    || (dm.mtime() == meta.mtime() && dm.mtime_nsec() >= meta.mtime_nsec()))
            {
                return true;
            }
            match self.o.clobber {
                Clobber::Never => return true,
                Clobber::Ask => {
                    if !confirm(&format!("{}: overwrite {}? ", self.prog, q(dst))) {
                        return true;
                    }
                }
                Clobber::Yes => {}
            }
        }

        // keep hard links together (-a / -d / --preserve=links)
        if self.o.keep_links && meta.nlink() > 1 && !meta.is_dir() {
            if let Some(first) = self.links.get(&(meta.dev(), meta.ino())).cloned() {
                if dmeta.is_some() {
                    let _ = fs::remove_file(dst);
                }
                return match fs::hard_link(&first, dst) {
                    Ok(()) => {
                        if self.o.verbose {
                            println!("{} -> {}", q(src), q(dst));
                        }
                        true
                    }
                    Err(e) => self.err(format!(
                        "cannot create hard link {} to {}: {}",
                        q(dst),
                        q(&first),
                        describe(&e)
                    )),
                };
            }
            self.links
                .insert((meta.dev(), meta.ino()), dst.to_path_buf());
        }

        if self.o.hard_link || self.o.sym_link {
            if dmeta.is_some() {
                let _ = fs::remove_file(dst);
            }
            let res = if self.o.hard_link {
                fs::hard_link(src, dst)
            } else {
                symlink(src, dst)
            };
            return match res {
                Ok(()) => {
                    if self.o.verbose {
                        println!("{} -> {}", q(src), q(dst));
                    }
                    true
                }
                Err(e) => {
                    let what = if self.o.hard_link {
                        "hard link"
                    } else {
                        "symbolic link"
                    };
                    self.err(format!(
                        "cannot create {what} {} to {}: {}",
                        q(dst),
                        q(src),
                        describe(&e)
                    ))
                }
            };
        }

        let ft = meta.file_type();
        if ft.is_symlink() {
            let target = match fs::read_link(src) {
                Ok(t) => t,
                Err(e) => {
                    return self.err(format!(
                        "cannot read symbolic link {}: {}",
                        q(src),
                        describe(&e)
                    ))
                }
            };
            if dmeta.is_some() {
                let _ = fs::remove_file(dst);
            }
            if let Err(e) = symlink(&target, dst) {
                return self.err(format!(
                    "cannot create symbolic link {}: {}",
                    q(dst),
                    describe(&e)
                ));
            }
            if self.o.keep_owner {
                if let Ok(c) = CString::new(dst.as_os_str().as_bytes()) {
                    // SAFETY: valid C string
                    unsafe { libc::lchown(c.as_ptr(), meta.uid(), meta.gid()) };
                }
            }
            if self.o.verbose {
                println!("{} -> {}", q(src), q(dst));
            }
            return true;
        }
        if !ft.is_file() {
            // fifo / device / socket: recreate the node
            use std::os::unix::fs::FileTypeExt;
            if dmeta.is_some() {
                let _ = fs::remove_file(dst);
            }
            let Ok(c) = CString::new(dst.as_os_str().as_bytes()) else {
                return false;
            };
            let kind = if ft.is_fifo() {
                libc::S_IFIFO
            } else if ft.is_char_device() {
                libc::S_IFCHR
            } else if ft.is_block_device() {
                libc::S_IFBLK
            } else {
                libc::S_IFSOCK
            };
            // SAFETY: valid C string
            let rc = unsafe { libc::mknod(c.as_ptr(), kind | (meta.mode() & 0o7777), meta.rdev()) };
            if rc != 0 {
                let e = io::Error::last_os_error();
                return self.err(format!(
                    "cannot create special file {}: {}",
                    q(dst),
                    describe(&e)
                ));
            }
            if self.o.verbose {
                println!("{} -> {}", q(src), q(dst));
            }
            self.finish(&meta, dst);
            return true;
        }

        // regular file
        let mut input = match File::open(src) {
            Ok(f) => f,
            Err(e) => {
                return self.err(format!(
                    "cannot open {} for reading: {}",
                    q(src),
                    describe(&e)
                ))
            }
        };
        let new_mode = meta.mode() & 0o777 & !self.umask;
        let open_dst = || {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(new_mode)
                .open(dst)
        };
        let mut out = match open_dst() {
            Ok(f) => f,
            Err(e) if self.o.force && dmeta.is_some() => {
                let _ = fs::remove_file(dst);
                match open_dst() {
                    Ok(f) => f,
                    Err(_) => {
                        return self.err(format!(
                            "cannot create regular file {}: {}",
                            q(dst),
                            describe(&e)
                        ))
                    }
                }
            }
            Err(e) => {
                return self.err(format!(
                    "cannot create regular file {}: {}",
                    q(dst),
                    describe(&e)
                ))
            }
        };
        if let Err(e) = io::copy(&mut input, &mut out) {
            return self.err(format!("error writing {}: {}", q(dst), describe(&e)));
        }
        if self.o.verbose {
            println!("{} -> {}", q(src), q(dst));
        }
        if self.o.keep_mode {
            let _ = fs::set_permissions(dst, fs::Permissions::from_mode(meta.mode() & 0o7777));
        }
        self.finish(&meta, dst);
        true
    }

    /// Ownership and timestamps, when asked to keep them.
    fn finish(&self, meta: &fs::Metadata, dst: &Path) {
        let Ok(c) = CString::new(dst.as_os_str().as_bytes()) else {
            return;
        };
        if self.o.keep_owner {
            // SAFETY: valid C string. Failing (not root) is fine, like GNU.
            unsafe { libc::chown(c.as_ptr(), meta.uid(), meta.gid()) };
            if self.o.keep_mode && meta.mode() & 0o6000 != 0 {
                let _ = fs::set_permissions(dst, fs::Permissions::from_mode(meta.mode() & 0o7777));
            }
        }
        if self.o.keep_times {
            let t = [
                libc::timespec {
                    tv_sec: meta.atime(),
                    tv_nsec: meta.atime_nsec(),
                },
                libc::timespec {
                    tv_sec: meta.mtime(),
                    tv_nsec: meta.mtime_nsec(),
                },
            ];
            // SAFETY: valid C string and 2-element array
            unsafe {
                libc::utimensat(
                    libc::AT_FDCWD,
                    c.as_ptr(),
                    t.as_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            };
        }
    }
}

pub fn main_cp(prog: &str, args: Vec<OsString>) -> u8 {
    let (ops, o) = match parse(prog, false, args) {
        Ok(x) => x,
        Err(c) => return c,
    };
    if o.clobber == Clobber::Never {
        eprintln!("{prog}: warning: behavior of -n is non-portable and may change in future; use --update=none instead");
    }
    let pairs = match plan(prog, &ops, &o) {
        Ok(p) => p,
        Err(c) => return c,
    };
    let mut c = Copier {
        prog,
        o: &o,
        umask: crate::mode::umask(),
        links: HashMap::new(),
    };
    let mut code = 0;
    for (src, dst) in pairs {
        if !c.copy(&src, &dst, true) {
            code = 1;
        }
    }
    code
}

pub fn main_mv(prog: &str, args: Vec<OsString>) -> u8 {
    let (ops, mut o) = match parse(prog, true, args) {
        Ok(x) => x,
        Err(c) => return c,
    };
    let pairs = match plan(prog, &ops, &o) {
        Ok(p) => p,
        Err(c) => return c,
    };
    let mut code = 0;
    for (src, dst) in pairs {
        if !move_one(prog, &src, &dst, &mut o) {
            code = 1;
        }
    }
    code
}

fn move_one(prog: &str, src: &Path, dst: &Path, o: &mut Opts) -> bool {
    let smeta = match fs::symlink_metadata(src) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{prog}: cannot stat {}: {}", q(src), describe(&e));
            return false;
        }
    };
    if let Ok(dm) = fs::symlink_metadata(dst) {
        if same_file(&smeta, &dm) {
            eprintln!("{prog}: {} and {} are the same file", q(src), q(dst));
            return false;
        }
        if dm.is_dir() && !smeta.is_dir() {
            eprintln!(
                "{prog}: cannot overwrite directory {} with non-directory",
                q(dst)
            );
            return false;
        }
        if !dm.is_dir() && smeta.is_dir() {
            eprintln!(
                "{prog}: cannot overwrite non-directory {} with directory {}",
                q(dst),
                q(src)
            );
            return false;
        }
        if o.update
            && (dm.mtime() > smeta.mtime()
                || (dm.mtime() == smeta.mtime() && dm.mtime_nsec() >= smeta.mtime_nsec()))
        {
            return true;
        }
        match o.clobber {
            Clobber::Never => {
                eprintln!("{prog}: not replacing {}", q(dst));
                return false;
            }
            Clobber::Ask => {
                if !confirm(&format!("{prog}: overwrite {}? ", q(dst))) {
                    return true;
                }
            }
            Clobber::Yes => {}
        }
    }
    if smeta.is_dir() && is_inside(src, dst) {
        eprintln!(
            "{prog}: cannot move {} to a subdirectory of itself, {}",
            q(src),
            q(dst)
        );
        return false;
    }

    match fs::rename(src, dst) {
        Ok(()) => {
            if o.verbose {
                println!("renamed {} -> {}", q(src), q(dst));
            }
            true
        }
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            // different filesystem: copy everything (keeping it all), then delete
            let mut co = o.clone();
            archive(&mut co);
            co.verbose = false;
            co.clobber = Clobber::Yes;
            let mut c = Copier {
                prog,
                o: &co,
                umask: crate::mode::umask(),
                links: HashMap::new(),
            };
            if let Ok(dm) = fs::symlink_metadata(dst) {
                if dm.is_dir() {
                    let _ = fs::remove_dir(dst);
                }
            }
            if !c.copy(src, dst, true) {
                return false;
            }
            let removed = if smeta.is_dir() {
                fs::remove_dir_all(src)
            } else {
                fs::remove_file(src)
            };
            if let Err(e) = removed {
                eprintln!("{prog}: cannot remove {}: {}", q(src), describe(&e));
                return false;
            }
            if o.verbose {
                println!("copied {} -> {}", q(src), q(dst));
                println!("removed {}", q(src));
            }
            true
        }
        Err(e) => {
            eprintln!(
                "{prog}: cannot move {} to {}: {}",
                q(src),
                q(dst),
                describe(&e)
            );
            false
        }
    }
}
