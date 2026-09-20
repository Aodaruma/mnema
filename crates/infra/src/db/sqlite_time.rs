use anyhow::Result;
use time::format_description::FormatItem;
use time::{OffsetDateTime, UtcOffset};

// SQLite compares persisted timestamps lexicographically. A fixed-width UTC
// representation keeps that ordering identical to chronological ordering,
// including exact-second boundaries where RFC 3339 would otherwise omit the
// fractional component.
const SQLITE_TIMESTAMP_FORMAT: &[FormatItem<'static>] = time::macros::format_description!(
    "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:9]Z"
);

pub(super) fn to_sqlite_timestamp(value: OffsetDateTime) -> Result<String> {
    value
        .to_offset(UtcOffset::UTC)
        .format(SQLITE_TIMESTAMP_FORMAT)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn timestamps_are_fixed_width_utc_at_exact_second_boundaries() {
        assert_eq!(
            to_sqlite_timestamp(datetime!(2026-08-17 09:00 +09:00)).unwrap(),
            "2026-08-17T00:00:00.000000000Z"
        );
        assert_eq!(
            to_sqlite_timestamp(datetime!(2026-08-17 00:00:00.123456789 UTC)).unwrap(),
            "2026-08-17T00:00:00.123456789Z"
        );
    }
}
