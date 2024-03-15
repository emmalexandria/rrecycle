use clap::{
    arg,
    builder::{BoolishValueParser, ValueParser},
    command, value_parser, Arg, ArgMatches,
};
use colored::Colorize;
use rrc_lib::files;

mod operations;
mod output;

fn main() {
    let help_template = format!(
        "{}{} - {}{}{}{}{}{}{}",
        "{before-help}",
        "{name} {version}".bold().green(),
        "{author-with-newline}".italic(),
        "{about-with-newline}",
        "{usage} \n",
        "\nSubcommands\n".bold().green(),
        "{subcommands}".normal(),
        "\nOptions\n".bold().green(),
        "{options}"
    );

    let files_arg = arg!(files: [files])
        .num_args(1..)
        .value_parser(value_parser!(String));

    let matches = command!()
        .help_template(help_template)
        .subcommand_required(true)
        .subcommand(
            command!("trash")
                .short_flag('t')
                .about("Move files to the recycle bin")
                .arg(files_arg.clone()),
        )
        .subcommand(
            command!("restore")
                .short_flag('r')
                .about("Restore files from the recycle bin")
                .arg(files_arg.clone()),
        )
        .subcommand(
            command!("purge")
                .short_flag('p')
                .about("Remove files from the recycle bin")
                .arg(arg!(all: -a --all))
                .arg(files_arg.clone()),
        )
        .subcommand(
            command!("delete")
                .short_flag('d')
                .about("Delete files permanently")
                .arg(files_arg.clone()),
        )
        .subcommand(
            command!("shred")
                .short_flag('s')
                .about("Securely delete files by overwriting them first")
                .arg(
                    arg!(ow_runs: -n --overwrite_runs <VALUE>)
                        .default_value("1")
                        .value_parser(value_parser!(usize)),
                )
                .arg(files_arg.clone()),
        )
        .subcommand(
            command!("list")
                .short_flag('l')
                .about("List files in the recycle bin"),
        )
        .arg(arg!(recurse: -R --recurse "Run delete and shred on directories without a prompt"))
        .get_matches_from(wild::args());

    match operations::run_operation_from_args(matches) {
        Ok(_) => {}
        Err(e) => eprintln!("{e}"),
    }
}
