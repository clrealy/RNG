//! ls — list directory contents.
//!
//! -a -A -l -h -1 -C -x -m -R -d -t -S -r -U -F -p -i -s -n -g -o -G -u -c -L
//! --color[=WHEN], --group-directories-first, --classify, --full-time
//!
//! Sorting uses the C library's collation (strcoll), so it follows the
//! locale exactly like GNU ls does.

use crate::util::{describe, try_help};
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::{self, Metadata};
use std::io::{self, BufWriter, IsTerminal, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
enum Format {
    Long,
    One,
    Columns,
    Across,
    Commas,
}

#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Name,
    Time,
    Size,
    None,
    Ext,
}

#[derive(Clone, Copy, PartialEq)]
enum Hidden {
    None,
    AlmostAll,
    All,
}

#[derive(Clone, Copy, PartialEq)]
enum Indicator {
    None,
    Slash,
    FileType,
    Classify,
}

#[derive(Clone, Copy, PartialEq)]
enum TimeKind {
    Modify,
    Access,
    Change,
}

struct Opts {
    format: Format,
    sort: Sort,
    reverse: bool,
    hidden: Hidden,
    recursive: bool,
    dirs_as_files: bool,
    indicator: Indicator,
    inode: bool,
    blocks: bool,
    human: bool,
    numeric: bool,
    show_owner: bool,
    show_group: bool,
    time: TimeKind,
    deref: bool,
    color: bool,
    dirs_first: bool,
    width: usize,
    tabsize: usize,
    hide_control: bool,
    quote: bool,
    time_style: TimeStyle,
}

#[derive(Clone, PartialEq)]
enum TimeStyle {
    Locale,
    FullIso,
    LongIso,
    Iso,
    Custom(String, String), // (old/default, recent)
}

struct Entry {
    name: OsString,
    path: PathBuf,
    meta: Option<Metadata>,
    /// for symlinks: what the link points at (stat, following the link)
    target_meta: Option<Metadata>,
    link_target: Option<PathBuf>,
}

impl Entry {
    fn is_dir(&self) -> bool {
        self.meta.as_ref().map(|m| m.is_dir()).unwrap_or(false)
    }
}

