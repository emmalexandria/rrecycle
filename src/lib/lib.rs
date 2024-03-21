use std::{
    error::Error,
    fmt::Display,
    fs::File,
    path::{Path, PathBuf},
};

pub mod files;
pub mod util;

///Trait to be used with the recurse_op_on_dir function.
/// Guarantee to the implementor: display_cb MUST be called before cb to allow for prompting etc.
pub trait RecursiveCallback {
    fn execute_callbacks(&mut self, path: &PathBuf, is_dir: bool) -> Result<bool, FileErr> {
        let display_cb_result = self.display_cb(path, is_dir);
        let cb_result = self.cb(path)?;
        Ok(display_cb_result && cb_result)
    }
    ///Processes a file that has been discovered while traversing the tree.
    /// Returns true if the traversal should continue
    fn cb(&mut self, path: &PathBuf) -> Result<bool, FileErr>;
    ///Displays any relevant output to the user about the current file being parsed.
    /// Returns true if the traversal should continue
    fn display_cb(&mut self, path: &PathBuf, is_dir: bool) -> bool;
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
pub fn recurse_on_paths<T: RecursiveCallback>(
    op: &mut T,
    paths: Vec<&Path>,
    recurse: bool,
) -> Result<usize, FileErr> {
    let mut counter: usize = 0;

    for path in paths {
        if path.is_dir() && recurse {
            match files::run_op_on_dir_recursive::<T>(op, path, 0) {
                Ok(c) => counter += c.0,
                Err(e) => {
                    return Err(e);
                }
            };
        } else {
            op.execute_callbacks(&PathBuf::from(path), false)?;
        }
    }

    Ok(counter)
}
