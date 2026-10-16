//! A single background thread that runs the most recently submitted job.
//!
//! Typing a query or switching folders quickly must not start one expensive job per event: while a job runs, a newer submission
//! replaces any older one that has not started yet, so at most one job runs and at most one waits.

use parking_lot::{Condvar, Mutex};
use std::sync::Arc;

type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Slot {
    pending: Option<Job>,
    shutdown: bool,
}

#[derive(Default)]
struct Shared {
    slot: Mutex<Slot>,
    wake: Condvar,
}

pub struct LatestJobWorker {
    shared: Arc<Shared>,
}

impl LatestJobWorker {
    pub fn spawn(name: &str) -> Self {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        let spawned = std::thread::Builder::new().name(name.to_string()).spawn(move || Self::run(&worker));
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[ListLoader] The list worker thread could not be started");
        }
        Self { shared }
    }

    fn run(shared: &Shared) {
        loop {
            let job = {
                let mut slot = shared.slot.lock();
                loop {
                    if slot.shutdown {
                        return;
                    }
                    if let Some(job) = slot.pending.take() {
                        break job;
                    }
                    shared.wake.wait(&mut slot);
                }
            };
            job();
        }
    }

    /// Queues `job`; an older job that has not started yet is dropped.
    pub fn submit(&self, job: impl FnOnce() + Send + 'static) {
        let mut slot = self.shared.slot.lock();
        if slot.shutdown {
            return;
        }
        slot.pending = Some(Box::new(job));
        drop(slot);
        self.shared.wake.notify_one();
    }

    /// Drops the waiting job and lets the thread end after the running one.
    pub fn shutdown(&self) {
        let mut slot = self.shared.slot.lock();
        slot.shutdown = true;
        slot.pending = None;
        drop(slot);
        self.shared.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn a_newer_submission_replaces_a_job_that_has_not_started() {
        let worker = LatestJobWorker::spawn("test-list-worker");
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let ran = Arc::new(AtomicUsize::new(0));

        worker.submit(move || {
            started_tx.send(()).ok();
            release_rx.recv().ok();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).expect("first job started");

        for value in 1..=3usize {
            let ran = ran.clone();
            worker.submit(move || ran.store(value, Ordering::SeqCst));
        }
        release_tx.send(()).ok();

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while ran.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(ran.load(Ordering::SeqCst), 3, "only the newest waiting job runs");
        worker.shutdown();
    }

    #[test]
    fn submissions_after_shutdown_are_ignored() {
        let worker = LatestJobWorker::spawn("test-list-worker-2");
        worker.shutdown();
        let ran = Arc::new(AtomicUsize::new(0));
        let flag = ran.clone();
        worker.submit(move || flag.store(1, Ordering::SeqCst));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }
}
