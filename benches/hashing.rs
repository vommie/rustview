use criterion::{Criterion, criterion_group, criterion_main};

fn hashing_benchmark(_criterion: &mut Criterion) {
    // BLAKE3 benchmarks will be implemented here.
}

criterion_group!(benches, hashing_benchmark);
criterion_main!(benches);
