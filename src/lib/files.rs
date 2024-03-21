use std::{
    borrow::Cow,
    ffi::OsStr,
    fs::{self, File},
    io::{self, BufWriter, Seek, Write},
    path::{Path, PathBuf},
};
use trash::{os_limited, TrashItem};

use crate::{FileErr, RecursiveCallback};

///Returns a losslessly converted string if possible, but if that errors return the lossy conversion.
//This function is used pretty much everywhere. While it may cause issues in some edge case,
// I'd rather avoid matching or unwrapping Options everywhere
pub fn path_to_string<P: AsRef<Path>>(path: P) -> String {
    match path.as_ref().to_str() {
        Some(s) => s.to_string(),
        None => path.as_ref().to_string_lossy().to_string(),
    }
}

//Same as above for os_str
pub fn os_str_to_str<'a>(path: &'a OsStr) -> Cow<'_, str> {
    match path.to_str() {
        Some(s) => Cow::Borrowed(s),
        None => path.to_string_lossy(),
    }
}

pub fn run_op_on_dir_recursive<T>(
    operation: &mut T,
    dir: &Path,
    mut count: usize,
) -> Result<(usize, bool), FileErr>
where
    T: RecursiveCallback,
{
    if dir.is_dir() {
        for entry in fs::read_dir(dir).map_err(|e| FileErr::map(e, dir))? {
            let entry = entry.map_err(|e| FileErr::map(e, dir))?;
            let path = entry.path();
            if path.is_dir() {
                if !run_op_on_dir_recursive(operation, &path, count)?.1 {
                    return Ok((count, false));
                }
            } else {
                count += 1;
                if !operation.execute_callbacks(&path, false)? {
                    return Ok((count, false));
                }
            }
        }
        count += 1;
        return Ok((
            count,
            operation.execute_callbacks(&PathBuf::from(dir), true)?,
        ));
    }
    Ok((count, true))
}

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

pub fn get_existent_trash_items(
    names: &Vec<String>,
    s_cb: impl Fn(Vec<TrashItem>) -> TrashItem,
    d_cb: impl Fn(&String),
) -> Vec<TrashItem> {
    names
        .iter()
        .filter_map(|n| match select_from_trash(n) {
            Some(i) => Some(s_cb(i)),
            None => {
                d_cb(n);
                None
            }
        })
        .collect()
}

pub fn overwrite_file(file: &File, runs: usize) -> std::io::Result<()> {
    const OW_BUFF_SIZE: usize = 10usize.pow(6);
    let file_len = file.metadata()?.len();

    if file.metadata()?.is_dir() {
        return Ok(());
    }

    let mut writer = BufWriter::new(file);

    let buf = vec![0u8; OW_BUFF_SIZE];

    for _ in 0..runs {
        writer.seek(io::SeekFrom::Start(0))?;

        //Keep track of our position in the file ourselves based on the delusion that it might impact
        //performace to seek on each loop iteration
        loop {
            let offset = writer.seek(io::SeekFrom::Current(0)).unwrap();
            if (file_len - offset) >= OW_BUFF_SIZE.try_into().unwrap() {
                writer.write_all(&buf)?;
            } else {
                writer.write_all(&vec![0u8; (file_len - offset).try_into().unwrap()])?;
                break;
            }
        }
    }

    Ok(())
}

pub fn remove_file_or_empty_dir(path: &PathBuf) -> std::io::Result<()> {
    if !path.exists() {
        return Err(std::io::ErrorKind::NotFound.into());
    }

    if path.is_dir() {
        fs::remove_dir(path)?;
    } else {
        fs::remove_file(path)?;
    }

    Ok(())
}

pub fn get_existent_paths<'a, T, U>(input_paths: &'a T, d_cb: impl Fn(U)) -> Vec<U>
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

pub fn trash_items_to_names(items: &Vec<TrashItem>) -> Vec<String> {
    items.iter().map(|i| i.name.clone()).collect()
}

pub fn trash_items_from_names(names: &Vec<String>, items: &Vec<TrashItem>) -> Vec<TrashItem> {
    let mut filtered_names = names.clone();
    filtered_names.dedup();
    items
        .iter()
        .filter_map(|i| {
            if names.contains(&i.name) {
                return Some(i);
            }
            None
        })
        .cloned()
        .collect()
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

        //write 1MiB of data to the test file
        let ones = vec![1u8; 1024 ^ 2];
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

        //write 1MB of data to the test file (MB not MiB)
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
