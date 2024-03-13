use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    time::Duration,
};

use rand::distributions::{Alphanumeric, DistString};
use shred::files::{self, overwrite_file};

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::{criterion_group, criterion_main};

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

fn bench_overwrite(c: &mut Criterion) {
    let MB = 10usize.pow(6);
    let mut group = c.benchmark_group("overwrite-file");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(150));
    group.warm_up_time(Duration::from_secs(10));

    for size in [10 * MB, 100 * MB, 500 * MB, 1000 * MB] {
        let filename = generate_random_filename();
        let file = gen_file(size, &filename);
        group.bench_with_input(
            BenchmarkId::from_parameter((size / MB).to_string()),
            &file,
            |b, f| b.iter(|| overwrite_file(&f, 1)),
        );
        std::fs::remove_file(filename).unwrap();
    }
}

criterion_group!(benches, bench_overwrite);
criterion_main!(benches);
