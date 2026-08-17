use rustview::library::Library;
use rustview::player::{PlayerController, RenderConfig};
use rustview::settings::{SettingsManager, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use rustview::thumbnails::ThumbnailManager;
use rustview::ui::{MainWindow, UiBridge};
use slint::{CloseRequestResponse, ComponentHandle};
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let data_dir = dirs_data_dir();
    let config_dir = dirs_config_dir();

    // 1. Check for standalone CLI migration arguments before starting UI
    if rustview::xnviewdb_migrate::is_migration_requested(&args) {
        tracing_subscriber::fmt()
            .with_target(false)
            .with_thread_names(false)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "rustview=info".into()),
            )
            .init();

        match rustview::xnviewdb_migrate::run_cli(&args, &data_dir, &config_dir) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("Migration Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    // 2. Standard GUI Startup with structured logging output
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_names(true)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustview=info".into()),
        )
        .init();

    tracing::info!("Launching RustView with Full Tracing & Persistent State...");

    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&config_dir)?;

    let db_path = data_dir.join("library.lmdb");
    let cache_path = data_dir.join("thumbnails");

    // Initialize Settings System
    let settings = SettingsManager::new(&config_dir);

    // Initialize Core LMDB Storage
    let library = Arc::new(Library::open(&db_path)?);

    // Initialize SIMD Thumbnail Manager
    let thumbnails = Arc::new(ThumbnailManager::new(
        library.clone(),
        &cache_path,
        settings.get().performance.l1_cache_slots,
        settings.get().performance.worker_threads,
    )?);

    // Initialize Native Player Controller
    let player = Arc::new(PlayerController::new(RenderConfig::default())?);

    // Initialize Slint Window
    let main_window = MainWindow::new()?;

    // Restore Window Size, Position & State (Guarded by SPoT Minimum Geometry Constants)
    let win_state = settings.get().window;
    if win_state.width >= MIN_WINDOW_WIDTH && win_state.height >= MIN_WINDOW_HEIGHT {
        main_window
            .window()
            .set_size(slint::PhysicalSize::new(win_state.width, win_state.height));
    }
    if let (Some(x), Some(y)) = (win_state.x, win_state.y) {
        main_window
            .window()
            .set_position(slint::PhysicalPosition::new(x, y));
    }
    if win_state.is_maximized {
        main_window.window().set_maximized(true);
    }

    // Persist Window Geometry on Close Request
    let settings_for_close = settings.clone();
    let main_weak = main_window.as_weak();
    main_window.window().on_close_requested(move || {
        if let Some(w) = main_weak.upgrade() {
            let win = w.window();
            let is_max = win.is_maximized();
            let size = win.size();
            let pos = win.position();

            let mut current_settings = settings_for_close.get();
            current_settings.window.is_maximized = is_max;
            if !is_max {
                current_settings.window.width = size.width;
                current_settings.window.height = size.height;
                current_settings.window.x = Some(pos.x);
                current_settings.window.y = Some(pos.y);
            }
            settings_for_close.save_quiet(&current_settings);
        }
        CloseRequestResponse::HideWindow
    });

    // Wire UI Bridge
    let _bridge = UiBridge::new(
        main_window.clone_strong(),
        library,
        thumbnails,
        player,
        settings,
        &data_dir,
    );

    tracing::info!("RustView UI & Storage Engine Ready.");
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

fn dirs_config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("rustview")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/rustview")
    } else {
        PathBuf::from("./config")
    }
}
