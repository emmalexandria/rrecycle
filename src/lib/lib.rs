use std::{
    error::Error,
    fmt::Display,
    path::{Path, PathBuf},
};

use files::get_existent_paths;
use indicatif::ProgressBar;

pub mod files;
pub mod util;

pub trait RecursiveOperation {
    fn cb(&self, path: &PathBuf) -> Result<(), FileErr>;
    fn display_cb(&mut self, path: &PathBuf, is_dir: bool);
}

#[derive(Debug)]
pub struct FileErr {
    source: std::io::Error,
    pub file: String,
}

impl Display for FileErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl FileErr {
    pub fn map<P: AsRef<Path>>(error: std::io::Error, path: P) -> FileErr {
        FileErr {
            source: error,
            file: files::path_to_string(path),
        }
    }
}

impl Error for FileErr {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

///This function runs the callbacks of a RecursiveOperation on a given set of paths.
/// Please note that it does not check if the path exists.
pub fn run_recursive_op<T: RecursiveOperation>(
    op: &mut T,
    paths: Vec<&Path>,
    recurse: bool,
) -> Result<usize, FileErr>
where
    T: RecursiveOperation,
{
    let mut counter: usize = 0;

    for path in paths {
        if path.is_dir() && recurse {
            match files::run_op_on_dir_recursive::<T>(op, path, 0) {
                Ok(c) => counter += c,
                Err(e) => {
                    return Err(e);
                }
            };
        } else {
            op.display_cb(&PathBuf::from(path), false);
            match op.cb(&PathBuf::from(path)) {
                Ok(_) => counter += 1,
                Err(e) => {
                    return Err(e);
                }
            };
        }
    }

    Ok(counter)
}
