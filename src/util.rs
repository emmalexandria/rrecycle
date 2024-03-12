use std::path::PathBuf;

use trash::TrashItem;

///This function is literally completely pointless but it took pain to write (before I found out that retain exists), so I'm keeping it.
pub fn remove_string_from_vec<T: AsRef<str>>(vec: &mut Vec<T>, needle: T)
where
    T: AsRef<str> + PartialEq,
{
    vec.retain(|v| v != &needle);
}

pub fn remove_first_string_from_vec<T: AsRef<str>>(vec: &mut Vec<T>, needle: T)
where
    T: AsRef<str> + PartialEq,
{
    if let Some(pos) = vec.iter().position(|x| *x == needle) {
        vec.remove(pos);
    }
}

#[test]
fn test_remove_string_from_vec() {
    let mut vec = vec!["Hello".to_string(), "Hi".to_string()];
    remove_string_from_vec(&mut vec, "Hi".to_string());

    assert_eq!(vec, vec!["Hello".to_string()]);

    let mut vec = vec!["Hello", "Hi"];
    remove_string_from_vec(&mut vec, "Hi");

    assert_eq!(vec, vec!["Hello"]);
}

#[test]
fn test_remove_first_string_from_vec() {
    let mut vec = vec!["Hello".to_string(), "Hi".to_string(), "Hi".to_string()];
    remove_first_string_from_vec(&mut vec, "Hi".to_string());

    assert_eq!(vec, vec!["Hello".to_string(), "Hi".to_string()]);

    let mut vec = vec!["Hello", "Hi", "Hi"];
    remove_first_string_from_vec(&mut vec, "Hi");

    assert_eq!(vec, vec!["Hello", "Hi"]);
}
