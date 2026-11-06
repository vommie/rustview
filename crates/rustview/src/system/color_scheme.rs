//! The desktop's color scheme through the XDG desktop portal (`org.freedesktop.appearance` `color-scheme`, via `ashpd`):
//! read once at the start and followed while the app runs. KDE Plasma answers it on X11 and on Wayland. Without a portal
//! the scheme stays unknown (the app is dark then). A thread of its own waits for the portal; the UI hears of it through
//! the event loop.

use futures_lite::StreamExt as _;
use rustview_core::settings::theme::SystemScheme;

fn scheme(value: ashpd::desktop::settings::ColorScheme) -> SystemScheme {
    match value {
        ashpd::desktop::settings::ColorScheme::PreferDark => SystemScheme::Dark,
        ashpd::desktop::settings::ColorScheme::PreferLight => SystemScheme::Light,
        ashpd::desktop::settings::ColorScheme::NoPreference => SystemScheme::Unknown,
    }
}

/// Starts watching; `changed` runs on the UI thread with the first value and with every change.
pub fn watch(changed: fn(SystemScheme)) {
    let spawned = std::thread::Builder::new().name("color-scheme".into()).spawn(move || {
        let send = move |value: SystemScheme| {
            if let Err(e) = slint::invoke_from_event_loop(move || changed(value)) {
                tracing::debug!(error = %e, "[ColorScheme] The event loop is gone");
            }
        };
        let result: Result<(), ashpd::Error> = async_io::block_on(async {
            let settings = ashpd::desktop::settings::Settings::new().await?;
            let first = settings.color_scheme().await?;
            tracing::info!(scheme = ?first, "[ColorScheme] The desktop portal answered");
            send(scheme(first));
            let mut changes = settings.receive_color_scheme_changed().await?;
            while let Some(value) = changes.next().await {
                send(scheme(value));
            }
            Ok(())
        });
        if let Err(e) = result {
            tracing::info!(error = %e, "[ColorScheme] No color scheme from the desktop portal; the theme stays dark when it follows the system");
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "[ColorScheme] The watcher thread could not start");
    }
}
