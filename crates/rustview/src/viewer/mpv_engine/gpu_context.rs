//! The GPU contexts mpv may use for an embedded video (pure, unit tested). A video draws into an X11 window of `RustView` (`wid`).
//! mpv's Wayland contexts ignore `wid` and open a window of their own, and mpv's automatic probe tries them first whenever the
//! session has a Wayland display: under `XWayland` every video would appear as a separate window. Only mpv's X11 contexts can draw
//! into `RustView`'s windows.

/// mpv's X11 contexts in mpv's own probe order (`mpv --gpu-context=help`).
const X11_CONTEXTS: [&str; 3] = ["x11vk", "x11egl", "x11"];

fn is_x11_context(entry: &str) -> bool {
    let name = entry.split_once(':').map_or(entry, |(name, _)| name);
    X11_CONTEXTS.contains(&name)
}

/// The `gpu-context` list for an embedded video, made from the list mpv has (`current`: the user's mpv configuration, comma
/// separated; empty or `auto` = mpv's probe, a trailing comma = the probe after the listed ones). The user's X11 contexts stay in
/// their order and every other one is dropped; where mpv would probe, it probes the X11 contexts only. `None` = `current` only
/// lists X11 contexts and can stay.
pub fn x11_gpu_contexts(current: &str) -> Option<String> {
    let current = current.trim();
    let entries: Vec<&str> = current.split(',').map(str::trim).collect();
    let probes = entries
        .iter()
        .any(|entry| entry.is_empty() || entry.eq_ignore_ascii_case("auto"));
    let mut kept: Vec<&str> = entries.iter().copied().filter(|entry| is_x11_context(entry)).collect();
    if kept.is_empty() || probes {
        let missing: Vec<&str> = X11_CONTEXTS
            .into_iter()
            .filter(|context| !kept.contains(context))
            .collect();
        kept.extend(missing);
    }
    let list = kept.join(",");
    (list != current).then_some(list)
}

#[cfg(test)]
mod tests {
    use super::x11_gpu_contexts;

    #[test]
    fn mpvs_own_probe_becomes_the_probe_over_the_x11_contexts() {
        for current in ["", "auto", " auto "] {
            assert_eq!(
                x11_gpu_contexts(current).as_deref(),
                Some("x11vk,x11egl,x11"),
                "{current:?}"
            );
        }
    }

    #[test]
    fn an_x11_only_list_stays() {
        for current in ["x11egl", "x11,x11egl", "x11vk,x11egl,x11"] {
            assert_eq!(x11_gpu_contexts(current), None, "{current:?}");
        }
    }

    #[test]
    fn wayland_contexts_are_dropped_and_the_x11_ones_keep_their_order() {
        assert_eq!(x11_gpu_contexts("wayland,x11egl").as_deref(), Some("x11egl"));
        assert_eq!(
            x11_gpu_contexts("waylandvk,x11,wayland,x11vk").as_deref(),
            Some("x11,x11vk")
        );
    }

    #[test]
    fn a_list_without_x11_context_probes_the_x11_ones() {
        for current in ["wayland", "waylandvk,wayland", "drm"] {
            assert_eq!(
                x11_gpu_contexts(current).as_deref(),
                Some("x11vk,x11egl,x11"),
                "{current:?}"
            );
        }
    }

    #[test]
    fn a_trailing_comma_probes_the_remaining_x11_contexts() {
        assert_eq!(x11_gpu_contexts("x11egl,").as_deref(), Some("x11egl,x11vk,x11"));
        assert_eq!(x11_gpu_contexts("wayland,x11,").as_deref(), Some("x11,x11vk,x11egl"));
    }

    #[test]
    fn options_of_an_entry_are_kept() {
        assert_eq!(x11_gpu_contexts("x11egl:foo=1"), None);
        assert_eq!(
            x11_gpu_contexts("wayland,x11egl:foo=1").as_deref(),
            Some("x11egl:foo=1")
        );
    }
}
