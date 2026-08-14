use criterion::{Criterion, criterion_group, criterion_main};
use rustview::library::Library;
use tempfile::tempdir;

fn database_benchmark(c: &mut Criterion) {
    let tmp_db = tempdir().unwrap();
    let lib = Library::open(tmp_db.path()).unwrap();
    let folder = lib.folders().add_folder(tmp_db.path()).unwrap();

    c.bench_function("folder_lookup_by_id", |b| {
        b.iter(|| {
            lib.folders().get_folder(folder.id).unwrap();
        });
    });
}

criterion_group!(benches, database_benchmark);
criterion_main!(benches);
