use std::time::Duration;

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

use crate::files;

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

///This function is, in fact, a war crime. It loops over items three times, and is just generally fucking dumb. It contains magic numbers,
/// unwrap statements that could panic, and other random bullshit. It works though.
fn get_sized_table(items: &Vec<TrashItem>, format: &TableFormat) -> Table {
    //Hypothetical 'desired' table that may not be printed
    let mut table = Table::new();
    let mut longest_row_len: usize = "Name".len() + "Original path".len() + "Time deleted".len();

    table.set_format(*format);

    table.set_titles(row![b->"Name", b->"Original path", b->"Time deleted"]);

    for item in items {
        let len = item.name.len()
            + files::path_to_string(&item.original_path()).len()
            + format_unix_date(item.time_deleted).len();

        if len > longest_row_len {
            longest_row_len = len;
        }
    }

    let term_width: usize = terminal_size().unwrap().0 .0.into();

    //If we're over or close to max width, recreate the table with truncated original path
    //Dumb magic number I know, but it's there to both add padding to the right side and account for the
    //width of seperators and padding. I could calculate that. I won't.
    if longest_row_len + 20 > term_width {
        let over = (longest_row_len + 20) - term_width;

        for item in items {
            table.add_row(row![
                item.name,
                "...".to_string() + &files::path_to_string(&item.original_path())[over..],
                format_unix_date(item.time_deleted)
            ]);
        }
    } else {
        for item in items {
            table.add_row(row![
                item.name,
                files::path_to_string(&item.original_path()),
                format_unix_date(item.time_deleted)
            ]);
        }
    }

    return table;
}

pub fn prompt_recursion(path: String) -> Result<bool, dialoguer::Error> {
    match dialoguer::Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "{} is a directory. Perform operation recursively?",
            path
        ))
        .interact()
    {
        Ok(b) => return Ok(b),
        Err(e) => return Err(e),
    }
}

pub fn file_conflict_prompt(prompt: String, items: Vec<String>) -> usize {
    return Select::with_theme(&ColorfulTheme::default())
        .with_prompt(prompt)
        .items(&items)
        .interact()
        .unwrap();
}

pub fn get_spinner() -> ProgressBar {
    let style = ProgressStyle::default_spinner()
        .tick_chars("✶✸✹✺✹✷✓")
        .template("{spinner:.cyan} {prefix:.bold} {wide_msg} [{elapsed_precise}]")
        .unwrap();

    let pb = ProgressBar::new_spinner()
        .with_style(style)
        .with_finish(ProgressFinish::AndLeave);
    pb.enable_steady_tick(Duration::from_millis(200));
    return pb;
}

//Slightly weird way we have to set the progressbar to avoid an empty prefix adding spaces between the finish char
//Basically, can't use pb.finish_with_message() because that'll leave the prefix (and the spaces around it)
pub fn finish_spinner_with_prefix(pb: &ProgressBar, message: &str) {
    pb.set_message("");
    pb.set_prefix(message.to_string());
    pb.tick();
    pb.finish();
}

pub fn print_success(message: String) {
    println!("{} {}", "✓".cyan(), message.bold())
}

pub fn print_error(output: String) {
    println!("{}", output.as_str().red())
}
