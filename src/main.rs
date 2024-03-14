use argh::FromArgs;

mod operations;
mod output;

#[derive(Debug, PartialEq)]
enum OPERATION {
    DELETE,
    TRASH,
    RESTORE,
    SHRED,
    LIST,
    PURGE { all_files: bool },
    NONE,
}

//This is really ugly but it works for now.
impl From<&Args> for OPERATION {
    fn from(a: &Args) -> Self {
        if a.list {
            return OPERATION::LIST;
        } else if a.restore {
            return OPERATION::RESTORE;
        } else if a.trash {
            return OPERATION::TRASH;
        } else if a.purge {
            return OPERATION::PURGE {
                all_files: a.files.contains(&"*".to_string()),
            };
        } else if a.delete {
            return OPERATION::DELETE;
        } else if a.shred {
            return OPERATION::SHRED;
        }

        return OPERATION::NONE;
    }
}

///Basic arguments
#[derive(FromArgs)]
struct Args {
    #[argh(switch, short = 't', description = "move a file to the trash bin")]
    trash: bool,
    #[argh(switch, short = 'r', description = "restore a file from the trash bin")]
    restore: bool,
    #[argh(
        switch,
        short = 'p',
        description = "delete a file from the trash. deletes all if '*' is passed"
    )]
    purge: bool,
    #[argh(
        switch,
        short = 'd',
        description = "delete a file permanently without overwriting"
    )]
    delete: bool,
    #[argh(
        switch,
        short = 's',
        description = "shred a file (overwrite and then delete)"
    )]
    shred: bool,

    #[argh(
        option,
        short = 'n',
        description = "number of times to overwrite file when using -s (default=1)",
        default = "1"
    )]
    ow_num: usize,

    #[argh(
        switch,
        short = 'l',
        description = "list all files in the system trash"
    )]
    list: bool,
    #[argh(
        switch,
        short = 'R',
        description = "recurse through directories without user confirmation"
    )]
    recurse: bool,

    #[argh(positional)]
    files: Vec<String>,
}

fn main() {
    let args: Args = argh::from_env();

    match operations::run_operation(OPERATION::from(&args), args) {
        Ok(_) => {}
        Err(e) => eprintln!("{e}"),
    }
}
