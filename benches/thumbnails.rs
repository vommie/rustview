use criterion::{Criterion, criterion_group, criterion_main};

fn thumbnail_benchmark(_criterion: &mut Criterion) {
    // Thumbnail pipeline benchmarks will be implemented here.
}

criterion_group!(benches, thumbnail_benchmark);
criterion_main!(benches);
