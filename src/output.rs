use std::{
    borrow::Cow,
    path::{self, Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use colored::Colorize;
use rrc_lib::files;
use terminal_size::terminal_size;

use chrono::{Local, TimeZone};
use dialoguer::{theme::ColorfulTheme, Select};
use indicatif::{ProgressBar, ProgressFinish, ProgressStyle};
use prettytable::{
    cell,
    format::{self, FormatBuilder, TableFormat},
    row, Row, Table,
};
use trash::TrashItem;

use crate::operations::OPERATION;

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

const LONG_DATE_FORMAT: &str = "%H:%M:%S %Y/%m/%d ";

pub fn format_unix_date(time: i64, format: &str) -> String {
    chrono::Local
        .timestamp_opt(time, 0)
        .unwrap()
        .format(format)
        .to_string()
}

pub struct TrashList {
    table: Table,
    max_width: u16,
}

impl Default for TrashList {
    fn default() -> Self {
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
        let mut table = Table::new();
        table.set_format(format);
        table.set_titles(row![b->"Name", b->"Path", b->"Date"]);

        Self {
            table,
            max_width: (terminal_size::terminal_size().unwrap().0 .0) - 20,
        }
    }
}

impl TrashList {
    pub fn set_items(&mut self, items: &Vec<TrashItem>) {
        items.iter().for_each(|i| {
            self.table.add_row(Self::row_from_trash_item(i));
        });
        self.size_table()
    }

    fn row_from_trash_item(item: &TrashItem) -> Row {
        row![
            item.name,
            files::path_to_string(item.original_path()),
            format_unix_date(item.time_deleted, LONG_DATE_FORMAT)
        ]
    }

    pub fn size_table(&mut self) {
        let width = self.calc_table_width();
        if width < self.max_width.into() {
            return;
        }

        let over_width = width - self.max_width as usize;

        self.table
            .row_iter_mut()
            .for_each(|r| Self::truncate_row_path(r, over_width));
    }

    pub fn calc_table_width(&mut self) -> usize {
        let mut width = 0;
        let padding = self.table.get_format().get_padding();
        let total_padding = padding.0 + padding.1;
        self.table
            .row_iter()
            .for_each(|r| width = width.max(self.calculate_row_width(r, total_padding)));

        width
    }

    fn calculate_row_width(&self, row: &Row, padding: usize) -> usize {
        let num_cells = row.len();

        let mut width = 0;
        for i in 0..num_cells {
            width += row.get_cell(i).unwrap().get_content().len();
        }

        // Add one character for each divider (1 divider per cell + additional end divider)
        width += num_cells + 1;
        // Add padding chars for each cell
        width += num_cells * padding;

        width
    }

    fn truncate_row_path(row: &mut Row, over_len: usize) {
        let path = row.get_cell(1).unwrap();
        if path.get_content().len() > over_len {
            let (trunc_path, remaining_len) =
                truncate_path(path.get_content(), path.get_content().len() - over_len);
            row.set_cell(cell!(trunc_path), 1).unwrap();
        }
    }

    pub fn print(&mut self) {
        self.table.printstd();
    }
}

///This function truncates a path to a desired length to the nearest path seperator and prepends '…'. Returns the truncated path and
///the delta between the desired length of the path and its actual length
fn truncate_path(path_string: String, desired_len: usize) -> (String, i64) {
    let mut trunc_path = String::new();

    path_string
        .split_inclusive(path::MAIN_SEPARATOR)
        .into_iter()
        .rev()
        .try_for_each(|c| {
            if trunc_path.len() == 0 {
                trunc_path.push_str(c);
                return Some(());
            }
            let dist_with = desired_len.saturating_sub(trunc_path.len() + c.len());
            let dist_without = desired_len.saturating_sub(trunc_path.len());
            if dist_with < dist_without {
                trunc_path.insert_str(0, c);
                Some(())
            } else {
                None
            }
        });

    if trunc_path.len() < path_string.len() {
        trunc_path.insert(0, '…');
    }

    let remaining_len = <usize as TryInto<i64>>::try_into(desired_len).unwrap()
        - <usize as TryInto<i64>>::try_into(trunc_path.len()).unwrap();
    (trunc_path, remaining_len + 1)
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
                + &format_unix_date(i.time_deleted, LONG_DATE_FORMAT)
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
