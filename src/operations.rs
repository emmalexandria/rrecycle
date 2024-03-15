use std::{
    clone,
    error::Error,
    fmt::Display,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

use clap::ArgMatches;
use colored::Colorize;
use indicatif::ProgressBar;
use rrc_lib::{
    files::{
        self, get_existent_paths, get_existent_trash_items, path_to_string,
        path_vec_from_string_vec,
    },
    util::{self},
    FileErr, RecursiveOperation,
};
use trash::{
    os_limited::{self, purge_all},
    TrashItem,
};

use crate::output::{self, prompt_recursion, OpSpinner};

#[derive(Debug, PartialEq)]
pub enum OPERATION {
    DELETE,
    TRASH,
    RESTORE,
    SHRED,
    LIST,
    PURGE { all_files: bool },
    NONE,
}

#[derive(Debug)]
pub struct OperationError {
    pub err: Box<dyn Error>,
    pub operation: OPERATION,
    pub file: Option<String>,
}

impl Error for OperationError {}

impl Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op_string = self.operation.to_infinitive();
        if self.operation == OPERATION::LIST {
            write!(f, "Error while getting trash list: {}", self.err)
        } else {
            let file;
            if self.file.is_some() {
                file = self.file.clone().unwrap();
            } else {
                file = "[no file set]".to_string()
            }
            write!(f, "Error while {} {}: {}", op_string, file, self.err)
        }
    }
}

impl OperationError {
    pub fn new(err: Box<dyn Error>, operation: OPERATION, file: Option<String>) -> OperationError {
        OperationError {
            err,
            operation,
            file,
        }
    }
}

pub fn run_operation_from_args(args: ArgMatches) -> Result<(), OperationError> {
    let recurse_default = args.get_flag("recurse");
    return match args.subcommand() {
        Some(("trash", m)) => BasicOperations::trash(get_files_from_sub(m)),
        Some(("restore", m)) => RestoreOperation::operate(get_files_from_sub(m)),
        Some(("delete", m)) => {
            DeleteOperation::default().operate(get_files_from_sub(m), recurse_default)
        }
        Some(("purge", m)) => BasicOperations::purge(get_files_from_sub(m), m.get_flag("all")),
        Some(("shred", m)) => ShredOperation::default(*m.get_one("ow_runs").unwrap())
            .operate(get_files_from_sub(m), recurse_default),
        Some(("list", _)) => BasicOperations::list(),
        _ => Ok(()),
    };
}

fn get_files_from_sub(args: &ArgMatches) -> Vec<String> {
    args.get_many::<String>("files")
        .map(|vals| vals.collect::<Vec<_>>())
        .unwrap_or_default()
        .iter()
        .map(|v| v.to_string())
        .collect()
}

///Operations which don't recurse over the directory tree while printing output
struct BasicOperations;
impl BasicOperations {
    pub fn list() -> Result<(), OperationError> {
        match os_limited::list() {
            Ok(l) => match output::print_trash_table(l) {
                Ok(_) => Ok(()),
                Err(e) => Err(OperationError::new(Box::new(e), OPERATION::LIST, None)),
            },
            Err(e) => Err(OperationError::new(Box::new(e), OPERATION::LIST, None)),
        }
    }
    pub fn purge(files: Vec<String>, all_files: bool) -> Result<(), OperationError> {
        let items: Vec<TrashItem>;
        let pb = OpSpinner::default(OPERATION::PURGE { all_files });

        if all_files {
            match os_limited::list() {
                Ok(l) => items = l,
                Err(e) => {
                    return Err(OperationError::new(Box::new(e), OPERATION::LIST, None));
                }
            }
        } else {
            items = get_existent_trash_items(&files, output::run_conflict_prompt, |f| {
                pb.print_no_file_warn(f);
            })
        }

        for file in &items {
            pb.set_file_str(file.name.clone());
            purge_all(vec![file]).unwrap();
        }

        pb.auto_finish(items.len());
        Ok(())
    }

    pub fn trash(files: Vec<String>) -> Result<(), OperationError> {
        let pb = OpSpinner::default(OPERATION::TRASH);

        let filtered_path_strings = files::get_existent_paths(&files, |s| {
            pb.print_no_file_warn(s);
        });
        let paths = files::path_vec_from_string_vec(filtered_path_strings);
        let len = paths.len();

        pb.start();

        for path in paths {
            pb.set_file_path(path);
            match trash::delete_all([path]) {
                Ok(_) => {}
                Err(e) => {
                    return Err(OperationError::new(
                        Box::new(e),
                        OPERATION::TRASH,
                        Some(files::path_to_string(path)),
                    ))
                }
            }
        }

        pb.auto_finish(len);

        Ok(())
    }
}

struct RestoreOperation;

