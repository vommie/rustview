use rustview::library::Library;
use rustview::settings::{LogObscurer, SettingsManager, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use rustview::system::SystemDisplayService;
use rustview::system::termination;
use rustview::thumbnails::ThumbnailManager;
use rustview::ui::{MainWindow, UiBridge, shutdown};
use rustview::util::startup_clock;
use slint::{CloseRequestResponse, ComponentHandle};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone)]
struct WindowGeometryTracker {
    last_unmaximized_width: u32,
    last_unmaximized_height: u32,
    last_unmaximized_x: Option<i32>,
    last_unmaximized_y: Option<i32>,
    active_monitor_x: i32,
    active_monitor_y: i32,
    is_maximized: bool,
    initialization_settled: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    startup_clock::begin();
    let args: Vec<String> = std::env::args().collect();
    let data_dir = dirs_data_dir();
    let config_dir = dirs_config_dir();

    // 1. Standalone Headless CLI API for external tools (e.g. PyCut)
    if rustview::cli_api::is_api_requested(&args) {
        match rustview::cli_api::run_cli_api(&args, &data_dir) {
            Ok(exit_code) => std::process::exit(exit_code),
            Err(e) => {
                eprintln!("API Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    // 2. Standalone CLI Migration Tool (XnViewMP)
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

    // 3. Standard GUI Startup with structured logging output
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_names(true)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustview=info".into()),
        )
        .init();

    tracing::info!("Launching RustView with Full Tracing & Persistent State...");
    startup_clock::mark("Main");

    // Ctrl+C, `kill` and a closed terminal run the same orderly shutdown as the close button (before any thread exists).
    termination::install_signal_handlers();

    // Diagnostic System, Distro, Kernel, Session, and Monitor info emission (Runs ONLY ONCE at startup)
    SystemDisplayService::log_system_diagnostics();
    startup_clock::mark("Diagnostics");

    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(&config_dir)?;

    let db_path = data_dir.join("library.lmdb");
    let cache_path = data_dir.join("thumbnails");

    // Initialize Settings System and live Log Privacy Obscurer
    let settings = SettingsManager::new(&config_dir);
    LogObscurer::set_obfuscate(settings.get().debug.obfuscate_logs);
    startup_clock::mark("Settings");

    // Initialize Core LMDB Storage
    let library = Arc::new(Library::open(&db_path)?);
    startup_clock::mark("Library");

    // TEMPORARY DEBUG FEATURE (remove together with src/library/folder_reset.rs):
    // `--reset-folder-tree` resets the folder tree (SPoT) once. Media records and their folder paths are preserved.
    rustview::library::folder_reset::run_if_requested(&args, &library, &settings);

    // Scope the in-memory folder tree snapshot to the configured root (None = empty tree, e.g. first start).
    let configured_root = settings
        .get()
        .general
        .root_folder
        .as_ref()
        .map(|p| rustview::library::folders::normalize_path(p));
    tracing::info!(
        has_root = configured_root.is_some(),
        "[FolderTree: Scope] Registering configured root as folder tree scope (no filesystem scan)"
    );
    library.set_folder_tree_root(configured_root);

    // Initialize SIMD Thumbnail Manager
    let thumbnails = Arc::new(ThumbnailManager::new(
        library.clone(),
        &cache_path,
        settings.get().performance.l1_cache_slots,
        settings.get().performance.worker_threads,
    )?);
    startup_clock::mark("Thumbnails");

    // Initialize Slint Window
    // The action bar of the Multiview floats in a window of its own; it has to be an override-redirect window, which Slint only
    // lets us request before the first window is created.
    slint::BackendSelector::new()
        .with_winit_window_attributes_hook(
            rustview::ui::multiview::bar_overlay::window_attributes_hook,
        )
        .select()?;

    startup_clock::mark("Backend");
    let main_window = MainWindow::new()?;
    startup_clock::mark("MainWindowCreated");
    // The restored window state is needed before the first frame: Slint renders it before it maps the window (X11), and the
    // maximized state must be in place at that moment.
    let win_state = settings.get().window;
    let should_maximize = win_state.is_maximized;
    let main_weak_render = main_window.as_weak();
    if let Err(e) = main_window
        .window()
        .set_rendering_notifier(move |state, _| match state {
            // Called while the window exists but is not mapped yet.
            slint::RenderingState::RenderingSetup => {
                startup_clock::mark("GlReady");
                if should_maximize
                    && let Some(window) = main_weak_render.upgrade()
                    && let Some(xid) = rustview::viewer::get_raw_window_xid(window.window())
                {
                    let written = rustview::viewer::set_x11_initial_maximized(xid);
                    tracing::info!(written, "[WindowState: Init] Maximized state written before the window is mapped");
                }
            }
            slint::RenderingState::AfterRendering => startup_clock::mark_first_frame(),
            _ => {}
        })
    {
        tracing::debug!(error = %e, "[Startup: Stage] No rendering notifier; the first frame is not logged");
    }

    // Register XDG Desktop Application ID after platform initialization
    if let Err(e) = slint::set_xdg_app_id("org.rustview.app") {
        tracing::warn!(error = %e, "[WindowState: Init] Failed to register XDG App ID");
    }

    // Restore Window Size, Position & State on the Target Monitor
    let initial_width = win_state.width.max(MIN_WINDOW_WIDTH);
    let initial_height = win_state.height.max(MIN_WINDOW_HEIGHT);
    // A window that was closed maximized is created with the size it had then: its first frame already has the size the
    // window manager maximizes it to, so nothing is laid out again.
    let (creation_width, creation_height) =
        if should_maximize && win_state.maximized_width > 0 && win_state.maximized_height > 0 {
            (win_state.maximized_width, win_state.maximized_height)
        } else {
            (initial_width, initial_height)
        };

    // The saved position is only used if it is on the screen (a monitor may have been unplugged). The size of the X screen is
    // enough for that and costs one short connection; the monitor detection starts subprocesses (kscreen-doctor, xrandr) and
    // must not delay the first window. Without an X screen (Wayland) the compositor places the window.
    let target_position = match (win_state.x, win_state.y) {
        (Some(x), Some(y)) => match rustview::viewer::x11_root_size() {
            Some((width, height)) if (0..width as i32).contains(&(x + 50)) && (0..height as i32).contains(&(y + 50)) => Some((x, y)),
            Some(_) => {
                tracing::info!("[WindowState: Init] The saved window position is outside the screen; the window manager places the window");
                None
            }
            None => Some((x, y)),
        },
        _ => None,
    };

    // Always set size and target position first so the window lands on the correct monitor
    main_window
        .window()
        .set_size(slint::PhysicalSize::new(creation_width, creation_height));

    if let Some((x, y)) = target_position {
        main_window.window().set_position(slint::PhysicalPosition::new(x, y));
    }

    if should_maximize {
        main_window.window().set_maximized(true);
    }

    // Geometry tracker maintaining clean unmaximized dimensions and active monitor coordinates
    let tracker = Arc::new(parking_lot::Mutex::new(WindowGeometryTracker {
        last_unmaximized_width: initial_width,
        last_unmaximized_height: initial_height,
        last_unmaximized_x: target_position.map(|(x, _)| x),
        last_unmaximized_y: target_position.map(|(_, y)| y),
        active_monitor_x: target_position.map_or(0, |(x, _)| x),
        active_monitor_y: target_position.map_or(0, |(_, y)| y),
        is_maximized: should_maximize,
        initialization_settled: false,
    }));

    // Active Window Mount & Maximization Watcher:
    let watcher_timer = Rc::new(slint::Timer::default());
    let watcher_timer_clone = watcher_timer.clone();
    let main_weak_watcher = main_window.as_weak();
    let tracker_watcher = tracker.clone();
    let mut attempts = 0usize;

    watcher_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(40),
        move || {
            attempts += 1;
            let Some(w) = main_weak_watcher.upgrade() else {
                watcher_timer_clone.stop();
                return;
            };

            let win = w.window();
            let xid_opt = rustview::viewer::get_raw_window_xid(win);
            let x11_max = xid_opt.and_then(rustview::viewer::is_x11_window_maximized);
            let cur_size = win.size();

            let is_window_ready = xid_opt.is_some() || cur_size.width > 0;

            if should_maximize {
                if x11_max == Some(true) {
                    let mut t = tracker_watcher.lock();
                    t.is_maximized = true;
                    t.initialization_settled = true;
                    watcher_timer_clone.stop();
                    return;
                }

                if is_window_ready && attempts < 30 {
                    win.set_maximized(true);

                    if let Some(xid) = xid_opt {
                        rustview::viewer::force_x11_maximized(xid, true);
                    }
                }
            } else if is_window_ready {
                let mut t = tracker_watcher.lock();
                t.is_maximized = false;
                t.initialization_settled = true;
                watcher_timer_clone.stop();
                return;
            }

            if attempts >= 35 {
                let mut t = tracker_watcher.lock();
                t.initialization_settled = true;
                watcher_timer_clone.stop();
            }
        },
    );

    // Continuous runtime window state tracker: accurately tracks monitor position and dimensions.
    // NOTE: Does NOT call display detection or target resolution in the loop, completely preventing CPU and log spam.
    let runtime_tracker_timer = slint::Timer::default();
    let main_weak_tracker = main_window.as_weak();
    let tracker_runtime = tracker.clone();

    runtime_tracker_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(350),
        move || {
            let Some(w) = main_weak_tracker.upgrade() else { return; };
            let win = w.window();

            // A termination signal is handled like a click on the close button of the window.
            if let Some(signum) = termination::received_signal()
                && !shutdown::is_started()
            {
                tracing::info!(
                    signal = termination::signal_name(signum),
                    "[Shutdown: Signal] Termination signal received; closing the application in order"
                );
                if let Err(e) = win.try_dispatch_event(slint::platform::WindowEvent::CloseRequested) {
                    tracing::error!(error = %e, "[Shutdown: Signal] The close request could not be delivered");
                    termination::exit_now(termination::EXIT_SHUTDOWN_TIMEOUT);
                }
                return;
            }

            let mut t = tracker_runtime.lock();
            if !t.initialization_settled {
                return;
            }

            let slint_max = win.is_maximized();
            let xid_opt = rustview::viewer::get_raw_window_xid(win);
            let x11_max = xid_opt.and_then(rustview::viewer::is_x11_window_maximized);
            let is_max = x11_max.unwrap_or(slint_max);

            let cur_size = win.size();
            let cur_pos = win.position();

            t.is_maximized = is_max;
            t.active_monitor_x = cur_pos.x;
            t.active_monitor_y = cur_pos.y;

            if !is_max && cur_size.width >= MIN_WINDOW_WIDTH && cur_size.height >= MIN_WINDOW_HEIGHT {
                t.last_unmaximized_width = cur_size.width;
                t.last_unmaximized_height = cur_size.height;
                t.last_unmaximized_x = Some(cur_pos.x);
                t.last_unmaximized_y = Some(cur_pos.y);
            } else if is_max {
                // When maximized, keep unmaximized anchor aligned with the active monitor so unmaximizing never jumps displays
                if let Some(lux) = t.last_unmaximized_x {
                    let on_active_display = (lux - cur_pos.x).abs() < 1920;
                    if !on_active_display {
                        t.last_unmaximized_x = Some(cur_pos.x + 60);
                        t.last_unmaximized_y = Some(cur_pos.y + 60);
                    }
                }
            }
        },
    );

    // Keep a handle for the background folder audit (the library is moved into the UI bridge below)
    let audit_db = library.raw_db().clone();

    // Wire UI Bridge
    let bridge = Rc::new(UiBridge::new(
        main_window.clone_strong(),
        library,
        thumbnails,
        settings.clone(),
        &data_dir,
    ));

    // Close request (title bar, Alt+F4, termination signal): persist the window state, then shut everything down in order.
    let main_weak_close = main_window.as_weak();
    let bridge_weak = Rc::downgrade(&bridge);
    main_window.window().on_close_requested(move || {
        if !shutdown::begin() {
            return CloseRequestResponse::HideWindow;
        }
        if let Some(w) = main_weak_close.upgrade() {
            persist_window_geometry(&w, &tracker, &settings);
        }
        if let Some(bridge) = bridge_weak.upgrade() {
            bridge.shutdown();
        }
        if let Err(e) = slint::quit_event_loop() {
            tracing::warn!(error = %e, "[Shutdown: EventLoop] The event loop could not be asked to quit");
        }
        CloseRequestResponse::HideWindow
    });

    tracing::info!("RustView UI & Storage Engine Ready.");
    startup_clock::mark("BridgeReady");

    // Read-only folder schema + media path resolution audit on a background thread (never blocks the UI)
    rustview::library::FolderDiagnostics::spawn_background_audit(audit_db, "startup");

    startup_clock::mark("EventLoop");
    let result = main_window.run();

    if shutdown::begin() {
        tracing::warn!("[Shutdown: EventLoop] The event loop ended without a close request; shutting down now");
        bridge.shutdown();
    }
    result?;

    tracing::info!("[RustView: Shutdown] Event loop finished. Application exiting cleanly.");
    Ok(())
}

/// Persists the geometry and the monitor of the main window (called when the window is asked to close).
fn persist_window_geometry(
    w: &MainWindow,
    tracker: &parking_lot::Mutex<WindowGeometryTracker>,
    settings: &SettingsManager,
) {
    let win = w.window();
    let slint_max = win.is_maximized();
    let xid_opt = rustview::viewer::get_raw_window_xid(win);
    let x11_max = xid_opt.and_then(rustview::viewer::is_x11_window_maximized);
    let cur_size = win.size();
    let cur_pos = win.position();

    let t = tracker.lock();
    let is_max = x11_max.unwrap_or(false) || slint_max || t.is_maximized;

    let final_width = if !is_max && cur_size.width >= MIN_WINDOW_WIDTH {
        cur_size.width
    } else {
        t.last_unmaximized_width
    };

    let final_height = if !is_max && cur_size.height >= MIN_WINDOW_HEIGHT {
        cur_size.height
    } else {
        t.last_unmaximized_height
    };

    let (final_x, final_y) = if is_max {
        // If maximized, persist the target screen's anchor position
        let anchor_x = t.last_unmaximized_x.unwrap_or(t.active_monitor_x + 60);
        let anchor_y = t.last_unmaximized_y.unwrap_or(t.active_monitor_y + 60);
        (Some(anchor_x), Some(anchor_y))
    } else {
        (t.last_unmaximized_x.or(Some(cur_pos.x)), t.last_unmaximized_y.or(Some(cur_pos.y)))
    };

    let mut current_settings = settings.get();
    current_settings.window.is_maximized = is_max;
    if is_max && cur_size.width >= MIN_WINDOW_WIDTH && cur_size.height >= MIN_WINDOW_HEIGHT {
        current_settings.window.maximized_width = cur_size.width;
        current_settings.window.maximized_height = cur_size.height;
    }
    current_settings.window.width = final_width;
    current_settings.window.height = final_height;
    current_settings.window.x = final_x;
    current_settings.window.y = final_y;
    settings.save_quiet(&current_settings);

    tracing::info!(
        is_max,
        saved_x = ?final_x,
        saved_y = ?final_y,
        saved_size = format!("{}x{}", final_width, final_height),
        active_screen_x = t.active_monitor_x,
        "[Shutdown: WindowState] Persisted window geometry and monitor anchor cleanly"
    );
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
