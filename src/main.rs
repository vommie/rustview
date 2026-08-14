slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tracing Logger initialisieren
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustview=debug,info".into()),
        )
        .init();

    tracing::info!("RustView Bootstrapping...");

    // Smoke Test 1: BLAKE3 Initialisierung prüfen
    let hash = blake3::hash(b"rustview_init");
    tracing::info!("BLAKE3 self-test hash: {}", hash.to_hex());

    // Smoke Test 2: Slint Window initialisieren
    let main_window = MainWindow::new()?;
    tracing::info!("UI Engine initialized successfully.");

    main_window.run()?;
    Ok(())
}
