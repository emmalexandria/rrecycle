//The following 3 functions are all designed to work together in the same context (being a mutable reference to a vector which should be changed in place)

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

pub fn count_occurences<'a, U, T>(iter: &'a T, needle: U) -> usize
where
    &'a T: IntoIterator<Item = U>,
    U: PartialEq,
{
    iter.into_iter().filter(|v| *v == needle).count()
}

///Handles a restore collision by
pub fn handle_collision(
    error: trash::Error,
    files: &mut Vec<String>,
    path: &String,
) -> Result<String, trash::Error> {
    match error {
        trash::Error::RestoreCollision {
            path: path_buf,
            remaining_items: _,
        } => {
            //This is a little dumb, but it lets me reuse existing code
            while count_occurences(files, &path) > 1 {
                remove_first_string_from_vec(files, path.to_string());
            }

            Ok(crate::files::path_to_string(path_buf))
        }
        _ => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_count_occurences() {
        let mut vec = vec!["Hi", "Hello", "How are you", "Hi", "Hi"];
        assert_eq!(count_occurences(&vec, &"Hi"), 3);

        remove_first_string_from_vec(&mut vec, &"Hi");

        assert_eq!(count_occurences(&vec, &"Hi"), 2);
    }
}
