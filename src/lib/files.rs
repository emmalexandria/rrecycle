use std::{
    error::Error,
    ffi::OsStr,
    fmt::Display,
    fs::{self, File},
    io::{self, BufWriter, Seek, Write},
    path::{Path, PathBuf},
};
use trash::{os_limited, TrashItem};

use crate::RecursiveOperation;

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
                operation.cb(&path)?;
            }
        }
        count += 1;
        operation.display_cb(&PathBuf::from(dir), true);
        operation.cb(&PathBuf::from(dir))?;
    }
    Ok(count)
}

/// Function to resolve conflicts when multiple files have the same name
pub fn select_from_trash(name: &String) -> Option<Vec<TrashItem>> {
    let mut items: Vec<TrashItem> = Vec::new();

    for item in os_limited::list().unwrap() {
        if name == &item.name {
            items.push(item);
        }
    }

    if items.is_empty() {
        return None;
    }
    Some(items)
}

pub fn overwrite_file(mut file: &File, runs: usize) -> std::io::Result<()> {
    const OW_BUFF_SIZE: usize = 4096;
    if file.metadata()?.is_dir() {
        return Ok(());
    }

    let file_len: usize = file.metadata()?.len().try_into().unwrap();
    let mut writer = BufWriter::new(file);

    let buf: [u8; OW_BUFF_SIZE] = [0u8; OW_BUFF_SIZE];

    for _ in 0..runs {
        writer.seek(io::SeekFrom::Start(0))?;

        //Keep track of our position in the file ourselves based on the delusion that it might impact
        //performace to seek on each loop iteration
        let mut offset: usize = 0;
        loop {
            if (file_len - offset) >= OW_BUFF_SIZE {
                //no need to retry if this write doesn't write the entire buffer. the data in the buffer isn't important
                offset += writer.write(&buf)?;
            } else {
                writer.write(&vec![0u8; file_len - offset])?;
                break;
            }
        }
    }

    writer.flush()?;

    Ok(())
}

pub fn remove_file_or_dir(path: &PathBuf) -> std::io::Result<()> {
    if !path.exists() {
        return Err(std::io::ErrorKind::NotFound.into());
    }

    if path.is_dir() {
        return fs::remove_dir(path);
    }
    fs::remove_file(path)
}

pub fn get_existent_paths<'a, T, U>(input_paths: &'a T, d_cb: &dyn Fn(U)) -> Vec<U>
where
    &'a T: IntoIterator<Item = U>,
    U: AsRef<Path> + 'a,
{
    input_paths
        .into_iter()
        .filter_map(|p| {
            if p.as_ref().exists() {
                Some(p)
            } else {
                d_cb(p);
                return None;
            }
        })
        .collect()
}

//Unfortunately I'm yet to find a more functional way to do this
pub fn path_vec_from_string_vec<'a>(strings: Vec<&'a String>) -> Vec<&'a Path> {
    let mut ret_vec = Vec::<&Path>::new();
    for s in strings {
        ret_vec.push(Path::new(s));
    }
    return ret_vec;
}

#[cfg(test)]
mod tests {
    use rand::distributions::{Alphanumeric, DistString};
    use std::{fs::OpenOptions, io::Read};

    use super::*;
    #[test]
    fn test_select_from_trash_exists_single() {
        let filename = generate_random_filename();

        File::create(&filename).unwrap();
        trash::delete(&filename).unwrap();

        let selected = select_from_trash(&filename);

        assert!(selected.is_some());
        let selected_val = selected.unwrap();

        assert!(selected_val.len() == 1);

        os_limited::purge_all([&(selected_val[0])]).unwrap();
    }

    #[test]
    fn test_select_from_trash_exists_multiple() {
        let filename = generate_random_filename();

        File::create(&filename).unwrap();
        trash::delete(&filename).unwrap();

        File::create(&filename).unwrap();
        trash::delete(&filename).unwrap();

        let selected = select_from_trash(&filename);

        assert!(selected.is_some());
        let selected_val = selected.unwrap();
        assert!(selected_val.len() == 2);

        os_limited::purge_all(selected_val).unwrap();
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

        if buf != vec![byte; file_len] {
            println!("{}/{}", buf.len(), file_len);
            return false;
        }
        true
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
        let ones = vec![1u8; 10usize.pow(6)];
        file.write_all(&ones).unwrap();
        file.flush().unwrap();

        overwrite_file(&file, 1).unwrap();

        if !is_file_of_single_byte(&file, 0u8) {
            fs::remove_file(&filename).unwrap();
            panic!();
        } else {
            fs::remove_file(&filename).unwrap();
        }
    }

    fn generate_random_filename() -> String {
        Alphanumeric.sample_string(&mut rand::thread_rng(), 8)
            + "."
            + &Alphanumeric.sample_string(&mut rand::thread_rng(), 3)
    }
}
