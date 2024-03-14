use std::{
    borrow::Cow,
    path::{self, Path, PathBuf},
    time::Duration,
};

use colored::Colorize;
use terminal_size::terminal_size;

use chrono::TimeZone;
use dialoguer::{theme::ColorfulTheme, Select};
use indicatif::{ProgressBar, ProgressFinish, ProgressStyle};
use prettytable::{
    format::{self, FormatBuilder, TableFormat},
    row, Table,
};
use trash::TrashItem;

use rrc_lib::files::{self};

use crate::OPERATION;

impl OPERATION {
    pub fn to_infinitive(&self) -> String {
        match self {
            OPERATION::DELETE => "deleting",
            OPERATION::TRASH => "trashing",
            OPERATION::RESTORE => "restoring",
            OPERATION::SHRED => "shredding",
            OPERATION::LIST => "listing",
            OPERATION::PURGE { all_files } => "purging",
            OPERATION::NONE => "",
        }
        .into()
    }
    pub fn to_past(&self) -> String {
        match self {
            OPERATION::DELETE => "deleted",
            OPERATION::TRASH => "trashed",
            OPERATION::RESTORE => "restored",
            OPERATION::SHRED => "shredded",
            OPERATION::LIST => "listed",
            OPERATION::PURGE { all_files } => "purged",
            OPERATION::NONE => "",
        }
        .into()
    }
}

pub fn format_unix_date(time: i64) -> String {
    chrono::Local
        .timestamp_opt(time, 0)
        .unwrap()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

///Just don't look at any code involved in printing the list table.
pub fn print_trash_table(items: Vec<TrashItem>) -> std::io::Result<()> {
    let format = FormatBuilder::new()
        .column_separator('│')
        .borders('│')
        .separator(
            format::LinePosition::Top,
            format::LineSeparator::new('─', '┬', '┌', '┐'),
        )
        .separator(
            format::LinePosition::Bottom,
            format::LineSeparator::new('─', '┴', '└', '┘'),
        )
        .separator(
            format::LinePosition::Intern,
            format::LineSeparator::new('─', '┼', '├', '┤'),
        )
        .padding(1, 1)
        .build();

    let table = get_sized_table(&items, &format);

    table.printstd();

    Ok(())
}

fn get_sized_table(items: &Vec<TrashItem>, format: &TableFormat) -> Table {
    //Hypothetical 'desired' table that may not be printed
    let mut table = Table::new();

    table.set_format(*format);
    table.set_titles(row![b->"Name", b->"Original path", b->"Time deleted"]);

    let title_len = "Name".len() + "Original path".len() + "Time deleted".len();
    let len = items.iter().fold(0, |m, v| {
        (v.name.len()
            + files::path_to_string(&v.original_path()).len()
            + format_unix_date(v.time_deleted).len())
        .max(m)
    });

    let max_len = len.max(title_len);
    let term_width: usize = terminal_size().unwrap().0 .0.into();
    let path_start;

    if max_len + 20 > term_width {
        path_start = (max_len + 20) - term_width
    } else {
        path_start = 0
    }
    //If we're over or close to max width, recreate the table with truncated original path
    //Dumb magic number I know, but it's there to both add padding to the right side and account for the
    //width of seperators and padding. I could calculate that. I won't.
    for item in items {
        table.add_row(row![
            item.name,
            truncate_path(&item.original_path(), path_start),
            format_unix_date(item.time_deleted)
        ]);
    }

    table
}

fn truncate_path(path: &PathBuf, len: usize) -> String {
    let str = files::path_to_string(path);
    if len > 0 {
        return "…".to_string() + &str[len..];
    }
    str
}

pub fn prompt_recursion(path: String) -> Result<bool, dialoguer::Error> {
    dialoguer::Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "{} is a directory. Perform operation recursively?",
            path
        ))
        .interact()
}

pub fn run_conflict_prompt(items: Vec<TrashItem>) -> TrashItem {
    if items.len() == 1 {
        return items[0].clone();
    }

    let item_names: Vec<String> = items
        .iter()
        .map(|i| {
            i.original_path().to_str().unwrap().to_string()
                + " | "
                + &format_unix_date(i.time_deleted)
        })
        .collect();

    let selection = file_conflict_prompt(
        "Please select which file to operate on.".to_string(),
        item_names,
    );

    return items[selection].clone();
}

pub fn file_conflict_prompt(prompt: String, items: Vec<String>) -> usize {
    return Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(&items)
        .interact()
        .unwrap();
}
pub fn print_success(message: String) {
    println!("{} {}", "✔".green(), message.bold())
}

pub fn print_error(output: String) {
    println!("{}", output.as_str().red().bold())
}

pub fn print_warn(output: String) {
    println!("{}", output.as_str().yellow())
}

///Capitalises the first letter of any valid ASCII string
//Better to pull in a crate for this kind of thing usually, but our usecase is very constrained so it's unecessary
fn capitalise_ascii<S: AsRef<str>>(s: S) -> String {
    let mut c = s.as_ref().chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
pub struct OpSpinner {
    pb: ProgressBar,
    op: OPERATION,
}

impl OpSpinner {
    pub fn default(op: OPERATION) -> Self {
        let style = ProgressStyle::default_spinner()
            .tick_chars("✶✸✹✺✹✷")
            .template("{spinner:.green} {prefix:.bold} {wide_msg} [{elapsed_precise}]")
            .unwrap();

        let pb = ProgressBar::new_spinner()
            .with_style(style)
            .with_finish(ProgressFinish::AndClear);
        Self { pb, op }
    }

    pub fn start(&self) {
        self.pb
            .set_prefix(capitalise_ascii(self.op.to_infinitive()));
        self.pb.enable_steady_tick(Duration::from_millis(150))
    }

    pub fn set_file_str<'a, S>(&self, file: S)
    where
        S: Into<Cow<'static, str>>,
    {
        self.pb.set_message(file)
    }

    pub fn set_file_path<P: AsRef<Path>>(&self, path: P) {
        self.pb.set_message(files::path_to_string(path))
    }

    pub fn print_error_msg<S: Colorize>(&self, msg: S) {
        self.pb.println(format!("{}", msg.red().bold()))
    }

    pub fn print_warn_msg<S: Colorize>(&self, msg: S) {
        self.pb.println(format!("{}", msg.yellow()))
    }

    //Files are filtered before the progress bar gets ticked, so this has to be a normal println
    pub fn print_no_file_warn<S: AsRef<str>>(&self, file: S) {
        self.print_warn_msg(format!("{} does not exist, skipping...", file.as_ref()).as_str())
    }

    pub fn auto_finish(&self, n: usize) {
        self.finish();
        let op_string = capitalise_ascii(self.op.to_past());
        if n == 0 {
            Self::print_no_op(format!("{} no files", op_string).as_str());
        } else if n == 1 {
            Self::print_success(format!("{} 1 file", op_string).as_str());
        } else {
            Self::print_success(format!("{} {} files", op_string, n).as_str());
        }
    }

    fn print_success<T: Colorize>(msg: T) {
        println!("{} {}", "✔".green(), msg.bold())
    }

    fn print_no_op<T: Colorize>(msg: T) {
        println!("{}", msg.bold())
    }

    pub fn finish(&self) {
        self.pb.finish_and_clear();
    }
}
