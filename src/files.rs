use std::{
    fs::{self, DirEntry, File, OpenOptions},
    io::{self, Seek, Write},
    path::{self, Path, PathBuf},
};
use trash::{os_limited, TrashItem};

use colored::Colorize;

use crate::{
    interface::{self, format_unix_date, prompt_recursion},
    operations::RecursiveOperation,
};

const SHRED_RUNS: u32 = 3;
const SHRED_BUFFER_SIZE: usize = 4096;

pub struct RestoreResult {
    pub files: Vec<(String, String)>,
}

impl std::fmt::Display for RestoreResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.files.len() == 0 {
            write!(f, "{}", "Restored no files".red())
        } else if self.files.len() == 1 {
            write!(f, "Restored 1 file")
        } else {
            write!(f, "Restored {} files", self.files.len())
        }
    }
}

pub fn run_on_dir_recursive(
    dir: &Path,
    cb: &dyn Fn(&PathBuf) -> std::io::Result<()>,
) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                run_on_dir_recursive(&path, cb)?;
            } else {
                cb(&entry.path())?;
            }
        }
        cb(&PathBuf::from(dir))?;
    }
    Ok(())
}

pub fn run_op_on_dir_recursive<T>(operation: &mut T, dir: &Path) -> std::io::Result<()>
where
    T: RecursiveOperation,
{
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                run_op_on_dir_recursive(operation, &path)?;
            } else {
                T::cb(&entry.path())?;
                operation.display_cb(&path, false);
            }
        }
        T::cb(&PathBuf::from(dir))?;
        operation.display_cb(&PathBuf::from(dir), true);
    }
    Ok(())
}

pub fn get_path_entries(path: &str) -> Vec<DirEntry> {
    let mut entries: Vec<DirEntry> = Vec::new();

    for dir in fs::read_dir(path) {
        for entry in dir {
            match entry {
                Ok(f) => entries.push(f),
                Err(e) => eprintln!("{}", e),
            }
        }
    }

    return entries;
}

pub fn file_exists(path: PathBuf) -> bool {
    return File::open(path).is_ok();
}

/// Function to resolve conflicts when multiple files have the same name
pub fn select_file_from_trash(name: &String) -> Option<TrashItem> {
    let mut items: Vec<TrashItem> = Vec::new();

    for item in os_limited::list().unwrap() {
        if name == &item.name {
            items.push(item);
        }
    }

    if items.len() > 1 {
        let item_names: Vec<String> = items
            .iter()
            .map(|i| {
                i.original_path().to_str().unwrap().to_string()
                    + " | "
                    + &format_unix_date(i.time_deleted)
            })
            .collect();

        let selection = interface::file_conflict_prompt(&name, item_names);

        return Some(items[selection].clone());
    }

    if items.len() == 1 {
        return Some(items[0].clone());
    }

    None
}

/* pub fn shred_files(path_strings: Vec<String>) -> std::io::Result<()> {
    let files_to_shred: Vec<&Path> = Vec::new();

    for path in path_strings {
        let p = Path::new(&path);

        if p.is_dir() {
          let files
        }
    }

    Ok(())
} */

/* pub fn shred_dir(path: &Path) {
    let files = get_path_entries(path);
} */

pub fn overwrite_file(mut file: &File) -> std::io::Result<()> {
    if file.metadata()?.is_dir() {
        return Ok(());
    }

    let buf: [u8; SHRED_BUFFER_SIZE] = [0; SHRED_BUFFER_SIZE];

    let file_size = file.metadata()?.len();

    for _ in 0..SHRED_RUNS {
        for _ in 0..(file_size / SHRED_BUFFER_SIZE as u64) {
            file.write_all(&buf)?;
        }

        let remaining_bytes = (file_size - file.stream_position()?) as usize;
        if remaining_bytes > 0 {
            file.write(&vec![0; remaining_bytes])?;
        }
        file.flush()?;
    }

    Ok(())
}

pub fn remove_file_or_dir(path: &PathBuf) -> std::io::Result<()> {
    if !path.exists() {
        return Err(std::io::ErrorKind::NotFound.into());
    }

    if path.is_dir() {
        return fs::remove_dir(path);
    }
    return fs::remove_file(path);
}
