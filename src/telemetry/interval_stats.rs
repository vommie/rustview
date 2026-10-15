//! Percentile summary of timing samples (timer ticks, frame gaps, render durations) for the performance probe.

use std::time::Duration;

/// Collects durations and summarizes them; sorting happens once in [`IntervalStats::summary`], never while recording.
#[derive(Debug, Default, Clone)]
pub struct IntervalStats {
    samples_us: Vec<u32>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IntervalSummary {
    pub count: usize,
    pub mean_us: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
    pub over_25ms: usize,
    pub over_50ms: usize,
    pub over_100ms: usize,
}

impl IntervalStats {
    /// `const` constructor for statics.
    pub const fn new_const() -> Self {
        Self { samples_us: Vec::new() }
    }

    pub fn record(&mut self, duration: Duration) {
        self.samples_us
            .push(u32::try_from(duration.as_micros()).unwrap_or(u32::MAX));
    }

    pub fn len(&self) -> usize {
        self.samples_us.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples_us.is_empty()
    }

    pub fn clear(&mut self) {
        self.samples_us.clear();
    }

    pub fn summary(&self) -> IntervalSummary {
        if self.samples_us.is_empty() {
            return IntervalSummary::default();
        }
        let mut sorted = self.samples_us.clone();
        sorted.sort_unstable();
        let count = sorted.len();
        let sum: u64 = sorted.iter().map(|&v| u64::from(v)).sum();
        let at = |q: f64| -> u64 {
            let rank = ((q * count as f64).ceil() as usize).clamp(1, count);
            u64::from(sorted[rank - 1])
        };
        let over = |limit_us: u32| sorted.len() - sorted.partition_point(|&v| v <= limit_us);
        IntervalSummary {
            count,
            mean_us: sum / count as u64,
            p50_us: at(0.50),
            p95_us: at(0.95),
            p99_us: at(0.99),
            max_us: u64::from(sorted[count - 1]),
            over_25ms: over(25_000),
            over_50ms: over(50_000),
            over_100ms: over(100_000),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats_of(ms: &[u64]) -> IntervalStats {
        let mut s = IntervalStats::default();
        for &m in ms {
            s.record(Duration::from_millis(m));
        }
        s
    }

    #[test]
    fn empty_stats_summarize_to_zero() {
        assert_eq!(IntervalStats::default().summary(), IntervalSummary::default());
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let s = stats_of(&(1..=100).collect::<Vec<_>>()).summary();
        assert_eq!(s.count, 100);
        assert_eq!(s.p50_us, 50_000);
        assert_eq!(s.p95_us, 95_000);
        assert_eq!(s.p99_us, 99_000);
        assert_eq!(s.max_us, 100_000);
    }

    #[test]
    fn jank_counters_are_strictly_greater_than_the_limit() {
        let s = stats_of(&[10, 25, 26, 50, 51, 100, 101]).summary();
        assert_eq!(s.over_25ms, 5);
        assert_eq!(s.over_50ms, 3);
        assert_eq!(s.over_100ms, 1);
    }
}
