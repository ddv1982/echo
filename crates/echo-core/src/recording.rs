use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecordingLimit(u32);

impl RecordingLimit {
    pub const MIN: Self = Self(1);
    pub const DEFAULT: Self = Self(600);
    pub const MAX: Self = Self(600);
    pub const PRESETS: [Self; 5] = [Self(30), Self(60), Self(120), Self(300), Self(600)];

    #[must_use]
    pub const fn new(seconds: u32) -> Option<Self> {
        if seconds >= Self::MIN.0 && seconds <= Self::MAX.0 {
            Some(Self(seconds))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn clamped(seconds: u64) -> Self {
        if seconds < Self::MIN.0 as u64 {
            Self::MIN
        } else if seconds > Self::MAX.0 as u64 {
            Self::MAX
        } else {
            Self(seconds as u32)
        }
    }

    #[must_use]
    pub const fn seconds(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn duration(self) -> Duration {
        Duration::from_secs(self.0 as u64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingLimitSource {
    Environment,
    File,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRecordingLimit {
    pub limit: RecordingLimit,
    pub source: RecordingLimitSource,
}

#[must_use]
pub fn resolve_recording_limit(
    environment: Option<&str>,
    file: Option<u32>,
) -> ResolvedRecordingLimit {
    if let Some(seconds) = environment.and_then(|value| value.parse::<u64>().ok()) {
        return ResolvedRecordingLimit {
            limit: RecordingLimit::clamped(seconds),
            source: RecordingLimitSource::Environment,
        };
    }
    if let Some(seconds) = file {
        return ResolvedRecordingLimit {
            limit: RecordingLimit::clamped(u64::from(seconds)),
            source: RecordingLimitSource::File,
        };
    }
    ResolvedRecordingLimit {
        limit: RecordingLimit::DEFAULT,
        source: RecordingLimitSource::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_constants_cover_the_supported_range() {
        assert_eq!(RecordingLimit::MIN.seconds(), 1);
        assert_eq!(RecordingLimit::DEFAULT.seconds(), 600);
        assert_eq!(RecordingLimit::MAX.seconds(), 600);
        assert_eq!(
            RecordingLimit::PRESETS.map(RecordingLimit::seconds),
            [30, 60, 120, 300, 600]
        );
    }

    #[test]
    fn strict_construction_accepts_only_supported_seconds() {
        assert_eq!(RecordingLimit::new(0), None);
        assert_eq!(RecordingLimit::new(1).map(RecordingLimit::seconds), Some(1));
        assert_eq!(
            RecordingLimit::new(600).map(RecordingLimit::seconds),
            Some(600)
        );
        assert_eq!(RecordingLimit::new(601), None);
        assert_eq!(RecordingLimit::new(u32::MAX), None);
    }

    #[test]
    fn compatibility_input_clamps_to_the_supported_range() {
        for (input, expected) in [
            (0, 1),
            (1, 1),
            (60, 60),
            (61, 61),
            (90, 90),
            (599, 599),
            (600, 600),
            (601, 600),
            (u64::from(u32::MAX), 600),
            (u64::MAX, 600),
        ] {
            assert_eq!(RecordingLimit::clamped(input).seconds(), expected);
        }
    }

    #[test]
    fn resolution_prefers_valid_environment_then_file_then_default() {
        for (environment, file, expected_seconds, expected_source) in [
            (Some("30"), Some(60), 30, RecordingLimitSource::Environment),
            (Some("0"), Some(60), 1, RecordingLimitSource::Environment),
            (
                Some("601"),
                Some(60),
                600,
                RecordingLimitSource::Environment,
            ),
            (
                Some("18446744073709551615"),
                Some(60),
                600,
                RecordingLimitSource::Environment,
            ),
            (Some("invalid"), Some(90), 90, RecordingLimitSource::File),
            (
                Some("18446744073709551616"),
                Some(120),
                120,
                RecordingLimitSource::File,
            ),
            (None, Some(0), 1, RecordingLimitSource::File),
            (None, Some(599), 599, RecordingLimitSource::File),
            (None, Some(u32::MAX), 600, RecordingLimitSource::File),
            (None, None, 600, RecordingLimitSource::Default),
        ] {
            let resolved = resolve_recording_limit(environment, file);
            assert_eq!(resolved.limit.seconds(), expected_seconds);
            assert_eq!(resolved.source, expected_source);
        }
    }
}
