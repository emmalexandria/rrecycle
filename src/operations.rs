use core::num;
use std::{
    default,
    error::Error,
    fmt::Display,
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use clap::{builder::Str, ArgMatches};

use fuzzy_search::distance::levenshtein;
use rrc_lib::{
    files::{
        self, get_existent_paths, get_existent_trash_items, path_to_string,
        path_vec_from_string_vec, trash_items_from_names, trash_items_to_names,
    },
    util, FileErr, RecursiveCallback,
};
use trash::{
    os_limited::{self, purge_all},
    TrashItem,
};

use crate::output::{self, prompt_recursion, OpSpinner, TrashList};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OPERATION {
    DELETE,
    TRASH,
    RESTORE,
    SHRED { num_runs: usize },
    LIST,
    PURGE { all_files: bool },
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
        Some(("trash", m)) => TrashOperation::trash(get_files_from_sub(m)),
        Some(("restore", m)) => RestoreOperation::operate(get_files_from_sub(m)),
        Some(("delete", m)) => {
            DeleteOperation::default().operate(get_files_from_sub(m), recurse_default)
        }
        Some(("purge", m)) => BasicOperations::purge(get_files_from_sub(m), m.get_flag("all")),
        Some(("shred", m)) => ShredOperation::new(*m.get_one("ow_runs").unwrap())
            .operate(get_files_from_sub(m), recurse_default),
        Some(("search", m)) => SearchOperation::new(
            m.get_one("command").unwrap(),
            m.get_one("target").unwrap(),
            m.get_one("dir").unwrap(),
        )
        .operate(),
        Some(("list", m)) => BasicOperations::list(m.get_one("search")),

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
    pub fn list(search_val: Option<&String>) -> Result<(), OperationError> {
        let mut trash_list = TrashList::default();
        let items = os_limited::list()
            .map_err(|e| OperationError::new(Box::new(e), OPERATION::LIST, None))?;

        if let Some(query) = search_val {
            let results = util::fuzzy_search(trash_items_to_names(&items), query.to_string());

            trash_list.set_items(&trash_items_from_names(&results, &items));
        } else {
            trash_list.set_items(&items);
        }
        trash_list.print();
        Ok(())
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
}

struct TrashOperation;

impl TrashOperation {
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

    pub fn trash_single(path: &PathBuf) -> Result<(), OperationError> {
        match trash::delete_all([path]) {
            Ok(_) => return Ok(()),
            Err(e) => {
                return Err(OperationError::new(
                    Box::new(e),
                    OPERATION::TRASH,
                    Some(files::path_to_string(path)),
                ))
            }
        }
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
    fn attempt_restore(files: &mut Vec<TrashItem>, pb: &OpSpinner) -> Result<bool, OperationError> {
        for file in files.clone() {
            pb.set_file_str(file.name.clone());
            match trash::os_limited::restore_all([file.clone()]) {
                Ok(_) => util::remove_from_vec(files, &file),
                Err(e) => {
                    util::handle_collision_item(e, files, &file).map_err(|err| {
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

impl Default for DeleteOperation {
    fn default() -> DeleteOperation {
        DeleteOperation {
            pb: OpSpinner::default(OPERATION::DELETE),
        }
    }
}

impl DeleteOperation {
    fn operate(&mut self, files: Vec<String>, recurse_default: bool) -> Result<(), OperationError> {
        let string_paths = get_existent_paths(&files, |f| self.pb.print_no_file_warn(f.as_str()));

        let paths = path_vec_from_string_vec(string_paths);
        let recurse = check_recursion(&paths, recurse_default);

        self.pb.start();

        match rrc_lib::recurse_on_paths(self, paths, recurse) {
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

impl RecursiveCallback for DeleteOperation {
    fn cb(&mut self, path: &PathBuf) -> Result<bool, FileErr> {
        match files::remove_file_or_empty_dir(path) {
            Ok(()) => return Ok(true),
            Err(e) => return Err(FileErr::map(e, path)),
        }
    }

    fn display_cb(&mut self, path: &PathBuf, _is_dir: bool) -> bool {
        let path_name = files::path_to_string(path);

        self.pb.set_file_str(path_name);
        true
    }
}

struct ShredOperation {
    pb: OpSpinner,
    num_runs: usize,
}

impl ShredOperation {
    fn new(num_runs: usize) -> ShredOperation {
        ShredOperation {
            pb: OpSpinner::default(OPERATION::SHRED { num_runs }),
            num_runs,
        }
    }

    fn operate(&mut self, files: Vec<String>, recurse_default: bool) -> Result<(), OperationError> {
        let string_paths = get_existent_paths(&files, |f| self.pb.print_no_file_warn(f));

        let paths = path_vec_from_string_vec(string_paths);
        let recurse = check_recursion(&paths, recurse_default);

        self.pb.start();

        match rrc_lib::recurse_on_paths(self, paths, recurse) {
            Ok(c) => {
                self.pb.auto_finish(c);
                Ok(())
            }
            Err(e) => {
                self.pb.finish();
                let file = e.file.clone();
                Err(OperationError::new(
                    Box::new(e),
                    OPERATION::SHRED {
                        num_runs: self.num_runs,
                    },
                    Some(file),
                ))
            }
        }
    }
}

impl RecursiveCallback for ShredOperation {
    fn cb(&mut self, path: &PathBuf) -> Result<bool, FileErr> {
        if !path.is_dir() {
            let mut file = OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|e| FileErr::map(e, path))?;
            files::overwrite_file(&mut file, self.num_runs).map_err(|e| FileErr::map(e, path))?;
        }

        files::remove_file_or_empty_dir(path).map_err(|e| FileErr::map(e, path))?;

        Ok(true)
    }

    fn display_cb(&mut self, path: &PathBuf, _is_dir: bool) -> bool {
        let path_name = files::path_to_string(path);
        self.pb.set_file_str(path_name);

        true
    }
}

struct SearchOperation {
    op: OPERATION,
    target: String,
    directory: String,
    pb: OpSpinner,
    operate_curr_file: bool,
}

impl SearchOperation {
    pub fn new(op_arg: &String, target: &String, directory: &String) -> SearchOperation {
        let op = match op_arg.as_str() {
            "t" => OPERATION::TRASH,
            "d" => OPERATION::DELETE,
            "s" => OPERATION::SHRED { num_runs: 1 }, //if none of these match, clap hasn't parsed our arguments properly and nothing can be trusted.
            _ => panic!(),
        };

        SearchOperation {
            op: op,
            pb: OpSpinner::default(op),
            target: target.to_string(),
            directory: directory.to_string(),
            operate_curr_file: false,
        }
    }

    fn operate(&mut self) -> Result<(), OperationError> {
        let dir_clone = self.directory.clone();
        let target_dir = Path::new(&dir_clone);
        match rrc_lib::recurse_on_paths(self, vec![target_dir], true) {
            Ok(_) => Ok(()),
            Err(e) => {
                let file = e.file.clone();
                Err(OperationError::new(Box::new(e), self.op, Some(file)))
            }
        }
    }

    fn run_op_single(&mut self, path: &PathBuf) -> std::io::Result<()> {
        match self.op {
            OPERATION::DELETE => files::remove_file_or_empty_dir(path)?,
            OPERATION::SHRED { num_runs } => {
                let file = OpenOptions::new()
                    .write(true)
                    .create(false)
                    .read(true)
                    .open(path)?;

                files::overwrite_file(&file, num_runs)?;
                files::remove_file_or_empty_dir(path)?;
            }
            OPERATION::TRASH => trash::delete_all([path]).unwrap(),
            _ => {}
        }

        Ok(())
    }
}

impl RecursiveCallback for SearchOperation {
    fn cb(&mut self, path: &PathBuf) -> Result<bool, FileErr> {
        if self.operate_curr_file {
            self.run_op_single(path);
            self.operate_curr_file = false;
        }
        Ok(true)
    }

    fn display_cb(&mut self, path: &PathBuf, is_dir: bool) -> bool {
        let file_name = &files::os_str_to_str(path.file_name().unwrap());
        if levenshtein(&file_name, &self.target) <= 1 {
            let selection = output::prompt_search_operation(
                &self.target,
                &file_name.to_string(),
                is_dir,
                self.op,
            )
            .unwrap();
            if selection.0 {
                self.operate_curr_file = true;
            }

            return selection.1;
        }

        true
    }
}

fn check_recursion<'a>(paths: &Vec<&Path>, recurse_default: bool) -> bool {
    if !recurse_default {
        for path in paths {
            if path.is_dir() {
                return prompt_recursion(path_to_string(path)).is_ok_and(|v| v);
            }
        }
    }
    recurse_default
}