pub fn main(prog: &str, args: Vec<OsString>) -> u8 {
    let tty = io::stdout().is_terminal();
    let mut o = Opts {
        format: if tty { Format::Columns } else { Format::One },
        sort: Sort::Name,
        reverse: false,
        hidden: Hidden::None,
        recursive: false,
        dirs_as_files: false,
        indicator: Indicator::None,
        inode: false,
        blocks: false,
        human: false,
        numeric: false,
        show_owner: true,
        show_group: true,
        time: TimeKind::Modify,
        deref: false,
        color: false,
        dirs_first: false,
        width: 80,
        tabsize: 8,
        hide_control: tty,
        quote: tty,
        time_style: TimeStyle::Locale,
    };
    let mut width_given = false;
    let mut sort_by_time = false;
    let mut files: Vec<OsString> = Vec::new();
    let mut only_files = false;

    let mut ai = 0;
    while ai < args.len() {
        let arg = &args[ai];
        ai += 1;
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
                "all" => o.hidden = Hidden::All,
                "almost-all" => o.hidden = Hidden::AlmostAll,
                "recursive" => o.recursive = true,
                "directory" => o.dirs_as_files = true,
                "classify" | "classify=always" | "classify=yes" | "classify=force" => {
                    o.indicator = Indicator::Classify
                }
                "classify=auto" | "classify=tty" | "classify=if-tty" => {
                    if tty {
                        o.indicator = Indicator::Classify
                    }
                }
                "classify=never" | "classify=no" | "classify=none" => o.indicator = Indicator::None,
                "file-type" => o.indicator = Indicator::FileType,
                "inode" => o.inode = true,
                "size" => o.blocks = true,
                "human-readable" => o.human = true,
                "numeric-uid-gid" => {
                    o.numeric = true;
                    o.format = Format::Long;
                }
                "reverse" => o.reverse = true,
                "dereference" => o.deref = true,
                "group-directories-first" => o.dirs_first = true,
                "no-group" => o.show_group = false,
                "full-time" => {
                    o.time_style = TimeStyle::FullIso;
                    o.format = Format::Long;
                }
                "color" | "colour" | "color=always" | "colour=always" | "color=yes"
                | "color=force" => o.color = true,
                "color=auto" | "colour=auto" | "color=tty" | "color=if-tty" => o.color = tty,
                "color=never" | "colour=never" | "color=no" | "color=none" => o.color = false,
                "help" => {
                    help(prog);
                    return 0;
                }
                "version" => {
                    println!("{prog} (rng) {}", crate::VERSION);
                    return 0;
                }
                _ if long.starts_with("sort=") => match &long[5..] {
                    "none" => o.sort = Sort::None,
                    "size" => o.sort = Sort::Size,
                    "time" => o.sort = Sort::Time,
                    "extension" => o.sort = Sort::Ext,
                    "name" => o.sort = Sort::Name,
                    other => {
                        eprintln!("{prog}: invalid argument '{other}' for '--sort'");
                        return 2;
                    }
                },
                _ if long.starts_with("format=") => match &long[7..] {
                    "long" | "verbose" => o.format = Format::Long,
                    "single-column" => o.format = Format::One,
                    "vertical" => o.format = Format::Columns,
                    "across" | "horizontal" => o.format = Format::Across,
                    "commas" => o.format = Format::Commas,
                    other => {
                        eprintln!("{prog}: invalid argument '{other}' for '--format'");
                        return 2;
                    }
                },
                _ if long.starts_with("width=") => match long[6..].parse() {
                    Ok(w) => {
                        o.width = w;
                        width_given = true;
                    }
                    Err(_) => {
                        eprintln!(
                            "{prog}: invalid line width: {}",
                            crate::util::quote(&long[6..])
                        );
                        return 2;
                    }
                },
                _ if long.starts_with("tabsize=") => match long[8..].parse() {
                    Ok(t) => o.tabsize = t,
                    Err(_) => {
                        eprintln!(
                            "{prog}: invalid tab size: {}",
                            crate::util::quote(&long[8..])
                        );
                        return 2;
                    }
                },
                "hide-control-chars" => o.hide_control = true,
                "show-control-chars" => o.hide_control = false,
                "literal" => o.quote = false,
                _ if long.starts_with("time-style=") => match parse_time_style(&long[11..]) {
                    Some(ts) => o.time_style = ts,
                    None => {
                        bad_time_style(prog, &long[11..]);
                        return 2;
                    }
                },
                _ => {
                    eprintln!("{prog}: unrecognized option '{s}'");
                    try_help(prog);
                    return 2;
                }
            }
            continue;
        }
        let chars: Vec<char> = s[1..].chars().collect();
        for (k, &c) in chars.iter().enumerate() {
            if c == 'w' || c == 'T' {
                let rest: String = chars[k + 1..].iter().collect();
                let v = if !rest.is_empty() {
                    rest
                } else if let Some(v) = args.get(ai) {
                    ai += 1;
                    v.to_string_lossy().into_owned()
                } else {
                    eprintln!("{prog}: option requires an argument -- '{c}'");
                    try_help(prog);
                    return 2;
                };
                match (c, v.parse::<usize>()) {
                    ('w', Ok(n)) => {
                        o.width = n;
                        width_given = true;
                    }
                    ('T', Ok(n)) => o.tabsize = n,
                    ('w', Err(_)) => {
                        eprintln!("{prog}: invalid line width: {}", crate::util::quote(&v));
                        return 2;
                    }
                    _ => {
                        eprintln!("{prog}: invalid tab size: {}", crate::util::quote(&v));
                        return 2;
                    }
                }
                break;
            }
            match c {
                'q' => o.hide_control = true,
                'N' => o.quote = false,
                'b' => o.hide_control = false,
                'a' => o.hidden = Hidden::All,
                'A' => o.hidden = Hidden::AlmostAll,
                'l' => o.format = Format::Long,
                '1' => o.format = Format::One,
                'C' => o.format = Format::Columns,
                'x' => o.format = Format::Across,
                'm' => o.format = Format::Commas,
                'R' => o.recursive = true,
                'd' => o.dirs_as_files = true,
                't' => {
                    o.sort = Sort::Time;
                    sort_by_time = true;
                }
                'S' => o.sort = Sort::Size,
                'X' => o.sort = Sort::Ext,
                'U' => o.sort = Sort::None,
                'f' => {
                    o.sort = Sort::None;
                    o.hidden = Hidden::All;
                    o.color = false;
                }
                'r' => o.reverse = true,
                'F' => o.indicator = Indicator::Classify,
                'p' => o.indicator = Indicator::Slash,
                'i' => o.inode = true,
                's' => o.blocks = true,
                'h' => o.human = true,
                'n' => {
                    o.numeric = true;
                    o.format = Format::Long;
                }
                'g' => {
                    o.show_owner = false;
                    o.format = Format::Long;
                }
                'o' => {
                    o.show_group = false;
                    o.format = Format::Long;
                }
                'G' => o.show_group = false,
                'u' => o.time = TimeKind::Access,
                'c' => o.time = TimeKind::Change,
                'L' => o.deref = true,
                _ => {
                    eprintln!("{prog}: invalid option -- '{c}'");
                    try_help(prog);
                    return 2;
                }
            }
        }
    }
    // like GNU: -u / -c sort by that time when not in long format
    if o.format != Format::Long
        && o.time != TimeKind::Modify
        && !sort_by_time
        && o.sort == Sort::Name
    {
        o.sort = Sort::Time;
    }
    if !width_given {
        o.width = term_width(tty);
    }
    if o.time_style == TimeStyle::Locale {
        if let Ok(ts) = std::env::var("TIME_STYLE") {
            if let Some(t) = parse_time_style(&ts) {
                o.time_style = t;
            }
        }
    }

    let mut code = 0u8;
    let explicit = !files.is_empty();
    if files.is_empty() {
        files.push(".".into());
    }

    // Split command-line operands into files and directories.
    let mut file_entries = Vec::new();
    let mut dir_entries = Vec::new();
    for f in &files {
        let path = PathBuf::from(f);
        let follow_cmdline = o.deref
            || !(o.format == Format::Long || o.dirs_as_files || o.indicator == Indicator::Classify);
        let lmeta = fs::symlink_metadata(&path);
        let meta = match lmeta {
            Ok(m) if m.file_type().is_symlink() && follow_cmdline => fs::metadata(&path).or(Ok(m)),
            other => other,
        };
        match meta {
            Ok(m) => {
                let e = make_entry(f.clone(), path, Some(m), o.deref);
                if e.is_dir() && !o.dirs_as_files {
                    dir_entries.push(e);
                } else {
                    file_entries.push(e);
                }
            }
            Err(e) => {
                eprintln!(
                    "{prog}: cannot access '{}': {}",
                    f.to_string_lossy(),
                    describe(&e)
                );
                code = 2;
            }
        }
    }

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let mut colors = Colors::new(o.color);

    sort_entries(&mut file_entries, &o);
    sort_entries(&mut dir_entries, &o);

    let mut printed_something = false;
    if !file_entries.is_empty() {
        print_list(&mut out, &file_entries, &o, &mut colors, false);
        printed_something = true;
    }
    let show_headers = dir_entries.len() + file_entries.len() > 1
        || o.recursive
        || (explicit && code != 0 && !dir_entries.is_empty());
    for d in &dir_entries {
        if list_dir(
            prog,
            &mut out,
            d,
            &o,
            &mut colors,
            show_headers,
            &mut printed_something,
            true,
        ) {
            code = code.max(2);
        }
    }
    colors.finish(&mut out);
    let _ = out.flush();
    code
}

