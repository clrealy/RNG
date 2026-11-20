use std::ffi::OsString;

// Like GNU, `true`/`false` ignore every argument except a lone --help/--version.
fn info(prog: &str, args: &[OsString], about: &str) {
    if args.len() == 1 {
        match args[0].to_str() {
            Some("--help") => println!("Usage: {prog} [ignored command line arguments]\n{about}"),
            Some("--version") => println!("{prog} (rng) {}", crate::VERSION),
            _ => {}
        }
    }
}

pub fn main_true(prog: &str, args: Vec<OsString>) -> u8 {
    info(prog, &args, "Exit with a status code indicating success.");
    0
}

pub fn main_false(prog: &str, args: Vec<OsString>) -> u8 {
    info(prog, &args, "Exit with a status code indicating failure.");
    1
}
