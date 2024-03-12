use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    time::Duration,
};

use rand::distributions::{Alphanumeric, DistString};
use shred::files;

use brunch::Bench;

fn generate_random_filename() -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), 8)
        + "."
        + &Alphanumeric.sample_string(&mut rand::thread_rng(), 3)
}

fn gen_file(size: usize, name: &str) -> File {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .read(true)
        .open(name)
        .unwrap();

    let ones = vec![1u8; size];
    file.write_all(&ones).unwrap();
    file.flush().unwrap();

    return file;
}

brunch::benches!(Bench::new("overwrite_speed")
    .with_timeout(Duration::from_secs(300))
    .with_samples(100)
    .run(|| {
        let filename = generate_random_filename();
        //100MB
        let file_size: usize = 10usize.pow(8);

        let mut file = gen_file(file_size, &filename);

        files::overwrite_file(&mut file, 1).unwrap();

        fs::remove_file(&filename).unwrap();
    }));
