use std::path::PathBuf;

use files::FileErr;
use indicatif::ProgressBar;

pub mod files;
pub mod util;

pub trait RecursiveOperation {
    fn cb(&self, path: &PathBuf) -> Result<(), FileErr>;
    fn display_cb(&mut self, path: &PathBuf, is_dir: bool);

    fn get_spinner(&self) -> &ProgressBar;
}
