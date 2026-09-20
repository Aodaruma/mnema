use crate::ids::{HabitId, HabitOccurrenceId, UserId};
use crate::scheduling::{DailyTimeRange, DayOfWeek};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HabitSchedule {
    Daily,
    Weekdays { weekdays: Vec<DayOfWeek> },
}

impl HabitSchedule {
    pub fn validate(&self) -> Result<(), HabitValidationError> {
        if let Self::Weekdays { weekdays } = self {
            if weekdays.is_empty() {
                return Err(HabitValidationError::EmptyWeekdaySchedule);
            }
            let mut unique = weekdays.clone();
            unique.sort_by_key(|day| *day as u8);
            unique.dedup();
            if unique.len() != weekdays.len() {
                return Err(HabitValidationError::DuplicateWeekday);
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn matches(&self, date: Date) -> bool {
        match self {
            Self::Daily => true,
            Self::Weekdays { weekdays } => weekdays.contains(&date.weekday().into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HabitFlexibility {
    Required,
    Flexible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Habit {
    pub id: HabitId,
    pub user_id: UserId,
    pub title: String,
    pub schedule: HabitSchedule,
    pub duration_minutes: u32,
    pub preferred_window: Option<DailyTimeRange>,
    pub flexibility: HabitFlexibility,
    pub enabled: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub disabled_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl Habit {
    pub fn validate(&self) -> Result<(), HabitValidationError> {
        if self.title.trim().is_empty() {
            return Err(HabitValidationError::EmptyTitle);
        }
        if self.duration_minutes == 0 {
            return Err(HabitValidationError::ZeroDuration);
        }
        self.schedule.validate()
    }

    #[must_use]
    pub fn occurs_on(&self, date: Date) -> bool {
        self.enabled && self.disabled_at.is_none() && self.schedule.matches(date)
    }

    pub fn disable(&mut self, now: OffsetDateTime) {
        self.enabled = false;
        self.disabled_at = Some(now);
        self.updated_at = now;
    }

    pub fn enable(&mut self, now: OffsetDateTime) {
        self.enabled = true;
        self.disabled_at = None;
        self.updated_at = now;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HabitOccurrenceState {
    Pending,
    Scheduled,
    Done,
    Skipped,
    Snoozed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HabitOccurrence {
    pub id: HabitOccurrenceId,
    pub habit_id: HabitId,
    pub occurrence_date: Date,
    pub state: HabitOccurrenceState,
    #[serde(with = "time::serde::rfc3339::option")]
    pub scheduled_start_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub scheduled_end_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub snoozed_until: Option<OffsetDateTime>,
    pub skip_reason: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl HabitOccurrence {
    #[must_use]
    pub fn pending(habit_id: HabitId, occurrence_date: Date, now: OffsetDateTime) -> Self {
        Self {
            id: HabitOccurrenceId::new(),
            habit_id,
            occurrence_date,
            state: HabitOccurrenceState::Pending,
            scheduled_start_at: None,
            scheduled_end_at: None,
            snoozed_until: None,
            skip_reason: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Creates the stable identity used when an occurrence is prepared by a
    /// non-mutating schedule preview before it exists in storage.
    #[must_use]
    pub fn pending_deterministic(
        habit_id: HabitId,
        occurrence_date: Date,
        now: OffsetDateTime,
    ) -> Self {
        const FNV_OFFSET_BASIS: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
        const FNV_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
        let mut hash = FNV_OFFSET_BASIS;
        let date_bytes = occurrence_date.to_julian_day().to_be_bytes();
        for byte in b"mnema:habit-occurrence:v1"
            .iter()
            .chain(habit_id.0.as_bytes())
            .chain(date_bytes.iter())
        {
            hash ^= u128::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        let mut bytes = hash.to_be_bytes();
        bytes[6] = (bytes[6] & 0x0f) | 0x50;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let mut occurrence = Self::pending(habit_id, occurrence_date, now);
        occurrence.id = HabitOccurrenceId(Uuid::from_bytes(bytes));
        occurrence
    }

    pub fn skip(&mut self, reason: Option<String>, now: OffsetDateTime) {
        self.state = HabitOccurrenceState::Skipped;
        self.scheduled_start_at = None;
        self.scheduled_end_at = None;
        self.skip_reason = reason;
        self.snoozed_until = None;
        self.updated_at = now;
    }

    pub fn snooze_until(
        &mut self,
        until: OffsetDateTime,
        now: OffsetDateTime,
    ) -> Result<(), HabitValidationError> {
        if until <= now {
            return Err(HabitValidationError::InvalidSnooze);
        }
        self.state = HabitOccurrenceState::Snoozed;
        self.scheduled_start_at = None;
        self.scheduled_end_at = None;
        self.snoozed_until = Some(until);
        self.skip_reason = None;
        self.updated_at = now;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HabitValidationError {
    #[error("habit title is required")]
    EmptyTitle,
    #[error("habit duration must be greater than zero")]
    ZeroDuration,
    #[error("weekday habit requires at least one weekday")]
    EmptyWeekdaySchedule,
    #[error("weekday habit contains a duplicate weekday")]
    DuplicateWeekday,
    #[error("snooze time must be in the future")]
    InvalidSnooze,
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    #[test]
    fn weekday_schedule_matches_only_selected_days() {
        let schedule = HabitSchedule::Weekdays {
            weekdays: vec![DayOfWeek::Monday, DayOfWeek::Wednesday],
        };

        assert!(schedule.matches(date!(2026 - 08 - 17)));
        assert!(!schedule.matches(date!(2026 - 08 - 18)));
    }

    #[test]
    fn occurrence_can_be_snoozed_and_skipped() {
        let now = datetime!(2026-08-15 00:00 UTC);
        let mut occurrence = HabitOccurrence::pending(HabitId::new(), date!(2026 - 08 - 15), now);
        occurrence.state = HabitOccurrenceState::Scheduled;
        occurrence.scheduled_start_at = Some(datetime!(2026-08-15 00:30 UTC));
        occurrence.scheduled_end_at = Some(datetime!(2026-08-15 01:00 UTC));

        occurrence
            .snooze_until(datetime!(2026-08-15 02:00 UTC), now)
            .unwrap();
        assert_eq!(occurrence.state, HabitOccurrenceState::Snoozed);
        assert_eq!(occurrence.scheduled_start_at, None);
        assert_eq!(occurrence.scheduled_end_at, None);

        occurrence.scheduled_start_at = Some(datetime!(2026-08-15 03:00 UTC));
        occurrence.scheduled_end_at = Some(datetime!(2026-08-15 03:30 UTC));

        occurrence.skip(Some("travel".into()), now);
        assert_eq!(occurrence.state, HabitOccurrenceState::Skipped);
        assert_eq!(occurrence.scheduled_start_at, None);
        assert_eq!(occurrence.scheduled_end_at, None);
        assert_eq!(occurrence.skip_reason.as_deref(), Some("travel"));
    }

    #[test]
    fn preview_occurrence_identity_is_stable_per_habit_and_date() {
        let now = datetime!(2026-08-15 00:00 UTC);
        let habit_id = HabitId::new();
        let first =
            HabitOccurrence::pending_deterministic(habit_id.clone(), date!(2026 - 08 - 15), now);
        let repeated =
            HabitOccurrence::pending_deterministic(habit_id.clone(), date!(2026 - 08 - 15), now);
        let next_day = HabitOccurrence::pending_deterministic(habit_id, date!(2026 - 08 - 16), now);

        assert_eq!(first.id, repeated.id);
        assert_ne!(first.id, next_day.id);
    }
}
