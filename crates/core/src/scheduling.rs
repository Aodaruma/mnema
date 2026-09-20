use std::collections::HashSet;

use crate::ids::{SchedulingPolicyId, UserId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{OffsetDateTime, Time, Weekday};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DayOfWeek {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl From<Weekday> for DayOfWeek {
    fn from(value: Weekday) -> Self {
        match value {
            Weekday::Monday => Self::Monday,
            Weekday::Tuesday => Self::Tuesday,
            Weekday::Wednesday => Self::Wednesday,
            Weekday::Thursday => Self::Thursday,
            Weekday::Friday => Self::Friday,
            Weekday::Saturday => Self::Saturday,
            Weekday::Sunday => Self::Sunday,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyTimeRange {
    pub start: Time,
    pub end: Time,
}

impl DailyTimeRange {
    pub fn new(start: Time, end: Time) -> Result<Self, SchedulingValidationError> {
        if start == end {
            return Err(SchedulingValidationError::EmptyDailyRange);
        }
        Ok(Self { start, end })
    }

    #[must_use]
    pub fn crosses_midnight(self) -> bool {
        self.end < self.start
    }

    #[must_use]
    pub fn contains(self, time: Time) -> bool {
        if self.crosses_midnight() {
            time >= self.start || time < self.end
        } else {
            time >= self.start && time < self.end
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeekdayTimeRanges {
    pub weekday: DayOfWeek,
    pub ranges: Vec<DailyTimeRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeeklyTimePolicy {
    pub name: String,
    pub hard: bool,
    pub days: Vec<WeekdayTimeRanges>,
}

impl WeeklyTimePolicy {
    pub fn validate(&self) -> Result<(), SchedulingValidationError> {
        if self.name.trim().is_empty() {
            return Err(SchedulingValidationError::EmptyPolicyName);
        }

        let mut weekdays = HashSet::new();
        for day in &self.days {
            if !weekdays.insert(day.weekday) {
                return Err(SchedulingValidationError::DuplicateWeekday(day.weekday));
            }
            for range in &day.ranges {
                if range.start == range.end {
                    return Err(SchedulingValidationError::EmptyDailyRange);
                }
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn ranges_for(&self, weekday: DayOfWeek) -> &[DailyTimeRange] {
        self.days
            .iter()
            .find(|day| day.weekday == weekday)
            .map_or(&[], |day| day.ranges.as_slice())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulingPreferences {
    pub id: SchedulingPolicyId,
    pub user_id: UserId,
    /// IANA timezone name (for example, `Asia/Tokyo`).
    pub timezone: String,
    /// Named availability policies such as `work`, `personal`, or `focus`.
    pub named_hours: Vec<WeeklyTimePolicy>,
    /// Sleep is a protected constraint and must be hard when present.
    pub sleep: Option<WeeklyTimePolicy>,
    pub default_travel_buffer_minutes: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl SchedulingPreferences {
    pub fn validate(&self) -> Result<(), SchedulingValidationError> {
        if self.timezone.trim().is_empty() {
            return Err(SchedulingValidationError::EmptyTimezone);
        }

        let mut names = HashSet::new();
        for policy in &self.named_hours {
            policy.validate()?;
            if !names.insert(policy.name.trim().to_ascii_lowercase()) {
                return Err(SchedulingValidationError::DuplicatePolicyName(
                    policy.name.clone(),
                ));
            }
        }

        if let Some(sleep) = &self.sleep {
            sleep.validate()?;
            if !sleep.hard {
                return Err(SchedulingValidationError::SleepMustBeHard);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SchedulingValidationError {
    #[error("daily time range must not be empty")]
    EmptyDailyRange,
    #[error("weekly time policy name is required")]
    EmptyPolicyName,
    #[error("timezone is required")]
    EmptyTimezone,
    #[error("duplicate weekday in weekly policy: {0:?}")]
    DuplicateWeekday(DayOfWeek),
    #[error("duplicate named-hours policy: {0}")]
    DuplicatePolicyName(String),
    #[error("sleep policy must be a hard constraint")]
    SleepMustBeHard,
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{datetime, time};

    #[test]
    fn supports_ranges_that_cross_midnight() {
        let range = DailyTimeRange::new(time!(23:00), time!(07:00)).unwrap();

        assert!(range.crosses_midnight());
        assert!(range.contains(time!(01:00)));
        assert!(!range.contains(time!(12:00)));
    }

    #[test]
    fn rejects_soft_sleep_policy() {
        let preferences = SchedulingPreferences {
            id: SchedulingPolicyId::new(),
            user_id: UserId::new(),
            timezone: "Asia/Tokyo".into(),
            named_hours: Vec::new(),
            sleep: Some(WeeklyTimePolicy {
                name: "sleep".into(),
                hard: false,
                days: Vec::new(),
            }),
            default_travel_buffer_minutes: 15,
            created_at: datetime!(2026-08-15 00:00 UTC),
            updated_at: datetime!(2026-08-15 00:00 UTC),
        };

        assert_eq!(
            preferences.validate(),
            Err(SchedulingValidationError::SleepMustBeHard)
        );
    }
}
