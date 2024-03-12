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
pub fn select_from_trash(name: &String) -> Option<TrashItem> {
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

pub fn overwrite_file(file: &mut File) -> std::io::Result<()> {
    if file.metadata()?.is_dir() {
        return Ok(());
    }

    file.seek(io::SeekFrom::Start(0))?;

    let buf: [u8; SHRED_BUFFER_SIZE] = [0u8; SHRED_BUFFER_SIZE];

    for _ in 0..SHRED_RUNS {
        loop {
            let remaining_len: usize = calc_remaining_len_in_file(file)?.try_into().unwrap();
            if remaining_len >= SHRED_BUFFER_SIZE {
                file.write_all(&buf)?;
            } else {
                file.write_all(&vec![0u8; remaining_len])?;
                break;
            }
        }

        file.flush()?;
    }

    Ok(())
}

fn calc_remaining_len_in_file(file: &mut File) -> std::io::Result<u64> {
    let current = file.seek(io::SeekFrom::Current(0))?;
    let end = file.seek(io::SeekFrom::End(0))?;
    file.seek(io::SeekFrom::Start(current))?;

    Ok(end - current)
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

#[cfg(test)]
mod tests {
    use rand::distributions::{Alphanumeric, DistString};
    use std::{fs::OpenOptions, io::Read};

    use super::*;

    #[test]
    fn test_select_from_trash_exists() {
        let filename = generate_random_filename();

        File::create(&filename).unwrap();
        trash::delete(&filename).unwrap();

        let selected = select_from_trash(&filename);

        assert!(selected.is_some());

        os_limited::purge_all([selected.unwrap()]).unwrap();
    }

    #[test]
    fn test_select_from_trash_fails() {
        assert!(select_from_trash(&generate_random_filename()).is_none());
    }

    fn is_file_of_single_byte(mut file: &File, byte: u8) -> bool {
        let file_len: usize = file.metadata().unwrap().len().try_into().unwrap();
        let mut buf = Vec::<u8>::with_capacity(file_len);
        file.seek(io::SeekFrom::Start(0)).unwrap();
        file.read_to_end(&mut buf).unwrap();

        if buf != Vec::<u8>::from(vec![byte; file_len]) {
            println!("{}/{}", buf.len(), file_len);
            return false;
        }
        return true;
    }

    #[test]
    fn test_is_file_of_single_byte() {
        //can't just use rand file name, must use seperate paths to avoid weird issues with creating a file immediately after deleting it
        let filename = generate_random_filename();

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .read(true)
            .open(&filename)
            .unwrap();

        //write 512MB of data to the test file (MB not MiB)
        let ones = vec![1u8; 128 * (10 ^ 6)];
        file.write_all(&ones).unwrap();
        file.flush().unwrap();

        if !is_file_of_single_byte(&file, 1u8) {
            fs::remove_file(&filename).unwrap();

            panic!()
        } else {
            fs::remove_file(&filename).unwrap();
        }
    }

    #[test]
    fn test_overwrite_file() {
        let filename = generate_random_filename();

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .read(true)
            .open(&filename)
            .unwrap();

        //write 512MB of data to the test file (MB not MiB)
        let ones = vec![1u8; 1usize * (10usize.pow(6))];
        file.write_all(&ones).unwrap();
        file.flush().unwrap();

        overwrite_file(&mut file).unwrap();

        if !is_file_of_single_byte(&file, 0u8) {
            panic!();
        } else {
            fs::remove_file(&filename).unwrap();
        }
    }

    fn generate_random_filename() -> String {
        return Alphanumeric.sample_string(&mut rand::thread_rng(), 8)
            + "."
            + &Alphanumeric.sample_string(&mut rand::thread_rng(), 3);
    }
}
