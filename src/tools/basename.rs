//! basename / dirname — the two halves of a path.

use crate::util::{bytes, try_help};
use std::ffi::OsString;
use std::io::{self, Write};

/// The last part of a path, GNU rules: trailing slashes are ignored and a
/// path that's only slashes gives "/".
pub fn base(path: &[u8]) -> &[u8] {
    if path.is_empty() {
        return b"";
    }
    let trimmed_len = path.iter().rposition(|&c| c != b'/').map(|i| i + 1);
    let Some(end) = trimmed_len else {
        return b"/";
    };
    let p = &path[..end];
    match p.iter().rposition(|&c| c == b'/') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

/// Everything but the last part, GNU rules.
pub fn dir(path: &[u8]) -> &[u8] {
    // drop trailing slashes
    let Some(end) = path.iter().rposition(|&c| c != b'/') else {
        return if path.is_empty() { b"." } else { b"/" };
    };
    let p = &path[..=end];
    // drop the last component
    let Some(slash) = p.iter().rposition(|&c| c == b'/') else {
        return b".";
    };
    // drop the slashes before it
    match p[..slash].iter().rposition(|&c| c != b'/') {
        Some(i) => &p[..=i],
        None => b"/",
    }
}

pub fn main_basename(prog: &str, args: Vec<OsString>) -> u8 {
    let mut multiple = false;
    let mut suffix: Option<Vec<u8>> = None;
    let mut zero = false;
    let mut names: Vec<Vec<u8>> = Vec::new();
    let mut i = 0;
    let mut only_names = false;

    while i < args.len() {
        let a = bytes(&args[i]);
        let s = String::from_utf8_lossy(&a).into_owned();
        i += 1;
        if only_names || !s.starts_with('-') || s == "-" {
            names.push(a);
            continue;
        }
        match s.as_str() {
            "--" => only_names = true,
            "--multiple" => multiple = true,
            "--zero" => zero = true,
            "--help" => {
                println!("Usage: {prog} NAME [SUFFIX]\n  or:  {prog} OPTION... NAME...\nPrint NAME with any leading directory components removed.\n\n  -a, --multiple       support multiple arguments and treat each as a NAME\n  -s, --suffix=SUFFIX  remove a trailing SUFFIX; implies -a\n  -z, --zero           end each output line with NUL, not newline");
                return 0;
            }
            "--version" => {
                println!("{prog} (rng) {}", crate::VERSION);
                return 0;
            }
            _ if s.starts_with("--suffix=") => {
                suffix = Some(a["--suffix=".len()..].to_vec());
                multiple = true;
            }
            "--suffix" => {
                let Some(v) = args.get(i) else {
                    eprintln!("{prog}: option '--suffix' requires an argument");
                    try_help(prog);
                    return 1;
                };
                suffix = Some(bytes(v));
                multiple = true;
                i += 1;
            }
            _ if s.starts_with("--") => {
                eprintln!("{prog}: unrecognized option '{s}'");
                try_help(prog);
                return 1;
            }
            _ => {
                let chars: Vec<char> = s[1..].chars().collect();
                let mut k = 0;
                while k < chars.len() {
                    match chars[k] {
                        'a' => multiple = true,
                        'z' => zero = true,
                        's' => {
                            let rest: String = chars[k + 1..].iter().collect();
                            if !rest.is_empty() {
                                suffix = Some(rest.into_bytes());
                            } else if let Some(v) = args.get(i) {
                                suffix = Some(bytes(v));
                                i += 1;
                            } else {
                                eprintln!("{prog}: option requires an argument -- 's'");
                                try_help(prog);
                                return 1;
                            }
                            multiple = true;
                            break;
                        }
                        c => {
                            eprintln!("{prog}: invalid option -- '{c}'");
                            try_help(prog);
                            return 1;
                        }
                    }
                    k += 1;
                }
            }
        }
    }

    if names.is_empty() {
        eprintln!("{prog}: missing operand");
        try_help(prog);
        return 1;
    }
    if !multiple {
        if names.len() > 2 {
            eprintln!(
                "{prog}: extra operand {}",
                crate::util::quote(&String::from_utf8_lossy(&names[2]))
            );
            try_help(prog);
            return 1;
        }
        if names.len() == 2 {
            suffix = Some(names.pop().unwrap());
        }
    }

    let end = if zero { b'\0' } else { b'\n' };
    let mut out = Vec::new();
    for n in &names {
        let mut b = base(n);
        if let Some(sfx) = &suffix {
            if !sfx.is_empty() && b.len() > sfx.len() && b.ends_with(sfx) {
                b = &b[..b.len() - sfx.len()];
            }
        }
        out.extend_from_slice(b);
        out.push(end);
    }
    let _ = io::stdout().lock().write_all(&out);
    0
}

pub fn main_dirname(prog: &str, args: Vec<OsString>) -> u8 {
    let mut zero = false;
    let mut names: Vec<Vec<u8>> = Vec::new();
    let mut only_names = false;
    for a in &args {
        let b = bytes(a);
        let s = String::from_utf8_lossy(&b).into_owned();
        if only_names || !s.starts_with('-') || s == "-" {
            names.push(b);
            continue;
        }
        match s.as_str() {
            "--" => only_names = true,
            "-z" | "--zero" => zero = true,
            "--help" => {
                println!("Usage: {prog} [OPTION] NAME...\nOutput each NAME with its last non-slash component and trailing slashes\nremoved; if NAME contains no /'s, output '.' (meaning the current directory).\n\n  -z, --zero     end each output line with NUL, not newline");
                return 0;
            }
            "--version" => {
                println!("{prog} (rng) {}", crate::VERSION);
                return 0;
            }
            _ if s.starts_with("--") => {
                eprintln!("{prog}: unrecognized option '{s}'");
                try_help(prog);
                return 1;
            }
            _ => {
                for c in s[1..].chars() {
                    if c != 'z' {
                        eprintln!("{prog}: invalid option -- '{c}'");
                        try_help(prog);
                        return 1;
                    }
                    zero = true;
                }
            }
        }
    }
    if names.is_empty() {
        eprintln!("{prog}: missing operand");
        try_help(prog);
        return 1;
    }
    let end = if zero { b'\0' } else { b'\n' };
    let mut out = Vec::new();
    for n in &names {
        out.extend_from_slice(dir(n));
        out.push(end);
    }
    let _ = io::stdout().lock().write_all(&out);
    0
}

#[cfg(test)]
mod tests {
    use super::{base, dir};

    #[test]
    fn basenames() {
        assert_eq!(base(b"/usr/lib/"), b"lib");
        assert_eq!(base(b"file.txt"), b"file.txt");
        assert_eq!(base(b"///"), b"/");
        assert_eq!(base(b"a/b//"), b"b");
    }

    #[test]
    fn dirnames() {
        assert_eq!(dir(b"/usr/lib/"), b"/usr");
        assert_eq!(dir(b"file"), b".");
        assert_eq!(dir(b"/file"), b"/");
        assert_eq!(dir(b"///"), b"/");
        assert_eq!(dir(b"a//b"), b"a");
        assert_eq!(dir(b""), b".");
    }
}
