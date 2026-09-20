use std::collections::BTreeMap;

use mnema_core::prelude::*;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

use crate::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddHabitRequest {
    pub user_id: UserId,
    pub title: String,
    pub schedule: HabitSchedule,
    pub duration_minutes: u32,
    pub preferred_window: Option<DailyTimeRange>,
    pub flexibility: HabitFlexibility,
}

pub struct HabitService<'a> {
    habits: &'a dyn HabitRepository,
    occurrences: &'a dyn HabitOccurrenceRepository,
}

#[derive(Debug, Clone)]
pub(crate) struct HabitOccurrenceExpansion {
    pub occurrences: Vec<HabitOccurrence>,
    pub new_occurrences: Vec<HabitOccurrence>,
}

impl<'a> HabitService<'a> {
    #[must_use]
    pub fn new(
        habits: &'a dyn HabitRepository,
        occurrences: &'a dyn HabitOccurrenceRepository,
    ) -> Self {
        Self {
            habits,
            occurrences,
        }
    }

    pub async fn add(&self, request: AddHabitRequest) -> AppResult<Habit> {
        self.add_at(request, OffsetDateTime::now_utc()).await
    }

    pub async fn add_at(&self, request: AddHabitRequest, now: OffsetDateTime) -> AppResult<Habit> {
        let habit = Habit {
            id: HabitId::new(),
            user_id: request.user_id,
            title: request.title.trim().to_owned(),
            schedule: request.schedule,
            duration_minutes: request.duration_minutes,
            preferred_window: request.preferred_window,
            flexibility: request.flexibility,
            enabled: true,
            disabled_at: None,
            created_at: now,
            updated_at: now,
        };
        habit
            .validate()
            .map_err(|error| AppError::InvalidHabit(error.to_string()))?;
        self.habits.upsert(habit.clone()).await?;
        Ok(habit)
    }

    pub async fn list(&self, user_id: UserId) -> AppResult<Vec<Habit>> {
        self.habits.list_enabled(user_id).await.map_err(Into::into)
    }

    pub async fn skip(
        &self,
        occurrence_id: HabitOccurrenceId,
        reason: Option<String>,
    ) -> AppResult<HabitOccurrence> {
        let mut occurrence = self
            .occurrences
            .find(occurrence_id)
            .await?
            .ok_or(AppError::HabitOccurrenceNotFound)?;
        occurrence.skip(reason, OffsetDateTime::now_utc());
        self.occurrences.upsert(occurrence.clone()).await?;
        Ok(occurrence)
    }

    pub async fn snooze(
        &self,
        occurrence_id: HabitOccurrenceId,
        until: OffsetDateTime,
    ) -> AppResult<HabitOccurrence> {
        let now = OffsetDateTime::now_utc();
        let mut occurrence = self
            .occurrences
            .find(occurrence_id)
            .await?
            .ok_or(AppError::HabitOccurrenceNotFound)?;
        occurrence
            .snooze_until(until, now)
            .map_err(|error| AppError::InvalidHabitOccurrence(error.to_string()))?;
        self.occurrences.upsert(occurrence.clone()).await?;
        Ok(occurrence)
    }

    pub async fn disable(&self, habit_id: HabitId) -> AppResult<Habit> {
        let now = OffsetDateTime::now_utc();
        let mut habit = self
            .habits
            .find(habit_id)
            .await?
            .ok_or(AppError::HabitNotFound)?;
        habit.disable(now);
        self.habits.upsert(habit.clone()).await?;
        Ok(habit)
    }

    /// Materializes one occurrence for every matching habit/date pair.
    ///
    /// Repository uniqueness on `(habit_id, occurrence_date)` plus the range
    /// lookup makes repeated explicit expansion idempotent. Auto-schedule
    /// Preview uses the non-mutating companion below instead.
    pub async fn expand_occurrences(
        &self,
        user_id: UserId,
        start: Date,
        end_exclusive: Date,
    ) -> AppResult<Vec<HabitOccurrence>> {
        let mut expansion = self
            .preview_occurrences(user_id, start, end_exclusive)
            .await?;
        for occurrence in &expansion.new_occurrences {
            if self
                .occurrences
                .find_for_date(occurrence.habit_id.clone(), occurrence.occurrence_date)
                .await?
                .is_none()
            {
                self.occurrences.upsert(occurrence.clone()).await?;
            }
        }
        for occurrence in &mut expansion.occurrences {
            if let Some(persisted) = self
                .occurrences
                .find_for_date(occurrence.habit_id.clone(), occurrence.occurrence_date)
                .await?
            {
                *occurrence = persisted;
            }
        }
        Ok(expansion.occurrences)
    }