fn help(prog: &str) {
    println!(
        "Usage: {prog} [OPTION]... [FILE]...
List information about the FILEs (the current directory by default).

  -a, --all                  do not ignore entries starting with .
  -A, --almost-all           do not list implied . and ..
  -c                         use time of last status change
  -C                         list entries by columns
      --color[=WHEN]         color the output; WHEN can be 'always', 'auto', or 'never'
  -d, --directory            list directories themselves, not their contents
  -f                         list all entries in directory order
  -F, --classify             append indicator (one of */=@|) to entries
  -g                         like -l, but do not list owner
  -G, --no-group             in a long listing, don't print group names
      --group-directories-first
  -h, --human-readable       with -l and -s, print sizes like 1K 234M 2G etc.
  -i, --inode                print the index number of each file
  -l                         use a long listing format
  -L, --dereference          show info for the file a symlink points to
  -m                         fill width with a comma separated list of entries
  -n, --numeric-uid-gid      like -l, but list numeric user and group IDs
  -o                         like -l, but do not list group information
  -p                         append / indicator to directories
  -r, --reverse              reverse order while sorting
  -R, --recursive            list subdirectories recursively
  -s, --size                 print the allocated size of each file, in blocks
  -S                         sort by file size, largest first
  -t                         sort by time, newest first
  -u                         use time of last access
  -U                         do not sort; list entries in directory order
  -x                         list entries by lines instead of by columns
  -X                         sort alphabetically by entry extension
  -1                         list one file per line"
    );
}

fn parse_time_style(v: &str) -> Option<TimeStyle> {
    if let Some(fmt) = v.strip_prefix('+') {
        let (old, recent) = match fmt.split_once('\n') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (fmt.to_string(), fmt.to_string()),
        };
        return Some(TimeStyle::Custom(old, recent));
    }
    // posix-STYLE means STYLE unless the locale is POSIX/C
    let (posix, style) = match v.strip_prefix("posix-") {
        Some(rest) => (true, rest),
        None => (false, v),
    };
    let c_locale = {
        let l = ["LC_ALL", "LC_TIME", "LANG"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|x| !x.is_empty()))
            .unwrap_or_default();
        l.is_empty() || l == "C" || l == "POSIX"
    };
    let ts = match style {
        "full-iso" => TimeStyle::FullIso,
        "long-iso" => TimeStyle::LongIso,
        "iso" => TimeStyle::Iso,
        "locale" => TimeStyle::Locale,
        _ => return None,
    };
    if posix && c_locale {
        return Some(TimeStyle::Locale);
    }
    Some(ts)
}

fn bad_time_style(prog: &str, v: &str) {
    eprintln!(
        "{prog}: invalid argument {} for {}",
        crate::util::quote(v),
        crate::util::quote("time style")
    );
    eprintln!("Valid arguments are:");
    eprintln!("  - [posix-]full-iso");
    eprintln!("  - [posix-]long-iso");
    eprintln!("  - [posix-]iso");
    eprintln!("  - [posix-]locale");
    eprintln!("  - +FORMAT (e.g., +%H:%M) for a 'date'-style format");
    try_help(prog);
}

fn term_width(tty: bool) -> usize {
    if let Ok(c) = std::env::var("COLUMNS") {
        if let Ok(n) = c.parse::<usize>() {
            if n > 0 {
                return n;
            }
        }
    }
    if tty {
        // SAFETY: winsize is plain data; ioctl fills it
        unsafe {
            let mut ws: libc::winsize = std::mem::zeroed();
            if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
                return ws.ws_col as usize;
            }
        }
    }
    80
}

