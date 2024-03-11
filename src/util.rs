use std::path::PathBuf;

pub fn pathbuf_to_string(path: &PathBuf) -> Option<String> {
    match path.to_str() {
        Some(s) => Some(s.to_string()),
        None => None,
    }
}

pub fn get_file_name(path: &PathBuf) -> Option<String> {
    match path.file_name() {
        Some(f) => Some(f.to_string_lossy().to_string()),
        None => None,
    }
}
