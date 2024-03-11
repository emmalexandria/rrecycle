use std::{
    path::{self, Path},
    process::ExitCode,
};

use argh::FromArgs;

mod files;
mod interface;
mod operations;
mod util;

enum OPERATION {
    DELETE,
    TRASH,
    RESTORE,
    SHRED { trash_relative: bool },
    LIST,
    PURGE { all_files: bool },
    NONE,
}

impl OPERATION {
    pub fn from_args(args: &Args) -> OPERATION {
        if args.list.is_some() {
            return OPERATION::LIST;
        }
        if args.purge.is_some() {
            return OPERATION::PURGE {
                all_files: args.files.contains(&"*".to_string()),
            };
        }

        if args.files.len() == 0 {
            return OPERATION::NONE;
        }

        if args.restore.is_some() {
            return OPERATION::RESTORE;
        }
        if args.delete.is_some() {
            return OPERATION::DELETE;
        }
        if args.trash.is_some() {
            return OPERATION::TRASH;
        }
        if args.shred.is_some() {
            return OPERATION::SHRED {
                trash_relative: args.trash.is_some(),
            };
        }

        return OPERATION::NONE;
    }
}

#[derive(FromArgs)]
///Basic arguments
struct Args {
    #[argh(switch, short = 't', description = "move a file to the trash bin")]
    trash: Option<bool>,
    #[argh(switch, short = 'r', description = "restore a file from the trash bin")]
    restore: Option<bool>,
    #[argh(
        switch,
        short = 'p',
        description = "delete a file from the trash bin. deletes all if '*' is passed"
    )]
    purge: Option<bool>,
    #[argh(
        switch,
        short = 'd',
        description = "delete a file permanently without overwriting"
    )]
    delete: Option<bool>,
    #[argh(
        switch,
        short = 's',
        description = "shred a file (overwrite and then delete). can be combined with -p to shred a file in the trash bin"
    )]
    shred: Option<bool>,
    #[argh(
        switch,
        short = 'l',
        description = "list all files in the system trash"
    )]
    list: Option<bool>,
    #[argh(
        switch,
        short = 'R',
        description = "recurse through directories without user confirmation"
    )]
    recurse: Option<bool>,
    #[argh(
        switch,
        short = 'q',
        description = "turn off nearly all output besides recursion prompts"
    )]
    quiet: Option<bool>,

    #[argh(positional)]
    files: Vec<String>,
}

fn main() {
    let args: Args = argh::from_env();

    match operations::run_operation(OPERATION::from_args(&args), args) {
        Ok(_) => {}
        Err(e) => eprintln!("Encountered error: {}", e),
    }
}
