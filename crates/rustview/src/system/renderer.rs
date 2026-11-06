//! Which Slint renderer draws the windows: femtovg (GLX on X11) or skia (EGL everywhere). Videos are drawn with mpv's OpenGL render
//! API into textures shared with the renderer's GL context (`viewer::video_surface`), so both are OpenGL renderers; skia's EGL
//! context lets mpv decode with VAAPI without copying the frames back.
//! `RUSTVIEW_RENDERER=femtovg|skia` overrides the default for one start (a check or a workaround for a driver).

use std::sync::OnceLock;

/// The environment variable that picks the renderer for one start.
pub const RENDERER_VARIABLE: &str = "RUSTVIEW_RENDERER";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    Femtovg,
    Skia,
}

impl Renderer {
    /// The renderer when nothing else is asked for (maintainer decision 2026-10-26: skia; on the virtualized test machine both drew the UI alike and both
    /// carried the videos, and skia's EGL context is the one mpv's hardware decoders need without copies).
    pub const DEFAULT: Self = Self::Skia;

    /// The name Slint's backend selector takes.
    pub fn slint_name(self) -> &'static str {
        match self {
            Self::Femtovg => "femtovg",
            Self::Skia => "skia-opengl",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Femtovg => "femtovg",
            Self::Skia => "skia",
        }
    }
}

/// The renderer `request` (the value of [`RENDERER_VARIABLE`]) asks for; `Err` carries a value that names no renderer.
pub fn parse(request: Option<&str>) -> Result<Renderer, String> {
    match request.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        None | Some("" | "auto") => Ok(Renderer::DEFAULT),
        Some("femtovg") => Ok(Renderer::Femtovg),
        Some("skia" | "skia-opengl") => Ok(Renderer::Skia),
        Some(other) => Err(other.to_owned()),
    }
}

static CHOSEN: OnceLock<Renderer> = OnceLock::new();

/// Chooses the renderer once (at the start of `main`, before Slint's backend is selected) and logs the choice.
pub fn decide() -> Renderer {
    *CHOSEN.get_or_init(|| {
        let request = std::env::var(RENDERER_VARIABLE).ok();
        let renderer = parse(request.as_deref()).unwrap_or_else(|unknown| {
            tracing::warn!(
                variable = RENDERER_VARIABLE,
                value = %unknown,
                "[Startup: Renderer] Unknown value ignored (known: femtovg, skia, auto)"
            );
            Renderer::DEFAULT
        });
        tracing::info!(
            renderer = renderer.label(),
            requested = request.is_some(),
            "[Startup: Renderer] Renderer chosen"
        );
        renderer
    })
}

/// The renderer chosen at startup (the default before [`decide`] ran).
pub fn current() -> Renderer {
    CHOSEN.get().copied().unwrap_or(Renderer::DEFAULT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variable_picks_a_renderer_and_anything_else_keeps_the_default() {
        assert_eq!(parse(None), Ok(Renderer::DEFAULT));
        assert_eq!(parse(Some("auto")), Ok(Renderer::DEFAULT));
        assert_eq!(parse(Some(" Skia ")), Ok(Renderer::Skia));
        assert_eq!(parse(Some("skia-opengl")), Ok(Renderer::Skia));
        assert_eq!(parse(Some("femtovg")), Ok(Renderer::Femtovg));
        assert_eq!(parse(Some("software")), Err("software".to_owned()));
    }
}
