//! Which window system the windows of `RustView` use, decided from what the environment says about the session (pure, unit tested).

/// The environment variable that picks the window system in a Wayland session: `x11` asks for `XWayland`, `wayland` for native Wayland
/// (the default since `video-in-slint` M7, user decision 2026-10-26); `auto` or unset is the default.
pub const REQUEST_VARIABLE: &str = "RUSTVIEW_WINDOW_SYSTEM";

/// The window system the windows of `RustView` are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSystem {
    X11,
    Wayland,
}

/// What the environment says about the session, only as far as the choice needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionEnv {
    /// `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set and not empty (the test winit makes).
    pub wayland_display: bool,
    /// `XDG_SESSION_TYPE` is `wayland`.
    pub wayland_session: bool,
    /// `DISPLAY` is set and not empty: there is an X server to ask (in a Wayland session: `XWayland`).
    pub x11_display: bool,
    /// The value of [`REQUEST_VARIABLE`], if it is set.
    pub request: Option<String>,
}

/// Why `RustView` runs on native Wayland.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaylandReason {
    /// A Wayland session: native Wayland is the default.
    Session,
    /// [`REQUEST_VARIABLE`] is `wayland`.
    Requested,
    /// `XWayland` was asked for, but `DISPLAY` is not set: the session has no `XWayland`.
    NoXDisplay,
    /// `XWayland` was asked for, but the X server of `DISPLAY` did not answer.
    XDisplayUnreachable,
}

/// The window system `RustView` uses, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// An X11 session (no Wayland display).
    X11Session,
    /// A Wayland session in which `XWayland` was asked for (or that has no Wayland display): `RustView` runs as an X11 program, so the
    /// X11 features (dialog parents, dragging files out, the pointer lock) work as on X11.
    XWayland,
    /// Native Wayland: the X11 features (dialog parents, dragging files out, the pointer lock) degrade.
    Wayland(WaylandReason),
    /// Neither a Wayland nor an X display: winit's own error ends the start, as it always did.
    NoDisplay,
}

/// Something about [`REQUEST_VARIABLE`] the log should mention besides the choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestNote {
    /// The variable has a value that means nothing; it is ignored.
    Unknown(String),
    /// Native Wayland was asked for, but the session has no Wayland display.
    WaylandUnavailable,
}

/// The result of [`choose`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub choice: Choice,
    pub note: Option<RequestNote>,
}

impl Choice {
    /// The window system of this choice; `None` without any display.
    pub fn window_system(self) -> Option<WindowSystem> {
        match self {
            Self::X11Session | Self::XWayland => Some(WindowSystem::X11),
            Self::Wayland(_) => Some(WindowSystem::Wayland),
            Self::NoDisplay => None,
        }
    }

    /// The name used in logs and telemetry.
    pub fn label(self) -> &'static str {
        match self {
            Self::X11Session => "X11",
            Self::XWayland => "XWayland",
            Self::Wayland(_) => "Wayland",
            Self::NoDisplay => "none",
        }
    }
}

enum Request {
    Auto,
    Wayland,
    X11,
}

fn parse_request(value: Option<&str>) -> (Request, Option<RequestNote>) {
    match value.map(str::trim) {
        None | Some("") => (Request::Auto, None),
        Some(value) if value.eq_ignore_ascii_case("auto") => (Request::Auto, None),
        Some(value) if value.eq_ignore_ascii_case("wayland") => (Request::Wayland, None),
        Some(value) if value.eq_ignore_ascii_case("x11") || value.eq_ignore_ascii_case("xwayland") => {
            (Request::X11, None)
        }
        Some(value) => (Request::Auto, Some(RequestNote::Unknown(value.to_owned()))),
    }
}

