use rustview::library::Library;
use rustview::player::{PlayerController, RenderConfig};
use rustview::thumbnails::ThumbnailManager;
use rustview::ui::{MainWindow, UiBridge};
use slint::ComponentHandle;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustview=debug,info".into()),
        )
        .init();

    tracing::info!("Launching RustView (Phase 4 libmpv Engine Active)...");

    let data_dir = dirs_data_dir();
    std::fs::create_dir_all(&data_dir)?;

    let db_path = data_dir.join("library.lmdb");
    let cache_path = data_dir.join("thumbnails");

    // 1. Initialize Core Storage
    let library = Arc::new(Library::open(&db_path)?);

    // 2. Initialize SIMD Thumbnail Manager
    let thumbnails = Arc::new(ThumbnailManager::new(library.clone(), &cache_path, 512, 4)?);

    // 3. Initialize Native Hardware Accelerated Player Controller
    let player = Arc::new(PlayerController::new(RenderConfig::default())?);

    // 4. Initialize Slint Window & UI Bridge
    let main_window = MainWindow::new()?;
    let _bridge = UiBridge::new(
        main_window.clone_strong(),
        library,
        thumbnails,
        player,
        &data_dir,
    );

    tracing::info!("RustView Player & UI online.");
    main_window.run()?;

    Ok(())
}

fn dirs_data_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("rustview")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/share/rustview")
    } else {
        PathBuf::from("./data")
    }
}