impl RestoreOperation {
    fn operate(files: Vec<String>) -> Result<(), OperationError> {
        let pb = OpSpinner::default(OPERATION::RESTORE);

        let mut items = get_existent_trash_items(&files, output::run_conflict_prompt, |f| {
            pb.print_no_file_warn(f);
        });

        if items.len() > 1 {
            pb.start();
        }

        loop {
            let len_before_attempt = items.len();
            let res = Self::attempt_restore(&mut items, &pb);
            match res {
                Ok(s) => {
                    if s {
                        pb.auto_finish(len_before_attempt);
                        return Ok(());
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }

    ///Attemps to restore the file. Handles any errors that might occur (or paths that don't actually exist in the trash)
    /// Returns a Ok(Some()) modified copy of the input files if changes had to be made, otherwise it returns Ok(None)
    fn attempt_restore(
        mut files: &mut Vec<TrashItem>,
        pb: &OpSpinner,
    ) -> Result<bool, OperationError> {
        for file in files.clone() {
            pb.set_file_str(file.name.clone());
            match trash::os_limited::restore_all([file.clone()]) {
                Ok(_) => util::remove_from_vec(&mut files, &file),
                Err(e) => {
                    util::handle_collision_item(e, &mut files, &file).map_err(|err| {
                        OperationError::new(Box::new(err), OPERATION::RESTORE, None)
                    })?;
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }
}

//This operation could technically be performed without the whole recursion shtick, but for reasons of output niceness it'll recurse. Deleting files is so
//fast that I doubt the performance hit will matter
struct DeleteOperation {
    pb: OpSpinner,
}
impl DeleteOperation {
    fn default() -> DeleteOperation {
        DeleteOperation {
            pb: OpSpinner::default(OPERATION::DELETE),
        }
    }

    fn operate(&mut self, files: Vec<String>, recurse_default: bool) -> Result<(), OperationError> {
        let string_paths = get_existent_paths(&files, |f| self.pb.print_no_file_warn(f.as_str()));

        let paths = path_vec_from_string_vec(string_paths);
        let recurse = check_recursion(&paths, recurse_default);

        self.pb.start();

        match rrc_lib::run_recursive_op(self, paths, recurse) {
            Ok(c) => {
                self.pb.auto_finish(c);
                Ok(())
            }
            Err(e) => {
                self.pb.finish();
                let file = e.file.clone();
                Err(OperationError::new(
                    Box::new(e),
                    OPERATION::DELETE,
                    Some(file),
                ))
            }
        }
    }
}

impl RecursiveOperation for DeleteOperation {
    fn cb(&self, path: &PathBuf) -> Result<(), FileErr> {
        if path.is_dir() {
            return fs::remove_dir(path).map_err(|e| FileErr::map(e, path));
        }
        fs::remove_file(path).map_err(|e| FileErr::map(e, path))
    }

    fn display_cb(&mut self, path: &PathBuf, is_dir: bool) {
        let path_name = files::path_to_string(path);

        self.pb.set_file_str(path_name);
    }
}

struct ShredOperation {
    pb: OpSpinner,
    num_runs: usize,
}
impl ShredOperation {
    fn default(num_runs: usize) -> ShredOperation {
        ShredOperation {
            pb: OpSpinner::default(OPERATION::SHRED),
            num_runs,
        }
    }

    fn operate(&mut self, files: Vec<String>, recurse_default: bool) -> Result<(), OperationError> {
        let string_paths = get_existent_paths(&files, |f| self.pb.print_no_file_warn(f));

        let paths = path_vec_from_string_vec(string_paths);
        let recurse = check_recursion(&paths, recurse_default);

        self.pb.start();

        match rrc_lib::run_recursive_op(self, paths, recurse) {
            Ok(c) => {
                self.pb.auto_finish(c);
                Ok(())
            }
            Err(e) => {
                self.pb.finish();
                let file = e.file.clone();
                Err(OperationError::new(
                    Box::new(e),
                    OPERATION::SHRED,
                    Some(file),
                ))
            }
        }
    }
}

impl RecursiveOperation for ShredOperation {
    fn cb(&self, path: &PathBuf) -> Result<(), FileErr> {
        if !path.is_dir() {
            let mut file = OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|e| FileErr::map(e, path))?;
            files::overwrite_file(&mut file, self.num_runs).map_err(|e| FileErr::map(e, path))?;
        }

        files::remove_file_or_dir(path).map_err(|e| FileErr::map(e, path))?;

        Ok(())
    }

    fn display_cb(&mut self, path: &PathBuf, is_dir: bool) {
        let path_name = files::path_to_string(path);
        self.pb.set_file_str(path_name);
    }
}

fn check_recursion<'a>(paths: &Vec<&Path>, recurse_default: bool) -> bool {
    if !recurse_default {
        for path in paths {
            if path.is_dir() {
                return prompt_recursion(path_to_string(path)).is_ok_and(|v| v == true);
            }
        }
    }
    return recurse_default;
}
