use std::{
    error::Error,
    fmt::Display,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

use colored::Colorize;
use indicatif::ProgressBar;
use shred_lib::{
    files::{self, FileErr},
    util, RecursiveOperation,
};
use trash::{
    os_limited::{self, purge_all},
    TrashItem,
};

use crate::{
    output::{self, format_unix_date, run_conflict_prompt},
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
        let mut files: Vec<TrashItem> = Vec::new();

        let pb = output::get_spinner();
        if all_files {
            match os_limited::list() {
                Ok(l) => files = l,
                Err(e) => {
                    return Err(OperationError::new(Box::new(e), OPERATION::LIST, None));
                }
            }
        } else {
            for file in &args.files {
                match files::select_from_trash(file) {
                    Some(i) => files.push(run_conflict_prompt(i)),
                    None => pb.println(format!(
                        "{} {}",
                        file.red(),
                        "did not match any file in the recycle bin".red()
                    )),
                }
            }
        }

        let len = files.len();
        for file in files {
            pb.set_prefix("Purging");
            pb.set_message(file.name.clone());
            purge_all(vec![file]).unwrap();
        }

        output::finish_spinner_with_prefix(&pb, &format!("Purged {} files", len));

        Ok(())
    }

    pub fn trash(args: &Args) -> Result<(), OperationError> {
        let mut files = Vec::<&Path>::new();
        for path in &args.files {
            let p = Path::new(path);
            if p.exists() {
                files.push(p);
            } else {
                output::print_error(format!(
                    "{} does not exist, skipping...",
                    files::path_to_string(path)
                ));
            }
        }

        let len = files.len();
        match trash::delete_all(files) {
            Ok(_) => {
                output::print_success(format!("Trashed {} files", len));
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
        //stores the current list of files that will have attempt_restore() called on them on each loop iteration
        let mut files = args.files.clone();

        loop {
            //must store the length of files in the restore attempt up here before files is moved into attempt_restore (for use in potential success message)
            let curr_file_len = files.len();
            let res_result = Self::attempt_restore(files);
            match res_result {
                Ok(new_files) => {
                    //If there are new files, set the local file variable and run the loop again
                    if new_files.is_some() {
                        files = new_files.unwrap();
                        continue;
                    } else {
                        //If there are no new files, the loop can print a success message and the function can return
                        output::print_success(format!("Restored {} files", curr_file_len));
                        return Ok(());
                    }
                }
                Err(e) => return Err(e),
            }
        }
    }

    ///Attemps to restore the file. Handles any errors that might occur (or paths that don't actually exist in the trash)
    /// Returns a Ok(Some()) modified copy of the input files if changes had to be made, otherwise it returns Ok(None)
    fn attempt_restore(files: Vec<String>) -> Result<Option<Vec<String>>, OperationError> {
        let mut new_files = files.clone();
        for path in files {
            match Self::restore_single(&path) {
                Ok(exists) => {
                    //If the file does not exist, print an error and remove it from the files but do not actually error
                    if !exists {
                        output::print_error(format!("{path} does not exist in trash, skipping..."));
                        util::remove_string_from_vec(&mut new_files, path);
                        continue;
                    }
                }
                Err(e) => match Self::handle_collision(e, &mut new_files, &path) {
                    Ok(s) => {
                        output::print_error(format!(
                            "File already exists at path {}, skipping...",
                            s
                        ));
                    }
                    Err(inner_e) => {
                        return Err(OperationError::new(
                            Box::new(inner_e),
                            OPERATION::RESTORE,
                            Some(path),
                        ))
                    }
                },
            }
        }

        Ok(None)
    }

    ///Attempt to restore a single file, returning Ok(true) if the file existed in the trash, and Ok(false) if the file did not.
    fn restore_single(file: &String) -> Result<bool, trash::Error> {
        let trash_item = files::select_from_trash(file);
        match trash_item {
            Some(i) => {
                os_limited::restore_all([run_conflict_prompt(i)])?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn handle_collision(
        error: trash::Error,
        files: &mut Vec<String>,
        path: &String,
    ) -> Result<String, trash::Error> {
        //RestoreTwins is also technically an error that we could handle in a similar way, but with how this program works its unecessary
        //RestoreTwins requires that the user passes in two files that have the same name (referencing them in another way), but because
        //we do not allow the user to reference two items that could have the same path anyway, it can go unhandled
        match error {
            trash::Error::RestoreCollision {
                path: path_buf,
                remaining_items: _,
            } => {
                //This function modifies a vec reference in place, so theres no need for a return value
                util::remove_first_string_from_vec(files, path.to_string());

                Ok(files::path_to_string(path_buf))
            }
            _ => Err(error),
        }
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
