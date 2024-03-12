use std::{
    error::Error,
    fmt::Display,
    fs::{self, File},
    io::{self, Seek, Write},
    path::{Path, PathBuf},
};
use trash::{os_limited, TrashItem};

use crate::{
    operations::RecursiveOperation,
    output::{self, format_unix_date},
};

const SHRED_RUNS: u32 = 1;
const SHRED_BUFFER_SIZE: usize = 4096;

#[derive(Debug)]
pub struct FileErr {
    pub error: std::io::Error,
    pub file: String,
}

impl Display for FileErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl FileErr {
    pub fn map<P: AsRef<Path>>(error: std::io::Error, path: P) -> FileErr {
        FileErr {
            error,
            file: path_to_string(path),
        }
    }
}

impl Error for FileErr {}

///Returns a losslessly converted string if possible, but if that errors return the lossy conversion.
//This is done because this function is used pretty much everywhere. Currently has 12 uses and counting.
//While it may cause issues in some edge case, I'd rather avoid matching or unwrapping Options everywhere
//and what am I gonna do if the path contains non valid unicode anyway? Die?
pub fn path_to_string<P: AsRef<Path>>(path: P) -> String {
    match path.as_ref().to_str() {
        Some(s) => s.to_string(),
        None => path.as_ref().to_string_lossy().to_string(),
    }
}

pub fn run_op_on_dir_recursive<T>(
    operation: &mut T,
    dir: &Path,
    mut count: u64,
) -> Result<u64, FileErr>
where
    T: RecursiveOperation,
{
    if dir.is_dir() {
        for entry in fs::read_dir(dir).map_err(|e| FileErr::map(e, dir))? {
            let entry = entry.map_err(|e| FileErr::map(e, dir))?;
            let path = entry.path();
            if path.is_dir() {
                run_op_on_dir_recursive(operation, &path, count)?;
            } else {
                count += 1;
                operation.display_cb(&path, false);
                T::cb(&path)?;
            }
        }
        count += 1;
        operation.display_cb(&PathBuf::from(dir), true);
        T::cb(&PathBuf::from(dir))?;
    }
    Ok(count)
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

        let selection = output::file_conflict_prompt(
            "Please select which file to operate on.".to_string(),
            item_names,
        );

        return Some(items[selection].clone());
    }

    if items.len() == 1 {
        return Some(items[0].clone());
    }

    None
}

pub fn overwrite_file(mut file: &File) -> std::io::Result<()> {
    if file.metadata()?.is_dir() {
        return Ok(());
    }

    let buf: [u8; SHRED_BUFFER_SIZE] = [0; SHRED_BUFFER_SIZE];

    let file_size = file.metadata()?.len();

    for _ in 0..SHRED_RUNS {
        for _ in 0..(file_size / SHRED_BUFFER_SIZE as u64) {
            file.write(&buf)?;
        }

        let remaining_bytes = (file_size - file.stream_position()?) as usize;
        if remaining_bytes > 0 {
            file.write(&vec![0; remaining_bytes])?;
        }
        file.flush()?;

        file.seek(io::SeekFrom::Start(0))?;
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
