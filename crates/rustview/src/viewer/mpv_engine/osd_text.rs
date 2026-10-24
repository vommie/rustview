//! Text that mpv draws over the picture (the time badge of a video in the tab grid): mpv's OSD through `osd-overlay` with ASS,
//! so it shows over the native video window without a window of its own.

use super::MpvPlayer;

/// The id of the text overlay (every player has a namespace of its own).
const TIME_BADGE_ID: &str = "1";

/// A line of text in the bottom right corner of the video window, in physical pixels of that window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsdText {
    pub text: String,
    /// The size of the video window.
    pub area: (i32, i32),
    /// The height of the font.
    pub size: i32,
    /// The distance of the text's box from the bottom and the right edge.
    pub margin: i32,
}

impl MpvPlayer {
    /// Shows `text` on a dark box, or takes it away (`None`).
    pub fn set_osd_text(&self, text: Option<&OsdText>) {
        let result = match text {
            Some(osd) => {
                // A box behind the text instead of an outline (the outline size is its padding); nothing else of RustView
                // uses mpv's OSD text.
                let _ = self.mpv.set_property("osd-border-style", "background-box");
                let _ = self.mpv.set_property("osd-back-color", "#C0000000");
                let (w, h) = (osd.area.0.max(1), osd.area.1.max(1));
                let pad = (osd.size / 4).max(1);
                let data = format!(
                    "{{\\an3\\pos({},{})\\fs{}\\bord{pad}\\shad0}}{}",
                    w - osd.margin - pad,
                    h - osd.margin - pad,
                    osd.size.max(1),
                    osd.text
                );
                self.mpv.command(
                    "osd-overlay",
                    &[
                        TIME_BADGE_ID,
                        "ass-events",
                        &data,
                        &w.to_string(),
                        &h.to_string(),
                        "0",
                        "no",
                        "no",
                    ],
                )
            }
            None => self.mpv.command("osd-overlay", &[TIME_BADGE_ID, "none", ""]),
        };
        if let Err(e) = result {
            tracing::debug!(error = ?e, "[MpvPlayer: OsdText] mpv did not take the text");
        }
    }
}