/// Decides the window system. A Wayland session runs on native Wayland unless `XWayland` is asked for; without a Wayland display it is X11.
/// `x11_answers` is called only when `XWayland` is asked for in a Wayland session that also has an X display: winit would take Wayland
/// there, and it cannot try Wayland after its X11 connection failed.
pub fn choose(env: &SessionEnv, x11_answers: impl FnOnce() -> bool) -> Decision {
    let (request, mut note) = parse_request(env.request.as_deref());
    if matches!(request, Request::Wayland) && !env.wayland_display {
        note = Some(RequestNote::WaylandUnavailable);
    }
    let choice = match (env.wayland_display, env.x11_display) {
        (true, x11_display) => match request {
            Request::X11 if !x11_display => Choice::Wayland(WaylandReason::NoXDisplay),
            Request::X11 if x11_answers() => Choice::XWayland,
            Request::X11 => Choice::Wayland(WaylandReason::XDisplayUnreachable),
            Request::Wayland => Choice::Wayland(WaylandReason::Requested),
            Request::Auto => Choice::Wayland(WaylandReason::Session),
        },
        (false, true) if env.wayland_session => Choice::XWayland,
        (false, true) => Choice::X11Session,
        (false, false) => Choice::NoDisplay,
    };
    Decision { choice, note }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn env(wayland_display: bool, wayland_session: bool, x11_display: bool, request: Option<&str>) -> SessionEnv {
        SessionEnv {
            wayland_display,
            wayland_session,
            x11_display,
            request: request.map(str::to_owned),
        }
    }

    /// Runs `choose` and returns the decision and how often the X display was asked.
    fn decide(env: &SessionEnv, x11_answers: bool) -> (Decision, u32) {
        let asked = Cell::new(0);
        let decision = choose(env, || {
            asked.set(asked.get() + 1);
            x11_answers
        });
        (decision, asked.get())
    }

    #[test]
    fn an_x11_session_stays_on_x11_without_asking_the_display() {
        for request in [None, Some("x11"), Some("auto")] {
            let (decision, asked) = decide(&env(false, false, true, request), false);
            assert_eq!(decision.choice, Choice::X11Session, "{request:?}");
            assert_eq!(decision.choice.window_system(), Some(WindowSystem::X11));
            assert_eq!(decision.note, None);
            assert_eq!(asked, 0);
        }
    }

    #[test]
    fn a_wayland_session_runs_on_native_wayland_without_asking_the_x_display() {
        for (wayland_session, x11_display) in [(true, true), (true, false), (false, true)] {
            let (decision, asked) = decide(&env(true, wayland_session, x11_display, None), true);
            assert_eq!(decision.choice, Choice::Wayland(WaylandReason::Session));
            assert_eq!(decision.choice.window_system(), Some(WindowSystem::Wayland));
            assert_eq!(asked, 0);
        }
    }

    #[test]
    fn xwayland_can_be_requested() {
        for value in ["x11", "X11", " xwayland "] {
            let (decision, asked) = decide(&env(true, true, true, Some(value)), true);
            assert_eq!(decision.choice, Choice::XWayland, "{value:?}");
            assert_eq!(decision.choice.window_system(), Some(WindowSystem::X11));
            assert_eq!(decision.note, None);
            assert_eq!(asked, 1);
        }
    }

    #[test]
    fn requested_xwayland_that_does_not_answer_falls_back_to_native_wayland() {
        let (decision, asked) = decide(&env(true, true, true, Some("x11")), false);
        assert_eq!(decision.choice, Choice::Wayland(WaylandReason::XDisplayUnreachable));
        assert_eq!(asked, 1);
    }

    #[test]
    fn requested_xwayland_without_x_display_stays_native() {
        let (decision, asked) = decide(&env(true, true, false, Some("x11")), true);
        assert_eq!(decision.choice, Choice::Wayland(WaylandReason::NoXDisplay));
        assert_eq!(asked, 0);
    }

    #[test]
    fn a_wayland_session_type_without_wayland_display_is_xwayland_without_asking() {
        // winit takes X11 here anyway (no Wayland display), so there is nothing to fall back to.
        let (decision, asked) = decide(&env(false, true, true, None), false);
        assert_eq!(decision.choice, Choice::XWayland);
        assert_eq!(asked, 0);
    }

    #[test]
    fn no_display_at_all_leaves_the_error_to_winit() {
        let (decision, asked) = decide(&env(false, true, false, None), true);
        assert_eq!(decision.choice, Choice::NoDisplay);
        assert_eq!(decision.choice.window_system(), None);
        assert_eq!(asked, 0);
    }

    #[test]
    fn native_wayland_can_be_requested() {
        for value in ["wayland", "Wayland", " WAYLAND "] {
            let (decision, asked) = decide(&env(true, true, true, Some(value)), true);
            assert_eq!(decision.choice, Choice::Wayland(WaylandReason::Requested), "{value:?}");
            assert_eq!(decision.note, None);
            assert_eq!(asked, 0);
        }
    }

    #[test]
    fn a_wayland_request_without_wayland_display_is_ignored_with_a_note() {
        let (decision, _) = decide(&env(false, false, true, Some("wayland")), true);
        assert_eq!(decision.choice, Choice::X11Session);
        assert_eq!(decision.note, Some(RequestNote::WaylandUnavailable));
    }

    #[test]
    fn auto_and_empty_requests_are_the_default() {
        for value in ["auto", "AUTO", "", "  "] {
            let (decision, _) = decide(&env(true, true, true, Some(value)), true);
            assert_eq!(decision.choice, Choice::Wayland(WaylandReason::Session), "{value:?}");
            assert_eq!(decision.note, None, "{value:?}");
        }
    }

    #[test]
    fn an_unknown_request_is_ignored_with_a_note() {
        let (decision, _) = decide(&env(true, true, true, Some("x12")), true);
        assert_eq!(decision.choice, Choice::Wayland(WaylandReason::Session));
        assert_eq!(decision.note, Some(RequestNote::Unknown("x12".to_owned())));
    }

    #[test]
    fn labels_name_the_window_system_in_use() {
        assert_eq!(Choice::X11Session.label(), "X11");
        assert_eq!(Choice::XWayland.label(), "XWayland");
        assert_eq!(Choice::Wayland(WaylandReason::NoXDisplay).label(), "Wayland");
        assert_eq!(Choice::NoDisplay.label(), "none");
    }
}
