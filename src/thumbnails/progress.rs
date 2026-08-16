use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelinePhase {
    Idle,
    PreloadingExisting,
    GeneratingMissing,
    Completed,
}

#[derive(Debug, Clone, Default)]
pub struct ThumbnailProgressSnapshot {
    pub phase: String,
    pub is_preloading: bool,
    pub preload_current: usize,
    pub preload_total: usize,
    pub is_generating: bool,
    pub generate_current: usize,
    pub generate_total: usize,
    pub active_workers: usize,
}

/// Thread-safe atomic progress tracker managing pipeline counters with underflow protection.
pub struct ThumbnailProgressTracker {
    preload_total: AtomicUsize,
    preload_done: AtomicUsize,
    generate_total: AtomicUsize,
    generate_done: AtomicUsize,
    active_workers: AtomicUsize,
}

impl ThumbnailProgressTracker {
    pub fn new() -> Self {
        Self {
            preload_total: AtomicUsize::new(0),
            preload_done: AtomicUsize::new(0),
            generate_total: AtomicUsize::new(0),
            generate_done: AtomicUsize::new(0),
            active_workers: AtomicUsize::new(0),
        }
    }

    pub fn reset(&self) {
        self.preload_total.store(0, Ordering::Release);
        self.preload_done.store(0, Ordering::Release);
        self.generate_total.store(0, Ordering::Release);
        self.generate_done.store(0, Ordering::Release);
        self.active_workers.store(0, Ordering::Release);
    }

    pub fn set_preload_total(&self, total: usize) {
        self.preload_total.store(total, Ordering::Release);
        self.preload_done.store(0, Ordering::Release);
    }

    pub fn increment_preload_done(&self) {
        self.preload_done.fetch_add(1, Ordering::AcqRel);
    }

    pub fn set_generate_total(&self, total: usize) {
        self.generate_total.store(total, Ordering::Release);
        self.generate_done.store(0, Ordering::Release);
    }

    pub fn increment_generate_done(&self) {
        self.generate_done.fetch_add(1, Ordering::AcqRel);
    }

    pub fn increment_active_workers(&self) {
        self.active_workers.fetch_add(1, Ordering::AcqRel);
    }

    pub fn decrement_active_workers(&self) {
        self.active_workers
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |val| {
                Some(val.saturating_sub(1))
            })
            .ok();
    }

    pub fn snapshot(&self) -> ThumbnailProgressSnapshot {
        let p_tot = self.preload_total.load(Ordering::Acquire);
        let p_cur = self.preload_done.load(Ordering::Acquire);
        let g_tot = self.generate_total.load(Ordering::Acquire);
        let g_cur = self.generate_done.load(Ordering::Acquire);
        let workers = self.active_workers.load(Ordering::Acquire);

        let is_preloading = p_tot > 0 && p_cur < p_tot;
        let is_generating = g_tot > 0 && g_cur < g_tot;

        let phase = if is_preloading {
            "Preloading Cached".to_string()
        } else if is_generating {
            "Generating".to_string()
        } else {
            "Ready".to_string()
        };

        ThumbnailProgressSnapshot {
            phase,
            is_preloading,
            preload_current: p_cur,
            preload_total: p_tot,
            is_generating,
            generate_current: g_cur,
            generate_total: g_tot,
            active_workers: workers,
        }
    }
}

impl Default for ThumbnailProgressTracker {
    fn default() -> Self {
        Self::new()
    }
}
