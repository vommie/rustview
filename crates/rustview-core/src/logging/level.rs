//! The log level the user chooses on Settings > Debug (`debug.log_level`).

/// How much `RustView` writes to its log. Stored in the settings as the lower-case name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// In the order of the drop-down of the Debug page.
    pub const ALL: [Self; 5] = [Self::Error, Self::Warn, Self::Info, Self::Debug, Self::Trace];

    /// The value stored in the settings and used in filter directives.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    /// The name shown in the drop-down.
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "Errors",
            Self::Warn => "Warnings",
            Self::Info => "Info",
            Self::Debug => "Debug",
            Self::Trace => "Trace",
        }
    }

    /// Reads a stored value (case does not matter); `None` for anything unknown.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        Self::ALL
            .into_iter()
            .find(|level| level.as_str().eq_ignore_ascii_case(value))
            .or_else(|| value.eq_ignore_ascii_case("warning").then_some(Self::Warn))
    }

    /// A stored value, or the default for anything unknown.
    pub fn from_setting(value: &str) -> Self {
        Self::parse(value).unwrap_or_default()
    }

    /// The position in [`Self::ALL`] (the index of the drop-down).
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|level| *level == self).unwrap_or(2)
    }

    /// The level at a drop-down index; out of range = the default.
    pub fn from_index(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }

    /// The filter of the log lines of `RustView` at this level (`rustview` covers both crates: the targets start with it).
    pub fn filter_directive(self) -> String {
        format!("rustview={}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::LogLevel;

    #[test]
    fn stored_values_round_trip_and_unknown_ones_fall_back_to_info() {
        for level in LogLevel::ALL {
            assert_eq!(LogLevel::from_setting(level.as_str()), level);
            assert_eq!(LogLevel::from_index(level.index()), level);
        }
        assert_eq!(LogLevel::from_setting(" DEBUG "), LogLevel::Debug);
        assert_eq!(LogLevel::from_setting("warning"), LogLevel::Warn);
        assert_eq!(LogLevel::from_setting("loud"), LogLevel::Info);
        assert_eq!(LogLevel::from_index(42), LogLevel::Info);
        assert_eq!(LogLevel::Debug.filter_directive(), "rustview=debug");
    }
}