fn make_entry(name: OsString, path: PathBuf, meta: Option<Metadata>, deref: bool) -> Entry {
    let mut meta = meta;
    let mut target_meta = None;
    let mut link_target = None;
    if let Some(m) = &meta {
        if m.file_type().is_symlink() {
            link_target = fs::read_link(&path).ok();
            target_meta = fs::metadata(&path).ok();
            if deref {
                if let Some(t) = &target_meta {
                    meta = Some(t.clone());
                }
            }
        }
    }
    Entry {
        name,
        path,
        meta,
        target_meta,
        link_target,
    }
}

fn collate(a: &OsStr, b: &OsStr) -> std::cmp::Ordering {
    match (CString::new(a.as_bytes()), CString::new(b.as_bytes())) {
        // SAFETY: two valid C strings
        (Ok(x), Ok(y)) => unsafe { libc::strcoll(x.as_ptr(), y.as_ptr()) }.cmp(&0),
        _ => a.cmp(b),
    }
}

fn time_of(m: &Metadata, k: TimeKind) -> (i64, i64) {
    match k {
        TimeKind::Modify => (m.mtime(), m.mtime_nsec()),
        TimeKind::Access => (m.atime(), m.atime_nsec()),
        TimeKind::Change => (m.ctime(), m.ctime_nsec()),
    }
}

fn extension(name: &OsStr) -> &[u8] {
    let b = name.as_bytes();
    match b.iter().rposition(|&c| c == b'.') {
        Some(i) if i > 0 => &b[i..],
        _ => b"",
    }
}

fn sort_entries(v: &mut [Entry], o: &Opts) {
    use std::cmp::Ordering;
    if o.sort != Sort::None {
        v.sort_by(|a, b| {
            let by_name = || collate(&a.name, &b.name);
            let ord = match (o.sort, &a.meta, &b.meta) {
                (Sort::Time, Some(x), Some(y)) => time_of(y, o.time)
                    .cmp(&time_of(x, o.time))
                    .then_with(by_name),
                (Sort::Size, Some(x), Some(y)) => y.len().cmp(&x.len()).then_with(by_name),
                (Sort::Ext, _, _) => extension(&a.name)
                    .cmp(extension(&b.name))
                    .then_with(by_name),
                _ => by_name(),
            };
            if o.reverse {
                ord.reverse()
            } else {
                ord
            }
        });
    } else if o.reverse {
        v.reverse();
    }
    if o.dirs_first {
        v.sort_by(|a, b| match (a.is_dir(), b.is_dir()) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => Ordering::Equal,
        });
    }
}

/// List one directory. Returns true on a serious error.
#[allow(clippy::too_many_arguments)]
fn list_dir<W: Write>(
    prog: &str,
    out: &mut W,
    d: &Entry,
    o: &Opts,
    colors: &mut Colors,
    header: bool,
    printed: &mut bool,
    top: bool,
) -> bool {
    let rd = match fs::read_dir(&d.path) {
        Ok(rd) => rd,
        Err(e) => {
            let _ = out.flush();
            eprintln!(
                "{prog}: cannot open directory '{}': {}",
                d.path.display(),
                describe(&e)
            );
            return top;
        }
    };
    if *printed {
        let _ = writeln!(out);
    }
    if header {
        let _ = out.write_all(d.path.as_os_str().as_bytes());
        let _ = writeln!(out, ":");
    }
    *printed = true;

    let mut entries: Vec<Entry> = Vec::new();
    if o.hidden == Hidden::All {
        for dot in [".", ".."] {
            let p = d.path.join(dot);
            let m = fs::symlink_metadata(&p).ok();
            entries.push(make_entry(dot.into(), p, m, o.deref));
        }
    }
    for e in rd.flatten() {
        let name = e.file_name();
        if o.hidden == Hidden::None && name.as_bytes().first() == Some(&b'.') {
            continue;
        }
        let p = d.path.join(&name);
        let m = fs::symlink_metadata(&p).ok();
        entries.push(make_entry(name, p, m, o.deref));
    }
    sort_entries(&mut entries, o);
    print_list(out, &entries, o, colors, true);

    let mut bad = false;
    if o.recursive {
        for e in &entries {
            let n = e.name.as_bytes();
            if n == b"." || n == b".." {
                continue;
            }
            let is_real_dir = e.meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            if is_real_dir && list_dir(prog, out, e, o, colors, true, printed, false) {
                bad = true;
            }
        }
    }
    bad
}

// ---------- formatting ----------

