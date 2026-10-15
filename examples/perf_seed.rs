//! Developer tool for the performance work: builds a persistent library (records + generated thumbnails) from image folders
//! with the same code paths the application uses, so that scroll and load benchmarks run against real data.
//!
//! Usage: `cargo run --example perf_seed -- <home> <thumbnail_size> <folder>...`
//! The first folder is the root (add the parent before its subfolders). The library lands in `<home>/.local/share/rustview/`
//! (the layout the application uses when started with `HOME=<home>`).

use rustview::library::Library;
use rustview::settings::schema::PerformanceSettings;
use rustview::thumbnails::{PreparedTarget, ThumbnailConfig, ThumbnailManager};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: perf_seed <home> <thumbnail_size> <folder>...");
        std::process::exit(2);
    }
    let home = PathBuf::from(&args[1]);
    let size: u32 = args[2].parse().unwrap_or(200);
    let folders: Vec<PathBuf> = args[3..].iter().map(PathBuf::from).collect();

    let data_dir = home.join(".local/share/rustview");
    std::fs::create_dir_all(&data_dir)?;
    let library = Arc::new(Library::open(&data_dir.join("library.lmdb"))?);

    let perf = PerformanceSettings {
        thumbnail_db_size: size,
        ..PerformanceSettings::default()
    };
    let config = ThumbnailConfig::from_performance_settings(&perf);
    let manager = ThumbnailManager::new(
        library.clone(),
        &data_dir.join("thumbnails"),
        256 * 1024 * 1024,
        perf.worker_threads,
    )?;

    for folder in &folders {
        seed_folder(&library, &manager, folder, &config)?;
    }
    std::thread::sleep(Duration::from_millis(500));
    println!("storage: {:?}", manager.storage_stats());
    Ok(())
}

fn seed_folder(
    library: &Arc<Library>,
    manager: &ThumbnailManager,
    folder: &Path,
    config: &ThumbnailConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let record = library.folders().add_folder(folder)?;
    let started = Instant::now();
    let report = library.refresher().refresh_folder(record.id)?;
    let media = library.media().get_by_folder(record.id)?;
    println!("{}: {} records ({} new) in {:?}", folder.display(), media.len(), report.new_media, started.elapsed());
    if media.is_empty() {
        return Ok(());
    }

    let targets: Vec<PreparedTarget> = media
        .iter()
        .map(|m| PreparedTarget {
            pipeline_id: 0,
            folder_id: m.folder_id,
            media_id: m.id,
            file_path: Path::new(&record.path).join(&m.filename),
        })
        .collect();
    let total = targets.len();
    let started = Instant::now();
    manager.start_pipeline(record.id, targets, config.clone());

    let mut drained = 0usize;
    let deadline = Instant::now() + Duration::from_secs(900);
    while drained < total && Instant::now() < deadline {
        let batch = manager.drain_ready_batch(256);
        drained += batch.len();
        if batch.is_empty() {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    println!("  thumbnails: {drained}/{total} in {:?}", started.elapsed());
    Ok(())
}
