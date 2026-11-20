//! RNG's Not GNU!!!
//!
//! One binary, many tools (BusyBox-style). Run a tool either as
//! `rng <tool> [args...]` or through a symlink named after the tool
//! (e.g. `ln -s rng cat`, then `./cat file.txt`).

mod tools;
mod util;

use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let mut args: Vec<OsString> = std::env::args_os().collect();
    let argv0 = args
        .first()
        .and_then(|a| Path::new(a).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rng".into());

    // Called through a symlink like `cat` → run that tool directly.
    if argv0 != "rng" {
        if let Some(tool) = tools::find(&argv0) {
            args.remove(0);
            return ExitCode::from(tool.run(&argv0, args));
        }
    }

    // Called as `rng <tool> ...`
    args.remove(0);
    let Some(first) = args.first().map(|a| a.to_string_lossy().into_owned()) else {
        print_usage();
        return ExitCode::from(1);
    };

    match first.as_str() {
        "--help" | "-h" => {
            print_usage();
            ExitCode::SUCCESS
        }
        "--version" | "-V" => {
            println!("rng {VERSION} — RNG's Not GNU!!!");
            ExitCode::SUCCESS
        }
        "--list" => {
            for t in tools::ALL {
                println!("{}", t.name);
            }
            ExitCode::SUCCESS
        }
        name => match tools::find(name) {
            Some(tool) => {
                args.remove(0);
                ExitCode::from(tool.run(name, args))
            }
            None => {
                eprintln!("rng: '{name}' isn't a tool (yet). Try `rng --list`.");
                ExitCode::from(127)
            }
        },
    }
}

fn print_usage() {
    println!("rng {VERSION} — RNG's Not GNU!!!");
    println!();
    println!("Usage: rng <tool> [args...]");
    println!("       <tool> [args...]   (when rng is symlinked as <tool>)");
    println!();
    println!("Tools:");
    for t in tools::ALL {
        println!("  {:<8} {}", t.name, t.about);
    }
    println!();
    println!("Roll the dice. Don't die. Ok?");
}
