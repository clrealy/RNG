use std::ffi::OsString;

mod cat;
mod echo;
mod truefalse;

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
        name: "cat",
        about: "concatenate files and print them",
        main: cat::main,
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
        name: "true",
        about: "do nothing, successfully",
        main: truefalse::main_true,
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    ALL.iter().find(|t| t.name == name)
}
