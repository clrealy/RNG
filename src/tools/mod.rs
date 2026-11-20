use std::ffi::OsString;

mod basename;
mod cat;
mod chmod;
mod chown;
mod cp;
mod cut;
mod echo;
mod headtail;
mod ln;
mod ls;
mod mkdir;
mod pwd;
mod rm;
mod sort;
mod touch;
mod tr;
mod truefalse;
mod uniq;
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
        name: "chgrp",
        about: "change group ownership",
        main: chown::main_chgrp,
    },
    Tool {
        name: "chmod",
        about: "change file mode bits",
        main: chmod::main,
    },
    Tool {
        name: "chown",
        about: "change file owner and group",
        main: chown::main_chown,
    },
    Tool {
        name: "cp",
        about: "copy files and directories",
        main: cp::main_cp,
    },
    Tool {
        name: "cut",
        about: "print selected parts of lines",
        main: cut::main,
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
        name: "ln",
        about: "make links between files",
        main: ln::main,
    },
    Tool {
        name: "ls",
        about: "list directory contents",
        main: ls::main,
    },
    Tool {
        name: "mkdir",
        about: "make directories",
        main: mkdir::main,
    },
    Tool {
        name: "mv",
        about: "move (rename) files",
        main: cp::main_mv,
    },
    Tool {
        name: "pwd",
        about: "print the current directory",
        main: pwd::main,
    },
    Tool {
        name: "rm",
        about: "remove files or directories (rm -rf / says Bye)",
        main: rm::main,
    },
    Tool {
        name: "sort",
        about: "sort lines of text",
        main: sort::main,
    },
    Tool {
        name: "tail",
        about: "print the last part of files",
        main: headtail::main_tail,
    },
    Tool {
        name: "touch",
        about: "change file timestamps",
        main: touch::main,
    },
    Tool {
        name: "tr",
        about: "translate or delete characters",
        main: tr::main,
    },
    Tool {
        name: "true",
        about: "do nothing, successfully",
        main: truefalse::main_true,
    },
    Tool {
        name: "uniq",
        about: "report or omit repeated lines",
        main: uniq::main,
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
