use std::{
    error::Error,
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
    files::{self, run_on_dir_recursive, FileErr},
    interface::{self, finish_spinner_with_prefix, prompt_recursion},
    util, Args, OPERATION,
};

#[derive(Debug)]
pub enum OperationError {
    PrintTrashList { message: String },
    GetTrashList { message: String },
    TrashFileError { message: String, file: String },
    DeleteFileError { message: String, file: String },
    PurgeFileError { message: String, file: String },
    ShredFileError { message: String, file: String },
    RestoreFileError { message: String, file: String },
    None,
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationError::GetTrashList { message } => write!(f, "{}", message),
            OperationError::PrintTrashList { message } => write!(f, "{}", message),
            OperationError::TrashFileError { message, file } => {
                write!(f, "{} (Path: {})", message, file)
            }
            OperationError::DeleteFileError { message, file } => write!(f, "{}", message),
            OperationError::PurgeFileError { message, file } => write!(f, "{}", message),
            OperationError::ShredFileError { message, file } => write!(f, "{}", message),
            OperationError::RestoreFileError { message, file } => write!(f, "{}", message),
            OperationError::None => write!(f, ""),
        }
    }
}

impl Error for OperationError {}

impl OperationError {
    pub fn from_operation(
        operation: OPERATION,
        file: String,
        err: Box<dyn Error>,
    ) -> OperationError {
        match operation {
            OPERATION::DELETE => OperationError::DeleteFileError {
                message: "Couldn't delete file".to_string(),
                file,
            },
            OPERATION::TRASH => OperationError::TrashFileError {
                message: "Couldn't trash file".to_string(),
                file,
            },
            OPERATION::RESTORE => OperationError::RestoreFileError {
                message: "Couldn't restore file".to_string(),
                file,
            },
            OPERATION::SHRED { trash_relative } => OperationError::ShredFileError {
                message: "Couldn't shred file".to_string(),
                file,
            },
            OPERATION::LIST => OperationError::PrintTrashList {
                message: "Couldn't print trash list".to_string(),
            },
            OPERATION::PURGE { all_files } => OperationError::PurgeFileError {
                message: "Couldn't purge file".to_string(),
                file,
            },
            OPERATION::NONE => OperationError::None,
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

        let pb = interface::get_spinner();
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
                match files::select_file_from_trash(file) {
                    Some(f) => files.push(f),
                    None => pb.println(format!("{file} did not match any file in the recycle bin")),
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
                        file: path.to_string_lossy().to_string(),
                    })
                }
            };
        }

        Ok(())
    }
}

pub struct DeleteOperation;
impl DeleteOperation {
    fn default() -> DeleteOperation {
        DeleteOperation {}
    }

    fn operate(&mut self, args: &Args) -> Result<(), OperationError> {
        match recurse_op(self, OPERATION::DELETE, args) {
            Ok(_) => {
                return Ok(());
            }
            Err(e) => return Err(e),
        };
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

    fn display_cb(&mut self, _path: &PathBuf, _is_dir: bool) {
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

    fn operate(&mut self, args: &Args, trash_relative: bool) -> Result<(), OperationError> {
        match recurse_op(self, OPERATION::SHRED { trash_relative }, args) {
            Ok(_) => {
                finish_spinner_with_prefix(&self.pb, "Files shredded");
                return Ok(());
            }
            Err(e) => return Err(e),
        };
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
            self.pb.set_prefix("Shredding file");
            self.pb.set_message(path_name);
        } else {
            self.pb.set_prefix("Deleting directory");
            self.pb.set_message(path_name);
        }
    }
}

fn recurse_op<T>(mut op: &mut T, op_type: OPERATION, args: &Args) -> Result<(), OperationError>
where
    T: RecursiveOperation,
{
    for file in &args.files {
        let path = Path::new(&file);
        if path.is_dir() {
            if args.recurse.is_some_and(|a| a == true) {
                if !prompt_recursion(path.to_str().unwrap().to_string()).unwrap() {
                    continue;
                }
            }
            match files::run_op_on_dir_recursive::<T>(op, path, 0) {
                Ok(_) => {}
                Err(e) => {
                    return Err(OperationError::from_operation(
                        op_type,
                        e.file.clone(),
                        Box::new(e),
                    ))
                }
            };
        } else {
            match T::cb(&PathBuf::from(path)) {
                Ok(_) => {}
                Err(e) => {
                    return Err(OperationError::from_operation(
                        op_type,
                        files::path_to_string(&path),
                        Box::new(e),
                    ))
                }
            };
        }
    }

    Ok(())
}

pub fn run_operation(operation: OPERATION, args: Args) -> Result<(), OperationError> {
    match operation {
        OPERATION::RESTORE => RestoreOperation::operate(&args),
        OPERATION::LIST => ListOperation::operate(),
        OPERATION::PURGE { all_files } => PurgeOperation::operate(&args, all_files),
        OPERATION::DELETE => DeleteOperation::default().operate(&args),
        OPERATION::TRASH => TrashOperation::operate(&args),
        OPERATION::SHRED { trash_relative } => {
            ShredOperation::default().operate(&args, trash_relative)
        }
        OPERATION::NONE => Ok(()),
    }
}
