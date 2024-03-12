use std::{
    borrow::Cow,
    error::Error,
    fmt::{write, Display},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use colored::Colorize;
use indicatif::ProgressBar;
use trash::{
    os_limited::{self, purge_all},
    TrashItem,
};

use crate::{
    files::{self, path_to_string, run_op_on_dir_recursive, FileErr},
    output::{self, finish_spinner_with_prefix, get_spinner, is_quiet, prompt_recursion},
    util, Args, OPERATION,
};

#[derive(Debug)]
pub struct OperationError {
    pub err: Box<dyn Error>,
    pub operation: OPERATION,
    pub file: Option<String>,
}

pub trait RecursiveOperation {
    fn cb(path: &PathBuf) -> Result<(), FileErr>;
    fn display_cb(&mut self, path: &PathBuf, is_dir: bool);

    fn get_spinner(&self) -> &ProgressBar;
}

impl Error for OperationError {}

impl Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op_string = match self.operation {
            OPERATION::DELETE => "deleting",
            OPERATION::TRASH => "trashing",
            OPERATION::RESTORE => "restoring",
            OPERATION::SHRED { trash_relative } => "shredding",
            OPERATION::PURGE { all_files } => "purging",
            _ => "",
        };
        if self.operation == OPERATION::LIST {
            return write!(
                f,
                "Error while getting trash list: {}",
                self.err.to_string()
            );
        } else {
            let file;
            if self.file.is_some() {
                file = self.file.clone().unwrap();
            } else {
                file = "[no file set]".to_string()
            }
            return write!(
                f,
                "Error while {} {}: {}",
                op_string,
                file,
                self.err.to_string()
            );
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
                Ok(_) => return Ok(()),
                Err(e) => {
                    return Err(OperationError::new(Box::new(e), OPERATION::LIST, None));
                }
            },
            Err(e) => {
                return Err(OperationError::new(Box::new(e), OPERATION::LIST, None));
            }
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
                match files::select_file_from_trash(file) {
                    Some(f) => files.push(f),
                    None => pb.println(format!(
                        "{} {}",
                        file.red(),
                        "did not match any file in the recycle bin".red()
                    )),
                }
            }
        }

        for file in files {
            pb.set_prefix("Purging");
            pb.set_message(file.name.clone());
            purge_all(vec![file]).unwrap();
        }

        finish_spinner_with_prefix(&pb, "Files purged");

        Ok(())
    }

    pub fn trash(args: &Args) -> Result<(), OperationError> {
        let mut files = Vec::<&Path>::new();
        for path in &args.files {
            let p = Path::new(path);
            if p.exists() {
                files.push(p);
            } else {
                println!(
                    "{} {}",
                    path_to_string(path).red(),
                    "does not exist, skipping...".red()
                );
            }
        }

        match trash::delete_all(files) {
            Ok(_) => return Ok(()),
            Err(e) => {
                return Err(OperationError::new(Box::new(e), OPERATION::TRASH, None));
            }
        };
    }
}

pub struct RestoreOperation;

impl RestoreOperation {
    fn operate(args: &Args) -> Result<(), OperationError> {
        let mut files_to_restore = Vec::<String>::new();

        loop {
            let result = Self::attempt_restore(args.files.clone());
            match result {
                Ok(_) => {
                    return Ok(());
                }
                Err(e) => match e {
                    trash::Error::RestoreCollision {
                        path,
                        remaining_items,
                    } => {
                        println!(
                            "{} {}{}",
                            "File already exists at path".red(),
                            util::pathbuf_to_string(&path).unwrap().red(),
                            ", skipping...".red()
                        );

                        files_to_restore = files_to_restore
                            .into_iter()
                            .filter(|f| f != &util::get_file_name(&path).unwrap())
                            .collect();
                        continue;
                    }
                    _ => eprintln!("Failed to restore file with error {}", e),
                },
            }
        }
    }

