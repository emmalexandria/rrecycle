use std::{
    error::Error,
    fmt::Display,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

use colored::Colorize;
use indicatif::ProgressBar;
use shred_lib::{
    files::{self, get_existent_trash_items, FileErr},
    util, RecursiveOperation,
};
use trash::{
    os_limited::{self, purge_all},
    TrashItem,
};

use crate::{
    output::{self, print_success, run_conflict_prompt},
    Args, OPERATION,
};

#[derive(Debug)]
pub struct OperationError {
    pub err: Box<dyn Error>,
    pub operation: OPERATION,
    pub file: Option<String>,
}

impl Error for OperationError {}

impl Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op_string = match self.operation {
            OPERATION::DELETE => "deleting",
            OPERATION::TRASH => "trashing",
            OPERATION::RESTORE => "restoring",
            OPERATION::SHRED { trash_relative: _ } => "shredding",
            OPERATION::PURGE { all_files: _ } => "purging",
            _ => "",
        };
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

pub struct BasicOperations;
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
    pub fn purge(args: &Args, all_files: bool) -> Result<(), OperationError> {
        let files: Vec<TrashItem>;
        let pb = output::get_spinner();

        if all_files {
            match os_limited::list() {
                Ok(l) => files = l,
                Err(e) => {
                    return Err(OperationError::new(Box::new(e), OPERATION::LIST, None));
                }
            }
        } else {
            files = get_existent_trash_items(&args.files, output::run_conflict_prompt, |f| {
                pb.println(format!(
                    "{} {}",
                    f,
                    "did not match any file in the recycle bin, skipping...".red()
                ))
            })
        }

        for file in &files {
            pb.set_prefix("Purging");
            pb.set_message(file.name.clone());
            purge_all(vec![file]).unwrap();
        }

        output::finish_spinner_with_prefix(&pb, &format!("Purged {} files", files.len()));

        Ok(())
    }

    pub fn trash(args: &Args) -> Result<(), OperationError> {
        let filtered_path_strings = files::get_existent_paths(&args.files, |s| {
            output::print_error(format!("{} does not exist, skipping...", s));
        });

        let paths = files::path_vec_from_string_vec(filtered_path_strings);
        match trash::delete_all(&paths) {
            Ok(_) => {
                output::print_success(format!("Trashed {} files", paths.len()));
                Ok(())
            }
            Err(e) => Err(OperationError::new(Box::new(e), OPERATION::TRASH, None)),
        }
    }
}

pub struct RestoreOperation;

//This was more complicated to implement than I expected, so there'll be a lot of comments
impl RestoreOperation {
    fn operate(args: &Args) -> Result<(), OperationError> {
        let mut items = get_existent_trash_items(&args.files, output::run_conflict_prompt, |f| {
            output::print_error(format!(
                "{} {}",
                f,
                "did not match any file in the recycle bin, skipping...".red()
            ))
        });

        loop {
            let res = Self::attempt_restore(&mut items);
            match res {
                Ok(s) => {
                    if s {
                        print_success(format!("Restored {} files", items.len()))
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
    fn attempt_restore(mut files: &mut Vec<TrashItem>) -> Result<bool, OperationError> {
        for file in files.clone() {
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

pub struct DeleteOperation {
    pb: ProgressBar,
}
impl DeleteOperation {
    fn default() -> DeleteOperation {
        DeleteOperation {
            pb: output::get_spinner(),
        }
    }

    fn operate(&mut self, args: &Args) -> Result<(), OperationError> {
        match recurse_op(self, OPERATION::DELETE, args) {
            Ok(c) => {
                output::finish_spinner_with_prefix(&self.pb, &format!("Removed {c} files"));
                Ok(())
            }
            Err(e) => {
                self.pb.finish_and_clear();
                Err(e)
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

        if !is_dir {
            self.pb.set_prefix("Removing file");
            self.pb.set_message(path_name);
        } else {
            self.pb.set_prefix("Removing directory");
            self.pb.set_message(path_name);
        }
    }

    fn get_spinner(&self) -> &ProgressBar {
        &self.pb
    }
}

struct ShredOperation {
    pb: ProgressBar,
    num_runs: usize,
}
impl ShredOperation {
    fn default(args: &Args) -> ShredOperation {
        ShredOperation {
            pb: output::get_spinner(),
            num_runs: args.ow_num,
        }
    }

    fn operate(&mut self, args: &Args, trash_relative: bool) -> Result<(), OperationError> {
        match recurse_op(self, OPERATION::SHRED { trash_relative }, args) {
            Ok(c) => {
                output::finish_spinner_with_prefix(&self.pb, &format!("Shredded {c} files"));
                Ok(())
            }
            Err(e) => {
                self.pb.finish_and_clear();
                Err(e)
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

        if !is_dir {
            self.pb.set_prefix("Shredding file");
            self.pb.set_message(path_name);
        } else {
            self.pb.set_prefix("Deleting directory");
            self.pb.set_message(path_name);
        }
    }

    fn get_spinner(&self) -> &ProgressBar {
        &self.pb
    }
}

fn recurse_op<T>(op: &mut T, op_type: OPERATION, args: &Args) -> Result<u64, OperationError>
where
    T: RecursiveOperation,
{
    let mut counter: u64 = 0;

    for file in &args.files {
        let path = Path::new(&file);
        if !path.exists() {
            op.get_spinner().println(format!(
                "{} {}",
                files::path_to_string(path).red(),
                "does not exist, skipping".red(),
            ));
            continue;
        }
        if path.is_dir() {
            if args.recurse.is_some_and(|a| a)
                && !output::prompt_recursion(path.to_str().unwrap().to_string()).unwrap()
            {
                continue;
            }
            match files::run_op_on_dir_recursive::<T>(op, path, 0) {
                Ok(c) => counter += c,
                Err(e) => {
                    let file = e.file.clone();
                    return Err(OperationError::new(Box::new(e), op_type, Some(file)));
                }
            };
        } else {
            op.display_cb(&PathBuf::from(path), false);
            match op.cb(&PathBuf::from(path)) {
                Ok(_) => counter += 1,
                Err(e) => {
                    let file = e.file.clone();
                    return Err(OperationError::new(Box::new(e), op_type, Some(file)));
                }
            };
        }
    }

    Ok(counter)
}

pub fn run_operation(operation: OPERATION, args: Args) -> Result<(), OperationError> {
    match operation {
        OPERATION::RESTORE => RestoreOperation::operate(&args),
        OPERATION::LIST => BasicOperations::list(),
        OPERATION::PURGE { all_files } => BasicOperations::purge(&args, all_files),
        OPERATION::DELETE => DeleteOperation::default().operate(&args),
        OPERATION::TRASH => BasicOperations::trash(&args),
        OPERATION::SHRED { trash_relative } => {
            ShredOperation::default(&args).operate(&args, trash_relative)
        }
        OPERATION::NONE => Ok(()),
    }
}
