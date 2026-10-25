//! The feedback pulse of a Multiview on screen (`viewer::multiview::media_feedback`): every interaction with a medium that shows
//! one (a rating key, a saved picture) starts it on the instance and then calls [`MultiviewHandle::feedback_started`]; the player
//! keys and the wheel over a video (mute icon, volume bar) start it inside the instance, and the host asks
//! `take_feedback_request` after them. The host that presents the instance draws it and owns a single-shot timer that runs
//! only while a pulse is shown (`arm`).

use super::handle::MultiviewHandle;
use super::session::{self, Session};
use super::tab_host;
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::route::HostMode;
use slint::{Timer, TimerMode};
use std::time::Instant;

impl MultiviewHandle {
    /// A pulse was started on `instance`: its host draws it and arms the timer. UI thread only.
    pub fn feedback_started(&self, instance: &SharedInstance) {
        let mode = instance.lock().host_mode();
        match mode {
            HostMode::Fullscreen => session::with_session(Session::feedback_changed),
            _ => tab_host::feedback_changed(),
        }
    }
}

/// Arms `timer` for the next change of the feedback of `instance`, or stops it when nothing is shown. The timer is a single
/// shot that is armed again by each frame, so it never runs while the feedback is not on screen.
pub(super) fn arm(timer: &Timer, instance: &SharedInstance, frame: impl FnMut() + 'static) {
    match instance.lock().feedback_wakeup(Instant::now()) {
        Some(wait) => timer.start(TimerMode::SingleShot, wait, frame),
        None => timer.stop(),
    }
}

impl Session {
    /// The feedback changed (started, restarted): draw it and arm the timer.
    pub(super) fn feedback_changed(&mut self) {
        self.refresh();
        self.arm_feedback_timer();
    }

    /// An input the session does not follow itself (a player key, the wheel over a video) may have started a pulse or a volume
    /// bar on the instance: arm the timer for it.
    pub(super) fn arm_requested_feedback(&self) {
        if self.instance.lock().take_feedback_request() {
            self.arm_feedback_timer();
        }
    }

    fn arm_feedback_timer(&self) {
        arm(&self.feedback_timer, &self.instance, || {
            session::with_session(Session::on_feedback_frame);
        });
    }

    fn on_feedback_frame(&mut self) {
        self.instance.lock().poll_feedback(Instant::now());
        self.refresh();
        self.arm_feedback_timer();
    }
}