    /// Builds the same occurrence set used by expansion without mutating a
    /// repository. Missing rows receive deterministic IDs so Preview can be
    /// cancelled safely and Apply can persist the exact referenced rows.
    pub(crate) async fn preview_occurrences(
        &self,
        user_id: UserId,
        start: Date,
        end_exclusive: Date,
    ) -> AppResult<HabitOccurrenceExpansion> {
        if start >= end_exclusive {
            return Err(AppError::InvalidDateRange);
        }

        let now = OffsetDateTime::now_utc();
        let habits = self.habits.list_enabled(user_id).await?;
        let mut expanded = Vec::new();
        let mut new_occurrences = Vec::new();

        for habit in habits {
            habit
                .validate()
                .map_err(|error| AppError::InvalidHabit(error.to_string()))?;
            let existing = self
                .occurrences
                .list_for_habit_range(habit.id.clone(), start, end_exclusive)
                .await?
                .into_iter()
                .map(|occurrence| (occurrence.occurrence_date, occurrence))
                .collect::<BTreeMap<_, _>>();

            let mut date = start;
            while date < end_exclusive {
                if habit.occurs_on(date) {
                    if let Some(occurrence) = existing.get(&date) {
                        expanded.push(occurrence.clone());
                    } else {
                        let occurrence =
                            HabitOccurrence::pending_deterministic(habit.id.clone(), date, now);
                        new_occurrences.push(occurrence.clone());
                        expanded.push(occurrence);
                    }
                }
                date = date.next_day().ok_or(AppError::DateRangeOverflow)?;
            }
        }

        expanded.sort_by(|left, right| {
            left.occurrence_date
                .cmp(&right.occurrence_date)
                .then_with(|| left.habit_id.cmp(&right.habit_id))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(HabitOccurrenceExpansion {
            occurrences: expanded,
            new_occurrences,
        })
    }

    /// Persists rows referenced by an auto-schedule preview without replacing
    /// an occurrence state that may have been changed concurrently.
    pub(crate) async fn persist_preview_occurrences(
        &self,
        occurrences: &[HabitOccurrence],
    ) -> AppResult<()> {
        for occurrence in occurrences {
            if let Some(existing) = self
                .occurrences
                .find_for_date(occurrence.habit_id.clone(), occurrence.occurrence_date)
                .await?
            {
                if existing.id != occurrence.id {
                    return Err(AppError::StaleSchedulePreview);
                }
                continue;
            }
            self.occurrences.upsert(occurrence.clone()).await?;
            let persisted = self
                .occurrences
                .find_for_date(occurrence.habit_id.clone(), occurrence.occurrence_date)
                .await?
                .ok_or(AppError::HabitOccurrenceNotFound)?;
            if persisted.id != occurrence.id {
                return Err(AppError::StaleSchedulePreview);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use time::macros::{date, datetime};

    use super::*;

    #[derive(Default)]
    struct MemoryHabitRepository {
        habits: Mutex<Vec<Habit>>,
    }

    #[async_trait]
    impl HabitRepository for MemoryHabitRepository {
        async fn upsert(&self, habit: Habit) -> CoreResult<()> {
            let mut habits = self.habits.lock().unwrap();
            if let Some(existing) = habits.iter_mut().find(|item| item.id == habit.id) {
                *existing = habit;
            } else {
                habits.push(habit);
            }
            Ok(())
        }

        async fn find(&self, id: HabitId) -> CoreResult<Option<Habit>> {
            Ok(self
                .habits
                .lock()
                .unwrap()
                .iter()
                .find(|habit| habit.id == id)
                .cloned())
        }

        async fn list_enabled(&self, user_id: UserId) -> CoreResult<Vec<Habit>> {
            Ok(self
                .habits
                .lock()
                .unwrap()
                .iter()
                .filter(|habit| habit.user_id == user_id && habit.enabled)
                .cloned()
                .collect())
        }
    }

    #[derive(Default)]
    struct MemoryOccurrenceRepository {
        occurrences: Mutex<Vec<HabitOccurrence>>,
    }

    #[async_trait]
    impl HabitOccurrenceRepository for MemoryOccurrenceRepository {
        async fn upsert(&self, occurrence: HabitOccurrence) -> CoreResult<()> {
            let mut occurrences = self.occurrences.lock().unwrap();
            if let Some(existing) = occurrences.iter_mut().find(|item| {
                item.habit_id == occurrence.habit_id
                    && item.occurrence_date == occurrence.occurrence_date
            }) {
                *existing = occurrence;
            } else {
                occurrences.push(occurrence);
            }
            Ok(())
        }

        async fn find(&self, id: HabitOccurrenceId) -> CoreResult<Option<HabitOccurrence>> {
            Ok(self
                .occurrences
                .lock()
                .unwrap()
                .iter()
                .find(|occurrence| occurrence.id == id)
                .cloned())
        }

        async fn find_for_date(
            &self,
            habit_id: HabitId,
            date: Date,
        ) -> CoreResult<Option<HabitOccurrence>> {
            Ok(self
                .occurrences
                .lock()
                .unwrap()
                .iter()
                .find(|item| item.habit_id == habit_id && item.occurrence_date == date)
                .cloned())
        }

        async fn list_for_habit_range(
            &self,
            habit_id: HabitId,
            start: Date,
            end_exclusive: Date,
        ) -> CoreResult<Vec<HabitOccurrence>> {
            Ok(self
                .occurrences
                .lock()
                .unwrap()
                .iter()
                .filter(|item| {
                    item.habit_id == habit_id
                        && item.occurrence_date >= start
                        && item.occurrence_date < end_exclusive
                })
                .cloned()
                .collect())
        }
    }

    fn daily_request(user_id: UserId) -> AddHabitRequest {
        AddHabitRequest {
            user_id,
            title: "Walk".into(),
            schedule: HabitSchedule::Daily,
            duration_minutes: 30,
            preferred_window: None,
            flexibility: HabitFlexibility::Flexible,
        }
    }

    #[tokio::test]
    async fn expansion_is_idempotent() {
        let habits = MemoryHabitRepository::default();
        let occurrences = MemoryOccurrenceRepository::default();
        let service = HabitService::new(&habits, &occurrences);
        let user_id = UserId::new();
        service
            .add_at(
                daily_request(user_id.clone()),
                datetime!(2026-08-15 00:00 UTC),
            )
            .await
            .unwrap();

        let first = service
            .expand_occurrences(
                user_id.clone(),
                date!(2026 - 08 - 15),
                date!(2026 - 08 - 18),
            )
            .await
            .unwrap();
        let second = service
            .expand_occurrences(user_id, date!(2026 - 08 - 15), date!(2026 - 08 - 18))
            .await
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.len(), 3);
        assert_eq!(occurrences.occurrences.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn skip_snooze_and_disable_are_persisted() {
        let habits = MemoryHabitRepository::default();
        let occurrences = MemoryOccurrenceRepository::default();
        let service = HabitService::new(&habits, &occurrences);
        let user_id = UserId::new();
        let habit = service
            .add_at(
                daily_request(user_id.clone()),
                datetime!(2026-08-15 00:00 UTC),
            )
            .await
            .unwrap();
        let occurrence = service
            .expand_occurrences(
                user_id.clone(),
                date!(2026 - 08 - 15),
                date!(2026 - 08 - 16),
            )
            .await
            .unwrap()
            .remove(0);
        {
            let mut stored = occurrences.occurrences.lock().unwrap();
            stored[0].state = HabitOccurrenceState::Scheduled;
            stored[0].scheduled_start_at = Some(datetime!(2026-08-15 09:00 UTC));
            stored[0].scheduled_end_at = Some(datetime!(2026-08-15 09:30 UTC));
        }

        let snoozed = service
            .snooze(
                occurrence.id.clone(),
                OffsetDateTime::now_utc() + time::Duration::hours(1),
            )
            .await
            .unwrap();
        assert_eq!(snoozed.state, HabitOccurrenceState::Snoozed);
        assert_eq!(snoozed.scheduled_start_at, None);
        assert_eq!(snoozed.scheduled_end_at, None);
        let skipped = service
            .skip(occurrence.id, Some("trip".into()))
            .await
            .unwrap();
        assert_eq!(skipped.state, HabitOccurrenceState::Skipped);
        assert_eq!(skipped.scheduled_start_at, None);
        assert_eq!(skipped.scheduled_end_at, None);

        let expanded_again = service
            .expand_occurrences(
                user_id.clone(),
                date!(2026 - 08 - 15),
                date!(2026 - 08 - 16),
            )
            .await
            .unwrap();
        assert_eq!(expanded_again.len(), 1);
        assert_eq!(expanded_again[0].state, HabitOccurrenceState::Skipped);
        assert_eq!(expanded_again[0].skip_reason.as_deref(), Some("trip"));

        let disabled = service.disable(habit.id).await.unwrap();
        assert!(!disabled.enabled);
        assert!(service.list(user_id).await.unwrap().is_empty());
    }
}
