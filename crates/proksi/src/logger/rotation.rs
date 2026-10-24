use time::{format_description::StaticFormatDescription, macros::format_description, Duration};

use crate::config::LogRotation;

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Rotation(pub LogRotation);

impl Rotation {
    /// Provides a minutely rotation
    pub const MINUTELY: Self = Self(LogRotation::Minutely);
    /// Provides an hourly rotation
    pub const HOURLY: Self = Self(LogRotation::Hourly);
    /// Provides a daily rotation
    pub const DAILY: Self = Self(LogRotation::Daily);
    /// Provides a rotation that never rotates.
    pub const NEVER: Self = Self(LogRotation::Never);

    pub(super) fn next_date(
        &self,
        current_date: &time::OffsetDateTime,
    ) -> Option<time::OffsetDateTime> {
        let unrounded_next_date = match *self {
            Rotation::MINUTELY => *current_date + Duration::minutes(1),
            Rotation::HOURLY => *current_date + Duration::hours(1),
            Rotation::DAILY => *current_date + Duration::days(1),
            Rotation::NEVER => return None,
        };
        Some(self.round_date(&unrounded_next_date))
    }

    pub(super) fn round_date(&self, date: &time::OffsetDateTime) -> time::OffsetDateTime {
        match *self {
            Rotation::MINUTELY => date.truncate_to_minute(),
            Rotation::HOURLY => date.truncate_to_hour(),
            Rotation::DAILY => date.truncate_to_day(),
            // Rotation::NEVER doesn't rotate, so there's nothing to round
            Rotation::NEVER => *date,
        }
    }

    pub(super) fn date_format(&self) -> StaticFormatDescription {
        match *self {
            Rotation::MINUTELY => {
                format_description!(version = 1, "[year]-[month]-[day]-[hour]-[minute]")
            }
            Rotation::HOURLY => format_description!(version = 1, "[year]-[month]-[day]-[hour]"),
            Rotation::DAILY => format_description!(version = 1, "[year]-[month]-[day]"),
            Rotation::NEVER => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Rotation;

    // 2026-10-15 15:30:45.123 UTC
    fn sample_date() -> time::OffsetDateTime {
        time::OffsetDateTime::from_unix_timestamp_nanos(1_792_078_245_123_000_000).unwrap()
    }

    fn suffix(rotation: &Rotation, date: time::OffsetDateTime) -> String {
        date.format(rotation.date_format()).unwrap()
    }

    #[test]
    fn test_rotation_round_date() {
        let date = sample_date();

        assert_eq!(
            Rotation::MINUTELY.round_date(&date).unix_timestamp_nanos(),
            1_792_078_200_000_000_000
        );
        assert_eq!(
            Rotation::HOURLY.round_date(&date).unix_timestamp_nanos(),
            1_792_076_400_000_000_000
        );
        assert_eq!(
            Rotation::DAILY.round_date(&date).unix_timestamp_nanos(),
            1_792_022_400_000_000_000
        );
    }

    #[test]
    fn test_rotation_date_format() {
        let date = sample_date();

        assert_eq!(suffix(&Rotation::MINUTELY, date), "2026-10-15-15-30");
        assert_eq!(suffix(&Rotation::HOURLY, date), "2026-10-15-15");
        assert_eq!(suffix(&Rotation::DAILY, date), "2026-10-15");
        assert_eq!(suffix(&Rotation::NEVER, date), "");
    }
}
