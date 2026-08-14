slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rustview=debug,info".into()),
        )
        .init();

    tracing::info!("RustView Core Initialized (Phase 1 Ready)");

    let main_window = MainWindow::new()?;
    main_window.run()?;
    Ok(())
}
