//! The last log lines in memory, each in both renderings (real and private).

use chrono::{DateTime, Local};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::Level;

/// One log line as the capture layer wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Increasing number, never reused (the log window asks for the lines after the last one it has).
    pub seq: u64,
    pub time: DateTime<Local>,
    pub level: Level,
    /// The message and its fields with every name shown.
    pub real: String,
    /// The same with file names, paths, folders, tags and searches hidden (`LogObscurer`).
    pub private: String,
}

impl LogLine {
    pub fn text(&self, private: bool) -> &str {
        if private { &self.private } else { &self.real }
    }

    /// The line as it is written to a log file (date, time, level, text).
    pub fn file_line(&self, private: bool) -> String {
        format!(
            "{} {:>5} {}\n",
            self.time.format("%Y-%m-%d %H:%M:%S%.3f"),
            self.level.as_str(),
            self.text(private)
        )
    }

    /// Whether the line passes the filter of the log window and the report (`Level::TRACE` = everything).
    pub fn at_least(&self, level: Level) -> bool {
        self.level <= level
    }
}

/// A bounded ring of [`LogLine`]s; the oldest line goes when a new one comes and the ring is full.
#[derive(Debug)]
pub struct LogRing {
    inner: Mutex<Ring>,
}

#[derive(Debug)]
struct Ring {
    capacity: usize,
    next_seq: u64,
    lines: VecDeque<Arc<LogLine>>,
}

impl LogRing {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Ring {
                capacity: capacity.max(1),
                next_seq: 1,
                lines: VecDeque::new(),
            }),
        }
    }

    /// Adds a line and returns it.
    pub fn push(&self, time: DateTime<Local>, level: Level, real: String, private: String) -> Arc<LogLine> {
        let mut ring = self.inner.lock();
        let line = Arc::new(LogLine {
            seq: ring.next_seq,
            time,
            level,
            real,
            private,
        });
        ring.next_seq += 1;
        if ring.lines.len() >= ring.capacity {
            ring.lines.pop_front();
        }
        ring.lines.push_back(line.clone());
        line
    }

    /// The lines after `seq` (0 = every line held), oldest first.
    pub fn since(&self, seq: u64) -> Vec<Arc<LogLine>> {
        let ring = self.inner.lock();
        let Some(first) = ring.lines.front().map(|line| line.seq) else {
            return Vec::new();
        };
        // The numbers in the ring have no gaps, so the first wanted line is found by its distance from the front.
        let skip = usize::try_from(seq.saturating_add(1).saturating_sub(first)).unwrap_or(usize::MAX);
        ring.lines.iter().skip(skip).cloned().collect()
    }

    /// The newest `count` lines, oldest first.
    pub fn last(&self, count: usize) -> Vec<Arc<LogLine>> {
        let ring = self.inner.lock();
        let skip = ring.lines.len().saturating_sub(count);
        ring.lines.iter().skip(skip).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The number of the newest line (0 before the first).
    pub fn latest_seq(&self) -> u64 {
        self.inner.lock().next_seq - 1
    }
}

#[cfg(test)]
mod tests {
    use super::LogRing;
    use chrono::Local;
    use tracing::Level;

    fn ring_with(count: u32, capacity: usize) -> LogRing {
        let ring = LogRing::new(capacity);
        for i in 0..count {
            ring.push(Local::now(), Level::INFO, format!("m{i}"), format!("p{i}"));
        }
        ring
    }

    #[test]
    fn the_ring_is_bounded_ordered_and_numbers_are_never_reused() {
        let ring = ring_with(25, 10);
        let all = ring.since(0);
        assert_eq!(all.len(), 10);
        assert_eq!(all[0].real, "m15");
        assert_eq!(all[9].private, "p24");
        assert_eq!(ring.latest_seq(), 25);
        let newer = ring.since(all[7].seq);
        assert_eq!(
            newer.iter().map(|l| l.real.as_str()).collect::<Vec<_>>(),
            ["m23", "m24"]
        );
        assert_eq!(ring.since(25).len(), 0);
        assert_eq!(ring.since(99).len(), 0);
        assert_eq!(ring.since(3).len(), 10, "lines that left the ring are simply gone");
    }

    #[test]
    fn last_returns_the_newest_lines_oldest_first() {
        let ring = ring_with(5, 100);
        assert_eq!(
            ring.last(2).iter().map(|l| l.real.as_str()).collect::<Vec<_>>(),
            ["m3", "m4"]
        );
        assert_eq!(ring.last(50).len(), 5);
        assert_eq!(LogRing::new(3).last(2).len(), 0);
    }

    #[test]
    fn level_filter_and_file_line() {
        let ring = LogRing::new(4);
        let warn = ring.push(Local::now(), Level::WARN, "real".into(), "private".into());
        assert!(warn.at_least(Level::WARN) && warn.at_least(Level::TRACE) && !warn.at_least(Level::ERROR));
        let line = warn.file_line(true);
        assert!(line.ends_with(" WARN private\n"), "{line}");
    }
}
