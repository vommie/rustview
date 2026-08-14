use criterion::{Criterion, criterion_group, criterion_main};
use rustview::media::MediaHasher;
use std::hint::black_box;

fn hashing_benchmark(c: &mut Criterion) {
    let data = vec![0xABu8; 1024 * 1024]; // 1 MiB chunk

    c.bench_function("blake3_1mib_buffer", |b| {
        b.iter(|| {
            MediaHasher::hash_bytes(black_box(&data));
        });
    });
}

criterion_group!(benches, hashing_benchmark);
criterion_main!(benches);
