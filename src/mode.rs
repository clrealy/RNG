//! File-mode strings, the chmod way: octal (`755`) or symbolic (`u+x,go-w`, `a=rX`).
//! Shared by mkdir -m now, and chmod/install later.

/// The current umask (read without changing it for good).
pub fn umask() -> u32 {
    // SAFETY: umask() can't fail; we put the old value straight back.
    unsafe {
        let old = libc::umask(0o022);
        libc::umask(old);
        old as u32
    }
}

/// Apply a mode string to `base`. `is_dir` matters for `X`.
/// `umask` is used for symbolic clauses that don't say who (like `+x`).
/// Returns None if the string is invalid.
pub fn apply(spec: &str, base: u32, is_dir: bool, umask: u32) -> Option<u32> {
    if spec.is_empty() {
        return None;
    }
    if spec.bytes().all(|c| (b'0'..=b'7').contains(&c)) {
        if spec.len() > 4 {
            return None;
        }
        let v = u32::from_str_radix(spec, 8).ok()?;
        // like GNU: a short octal mode on a directory keeps setuid/setgid
        if is_dir && spec.len() < 5 && v < 0o1000 {
            return Some(v | (base & 0o6000));
        }
        return Some(v);
    }

    let mut mode = base & 0o7777;
    for clause in spec.split(',') {
        let bytes = clause.as_bytes();
        let mut i = 0;
        let mut who = 0u32;
        while i < bytes.len() && matches!(bytes[i], b'u' | b'g' | b'o' | b'a') {
            who |= match bytes[i] {
                b'u' => 0o4700,
                b'g' => 0o2070,
                b'o' => 0o1007,
                _ => 0o7777,
            };
            i += 1;
        }
        let who_given = who != 0;
        if !who_given {
            who = 0o7777;
        }
        if i >= bytes.len() {
            return None; // "u" alone isn't a mode
        }
        while i < bytes.len() {
            let op = bytes[i];
            if !matches!(op, b'+' | b'-' | b'=') {
                return None;
            }
            i += 1;
            let mut perm = 0u32;
            // copying another class's bits: g=u, o+g ...
            if i < bytes.len() && matches!(bytes[i], b'u' | b'g' | b'o') {
                let src = match bytes[i] {
                    b'u' => (mode >> 6) & 7,
                    b'g' => (mode >> 3) & 7,
                    _ => mode & 7,
                };
                perm = src * 0o111;
                i += 1;
            } else {
                while i < bytes.len() && matches!(bytes[i], b'r' | b'w' | b'x' | b'X' | b's' | b't')
                {
                    perm |= match bytes[i] {
                        b'r' => 0o444,
                        b'w' => 0o222,
                        b'x' => 0o111,
                        b'X' => {
                            if is_dir || mode & 0o111 != 0 {
                                0o111
                            } else {
                                0
                            }
                        }
                        b's' => 0o6000,
                        _ => 0o1000, // t
                    };
                    i += 1;
                }
            }
            let mut affect = who;
            if !who_given {
                // no "who": everything except what the umask blocks
                affect &= !umask;
            }
            let bits = perm & affect;
            match op {
                b'+' => mode |= bits,
                b'-' => mode &= !bits,
                _ => {
                    // '=' clears the named classes' rwx (and their s/t), then sets
                    let mut clear = who & 0o777;
                    if who & 0o700 != 0 {
                        clear |= 0o4000;
                    }
                    if who & 0o070 != 0 {
                        clear |= 0o2000;
                    }
                    if who & 0o007 != 0 && who_given {
                        clear |= 0o1000;
                    }
                    if !who_given {
                        clear &= !umask;
                    }
                    // directories keep setuid/setgid unless asked
                    if is_dir {
                        clear &= !0o6000 | (perm & 0o6000);
                    }
                    mode = (mode & !clear) | bits;
                }
            }
        }
    }
    Some(mode)
}

#[cfg(test)]
mod tests {
    use super::apply;

    #[test]
    fn octal() {
        assert_eq!(apply("755", 0, false, 0o022), Some(0o755));
        assert_eq!(apply("0644", 0o777, false, 0o022), Some(0o644));
        assert_eq!(apply("8", 0, false, 0), None);
    }

    #[test]
    fn symbolic() {
        assert_eq!(apply("u+x", 0o644, false, 0o022), Some(0o744));
        assert_eq!(apply("go-w", 0o777, false, 0o022), Some(0o755));
        assert_eq!(apply("a=r", 0o777, false, 0o022), Some(0o444));
        assert_eq!(apply("u=rwx,g=rx,o=", 0, false, 0o022), Some(0o750));
        assert_eq!(apply("+x", 0o644, false, 0o022), Some(0o755));
        assert_eq!(apply("+w", 0o444, false, 0o022), Some(0o644));
        assert_eq!(apply("a+X", 0o644, true, 0o022), Some(0o755));
        assert_eq!(apply("a+X", 0o644, false, 0o022), Some(0o644));
        assert_eq!(apply("g=u", 0o740, false, 0o022), Some(0o770));
        assert_eq!(apply("u+s", 0o755, false, 0o022), Some(0o4755));
        assert_eq!(apply("+t", 0o777, true, 0o022), Some(0o1777));
        assert_eq!(apply("u", 0o777, false, 0), None);
        assert_eq!(apply("z+x", 0o777, false, 0), None);
    }
}