fn mode_string(m: &Metadata, path: &Path) -> String {
    let ft = m.file_type();
    let t = if ft.is_dir() {
        'd'
    } else if ft.is_symlink() {
        'l'
    } else if ft.is_char_device() {
        'c'
    } else if ft.is_block_device() {
        'b'
    } else if ft.is_fifo() {
        'p'
    } else if ft.is_socket() {
        's'
    } else {
        '-'
    };
    let mode = m.mode();
    let mut s = String::with_capacity(11);
    s.push(t);
    let bit = |b: u32, c: char| if mode & b != 0 { c } else { '-' };
    let special = |x: u32, sp: u32, on: char, off: char| match (mode & x != 0, mode & sp != 0) {
        (true, true) => on,
        (false, true) => off,
        (true, false) => 'x',
        (false, false) => '-',
    };
    s.push(bit(0o400, 'r'));
    s.push(bit(0o200, 'w'));
    s.push(special(0o100, 0o4000, 's', 'S'));
    s.push(bit(0o040, 'r'));
    s.push(bit(0o020, 'w'));
    s.push(special(0o010, 0o2000, 's', 'S'));
    s.push(bit(0o004, 'r'));
    s.push(bit(0o002, 'w'));
    s.push(special(0o001, 0o1000, 't', 'T'));
    s.push_str(xattr_flag(path));
    s
}

/// GNU marks files with an SELinux context with '.', and ACLs with '+'.
fn xattr_flag(path: &Path) -> &'static str {
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return "";
    };
    let has = |name: &CStr| -> bool {
        // SAFETY: valid C strings; size 0 just asks for the length
        unsafe { libc::lgetxattr(c.as_ptr(), name.as_ptr(), std::ptr::null_mut(), 0) > 0 }
    };
    if has(c"system.posix_acl_access") || has(c"system.posix_acl_default") {
        "+"
    } else if has(c"security.selinux") {
        "."
    } else {
        ""
    }
}

fn user_name(uid: u32, numeric: bool) -> String {
    if !numeric {
        // SAFETY: getpwuid returns a pointer into static storage or null
        unsafe {
            let pw = libc::getpwuid(uid);
            if !pw.is_null() {
                return CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned();
            }
        }
    }
    uid.to_string()
}

fn group_name(gid: u32, numeric: bool) -> String {
    if !numeric {
        // SAFETY: getgrgid returns a pointer into static storage or null
        unsafe {
            let gr = libc::getgrgid(gid);
            if !gr.is_null() {
                return CStr::from_ptr((*gr).gr_name).to_string_lossy().into_owned();
            }
        }
    }
    gid.to_string()
}

/// GNU -h: powers of 1024, rounded up, one decimal below 10.
pub fn human(n: u64) -> String {
    if n < 1024 {
        return n.to_string();
    }
    let units = ['K', 'M', 'G', 'T', 'P', 'E'];
    let mut div: u128 = 1024;
    let mut u = 0;
    while (n as u128) >= div * 1024 && u + 1 < units.len() {
        div *= 1024;
        u += 1;
    }
    let n = n as u128;
    // tenths, rounded up
    let tenths = (n * 10).div_ceil(div);
    if tenths < 100 {
        return format!("{}.{}{}", tenths / 10, tenths % 10, units[u]);
    }
    let whole = n.div_ceil(div);
    if whole >= 1024 && u + 1 < units.len() {
        return format!("1.0{}", units[u + 1]);
    }
    format!("{whole}{}", units[u])
}

fn blocks_1k(m: &Metadata) -> u64 {
    (m.blocks() * 512).div_ceil(1024)
}

fn strftime(fmt: &str, tm: &libc::tm, nsecs: i64) -> String {
    // %N isn't standard C; fill it in ourselves like GNU does
    let fmt = fmt.replace("%N", &format!("{nsecs:09}"));
    let Ok(c) = CString::new(fmt) else {
        return String::new();
    };
    let mut buf = vec![0u8; 256];
    // SAFETY: buffer and format are valid; strftime never writes past len
    let n = unsafe {
        libc::strftime(
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            c.as_ptr(),
            tm,
        )
    };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

fn fmt_time(secs: i64, nsecs: i64, style: &TimeStyle) -> String {
    let full = *style == TimeStyle::FullIso;
    // SAFETY: localtime_r fills a plain-data struct
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    };
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    if full {
        let off = tm.tm_gmtoff;
        let sign = if off < 0 { '-' } else { '+' };
        let off = off.abs();
        return format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:09} {}{:02}{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
            nsecs,
            sign,
            off / 3600,
            (off % 3600) / 60
        );
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let six_months = 31_556_952 / 2;
    let recent = secs > now - six_months && secs <= now;
    match style {
        TimeStyle::LongIso => return strftime("%Y-%m-%d %H:%M", &tm, nsecs),
        TimeStyle::Iso => {
            return if recent {
                strftime("%m-%d %H:%M", &tm, nsecs)
            } else {
                strftime("%Y-%m-%d ", &tm, nsecs)
            }
        }
        TimeStyle::Custom(old, new) => return strftime(if recent { new } else { old }, &tm, nsecs),
        _ => {}
    }
    let mon = MON[tm.tm_mon.clamp(0, 11) as usize];
    if recent {
        format!("{mon} {:>2} {:02}:{:02}", tm.tm_mday, tm.tm_hour, tm.tm_min)
    } else {
        format!("{mon} {:>2}  {}", tm.tm_mday, tm.tm_year + 1900)
    }
}

