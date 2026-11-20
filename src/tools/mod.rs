use std::ffi::OsString;

mod basename;
mod cat;
mod echo;
mod headtail;
mod pwd;
mod truefalse;
mod wc;
mod yes;

/// One tool inside the rng multicall binary.
pub struct Tool {
    pub name: &'static str,
    pub about: &'static str,
    /// `prog` is the name the user called it by (used in error messages);
    /// `args` excludes the program name. Returns the exit code.
    main: fn(prog: &str, args: Vec<OsString>) -> u8,
}

impl Tool {
    pub fn run(&self, prog: &str, args: Vec<OsString>) -> u8 {
        (self.main)(prog, args)
    }
}

pub const ALL: &[Tool] = &[
    Tool {
        name: "basename",
        about: "strip directory and suffix from a path",
        main: basename::main_basename,
    },
    Tool {
        name: "cat",
        about: "concatenate files and print them",
        main: cat::main,
    },
    Tool {
        name: "dirname",
        about: "strip the last part from a path",
        main: basename::main_dirname,
    },
    Tool {
        name: "echo",
        about: "print a line of text",
        main: echo::main,
    },
    Tool {
        name: "false",
        about: "do nothing, unsuccessfully",
        main: truefalse::main_false,
    },
    Tool {
        name: "head",
        about: "print the first part of files",
        main: headtail::main_head,
    },
    Tool {
        name: "pwd",
        about: "print the current directory",
        main: pwd::main,
    },
    Tool {
        name: "tail",
        about: "print the last part of files",
        main: headtail::main_tail,
    },
    Tool {
        name: "true",
        about: "do nothing, successfully",
        main: truefalse::main_true,
    },
    Tool {
        name: "wc",
        about: "count lines, words and bytes",
        main: wc::main,
    },
    Tool {
        name: "yes",
        about: "print a line over and over",
        main: yes::main,
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    ALL.iter().find(|t| t.name == name)
}
