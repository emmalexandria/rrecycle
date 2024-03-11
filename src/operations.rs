use std::{
    fmt::write,
    fs::{self, OpenOptions},
    io::Write,
    path::{Display, Path, PathBuf},
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
    files::{self, run_on_dir_recursive},
    interface::{self, prompt_recursion},
    util, Args, OPERATION,
};

#[derive(Debug)]
pub enum OperationError {
    PrintTrashList { message: String },
    GetTrashList { message: String },
    TrashFileError { message: String },
    DeleteFileError { message: String },
    RemoveFileError { message: String },
    ShredFileError { message: String },
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationError::GetTrashList { message } => write!(f, "{}", message),
            OperationError::PrintTrashList { message } => write!(f, "{}", message),
            OperationError::TrashFileError { message } => write!(f, "{}", message),
            OperationError::DeleteFileError { message } => write!(f, "{}", message),
            OperationError::RemoveFileError { message } => write!(f, "{}", message),
            OperationError::ShredFileError { message } => write!(f, "{}", message),
        }
    }
}

pub trait RecursiveOperation {
    fn cb(path: &PathBuf) -> std::io::Result<()>;
    fn display_cb(&mut self, path: &PathBuf, is_dir: bool);
}

pub struct ListOperation;

impl ListOperation {
    fn operate() -> Result<(), OperationError> {
        match os_limited::list() {
            Ok(l) => match interface::print_trash_table(l) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    eprintln!("Failed to print trash list with error {}", e);
                    return Err(OperationError::PrintTrashList {
                        message: e.to_string(),
                    });
                }
            },
            Err(e) => {
                eprintln!("Failed to get trash list with error {}", e);
                return Err(OperationError::GetTrashList {
                    message: e.to_string(),
                });
            }
        }
    }
}

pub struct PurgeOperation;

impl PurgeOperation {
    fn operate(args: &Args, all_files: bool) -> Result<(), OperationError> {
        let mut files: Vec<TrashItem> = Vec::new();
        if all_files {
            match os_limited::list() {
                Ok(l) => files = l,
                Err(e) => {
                    return Err(OperationError::GetTrashList {
                        message: e.to_string(),
                    })
                }
            }
        } else {
            for file in &args.files {
                files.push(files::select_file_from_trash(file).unwrap());
            }
        }

        let pb = interface::get_spinner();
        for file in files {
            pb.set_message(format!("Purging {}", file.name));
            purge_all(vec![file]).unwrap();
        }

        pb.finish_with_message("Files purged");

        Ok(())
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

pub struct TrashOperation;

impl RecursiveOperation for TrashOperation {
    fn cb(path: &PathBuf) -> std::io::Result<()> {
        todo!()
    }

    fn display_cb(&mut self, path: &PathBuf, is_dir: bool) {
        todo!()
    }
}

impl TrashOperation {
    fn operate(args: &Args) -> Result<(), OperationError> {
        for file in &args.files {
            let path = Path::new(file);

            match trash::delete(path) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    return Err(OperationError::TrashFileError {
                        message: e.to_string(),
                    })
                }
            };
        }

        Ok(())
    }
}

pub struct DeleteOperation;
impl DeleteOperation {
    fn operate(args: &Args) -> Result<(), OperationError> {
        for file in &args.files {
            let path = Path::new(&file);
            if path.is_dir() {
                if args.recurse.is_some_and(|a| a == true) {
                    if !prompt_recursion(path.to_str().unwrap().to_string()).unwrap() {
                        continue;
                    }
                }
                match run_on_dir_recursive(path, &Self::cb) {
                    Ok(_) => {}
                    Err(e) => {
                        return Err(OperationError::DeleteFileError {
                            message: e.to_string(),
                        })
                    }
                };
            } else {
                match Self::cb(&PathBuf::from(path)) {
                    Ok(_) => {}
                    Err(e) => {
                        return Err(OperationError::DeleteFileError {
                            message: e.to_string(),
                        })
                    }
                };
            }
        }
        Ok(())
    }
}

impl RecursiveOperation for DeleteOperation {
    fn cb(path: &PathBuf) -> std::io::Result<()> {
        if path.is_dir() {
            return fs::remove_dir(path);
        }
        return fs::remove_file(path);
    }

    fn display_cb(&mut self, path: &PathBuf, is_dir: bool) {
        return;
    }
}

struct ShredOperation {
    pb: ProgressBar,
}
impl ShredOperation {
    fn default() -> ShredOperation {
        ShredOperation {
            pb: interface::get_spinner(),
        }
    }

    fn operate(&mut self, args: &Args, _trash_relative: bool) -> Result<(), OperationError> {
        for file in &args.files {
            let path = Path::new(&file);
            if path.is_dir() {
                if args.recurse.is_some_and(|a| a == true) {
                    if !prompt_recursion(path.to_str().unwrap().to_string()).unwrap() {
                        continue;
                    }
                }
                match files::run_op_on_dir_recursive::<Self>(self, path) {
                    Ok(_) => {}
                    Err(e) => {
                        return Err(OperationError::ShredFileError {
                            message: e.to_string(),
                        })
                    }
                };
            } else {
                match Self::cb(&PathBuf::from(path)) {
                    Ok(_) => {}
                    Err(e) => {
                        return Err(OperationError::ShredFileError {
                            message: e.to_string(),
                        })
                    }
                };
            }
        }

        self.pb.finish_with_message("Shredded all files");

        Ok(())
    }
}

impl RecursiveOperation for ShredOperation {
    fn cb(path: &PathBuf) -> std::io::Result<()> {
        if !path.is_dir() {
            let file = OpenOptions::new().write(true).open(path)?;
            files::overwrite_file(&file)?;
        }

        files::remove_file_or_dir(path)?;

        Ok(())
    }

    fn display_cb(&mut self, path: &PathBuf, is_dir: bool) {
        let path_name = match util::pathbuf_to_string(path) {
            Some(n) => n,
            None => "[Error converting path to name]".to_string(),
        };

        if !is_dir {
            self.pb.set_message(format!("Shredding file {}", path_name));
        } else {
            self.pb
                .set_message(format!("Deleting directory {}", path_name));
        }
    }
}

pub fn run_operation(operation: OPERATION, args: Args) -> Result<(), OperationError> {
    match operation {
        OPERATION::RESTORE => RestoreOperation::operate(&args),
        OPERATION::LIST => ListOperation::operate(),
        OPERATION::PURGE { all_files } => PurgeOperation::operate(&args, all_files),
        OPERATION::DELETE => DeleteOperation::operate(&args),
        OPERATION::TRASH => TrashOperation::operate(&args),
        OPERATION::SHRED { trash_relative } => {
            ShredOperation::default().operate(&args, trash_relative)
        }
        OPERATION::NONE => Ok(()),
    }
}