fn indicator(m: Option<&Metadata>, ind: Indicator) -> &'static str {
    let Some(m) = m else { return "" };
    let ft = m.file_type();
    match ind {
        Indicator::None => "",
        Indicator::Slash => {
            if ft.is_dir() {
                "/"
            } else {
                ""
            }
        }
        Indicator::FileType | Indicator::Classify => {
            if ft.is_dir() {
                "/"
            } else if ft.is_symlink() {
                "@"
            } else if ft.is_fifo() {
                "|"
            } else if ft.is_socket() {
                "="
            } else if ind == Indicator::Classify && ft.is_file() && m.mode() & 0o111 != 0 {
                "*"
            } else {
                ""
            }
        }
    }
}

/// Shell-style quoting for names shown on a terminal (GNU's default there).
fn display_name(name: &OsStr, o: &Opts) -> String {
    let raw = name.to_string_lossy();
    let s: String = if o.hide_control {
        raw.chars()
            .map(|c| if c.is_control() { '?' } else { c })
            .collect()
    } else {
        raw.into_owned()
    };
    if !o.quote {
        return s;
    }
    let needs = s.is_empty()
        || s.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    ' ' | '\''
                        | '"'
                        | '\\'
                        | '$'
                        | '`'
                        | '*'
                        | '?'
                        | '['
                        | ']'
                        | '('
                        | ')'
                        | '{'
                        | '}'
                        | '<'
                        | '>'
                        | '|'
                        | '&'
                        | ';'
                        | '!'
                        | '#'
                        | '~'
                        | '='
                )
        });
    if !needs {
        return s;
    }
    if s.chars().any(|c| c.is_control()) {
        let mut r = String::from("$'");
        for c in s.chars() {
            match c {
                '\n' => r.push_str("\\n"),
                '\t' => r.push_str("\\t"),
                '\'' => r.push_str("\\'"),
                '\\' => r.push_str("\\\\"),
                c if c.is_control() => r.push_str(&format!("\\{:03o}", c as u32)),
                c => r.push(c),
            }
        }
        r.push('\'');
        return r;
    }
    if s.contains('\'') {
        return format!(
            "\"{}\"",
            s.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
                .replace('`', "\\`")
        );
    }
    format!("'{s}'")
}

fn text_width(s: &str) -> usize {
    s.chars().count()
}

struct Colors {
    on: bool,
    table: Vec<(String, String)>,
    used: bool,
}

impl Colors {
    fn new(on: bool) -> Self {
        let mut table: Vec<(String, String)> = vec![
            ("rs".into(), "0".into()),
            ("di".into(), "01;34".into()),
            ("ln".into(), "01;36".into()),
            ("pi".into(), "33".into()),
            ("so".into(), "01;35".into()),
            ("bd".into(), "01;33".into()),
            ("cd".into(), "01;33".into()),
            ("ex".into(), "01;32".into()),
            ("su".into(), "37;41".into()),
            ("sg".into(), "30;43".into()),
            ("tw".into(), "30;42".into()),
            ("ow".into(), "34;42".into()),
            ("st".into(), "37;44".into()),
        ];
        if on {
            if let Ok(lc) = std::env::var("LS_COLORS") {
                for item in lc.split(':') {
                    if let Some((k, v)) = item.split_once('=') {
                        if let Some(slot) = table.iter_mut().find(|(key, _)| key == k) {
                            slot.1 = v.to_string();
                        } else {
                            table.push((k.to_string(), v.to_string()));
                        }
                    }
                }
            }
        }
        Colors {
            on,
            table,
            used: false,
        }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.table
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    }

    fn code_for(&self, e: &Entry) -> Option<String> {
        let m = e.meta.as_ref()?;
        let ft = m.file_type();
        let mode = m.mode();
        let key = if ft.is_dir() {
            if mode & 0o1000 != 0 && mode & 0o002 != 0 {
                "tw"
            } else if mode & 0o002 != 0 {
                "ow"
            } else if mode & 0o1000 != 0 {
                "st"
            } else {
                "di"
            }
        } else if ft.is_symlink() {
            if e.target_meta.is_none() {
                if self.get("or").is_some() {
                    "or"
                } else {
                    "ln"
                }
            } else {
                "ln"
            }
        } else if ft.is_fifo() {
            "pi"
        } else if ft.is_socket() {
            "so"
        } else if ft.is_block_device() {
            "bd"
        } else if ft.is_char_device() {
            "cd"
        } else if mode & 0o4000 != 0 {
            "su"
        } else if mode & 0o2000 != 0 {
            "sg"
        } else if mode & 0o111 != 0 {
            "ex"
        } else {
            // extension rules like *.tar=01;31
            let name = e.name.to_string_lossy();
            for (k, v) in &self.table {
                if let Some(ext) = k.strip_prefix('*') {
                    if name.ends_with(ext) {
                        return Some(v.clone());
                    }
                }
            }
            return self.get("fi").map(str::to_string);
        };
        self.get(key).map(str::to_string)
    }

    fn paint<W: Write>(&mut self, out: &mut W, e: &Entry, text: &str) {
        if self.on {
            if let Some(code) = self.code_for(e) {
                if !self.used {
                    let _ = write!(out, "\x1b[0m");
                    self.used = true;
                }
                let _ = write!(out, "\x1b[{code}m{text}\x1b[0m");
                return;
            }
        }
        let _ = out.write_all(text.as_bytes());
    }

    fn finish<W: Write>(&self, _out: &mut W) {}
}

