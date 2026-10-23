//! The identity under which the desktop knows `RustView`.

/// Application id (reverse DNS). It is the Wayland `app_id` / X11 `WM_CLASS` of every window and must equal the name of the desktop
/// file (`packaging/io.github.vommie.rustview.desktop`) and of the `AppStream` component, so the compositor finds the launcher icon.
pub const APP_ID: &str = "io.github.vommie.rustview";