    fn attempt_restore(files: Vec<String>) -> Result<(), trash::Error> {
        let mut restore_files = Vec::<TrashItem>::new();

        for path in files {
            match files::select_file_from_trash(&path) {
                None => continue,
                Some(t) => {
                    restore_files.push(t.clone());
                }
            }
        }

        os_limited::restore_all(restore_files.clone())?;

        Ok(())
    }
}

pub struct DeleteOperation {
    pb: ProgressBar,
}
impl DeleteOperation {
    fn default() -> DeleteOperation {
        DeleteOperation { pb: get_spinner() }
    }

    fn operate(&mut self, args: &Args) -> Result<(), OperationError> {
        match recurse_op(self, OPERATION::DELETE, args) {
            Ok(c) => {
                finish_spinner_with_prefix(&self.pb, &format!("Removed {c} files"));
                return Ok(());
            }
            Err(e) => {
                self.pb.finish_and_clear();
                return Err(e);
            }
        };
    }
}

impl RecursiveOperation for DeleteOperation {
    fn cb(path: &PathBuf) -> Result<(), FileErr> {
        if path.is_dir() {
            return fs::remove_dir(path).map_err(|e| FileErr::map(e, path));
        }
        return fs::remove_file(path).map_err(|e| FileErr::map(e, path));
    }

    fn display_cb(&mut self, path: &PathBuf, is_dir: bool) {
        let path_name = match util::pathbuf_to_string(path) {
            Some(n) => n,
            None => "[Error converting path to name]".to_string(),
        };

        if !is_dir {
            self.pb.set_prefix("Removing file");
            self.pb.set_message(path_name);
        } else {
            self.pb.set_prefix("Removing directory");
            self.pb.set_message(path_name);
        }
    }

    fn get_spinner(&self) -> &ProgressBar {
        return &self.pb;
    }
}

struct ShredOperation {
    pb: ProgressBar,
}
impl ShredOperation {
    fn default() -> ShredOperation {
        ShredOperation {
            pb: output::get_spinner(),
        }
    }

    fn operate(&mut self, args: &Args, trash_relative: bool) -> Result<(), OperationError> {
        match recurse_op(self, OPERATION::SHRED { trash_relative }, args) {
            Ok(c) => {
                finish_spinner_with_prefix(&self.pb, &format!("Shredded {c} files"));
                return Ok(());
            }
            Err(e) => {
                self.pb.finish_and_clear();
                return Err(e);
            }
        };
    }
}

impl RecursiveOperation for ShredOperation {
    fn cb(path: &PathBuf) -> Result<(), FileErr> {
        if !path.is_dir() {
            let file = OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|e| FileErr::map(e, path))?;
            files::overwrite_file(&file).map_err(|e| FileErr::map(e, path))?;
        }

        files::remove_file_or_dir(path).map_err(|e| FileErr::map(e, path))?;

        Ok(())
    }

    fn display_cb(&mut self, path: &PathBuf, is_dir: bool) {
        let path_name = match util::pathbuf_to_string(path) {
            Some(n) => n,
            None => "[Error converting path to name]".to_string(),
        };

        if !is_dir {
            self.pb.set_prefix("Shredding file");
            self.pb.set_message(path_name);
        } else {
            self.pb.set_prefix("Deleting directory");
            self.pb.set_message(path_name);
        }
    }

    fn get_spinner(&self) -> &ProgressBar {
        return &self.pb;
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
                files::path_to_string(&path).red(),
                "does not exist, skipping".red(),
            ));
            continue;
        }
        if path.is_dir() {
            if args.recurse.is_some_and(|a| a == true) {
                if !prompt_recursion(path.to_str().unwrap().to_string()).unwrap() {
                    continue;
                }
            }
            match files::run_op_on_dir_recursive::<T>(op, path, 0) {
                Ok(c) => counter += c,
                Err(e) => {
                    let file = e.file.clone();
                    return Err(OperationError::new(Box::new(e), op_type, Some(file)));
                }
            };
        } else {
            match T::cb(&PathBuf::from(path)) {
                Ok(c) => counter += 1,
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
            ShredOperation::default().operate(&args, trash_relative)
        }
        OPERATION::NONE => Ok(()),
    }
}