/// Print a list of entries in the chosen format.
fn print_list<W: Write>(
    out: &mut W,
    entries: &[Entry],
    o: &Opts,
    colors: &mut Colors,
    in_dir: bool,
) {
    let block_str = |m: Option<&Metadata>| -> String {
        match m {
            Some(m) if o.human => human(m.blocks() * 512),
            Some(m) => blocks_1k(m).to_string(),
            None => "?".into(),
        }
    };

    if in_dir && (o.format == Format::Long || o.blocks) {
        let total: u64 = entries
            .iter()
            .filter_map(|e| e.meta.as_ref())
            .map(|m| m.blocks())
            .sum();
        let t = if o.human {
            human(total * 512)
        } else {
            (total * 512).div_ceil(1024).to_string()
        };
        let _ = writeln!(out, "total {t}");
    }

    let inode_w = if o.inode {
        entries
            .iter()
            .map(|e| {
                e.meta
                    .as_ref()
                    .map(|m| m.ino().to_string().len())
                    .unwrap_or(1)
            })
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    let block_w = if o.blocks {
        entries
            .iter()
            .map(|e| block_str(e.meta.as_ref()).len())
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    let prefix = |e: &Entry| -> String {
        let mut p = String::new();
        if o.inode {
            let ino = e
                .meta
                .as_ref()
                .map(|m| m.ino().to_string())
                .unwrap_or("?".into());
            p.push_str(&format!("{ino:>inode_w$} "));
        }
        if o.blocks {
            p.push_str(&format!("{:>block_w$} ", block_str(e.meta.as_ref())));
        }
        p
    };

    if o.format == Format::Long {
        print_long(out, entries, o, colors, &prefix);
        return;
    }

    // name cells: (prefix, display name, indicator)
    let cells: Vec<(String, String, &'static str)> = entries
        .iter()
        .map(|e| {
            (
                prefix(e),
                display_name(&e.name, o),
                indicator(e.meta.as_ref(), o.indicator),
            )
        })
        .collect();
    let widths: Vec<usize> = cells
        .iter()
        .map(|(p, n, i)| text_width(p) + text_width(n) + i.len())
        .collect();

    match o.format {
        Format::One => {
            for (e, (p, n, i)) in entries.iter().zip(&cells) {
                let _ = out.write_all(p.as_bytes());
                colors.paint(out, e, n);
                let _ = writeln!(out, "{i}");
            }
        }
        Format::Commas => {
            let mut col = 0usize;
            for (idx, (e, (p, n, i))) in entries.iter().zip(&cells).enumerate() {
                let w = widths[idx];
                let last = idx + 1 == entries.len();
                let extra = if last { 0 } else { 1 };
                if idx > 0 {
                    if col + 1 + w + extra > o.width && o.width > 0 {
                        let _ = writeln!(out);
                        col = 0;
                    } else {
                        let _ = write!(out, " ");
                        col += 1;
                    }
                }
                let _ = out.write_all(p.as_bytes());
                colors.paint(out, e, n);
                let _ = write!(out, "{i}");
                col += w;
                if !last {
                    let _ = write!(out, ",");
                    col += 1;
                }
            }
            if !entries.is_empty() {
                let _ = writeln!(out);
            }
        }
        Format::Columns | Format::Across => {
            let n = entries.len();
            if n == 0 {
                return;
            }
            let across = o.format == Format::Across;
            let (cols, col_w) = fit_columns(&widths, o.width, across);
            let rows = n.div_ceil(cols);
            for r in 0..rows {
                let mut pos = 0usize;
                for c in 0..cols {
                    let idx = if across { r * cols + c } else { c * rows + r };
                    if idx >= n {
                        break;
                    }
                    let next_exists = {
                        let ni = if across {
                            r * cols + c + 1
                        } else {
                            (c + 1) * rows + r
                        };
                        c + 1 < cols && ni < n
                    };
                    let (p, nm, i) = &cells[idx];
                    let _ = out.write_all(p.as_bytes());
                    colors.paint(out, &entries[idx], nm);
                    let _ = write!(out, "{i}");
                    pos += widths[idx];
                    if next_exists {
                        let target: usize = col_w[..=c].iter().sum();
                        indent(out, pos, target, o.tabsize);
                        pos = target;
                    }
                }
                let _ = writeln!(out);
            }
        }
        Format::Long => unreachable!(),
    }
}

/// Pad from column `from` to `to`, using tabs where a whole tab fits (GNU does this).
fn indent<W: Write>(out: &mut W, mut from: usize, to: usize, tabsize: usize) {
    while from < to {
        if tabsize != 0 && to / tabsize > (from + 1) / tabsize {
            let _ = out.write_all(b"\t");
            from += tabsize - from % tabsize;
        } else {
            let _ = out.write_all(b" ");
            from += 1;
        }
    }
}

/// GNU's column fitting: the most columns whose widths (+2 gap) fit the line.
fn fit_columns(widths: &[usize], line: usize, across: bool) -> (usize, Vec<usize>) {
    let n = widths.len();
    let max_cols = (line / 3).clamp(1, n.max(1));
    for cols in (1..=max_cols).rev() {
        let rows = n.div_ceil(cols);
        // every column must actually have something in it
        if !across && (cols - 1) * rows >= n {
            continue;
        }
        let mut col_w = vec![0usize; cols];
        for (i, &w) in widths.iter().enumerate() {
            let c = if across { i % cols } else { i / rows };
            let gap = if c + 1 == cols { 0 } else { 2 };
            col_w[c] = col_w[c].max(w + gap);
        }
        if col_w.iter().sum::<usize>() < line || cols == 1 {
            return (cols, col_w);
        }
    }
    (1, vec![widths.iter().copied().max().unwrap_or(0)])
}

fn print_long<W: Write>(
    out: &mut W,
    entries: &[Entry],
    o: &Opts,
    colors: &mut Colors,
    prefix: &dyn Fn(&Entry) -> String,
) {
    struct Row {
        prefix: String,
        mode: String,
        links: String,
        owner: String,
        group: String,
        size: String,
        major_minor: Option<(String, String)>,
        time: String,
    }
    let rows: Vec<Option<Row>> = entries
        .iter()
        .map(|e| {
            let m = e.meta.as_ref()?;
            let (t, tn) = time_of(m, o.time);
            let ft = m.file_type();
            let major_minor = if ft.is_char_device() || ft.is_block_device() {
                let r = m.rdev();
                Some((libc::major(r).to_string(), libc::minor(r).to_string()))
            } else {
                None
            };
            Some(Row {
                prefix: prefix(e),
                mode: mode_string(m, &e.path),
                links: m.nlink().to_string(),
                owner: user_name(m.uid(), o.numeric),
                group: group_name(m.gid(), o.numeric),
                size: if o.human {
                    human(m.len())
                } else {
                    m.len().to_string()
                },
                major_minor,
                time: fmt_time(t, tn, &o.time_style),
            })
        })
        .collect();
    let w = |f: &dyn Fn(&Row) -> usize| rows.iter().flatten().map(f).max().unwrap_or(0);
    let mode_w = w(&|r| r.mode.len());
    let links_w = w(&|r| r.links.len());
    let owner_w = w(&|r| text_width(&r.owner));
    let group_w = w(&|r| text_width(&r.group));
    let major_w = w(&|r| r.major_minor.as_ref().map(|m| m.0.len()).unwrap_or(0));
    let minor_w = w(&|r| r.major_minor.as_ref().map(|m| m.1.len()).unwrap_or(0));
    let dev_w = if major_w > 0 {
        major_w + 2 + minor_w
    } else {
        0
    };
    let size_w = w(&|r| r.size.len()).max(dev_w);

    for (e, row) in entries.iter().zip(&rows) {
        let Some(r) = row else {
            let _ = writeln!(out, "?????????? ? ? ? ? ? {}", e.name.to_string_lossy());
            continue;
        };
        let mut line = format!("{}{:<mode_w$} {:>links_w$} ", r.prefix, r.mode, r.links);
        if o.show_owner {
            line.push_str(&format!("{:<owner_w$} ", r.owner));
        }
        if o.show_group {
            line.push_str(&format!("{:<group_w$} ", r.group));
        }
        let size = match &r.major_minor {
            Some((maj, min)) => {
                let s = format!("{maj:>major_w$}, {min:>minor_w$}");
                format!("{s:>size_w$}")
            }
            None => format!("{:>size_w$}", r.size),
        };
        line.push_str(&size);
        line.push(' ');
        line.push_str(&r.time);
        line.push(' ');
        let _ = out.write_all(line.as_bytes());
        colors.paint(out, e, &display_name(&e.name, o));
        let is_link = e
            .meta
            .as_ref()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if is_link {
            if let Some(t) = &e.link_target {
                let _ = write!(out, " -> ");
                let tname = display_name(t.as_os_str(), o);
                let target_entry = Entry {
                    name: t.as_os_str().to_os_string(),
                    path: t.clone(),
                    meta: e.target_meta.clone(),
                    target_meta: e.target_meta.clone(),
                    link_target: None,
                };
                colors.paint(out, &target_entry, &tname);
                let _ = write!(out, "{}", indicator(e.target_meta.as_ref(), o.indicator));
            }
        } else {
            let _ = write!(out, "{}", indicator(e.meta.as_ref(), o.indicator));
        }
        let _ = writeln!(out);
    }
}

#[cfg(test)]
mod tests {
    use super::{fit_columns, human};

    #[test]
    fn human_sizes() {
        assert_eq!(human(0), "0");
        assert_eq!(human(1023), "1023");
        assert_eq!(human(1024), "1.0K");
        assert_eq!(human(1025), "1.1K");
        assert_eq!(human(4096), "4.0K");
        assert_eq!(human(10_239), "10K");
        assert_eq!(human(1_048_575), "1.0M");
        assert_eq!(human(5 * 1024 * 1024 * 1024), "5.0G");
    }

    #[test]
    fn columns() {
        let w = vec![3; 10];
        let (cols, _) = fit_columns(&w, 80, false);
        assert_eq!(cols, 10);
        let (cols, _) = fit_columns(&w, 20, false);
        assert!(cols < 10);
    }
}
