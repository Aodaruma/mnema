use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use mnema_core::prelude::*;
use mnema_scheduler::{
    AvailabilityWindow, BusyBlock, BusyBlockSource, GreedyScheduler, ItemSchedulingInput,
    ItemSchedulingOutput, SchedulableItem, ScheduleItemKind, ScheduleItemRef, ScheduleItemTier,
    TimeWindow,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{Date, Duration, OffsetDateTime, Time};
use time_tz::{OffsetResult, PrimitiveDateTimeExt, timezones};

use crate::{
    AppError, AppResult, HabitService, PlanChangeKind, TravelBufferPolicy, travel_busy_blocks,
};

/// Discovery guard for per-event travel overrides. Once candidate events are
/// loaded, the query is narrowed or expanded to their exact effective maxima.
const TRAVEL_OVERRIDE_DISCOVERY_MINUTES: u32 = 24 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoScheduleRequest {
    pub user_id: UserId,
    pub start_date: Date,
    pub end_date_exclusive: Date,
    /// IANA timezone hint. Preview normalizes this to the persisted scheduling
    /// preferences, which are the source of truth for local dates and policies.
    pub timezone: String,
    /// Empty selects every configured named-hours policy.
    pub named_hours: Vec<String>,
    /// Freeze this cutoff in the preview so Apply can revalidate it exactly.
    #[serde(default)]
    pub not_before: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AutoScheduleItemKey {
    Task(TaskId),
    HabitOccurrence(HabitOccurrenceId),
}

impl From<&ScheduleItemRef> for AutoScheduleItemKey {
    fn from(value: &ScheduleItemRef) -> Self {
        match value {
            ScheduleItemRef::Task(id) => Self::Task(id.clone()),
            ScheduleItemRef::HabitOccurrence(id) => Self::HabitOccurrence(id.clone()),
        }
    }
}

impl From<&AutoScheduleItemKey> for ScheduleItemRef {
    fn from(value: &AutoScheduleItemKey) -> Self {
        match value {
            AutoScheduleItemKey::Task(id) => Self::Task(id.clone()),
            AutoScheduleItemKey::HabitOccurrence(id) => Self::HabitOccurrence(id.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoPlanBlockSnapshot {
    pub block_id: Option<ScheduleBlockId>,
    pub item: AutoScheduleItemKey,
    pub title: String,
    #[serde(with = "time::serde::rfc3339")]
    pub start_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub end_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoPlanChange {
    pub kind: PlanChangeKind,
    pub item: AutoScheduleItemKey,
    pub title: String,
    pub before: Option<AutoPlanBlockSnapshot>,
    pub after: Option<AutoPlanBlockSnapshot>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoScheduleDiff {
    pub fingerprint: String,
    pub changes: Vec<AutoPlanChange>,
}

impl AutoScheduleDiff {
    #[must_use]
    pub fn has_changes(&self) -> bool {
        self.changes
            .iter()
            .any(|change| change.kind != PlanChangeKind::Unchanged)
    }

    #[must_use]
    pub fn changed_count(&self) -> usize {
        self.changes
            .iter()
            .filter(|change| change.kind != PlanChangeKind::Unchanged)
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoSchedulePreview {
    pub request: AutoScheduleRequest,
    pub planning_window: TimeWindow,
    pub availability: Vec<AvailabilityWindow>,
    pub hard_busy: Vec<BusyBlock>,
    /// Missing habit occurrences prepared in-memory by Preview and persisted
    /// only by Apply.
    #[serde(default)]
    pub new_habit_occurrences: Vec<HabitOccurrence>,
    pub output: ItemSchedulingOutput,
    pub proposed_blocks: Vec<ScheduleBlock>,
    pub diff: AutoScheduleDiff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoScheduleApplyResult {
    pub planning_window: TimeWindow,
    pub fingerprint: String,
    pub blocks: Vec<ScheduleBlock>,
}

pub struct AutoScheduleService<'a> {
    tasks: &'a dyn TaskRepository,
    statuses: &'a dyn StatusRepository,
    schedule_blocks: &'a dyn ScheduleBlockRepository,
    external_events: &'a dyn ExternalEventRepository,
    habits: &'a dyn HabitRepository,
    occurrences: &'a dyn HabitOccurrenceRepository,
    preferences: &'a dyn SchedulingPreferencesRepository,
    scheduler: GreedyScheduler,
    default_task_minutes: u32,
}

#[derive(Debug, Clone)]
struct OccurrenceScheduleUpdate {
    before: HabitOccurrence,
    after: HabitOccurrence,
}

impl<'a> AutoScheduleService<'a> {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        tasks: &'a dyn TaskRepository,
        statuses: &'a dyn StatusRepository,
        schedule_blocks: &'a dyn ScheduleBlockRepository,
        external_events: &'a dyn ExternalEventRepository,
        habits: &'a dyn HabitRepository,
        occurrences: &'a dyn HabitOccurrenceRepository,
        preferences: &'a dyn SchedulingPreferencesRepository,
    ) -> Self {
        Self {
            tasks,
            statuses,
            schedule_blocks,
            external_events,
            habits,
            occurrences,
            preferences,
            scheduler: GreedyScheduler::default(),
            default_task_minutes: 30,
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn with_scheduler(
        tasks: &'a dyn TaskRepository,
        statuses: &'a dyn StatusRepository,
        schedule_blocks: &'a dyn ScheduleBlockRepository,
        external_events: &'a dyn ExternalEventRepository,
        habits: &'a dyn HabitRepository,
        occurrences: &'a dyn HabitOccurrenceRepository,
        preferences: &'a dyn SchedulingPreferencesRepository,
        scheduler: GreedyScheduler,
        default_task_minutes: u32,
    ) -> Self {
        Self {
            tasks,
            statuses,
            schedule_blocks,
            external_events,
            habits,
            occurrences,
            preferences,
            scheduler,
            default_task_minutes,
        }
    }

    pub async fn preview(
        &self,
        mut request: AutoScheduleRequest,
    ) -> AppResult<AutoSchedulePreview> {
        if request.start_date >= request.end_date_exclusive {
            return Err(AppError::InvalidDateRange);
        }

        let preferences = self
            .preferences
            .get_for_user(request.user_id.clone())
            .await?
            .ok_or(AppError::MissingSchedulingPreferences)?;
        preferences
            .validate()
            .map_err(|error| AppError::InvalidSchedulingPreferences(error.to_string()))?;
        request.timezone = preferences.timezone.trim().to_owned();
        let timezone = request.timezone.as_str();
        let planning_window =
            iana_date_range(request.start_date, request.end_date_exclusive, timezone)?;
        let availability = named_hours_availability(
            &preferences,
            &request.named_hours,
            request.start_date,
            request.end_date_exclusive,
            timezone,
            planning_window,
        )?;

        let existing = self
            .schedule_blocks
            .list_overlapping(planning_window.start, planning_window.end)
            .await?;
        // An elapsed or already-started proposal is preserved byte-for-byte.
        // Treat it as fixed while planning the remaining time.
        let mut planning_existing = existing.clone();
        for block in &mut planning_existing {
            if request
                .not_before
                .is_some_and(|cutoff| block.start_at < cutoff)
            {
                block.locked = true;
            }
        }
        let travel_policy =
            TravelBufferPolicy::symmetric(preferences.default_travel_buffer_minutes);
        let mut queried_travel_policy = TravelBufferPolicy {
            default_before_minutes: travel_policy
                .default_before_minutes
                .max(TRAVEL_OVERRIDE_DISCOVERY_MINUTES),
            default_after_minutes: travel_policy
                .default_after_minutes
                .max(TRAVEL_OVERRIDE_DISCOVERY_MINUTES),
        };
        let mut event_query_window =
            travel_event_query_window(planning_window, queried_travel_policy);
        let mut events = self
            .external_events
            .list_overlapping(event_query_window.start, event_query_window.end)
            .await?;
        let override_policy = travel_policy_covering_events(&events, travel_policy);
        if override_policy != queried_travel_policy {
            queried_travel_policy = override_policy;
            event_query_window = travel_event_query_window(planning_window, queried_travel_policy);
            events = self
                .external_events
                .list_overlapping(event_query_window.start, event_query_window.end)
                .await?;
        }
        let fixed_items = fixed_schedule_item_keys(&planning_existing);
        let mut hard_busy = schedule_block_busy_blocks(&planning_existing, planning_window);
        hard_busy.extend(external_event_busy_blocks(&events, planning_window));
        if let Some(sleep) = &preferences.sleep {
            hard_busy.extend(
                expand_weekly_policy(
                    sleep,
                    request.start_date,
                    request.end_date_exclusive,
                    timezone,
                    planning_window,
                )?
                .into_iter()
                .map(|window| BusyBlock {
                    window,
                    source: BusyBlockSource::Manual,
                    label: Some(sleep.name.clone()),
                }),
            );
        }
        hard_busy.extend(travel_busy_blocks(&events, planning_window, travel_policy));
        hard_busy.sort_by_key(|block| (block.window.start, block.window.end));

        let tasks = self.tasks.list_all().await?;
        let done_status_ids = self.done_status_ids(&tasks).await?;
        let completed = tasks
            .iter()
            .filter(|task| done_status_ids.contains(&task.status_id))
            .map(|task| task.id.clone())
            .collect::<HashSet<_>>();
        let timezone_ref = timezones::get_by_name(timezone)
            .ok_or_else(|| AppError::InvalidTimezone(timezone.to_owned()))?;
        let fixed_ends = planning_existing
            .iter()
            .filter(|block| !is_replaceable_proposal(block))
            .filter(|block| {
                !matches!(
                    block.state,
                    ScheduleBlockState::Cancelled | ScheduleBlockState::Missed
                )
            })
            .filter_map(|block| block.task_id.clone().map(|id| (id, block.end_at)))
            .fold(
                HashMap::<TaskId, OffsetDateTime>::new(),
                |mut ends, (id, end)| {
                    ends.entry(id)
                        .and_modify(|value| *value = (*value).max(end))
                        .or_insert(end);
                    ends
                },
            );
        let fixed_minutes = planning_existing
            .iter()
            .filter(|block| !is_replaceable_proposal(block))
            .filter(|block| {
                !matches!(
                    block.state,
                    ScheduleBlockState::Cancelled | ScheduleBlockState::Missed
                )
            })
            .filter_map(|block| {
                block.task_id.clone().map(|id| {
                    (
                        id,
                        u32::try_from((block.end_at - block.start_at).whole_minutes())
                            .unwrap_or_default(),
                    )
                })
            })
            .fold(
                HashMap::<TaskId, u32>::new(),
                |mut minutes, (id, duration)| {
                    let value = minutes.entry(id).or_default();
                    *value = value.saturating_add(duration);
                    minutes
                },
            );
        let fully_fixed = tasks
            .iter()
            .filter(|task| {
                fixed_minutes.get(&task.id).is_some_and(|minutes| {
                    *minutes
                        >= task
                            .estimated_minutes
                            .filter(|minutes| *minutes > 0)
                            .unwrap_or(self.default_task_minutes)
                })
            })
            .map(|task| task.id.clone())
            .collect::<HashSet<_>>();
        let mut items = tasks
            .into_iter()
            .filter(|task| task.deleted_at.is_none())
            .filter(|task| !done_status_ids.contains(&task.status_id))
            .filter(|task| !fully_fixed.contains(&task.id))
            .filter(|task| {
                task.start_date
                    .is_none_or(|start| start < request.end_date_exclusive)
            })
            .map(|task| {
                let start = task
                    .start_date
                    .map(|date| local_datetime(date, Time::MIDNIGHT, timezone_ref, true, timezone))
                    .transpose()?;
                let dependency_end = task
                    .dependencies
                    .iter()
                    .filter_map(|id| fixed_ends.get(id))
                    .max()
                    .copied();
                Ok(SchedulableItem {
                    item_ref: ScheduleItemRef::Task(task.id.clone()),
                    kind: ScheduleItemKind::Task,
                    tier: ScheduleItemTier::Task,
                    title: task.title,
                    duration_minutes: task
                        .estimated_minutes
                        .filter(|minutes| *minutes > 0)
                        .unwrap_or(self.default_task_minutes)
                        .saturating_sub(fixed_minutes.get(&task.id).copied().unwrap_or_default()),
                    due: task.due_date,
                    importance: task.cost_points.unwrap_or_default(),
                    created_at: task.created_at,
                    allowed_windows: Vec::new(),
                    not_before: start
                        .into_iter()
                        .chain(request.not_before)
                        .chain(dependency_end)
                        .chain(fixed_ends.get(&task.id).copied())
                        .max(),
                    dependencies: task
                        .dependencies
                        .iter()
                        .filter(|id| !completed.contains(id) && !fully_fixed.contains(id))
                        .cloned()
                        .map(ScheduleItemRef::Task)
                        .collect(),
                    minimum_chunk_minutes: Some(15),
                })
            })
            .collect::<AppResult<Vec<_>>>()?;

        let habit_service = HabitService::new(self.habits, self.occurrences);
        let occurrence_expansion = habit_service
            .preview_occurrences(
                request.user_id.clone(),
                request.start_date,
                request.end_date_exclusive,
            )
            .await?;
        let habits = self
            .habits
            .list_enabled(request.user_id.clone())
            .await?
            .into_iter()
            .map(|habit| (habit.id.clone(), habit))
            .collect::<BTreeMap<_, _>>();
        for occurrence in occurrence_expansion.occurrences {
            if matches!(
                occurrence.state,
                HabitOccurrenceState::Done | HabitOccurrenceState::Skipped
            ) {
                continue;
            }
            if fixed_items.contains(&AutoScheduleItemKey::HabitOccurrence(occurrence.id.clone())) {
                continue;
            }
            let Some(habit) = habits.get(&occurrence.habit_id) else {
                continue;
            };
            let allowed_windows =
                habit_allowed_windows(habit, &occurrence, timezone, planning_window)?;
            let item_ref = ScheduleItemRef::HabitOccurrence(occurrence.id.clone());
            items.push(SchedulableItem {
                kind: item_ref.kind(),
                item_ref,
                tier: match habit.flexibility {
                    HabitFlexibility::Required => ScheduleItemTier::RequiredHabit,
                    HabitFlexibility::Flexible => ScheduleItemTier::FlexibleHabit,
                },
                title: habit.title.clone(),
                duration_minutes: habit.duration_minutes,
                due: Some(occurrence.occurrence_date),
                importance: 0,
                created_at: occurrence.created_at,
                allowed_windows,
                not_before: request.not_before,
                dependencies: Vec::new(),
                minimum_chunk_minutes: None,
            });
        }

        let mut output = self.scheduler.plan_items(ItemSchedulingInput {
            items,
            availability: availability.clone(),
            hard_busy: hard_busy.clone(),
        });
        for block in existing
            .iter()
            .filter(|block| is_replaceable_proposal(block))
            .filter(|block| {
                request
                    .not_before
                    .is_some_and(|cutoff| block.start_at < cutoff)
            })
        {
            if let Some(item) = block_item_key(block) {
                let item_ref = ScheduleItemRef::from(&item);
                output
                    .blocks
                    .push(mnema_scheduler::ProposedItemScheduleBlock {
                        kind: item_ref.kind(),
                        item_ref,
                        tier: if block.habit_occurrence_id.is_some() {
                            ScheduleItemTier::RequiredHabit
                        } else {
                            ScheduleItemTier::Task
                        },
                        title: block.title_snapshot.clone().unwrap_or_default(),
                        window: TimeWindow::new(block.start_at, block.end_at),
                        required_minutes: block.required_minutes.unwrap_or_default(),
                    });
            }
        }
        let mut proposed_blocks = proposed_schedule_blocks(&output, OffsetDateTime::now_utc());
        for proposed in &mut proposed_blocks {
            if request
                .not_before
                .is_some_and(|cutoff| proposed.start_at < cutoff)
                && let Some(original) = existing.iter().find(|block| {
                    block_item_key(block) == block_item_key(proposed)
                        && block.start_at == proposed.start_at
                        && block.end_at == proposed.end_at
                })
            {
                *proposed = original.clone();
            }
        }
        let mut diff = diff_auto_schedule(&existing, &output);
        diff.fingerprint = auto_schedule_context_fingerprint(
            &request,
            &preferences,
            &existing,
            &events,
            &availability,
            &hard_busy,
            &output,
        );

        Ok(AutoSchedulePreview {
            request,
            planning_window,
            availability,
            hard_busy,
            new_habit_occurrences: occurrence_expansion.new_occurrences,
            output,
            proposed_blocks,
            diff,
        })
    }

    pub async fn apply(
        &self,
        preview: &AutoSchedulePreview,
        expected_fingerprint: &str,
    ) -> AppResult<AutoScheduleApplyResult> {
        if preview.diff.fingerprint != expected_fingerprint {
            return Err(AppError::StaleSchedulePreview);
        }
        let fresh = self.preview(preview.request.clone()).await?;
        if fresh.diff.fingerprint != expected_fingerprint {
            return Err(AppError::StaleSchedulePreview);
        }
        let before_guard = self
            .schedule_blocks
            .list_overlapping(fresh.planning_window.start, fresh.planning_window.end)
            .await?;
        let guarded = self.preview(fresh.request.clone()).await?;
        if guarded.diff.fingerprint != expected_fingerprint {
            return Err(AppError::StaleSchedulePreview);
        }
        let current = self
            .schedule_blocks
            .list_overlapping(guarded.planning_window.start, guarded.planning_window.end)
            .await?;
        if !same_schedule_block_snapshot(&before_guard, &current) {
            return Err(AppError::StaleSchedulePreview);
        }

        let habit_service = HabitService::new(self.habits, self.occurrences);
        habit_service
            .persist_preview_occurrences(&guarded.new_habit_occurrences)
            .await?;
        let blocks = reconcile_proposed_block_ids(&current, &guarded.proposed_blocks);
        let occurrence_updates = self
            .prepare_occurrence_schedule_updates(&current, &blocks)
            .await?;
        self.apply_occurrence_schedule_updates(&occurrence_updates)
            .await?;

        if let Err(error) = self
            .schedule_blocks
            .replace_proposed_in_range(
                guarded.planning_window.start,
                guarded.planning_window.end,
                blocks.clone(),
            )
            .await
        {
            if let Err(rollback_error) = self
                .rollback_occurrence_schedule_updates(&occurrence_updates)
                .await
            {
                return Err(AppError::ScheduleApplyInconsistent(format!(
                    "schedule block replacement failed: {error}; occurrence rollback failed: {rollback_error}"
                )));
            }
            return Err(error.into());
        }

        Ok(AutoScheduleApplyResult {
            planning_window: guarded.planning_window,
            fingerprint: expected_fingerprint.to_owned(),
            blocks,
        })
    }

    async fn done_status_ids(&self, tasks: &[Task]) -> AppResult<HashSet<StatusId>> {
        let groups = self
            .statuses
            .list_groups()
            .await?
            .into_iter()
            .map(|group| (group.id, group.kind))
            .collect::<HashMap<_, _>>();
        let mut scopes = tasks
            .iter()
            .filter_map(|task| task.project_id.clone())
            .map(Some)
            .collect::<HashSet<_>>();
        scopes.insert(None);
        let mut done = HashSet::new();
        for scope in scopes {
            for status in self.statuses.list_statuses_for_project(scope).await? {
                if matches!(groups.get(&status.group_id), Some(StatusGroupKind::Done)) {
                    done.insert(status.id);
                }
            }
        }
        Ok(done)
    }

    async fn prepare_occurrence_schedule_updates(
        &self,
        before: &[ScheduleBlock],
        after: &[ScheduleBlock],
    ) -> AppResult<Vec<OccurrenceScheduleUpdate>> {
        let scheduled_after = after
            .iter()
            .filter_map(|block| {
                block
                    .habit_occurrence_id
                    .clone()
                    .map(|id| (id, (block.start_at, block.end_at)))
            })
            .collect::<BTreeMap<_, _>>();
        let mut occurrence_ids = before
            .iter()
            .filter(|block| is_replaceable_proposal(block))
            .filter_map(|block| block.habit_occurrence_id.clone())
            .collect::<BTreeSet<_>>();
        occurrence_ids.extend(scheduled_after.keys().cloned());
        let now = OffsetDateTime::now_utc();
        let mut updates = Vec::new();

        for occurrence_id in occurrence_ids {
            let Some(occurrence) = self.occurrences.find(occurrence_id.clone()).await? else {
                if scheduled_after.contains_key(&occurrence_id) {
                    return Err(AppError::HabitOccurrenceNotFound);
                }
                continue;
            };
            let mut updated = occurrence.clone();
            if let Some((start, end)) = scheduled_after.get(&occurrence_id) {
                updated.state = HabitOccurrenceState::Scheduled;
                updated.scheduled_start_at = Some(*start);
                updated.scheduled_end_at = Some(*end);
                updated.snoozed_until = None;
                updated.skip_reason = None;
                updated.updated_at = now;
            } else if updated.state == HabitOccurrenceState::Scheduled {
                updated.state = HabitOccurrenceState::Pending;
                updated.scheduled_start_at = None;
                updated.scheduled_end_at = None;
                updated.updated_at = now;
            } else {
                continue;
            }
            updates.push(OccurrenceScheduleUpdate {
                before: occurrence,
                after: updated,
            });
        }
        Ok(updates)
    }

    async fn apply_occurrence_schedule_updates(
        &self,
        updates: &[OccurrenceScheduleUpdate],
    ) -> AppResult<()> {
        for (index, update) in updates.iter().enumerate() {
            if let Err(error) = self.occurrences.upsert(update.after.clone()).await {
                if let Err(rollback_error) = self
                    .rollback_occurrence_schedule_updates(&updates[..=index])
                    .await
                {
                    return Err(AppError::ScheduleApplyInconsistent(format!(
                        "occurrence update failed: {error}; rollback failed: {rollback_error}"
                    )));
                }
                return Err(error.into());
            }
        }
        Ok(())
    }

    async fn rollback_occurrence_schedule_updates(
        &self,
        updates: &[OccurrenceScheduleUpdate],
    ) -> AppResult<()> {
        for update in updates.iter().rev() {
            self.occurrences.upsert(update.before.clone()).await?;
        }
        Ok(())
    }
}

#[must_use]
pub fn default_scheduling_preferences(
    user_id: UserId,
    timezone: impl Into<String>,
    now: OffsetDateTime,
) -> SchedulingPreferences {
    let weekdays = [
        DayOfWeek::Monday,
        DayOfWeek::Tuesday,
        DayOfWeek::Wednesday,
        DayOfWeek::Thursday,
        DayOfWeek::Friday,
    ];
    let every_day = [
        DayOfWeek::Monday,
        DayOfWeek::Tuesday,
        DayOfWeek::Wednesday,
        DayOfWeek::Thursday,
        DayOfWeek::Friday,
        DayOfWeek::Saturday,
        DayOfWeek::Sunday,
    ];
    SchedulingPreferences {
        id: SchedulingPolicyId::new(),
        user_id,
        timezone: timezone.into(),
        named_hours: vec![WeeklyTimePolicy {
            name: "work".into(),
            hard: false,
            days: weekdays
                .into_iter()
                .map(|weekday| WeekdayTimeRanges {
                    weekday,
                    ranges: vec![DailyTimeRange {
                        start: Time::from_hms(9, 0, 0).expect("valid default time"),
                        end: Time::from_hms(17, 0, 0).expect("valid default time"),
                    }],
                })
                .collect(),
        }],
        sleep: Some(WeeklyTimePolicy {
            name: "sleep".into(),
            hard: true,
            days: every_day
                .into_iter()
                .map(|weekday| WeekdayTimeRanges {
                    weekday,
                    ranges: vec![DailyTimeRange {
                        start: Time::from_hms(23, 0, 0).expect("valid default time"),
                        end: Time::from_hms(7, 0, 0).expect("valid default time"),
                    }],
                })
                .collect(),
        }),
        default_travel_buffer_minutes: 15,
        created_at: now,
        updated_at: now,
    }
}

/// Expands the repository query so events immediately outside the planning
/// range are still available when their travel buffer crosses the boundary.
#[must_use]
pub fn travel_event_query_window(
    planning_window: TimeWindow,
    policy: TravelBufferPolicy,
) -> TimeWindow {
    let start = planning_window
        .start
        .checked_sub(Duration::minutes(i64::from(policy.default_after_minutes)))
        .unwrap_or(planning_window.start);
    let end = planning_window
        .end
        .checked_add(Duration::minutes(i64::from(policy.default_before_minutes)))
        .unwrap_or(planning_window.end);
    TimeWindow::new(start, end)
}

fn travel_policy_covering_events(
    events: &[ExternalEvent],
    default: TravelBufferPolicy,
) -> TravelBufferPolicy {
    events.iter().fold(default, |mut policy, event| {
        policy.default_before_minutes = policy
            .default_before_minutes
            .max(event.travel_before_minutes.unwrap_or_default());
        policy.default_after_minutes = policy
            .default_after_minutes
            .max(event.travel_after_minutes.unwrap_or_default());
        policy
    })
}

#[allow(clippy::too_many_arguments)]
fn auto_schedule_context_fingerprint(
    request: &AutoScheduleRequest,
    preferences: &SchedulingPreferences,
    existing: &[ScheduleBlock],
    events: &[ExternalEvent],
    availability: &[AvailabilityWindow],
    hard_busy: &[BusyBlock],
    output: &ItemSchedulingOutput,
) -> String {
    let mut existing = existing.to_vec();
    existing.sort_by_key(|block| (block.start_at, block.end_at, block.id.clone()));
    let mut events = events.to_vec();
    events.sort_by_key(|event| (event.start_at, event.end_at, event.id.clone()));
    let bytes = serde_json::to_vec(&(
        request,
        preferences,
        existing,
        events,
        availability,
        hard_busy,
        output,
    ))
    .expect("auto-schedule context is serializable");
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn iana_date_range(start: Date, end_exclusive: Date, timezone: &str) -> AppResult<TimeWindow> {
    if start >= end_exclusive {
        return Err(AppError::InvalidDateRange);
    }
    let timezone_ref = timezones::get_by_name(timezone)
        .ok_or_else(|| AppError::InvalidTimezone(timezone.to_owned()))?;
    let start_at = local_datetime(start, Time::MIDNIGHT, timezone_ref, true, timezone)?;
    let end_at = local_datetime(end_exclusive, Time::MIDNIGHT, timezone_ref, false, timezone)?;
    Ok(TimeWindow::new(start_at, end_at))
}

pub fn expand_weekly_policy(
    policy: &WeeklyTimePolicy,
    start: Date,
    end_exclusive: Date,
    timezone: &str,
    planning_window: TimeWindow,
) -> AppResult<Vec<TimeWindow>> {
    let timezone_ref = timezones::get_by_name(timezone)
        .ok_or_else(|| AppError::InvalidTimezone(timezone.to_owned()))?;
    let mut date = start.previous_day().unwrap_or(start);
    let mut windows = Vec::new();
    while date < end_exclusive {
        for daily in policy.ranges_for(date.weekday().into()) {
            let end_date = if daily.crosses_midnight() {
                date.next_day().ok_or(AppError::DateRangeOverflow)?
            } else {
                date
            };
            let window_start = local_datetime(date, daily.start, timezone_ref, true, timezone)?;
            let window_end = local_datetime(end_date, daily.end, timezone_ref, false, timezone)?;
            if let Some(window) = clipped_window(window_start, window_end, planning_window) {
                windows.push(window);
            }
        }
        date = date.next_day().ok_or(AppError::DateRangeOverflow)?;
    }
    windows.sort_by_key(|window| (window.start, window.end));
    Ok(windows)
}

fn named_hours_availability(
    preferences: &SchedulingPreferences,
    selected: &[String],
    start: Date,
    end_exclusive: Date,
    timezone: &str,
    planning_window: TimeWindow,
) -> AppResult<Vec<AvailabilityWindow>> {
    let selected_names = selected
        .iter()
        .map(|name| name.trim().to_ascii_lowercase())
        .collect::<HashSet<_>>();
    for requested in &selected_names {
        if !preferences
            .named_hours
            .iter()
            .any(|policy| policy.name.trim().eq_ignore_ascii_case(requested))
        {
            return Err(AppError::NamedHoursNotFound(requested.clone()));
        }
    }

    let mut windows = Vec::new();
    for policy in &preferences.named_hours {
        if !selected_names.is_empty()
            && !selected_names.contains(&policy.name.trim().to_ascii_lowercase())
        {
            continue;
        }
        windows.extend(
            expand_weekly_policy(policy, start, end_exclusive, timezone, planning_window)?
                .into_iter()
                .map(|window| AvailabilityWindow { window }),
        );
    }
    windows.sort_by_key(|availability| (availability.window.start, availability.window.end));
    Ok(windows)
}

fn habit_allowed_windows(
    habit: &Habit,
    occurrence: &HabitOccurrence,
    timezone: &str,
    planning_window: TimeWindow,
) -> AppResult<Vec<TimeWindow>> {
    let timezone_ref = timezones::get_by_name(timezone)
        .ok_or_else(|| AppError::InvalidTimezone(timezone.to_owned()))?;
    let mut windows = if let Some(preferred) = habit.preferred_window {
        let end_date = if preferred.crosses_midnight() {
            occurrence
                .occurrence_date
                .next_day()
                .ok_or(AppError::DateRangeOverflow)?
        } else {
            occurrence.occurrence_date
        };
        vec![TimeWindow::new(
            local_datetime(
                occurrence.occurrence_date,
                preferred.start,
                timezone_ref,
                true,
                timezone,
            )?,
            local_datetime(end_date, preferred.end, timezone_ref, false, timezone)?,
        )]
    } else {
        let next_date = occurrence
            .occurrence_date
            .next_day()
            .ok_or(AppError::DateRangeOverflow)?;
        vec![TimeWindow::new(
            local_datetime(
                occurrence.occurrence_date,
                Time::MIDNIGHT,
                timezone_ref,
                true,
                timezone,
            )?,
            local_datetime(next_date, Time::MIDNIGHT, timezone_ref, false, timezone)?,
        )]
    };

    windows = windows
        .into_iter()
        .filter_map(|window| clipped_window(window.start, window.end, planning_window))
        .collect();

    if occurrence.state != HabitOccurrenceState::Snoozed {
        return Ok(windows);
    }
    let Some(until) = occurrence.snoozed_until else {
        return Ok(windows);
    };
    if until <= planning_window.start {
        return Ok(windows);
    }
    if windows.is_empty() {
        return Ok(if until < planning_window.end {
            vec![TimeWindow::new(until, planning_window.end)]
        } else {
            vec![outside_window(planning_window)]
        });
    }

    windows = windows
        .into_iter()
        .filter_map(|window| clipped_window(window.start.max(until), window.end, planning_window))
        .collect();
    if windows.is_empty() {
        windows.push(outside_window(planning_window));
    }
    Ok(windows)
}

#[must_use]
pub fn schedule_block_busy_blocks(
    blocks: &[ScheduleBlock],
    planning_window: TimeWindow,
) -> Vec<BusyBlock> {
    let mut busy = blocks
        .iter()
        .filter(|block| !is_replaceable_proposal(block))
        .filter(|block| {
            !matches!(
                block.state,
                ScheduleBlockState::Cancelled | ScheduleBlockState::Missed
            )
        })
        .filter_map(|block| {
            clipped_window(block.start_at, block.end_at, planning_window).map(|window| BusyBlock {
                window,
                source: match block.source {
                    ScheduleBlockSource::ExternalCalendar => BusyBlockSource::ExternalCalendar,
                    ScheduleBlockSource::Manual => BusyBlockSource::Manual,
                    ScheduleBlockSource::Scheduler | ScheduleBlockSource::Repair => {
                        BusyBlockSource::LockedSchedule
                    }
                },
                label: block.title_snapshot.clone(),
            })
        })
        .collect::<Vec<_>>();
    busy.sort_by_key(|block| (block.window.start, block.window.end));
    busy
}

fn fixed_schedule_item_keys(blocks: &[ScheduleBlock]) -> HashSet<AutoScheduleItemKey> {
    blocks
        .iter()
        .filter(|block| !is_replaceable_proposal(block))
        .filter(|block| {
            !matches!(
                block.state,
                ScheduleBlockState::Cancelled | ScheduleBlockState::Missed
            )
        })
        .filter_map(block_item_key)
        .collect()
}

#[must_use]
pub fn external_event_busy_blocks(
    events: &[ExternalEvent],
    planning_window: TimeWindow,
) -> Vec<BusyBlock> {
    let mut busy = events
        .iter()
        .filter(|event| event.blocks_time())
        .filter_map(|event| {
            clipped_window(event.start_at, event.end_at, planning_window).map(|window| BusyBlock {
                window,
                source: BusyBlockSource::ExternalCalendar,
                label: Some(event.title.clone()),
            })
        })
        .collect::<Vec<_>>();
    busy.sort_by_key(|block| (block.window.start, block.window.end));
    busy
}

#[must_use]
pub fn proposed_schedule_blocks(
    output: &ItemSchedulingOutput,
    now: OffsetDateTime,
) -> Vec<ScheduleBlock> {
    output
        .blocks
        .iter()
        .map(|block| {
            let (task_id, habit_occurrence_id, block_type) = match &block.item_ref {
                ScheduleItemRef::Task(id) => (Some(id.clone()), None, ScheduleBlockType::Task),
                ScheduleItemRef::HabitOccurrence(id) => {
                    (None, Some(id.clone()), ScheduleBlockType::Habit)
                }
            };
            ScheduleBlock {
                id: ScheduleBlockId::new(),
                task_id,
                habit_occurrence_id,
                title_snapshot: Some(block.title.clone()),
                start_at: block.window.start,
                end_at: block.window.end,
                block_type,
                state: ScheduleBlockState::Proposed,
                locked: false,
                source: ScheduleBlockSource::Scheduler,
                required_minutes: Some(block.required_minutes),
                created_at: now,
                updated_at: now,
            }
        })
        .collect()
}

/// Keeps stable local IDs (and therefore managed calendar links) when an item
/// remains in the auto-scheduled plan, even if its time or duration changes.
#[must_use]
pub fn reconcile_proposed_block_ids(
    existing: &[ScheduleBlock],
    proposed: &[ScheduleBlock],
) -> Vec<ScheduleBlock> {
    let mut candidates = existing
        .iter()
        .filter(|block| is_replaceable_proposal(block))
        .collect::<Vec<_>>();
    candidates.sort_by_key(|block| (block.start_at, block.end_at, block.id.clone()));
    let mut result = proposed.to_vec();
    let mut matched = HashSet::new();
    // Match unchanged chunks first; inserting a chunk must not steal their IDs.
    for exact in [true, false] {
        for (index, block) in result.iter_mut().enumerate() {
            if matched.contains(&index) {
                continue;
            }
            if let Some(position) = candidates.iter().position(|old| {
                block_item_key(old) == block_item_key(block)
                    && (!exact || (old.start_at == block.start_at && old.end_at == block.end_at))
            }) {
                let old = candidates.remove(position);
                block.id = old.id.clone();
                block.created_at = old.created_at;
                matched.insert(index);
            }
        }
    }
    result
}

fn same_schedule_block_snapshot(left: &[ScheduleBlock], right: &[ScheduleBlock]) -> bool {
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    left.sort_by_key(|block| block.id.clone());
    right.sort_by_key(|block| block.id.clone());
    left == right
}

#[must_use]
pub fn diff_auto_schedule(
    existing: &[ScheduleBlock],
    proposed: &ItemSchedulingOutput,
) -> AutoScheduleDiff {
    let mut before = BTreeMap::new();
    let mut sorted_existing = existing
        .iter()
        .filter(|block| is_replaceable_proposal(block))
        .collect::<Vec<_>>();
    sorted_existing.sort_by_key(|block| (block.start_at, block.end_at, block.id.clone()));
    for block in sorted_existing {
        let Some(item) = block_item_key(block) else {
            continue;
        };
        before
            .entry(item.clone())
            .or_insert_with(Vec::new)
            .push(AutoPlanBlockSnapshot {
                block_id: Some(block.id.clone()),
                item,
                title: block
                    .title_snapshot
                    .clone()
                    .unwrap_or_else(|| "(untitled)".into()),
                start_at: block.start_at,
                end_at: block.end_at,
            });
    }
    let mut after = BTreeMap::<AutoScheduleItemKey, Vec<AutoPlanBlockSnapshot>>::new();
    for block in &proposed.blocks {
        let item = AutoScheduleItemKey::from(&block.item_ref);
        after
            .entry(item.clone())
            .or_default()
            .push(AutoPlanBlockSnapshot {
                block_id: None,
                item,
                title: block.title.clone(),
                start_at: block.window.start,
                end_at: block.window.end,
            });
    }
    let keys = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut changes = Vec::with_capacity(keys.len());
    for item in keys {
        let mut old = before.remove(&item).unwrap_or_default();
        let mut new = after.remove(&item).unwrap_or_default();
        new.sort_by_key(|block| (block.start_at, block.end_at));
        let mut pairs = Vec::new();
        for block in new {
            let exact = old
                .iter()
                .position(|prior| prior.start_at == block.start_at && prior.end_at == block.end_at);
            let prior = exact.map(|index| old.remove(index));
            pairs.push((prior, Some(block)));
        }
        for (prior, _) in &mut pairs {
            if prior.is_none() && !old.is_empty() {
                *prior = Some(old.remove(0));
            }
        }
        pairs.extend(old.into_iter().map(|prior| (Some(prior), None)));
        for (old, new) in pairs {
            let (kind, reason) = auto_change_kind(old.as_ref(), new.as_ref());
            let title = new
                .as_ref()
                .or(old.as_ref())
                .map(|block| block.title.clone())
                .unwrap_or_default();
            changes.push(AutoPlanChange {
                kind,
                item: item.clone(),
                title,
                before: old,
                after: new,
                reason,
            });
        }
    }
    let bytes = serde_json::to_vec(&changes).expect("auto-schedule diff is serializable");
    let fingerprint = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    AutoScheduleDiff {
        fingerprint,
        changes,
    }
}

fn auto_change_kind(
    before: Option<&AutoPlanBlockSnapshot>,
    after: Option<&AutoPlanBlockSnapshot>,
) -> (PlanChangeKind, String) {
    match (before, after) {
        (None, Some(_)) => (PlanChangeKind::Create, "新しい空き時間へ配置します".into()),
        (Some(_), None) => (PlanChangeKind::Remove, "自動提案を取り除きます".into()),
        (Some(before), Some(after)) => {
            let moved = before.start_at != after.start_at;
            let resized = (before.end_at - before.start_at) != (after.end_at - after.start_at);
            match (moved, resized) {
                (false, false) => (PlanChangeKind::Unchanged, "変更はありません".into()),
                (true, false) => (PlanChangeKind::Move, "空き時間に合わせて移動します".into()),
                (false, true) => (PlanChangeKind::Resize, "所要時間を更新します".into()),
                (true, true) => (
                    PlanChangeKind::MoveAndResize,
                    "空き時間と所要時間に合わせて更新します".into(),
                ),
            }
        }
        (None, None) => unreachable!("an item is present on at least one side"),
    }
}

fn block_item_key(block: &ScheduleBlock) -> Option<AutoScheduleItemKey> {
    block
        .task_id
        .clone()
        .map(AutoScheduleItemKey::Task)
        .or_else(|| {
            block
                .habit_occurrence_id
                .clone()
                .map(AutoScheduleItemKey::HabitOccurrence)
        })
}

fn is_replaceable_proposal(block: &ScheduleBlock) -> bool {
    block.state == ScheduleBlockState::Proposed
        && !block.locked
        && matches!(
            block.source,
            ScheduleBlockSource::Scheduler | ScheduleBlockSource::Repair
        )
}

fn clipped_window(
    start: OffsetDateTime,
    end: OffsetDateTime,
    bounds: TimeWindow,
) -> Option<TimeWindow> {
    let start = start.max(bounds.start);
    let end = end.min(bounds.end);
    (start < end).then(|| TimeWindow::new(start, end))
}

fn outside_window(bounds: TimeWindow) -> TimeWindow {
    if let Some(end) = bounds.end.checked_add(Duration::seconds(1)) {
        TimeWindow::new(bounds.end, end)
    } else {
        TimeWindow::new(
            bounds
                .start
                .checked_sub(Duration::seconds(1))
                .expect("a valid planning window has representable adjacent time"),
            bounds.start,
        )
    }
}

fn local_datetime(
    date: Date,
    time: Time,
    timezone: &'static time_tz::Tz,
    prefer_earlier: bool,
    timezone_name: &str,
) -> AppResult<OffsetDateTime> {
    match date.with_time(time).assume_timezone(timezone) {
        OffsetResult::Some(value) => Ok(value),
        OffsetResult::Ambiguous(first, second) => Ok(if prefer_earlier {
            first.min(second)
        } else {
            first.max(second)
        }),
        OffsetResult::None => Err(AppError::InvalidLocalTime {
            timezone: timezone_name.to_owned(),
            date,
            time,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use time::macros::{date, datetime, time};

    use super::*;

    struct MemoryTaskRepository(Vec<Task>);

    #[async_trait]
    impl TaskRepository for MemoryTaskRepository {
        async fn insert(&self, _task: Task) -> CoreResult<()> {
            Ok(())
        }

        async fn find(&self, id: TaskId) -> CoreResult<Option<Task>> {
            Ok(self.0.iter().find(|task| task.id == id).cloned())
        }

        async fn update(&self, _task: Task) -> CoreResult<()> {
            Ok(())
        }

        async fn list_all(&self) -> CoreResult<Vec<Task>> {
            Ok(self.0.clone())
        }

        async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Task>> {
            Ok(self
                .0
                .iter()
                .filter(|task| task.project_id.as_ref() == Some(&project_id))
                .cloned()
                .collect())
        }

        async fn list_by_list(&self, list_id: ListId) -> CoreResult<Vec<Task>> {
            Ok(self
                .0
                .iter()
                .filter(|task| task.list_id.as_ref() == Some(&list_id))
                .cloned()
                .collect())
        }

        async fn soft_delete(&self, _id: TaskId, _deleted_at: OffsetDateTime) -> CoreResult<()> {
            Ok(())
        }
    }

    struct MemoryStatusRepository {
        group: StatusGroup,
        status: Status,
    }

    #[async_trait]
    impl StatusRepository for MemoryStatusRepository {
        async fn insert_group(&self, _group: StatusGroup) -> CoreResult<()> {
            Ok(())
        }

        async fn insert_status(&self, _status: Status) -> CoreResult<()> {
            Ok(())
        }

        async fn list_groups(&self) -> CoreResult<Vec<StatusGroup>> {
            Ok(vec![self.group.clone()])
        }

        async fn list_statuses_for_project(
            &self,
            project_id: Option<ProjectId>,
        ) -> CoreResult<Vec<Status>> {
            Ok((project_id.is_none())
                .then(|| self.status.clone())
                .into_iter()
                .collect())
        }
    }

    #[derive(Default)]
    struct MemoryBlockRepository(Mutex<Vec<ScheduleBlock>>);

    #[async_trait]
    impl ScheduleBlockRepository for MemoryBlockRepository {
        async fn insert(&self, block: ScheduleBlock) -> CoreResult<()> {
            self.0.lock().unwrap().push(block);
            Ok(())
        }

        async fn find(&self, id: ScheduleBlockId) -> CoreResult<Option<ScheduleBlock>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|block| block.id == id)
                .cloned())
        }

        async fn update(&self, block: ScheduleBlock) -> CoreResult<()> {
            let mut blocks = self.0.lock().unwrap();
            if let Some(existing) = blocks.iter_mut().find(|item| item.id == block.id) {
                *existing = block;
            }
            Ok(())
        }

        async fn list_for_day(&self, _day: Date) -> CoreResult<Vec<ScheduleBlock>> {
            Ok(self.0.lock().unwrap().clone())
        }

        async fn list_overlapping(
            &self,
            start: OffsetDateTime,
            end: OffsetDateTime,
        ) -> CoreResult<Vec<ScheduleBlock>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|block| block.start_at < end && block.end_at > start)
                .cloned()
                .collect())
        }

        async fn replace_proposed_for_day(
            &self,
            _day: Date,
            blocks: Vec<ScheduleBlock>,
        ) -> CoreResult<()> {
            *self.0.lock().unwrap() = blocks;
            Ok(())
        }

        async fn replace_proposed_in_range(
            &self,
            start: OffsetDateTime,
            end: OffsetDateTime,
            blocks: Vec<ScheduleBlock>,
        ) -> CoreResult<()> {
            let mut stored = self.0.lock().unwrap();
            stored.retain(|block| {
                !(is_replaceable_proposal(block) && block.start_at < end && block.end_at > start)
            });
            stored.extend(blocks);
            Ok(())
        }
    }

    struct MemoryExternalEventRepository(Mutex<Vec<ExternalEvent>>);

    #[async_trait]
    impl ExternalEventRepository for MemoryExternalEventRepository {
        async fn upsert(&self, _event: ExternalEvent) -> CoreResult<()> {
            Ok(())
        }

        async fn find(&self, id: ExternalEventId) -> CoreResult<Option<ExternalEvent>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|event| event.id == id)
                .cloned())
        }

        async fn find_by_provider_event(
            &self,
            account_id: CalendarAccountId,
            calendar_id: String,
            provider_event_id: String,
        ) -> CoreResult<Option<ExternalEvent>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|event| {
                    event.account_id == account_id
                        && event.calendar_id == calendar_id
                        && event.provider_event_id == provider_event_id
                })
                .cloned())
        }

        async fn list_overlapping(
            &self,
            start: OffsetDateTime,
            end: OffsetDateTime,
        ) -> CoreResult<Vec<ExternalEvent>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.overlaps(start, end))
                .cloned()
                .collect())
        }

        async fn list_overlapping_for_account(
            &self,
            account_id: CalendarAccountId,
            start: OffsetDateTime,
            end: OffsetDateTime,
        ) -> CoreResult<Vec<ExternalEvent>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.account_id == account_id && event.overlaps(start, end))
                .cloned()
                .collect())
        }
    }

    struct MemoryHabitRepository(Mutex<Vec<Habit>>);

    #[async_trait]
    impl HabitRepository for MemoryHabitRepository {
        async fn upsert(&self, habit: Habit) -> CoreResult<()> {
            let mut habits = self.0.lock().unwrap();
            if let Some(existing) = habits.iter_mut().find(|item| item.id == habit.id) {
                *existing = habit;
            } else {
                habits.push(habit);
            }
            Ok(())
        }

        async fn find(&self, id: HabitId) -> CoreResult<Option<Habit>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|habit| habit.id == id)
                .cloned())
        }

        async fn list_enabled(&self, user_id: UserId) -> CoreResult<Vec<Habit>> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|habit| habit.user_id == user_id && habit.enabled)
                .cloned()
                .collect())
        }
    }

    #[derive(Default)]
    struct MemoryOccurrenceRepository(Mutex<Vec<HabitOccurrence>>, AtomicUsize);

    #[async_trait]
    impl HabitOccurrenceRepository for MemoryOccurrenceRepository {
        async fn upsert(&self, occurrence: HabitOccurrence) -> CoreResult<()> {
            if self
                .1
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(CoreError::Storage("injected occurrence failure".into()));
            }
            let mut occurrences = self.0.lock().unwrap();
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
                .0
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
                .0
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
                .0
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

    struct MemoryPreferencesRepository(SchedulingPreferences);

    #[async_trait]
    impl SchedulingPreferencesRepository for MemoryPreferencesRepository {
        async fn upsert(&self, _preferences: SchedulingPreferences) -> CoreResult<()> {
            Ok(())
        }

        async fn get_for_user(&self, user_id: UserId) -> CoreResult<Option<SchedulingPreferences>> {
            Ok((self.0.user_id == user_id).then(|| self.0.clone()))
        }
    }

    fn test_status_repository() -> MemoryStatusRepository {
        let group = StatusGroup {
            id: StatusGroupId::new(),
            name: "To do".into(),
            kind: StatusGroupKind::NotStarted,
        };
        MemoryStatusRepository {
            status: Status {
                id: StatusId::new(),
                project_id: None,
                name: "To do".into(),
                group_id: group.id.clone(),
                order: 0,
            },
            group,
        }
    }

    fn test_preferences(
        user_id: UserId,
        timezone: &str,
        weekday: DayOfWeek,
        travel_minutes: u32,
    ) -> SchedulingPreferences {
        SchedulingPreferences {
            id: SchedulingPolicyId::new(),
            user_id,
            timezone: timezone.into(),
            named_hours: vec![WeeklyTimePolicy {
                name: "work".into(),
                hard: false,
                days: vec![WeekdayTimeRanges {
                    weekday,
                    ranges: vec![DailyTimeRange {
                        start: time!(09:00),
                        end: time!(17:00),
                    }],
                }],
            }],
            sleep: None,
            default_travel_buffer_minutes: travel_minutes,
            created_at: datetime!(2026-01-01 00:00 UTC),
            updated_at: datetime!(2026-01-01 00:00 UTC),
        }
    }

    #[test]
    fn iana_range_uses_real_dst_offsets() {
        let range = iana_date_range(
            date!(2026 - 03 - 08),
            date!(2026 - 03 - 09),
            "America/New_York",
        )
        .unwrap();

        assert_eq!((range.end - range.start).whole_hours(), 23);
    }

    #[test]
    fn crossing_midnight_policy_is_clipped_at_range_start() {
        let policy = WeeklyTimePolicy {
            name: "sleep".into(),
            hard: true,
            days: vec![WeekdayTimeRanges {
                weekday: DayOfWeek::Friday,
                ranges: vec![DailyTimeRange {
                    start: time!(23:00),
                    end: time!(07:00),
                }],
            }],
        };
        let planning =
            iana_date_range(date!(2026 - 08 - 15), date!(2026 - 08 - 16), "Asia/Tokyo").unwrap();
        let windows = expand_weekly_policy(
            &policy,
            date!(2026 - 08 - 15),
            date!(2026 - 08 - 16),
            "Asia/Tokyo",
            planning,
        )
        .unwrap();

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].start, planning.start);
        assert_eq!((windows[0].end - windows[0].start).whole_hours(), 7);
    }

    #[test]
    fn generic_diff_tracks_habit_occurrence() {
        let occurrence_id = HabitOccurrenceId::new();
        let output = ItemSchedulingOutput {
            blocks: vec![mnema_scheduler::ProposedItemScheduleBlock {
                item_ref: ScheduleItemRef::HabitOccurrence(occurrence_id.clone()),
                kind: ScheduleItemKind::HabitOccurrence,
                tier: ScheduleItemTier::RequiredHabit,
                title: "Stretch".into(),
                window: TimeWindow::new(
                    datetime!(2026-08-15 09:00 UTC),
                    datetime!(2026-08-15 09:30 UTC),
                ),
                required_minutes: 30,
            }],
            unscheduled: vec![],
            issues: vec![],
        };

        let diff = diff_auto_schedule(&[], &output);

        assert_eq!(diff.changed_count(), 1);
        assert_eq!(diff.changes[0].kind, PlanChangeKind::Create);
        assert_eq!(
            diff.changes[0].item,
            AutoScheduleItemKey::HabitOccurrence(occurrence_id)
        );
        assert_eq!(diff.fingerprint.len(), 64);
    }

    #[test]
    fn reconciliation_keeps_id_when_item_moves() {
        let task_id = TaskId::new();
        let output_at = |start, end| ItemSchedulingOutput {
            blocks: vec![mnema_scheduler::ProposedItemScheduleBlock {
                item_ref: ScheduleItemRef::Task(task_id.clone()),
                kind: ScheduleItemKind::Task,
                tier: ScheduleItemTier::Task,
                title: "Focus".into(),
                window: TimeWindow::new(start, end),
                required_minutes: 30,
            }],
            unscheduled: vec![],
            issues: vec![],
        };
        let created_at = datetime!(2026-08-15 00:00 UTC);
        let existing = proposed_schedule_blocks(
            &output_at(
                datetime!(2026-08-17 09:00 UTC),
                datetime!(2026-08-17 09:30 UTC),
            ),
            created_at,
        );
        let original_id = existing[0].id.clone();
        let moved = proposed_schedule_blocks(
            &output_at(
                datetime!(2026-08-17 10:00 UTC),
                datetime!(2026-08-17 10:30 UTC),
            ),
            datetime!(2026-08-16 00:00 UTC),
        );

        let reconciled = reconcile_proposed_block_ids(&existing, &moved);

        assert_eq!(reconciled[0].id, original_id);
        assert_eq!(reconciled[0].created_at, created_at);
        assert_eq!(reconciled[0].start_at, datetime!(2026-08-17 10:00 UTC));
    }

    #[test]
    fn defaults_are_valid_and_protect_sleep() {
        let preferences = default_scheduling_preferences(
            UserId::new(),
            "Asia/Tokyo",
            datetime!(2026-08-15 00:00 UTC),
        );

        assert!(preferences.validate().is_ok());
        assert_eq!(preferences.named_hours[0].name, "work");
        assert!(preferences.sleep.as_ref().unwrap().hard);
        assert_eq!(preferences.default_travel_buffer_minutes, 15);
    }

    #[test]
    fn habits_without_preferred_windows_stay_on_their_occurrence_days() {
        let user_id = UserId::new();
        let habit = Habit {
            id: HabitId::new(),
            user_id,
            title: "Walk".into(),
            schedule: HabitSchedule::Daily,
            duration_minutes: 30,
            preferred_window: None,
            flexibility: HabitFlexibility::Required,
            enabled: true,
            disabled_at: None,
            created_at: datetime!(2026-08-15 00:00 UTC),
            updated_at: datetime!(2026-08-15 00:00 UTC),
        };
        let start = date!(2026 - 08 - 15);
        let end = date!(2026 - 08 - 22);
        let planning = iana_date_range(start, end, "Asia/Tokyo").unwrap();
        let mut date = start;
        let mut expected_windows = BTreeMap::new();
        let mut items = Vec::new();
        while date < end {
            let occurrence = HabitOccurrence::pending_deterministic(
                habit.id.clone(),
                date,
                datetime!(2026-08-15 00:00 UTC),
            );
            let allowed =
                habit_allowed_windows(&habit, &occurrence, "Asia/Tokyo", planning).unwrap();
            assert_eq!(allowed.len(), 1);
            expected_windows.insert(occurrence.id.clone(), allowed[0]);
            items.push(SchedulableItem {
                item_ref: ScheduleItemRef::HabitOccurrence(occurrence.id),
                kind: ScheduleItemKind::HabitOccurrence,
                tier: ScheduleItemTier::RequiredHabit,
                title: habit.title.clone(),
                duration_minutes: habit.duration_minutes,
                due: Some(date),
                importance: 0,
                created_at: occurrence.created_at,
                allowed_windows: allowed,
                not_before: None,
                dependencies: vec![],
                minimum_chunk_minutes: None,
            });
            date = date.next_day().unwrap();
        }

        let output = GreedyScheduler::default().plan_items(ItemSchedulingInput {
            items,
            availability: vec![AvailabilityWindow { window: planning }],
            hard_busy: vec![],
        });

        assert_eq!(output.blocks.len(), 7);
        for block in output.blocks {
            let ScheduleItemRef::HabitOccurrence(id) = block.item_ref else {
                panic!("expected a habit occurrence");
            };
            let expected = expected_windows.get(&id).unwrap();
            assert!(block.window.start >= expected.start);
            assert!(block.window.end <= expected.end);
        }
    }

    #[tokio::test]
    async fn persisted_timezone_normalizes_request_and_keeps_dst_boundaries() {
        let user_id = UserId::new();
        let tasks = MemoryTaskRepository(vec![]);
        let statuses = test_status_repository();
        let blocks = MemoryBlockRepository::default();
        let events = MemoryExternalEventRepository(Mutex::new(vec![]));
        let habits = MemoryHabitRepository(Mutex::new(vec![]));
        let occurrences = MemoryOccurrenceRepository::default();
        let preferences = MemoryPreferencesRepository(test_preferences(
            user_id.clone(),
            "America/New_York",
            DayOfWeek::Sunday,
            0,
        ));
        let service = AutoScheduleService::new(
            &tasks,
            &statuses,
            &blocks,
            &events,
            &habits,
            &occurrences,
            &preferences,
        );

        let preview = service
            .preview(AutoScheduleRequest {
                user_id,
                start_date: date!(2026 - 03 - 08),
                end_date_exclusive: date!(2026 - 03 - 09),
                timezone: "Asia/Tokyo".into(),
                named_hours: vec!["work".into()],
                not_before: None,
            })
            .await
            .unwrap();

        assert_eq!(preview.request.timezone, "America/New_York");
        assert_eq!(
            (preview.planning_window.end - preview.planning_window.start).whole_hours(),
            23
        );
        assert_eq!(
            preview.availability[0].window.start,
            datetime!(2026-03-08 09:00 -04:00)
        );
        assert_eq!(
            preview.availability[0].window.end,
            datetime!(2026-03-08 17:00 -04:00)
        );
    }

    #[tokio::test]
    async fn event_outside_default_query_uses_after_travel_override() {
        let user_id = UserId::new();
        let tasks = MemoryTaskRepository(vec![]);
        let statuses = test_status_repository();
        let blocks = MemoryBlockRepository::default();
        let events = MemoryExternalEventRepository(Mutex::new(vec![ExternalEvent {
            id: ExternalEventId::new(),
            account_id: CalendarAccountId::new(),
            calendar_id: "primary".into(),
            provider_event_id: "previous-evening".into(),
            recurring_event_id: None,
            original_start_at: None,
            title: "Late client visit".into(),
            description: None,
            location: Some("Yokohama".into()),
            start_at: datetime!(2026-08-14 22:30 UTC),
            end_at: datetime!(2026-08-14 23:00 UTC),
            all_day: false,
            timezone: Some("UTC".into()),
            status: ExternalEventStatus::Confirmed,
            transparency: ExternalEventTransparency::Opaque,
            needs_travel: Some(true),
            travel_before_minutes: None,
            travel_after_minutes: Some(90),
            etag: None,
            provider_updated_at: None,
            created_at: datetime!(2026-08-14 00:00 UTC),
            updated_at: datetime!(2026-08-14 00:00 UTC),
        }]));
        let habits = MemoryHabitRepository(Mutex::new(vec![]));
        let occurrences = MemoryOccurrenceRepository::default();
        let preferences = MemoryPreferencesRepository(test_preferences(
            user_id.clone(),
            "UTC",
            DayOfWeek::Saturday,
            30,
        ));
        let service = AutoScheduleService::new(
            &tasks,
            &statuses,
            &blocks,
            &events,
            &habits,
            &occurrences,
            &preferences,
        );

        let preview = service
            .preview(AutoScheduleRequest {
                user_id,
                start_date: date!(2026 - 08 - 15),
                end_date_exclusive: date!(2026 - 08 - 16),
                timezone: "UTC".into(),
                named_hours: vec!["work".into()],
                not_before: None,
            })
            .await
            .unwrap();

        assert!(preview.hard_busy.iter().any(|busy| {
            busy.window.start == datetime!(2026-08-15 00:00 UTC)
                && busy.window.end == datetime!(2026-08-15 00:30 UTC)
                && busy.label.as_deref() == Some("Travel from Late client visit")
        }));
    }

    #[tokio::test]
    async fn preview_and_apply_combine_constraints_and_keep_block_ids_stable() {
        let user_id = UserId::new();
        let todo_group = StatusGroup {
            id: StatusGroupId::new(),
            name: "To do".into(),
            kind: StatusGroupKind::NotStarted,
        };
        let todo_status = Status {
            id: StatusId::new(),
            project_id: None,
            name: "To do".into(),
            group_id: todo_group.id.clone(),
            order: 0,
        };
        let task_id = TaskId::new();
        let fixed_task_id = TaskId::new();
        let task = |id: TaskId, title: &str, minutes| Task {
            id,
            title: title.into(),
            description: None,
            project_id: None,
            list_id: None,
            status_id: todo_status.id.clone(),
            due_date: Some(date!(2026 - 08 - 17)),
            start_date: None,
            estimated_minutes: Some(minutes),
            cost_points: None,
            dependencies: vec![],
            milestone_id: None,
            created_at: datetime!(2026-08-15 00:00 UTC),
            updated_at: datetime!(2026-08-15 00:00 UTC),
            deleted_at: None,
        };
        let tasks = MemoryTaskRepository(vec![
            task(task_id.clone(), "Deep work", 90),
            task(fixed_task_id.clone(), "Already fixed", 30),
        ]);
        let statuses = MemoryStatusRepository {
            group: todo_group,
            status: todo_status,
        };
        let locked = ScheduleBlock {
            id: ScheduleBlockId::new(),
            task_id: Some(fixed_task_id.clone()),
            habit_occurrence_id: None,
            title_snapshot: Some("Already fixed".into()),
            start_at: datetime!(2026-08-17 09:00 +09:00),
            end_at: datetime!(2026-08-17 09:30 +09:00),
            block_type: ScheduleBlockType::Task,
            state: ScheduleBlockState::Scheduled,
            locked: true,
            source: ScheduleBlockSource::Manual,
            required_minutes: Some(30),
            created_at: datetime!(2026-08-15 00:00 UTC),
            updated_at: datetime!(2026-08-15 00:00 UTC),
        };
        let blocks = MemoryBlockRepository(Mutex::new(vec![locked]));
        let events = MemoryExternalEventRepository(Mutex::new(vec![ExternalEvent {
            id: ExternalEventId::new(),
            account_id: CalendarAccountId::new(),
            calendar_id: "primary".into(),
            provider_event_id: "meeting".into(),
            recurring_event_id: None,
            original_start_at: None,
            title: "Customer meeting".into(),
            description: None,
            location: Some("Shibuya".into()),
            start_at: datetime!(2026-08-17 10:30 +09:00),
            end_at: datetime!(2026-08-17 11:00 +09:00),
            all_day: false,
            timezone: Some("Asia/Tokyo".into()),
            status: ExternalEventStatus::Confirmed,
            transparency: ExternalEventTransparency::Opaque,
            needs_travel: Some(true),
            travel_before_minutes: None,
            travel_after_minutes: None,
            etag: None,
            provider_updated_at: None,
            created_at: datetime!(2026-08-15 00:00 UTC),
            updated_at: datetime!(2026-08-15 00:00 UTC),
        }]));
        let habit = Habit {
            id: HabitId::new(),
            user_id: user_id.clone(),
            title: "Stretch".into(),
            schedule: HabitSchedule::Weekdays {
                weekdays: vec![DayOfWeek::Monday],
            },
            duration_minutes: 30,
            preferred_window: Some(DailyTimeRange {
                start: time!(09:30),
                end: time!(10:30),
            }),
            flexibility: HabitFlexibility::Required,
            enabled: true,
            disabled_at: None,
            created_at: datetime!(2026-08-15 00:00 UTC),
            updated_at: datetime!(2026-08-15 00:00 UTC),
        };
        let habits = MemoryHabitRepository(Mutex::new(vec![habit]));
        let occurrences = MemoryOccurrenceRepository::default();
        let preferences = MemoryPreferencesRepository(SchedulingPreferences {
            id: SchedulingPolicyId::new(),
            user_id: user_id.clone(),
            timezone: "Asia/Tokyo".into(),
            named_hours: vec![WeeklyTimePolicy {
                name: "work".into(),
                hard: false,
                days: vec![WeekdayTimeRanges {
                    weekday: DayOfWeek::Monday,
                    ranges: vec![DailyTimeRange {
                        start: time!(09:00),
                        end: time!(17:00),
                    }],
                }],
            }],
            sleep: Some(WeeklyTimePolicy {
                name: "sleep".into(),
                hard: true,
                days: vec![WeekdayTimeRanges {
                    weekday: DayOfWeek::Monday,
                    ranges: vec![DailyTimeRange {
                        start: time!(12:30),
                        end: time!(13:30),
                    }],
                }],
            }),
            default_travel_buffer_minutes: 30,
            created_at: datetime!(2026-08-15 00:00 UTC),
            updated_at: datetime!(2026-08-15 00:00 UTC),
        });
        let service = AutoScheduleService::new(
            &tasks,
            &statuses,
            &blocks,
            &events,
            &habits,
            &occurrences,
            &preferences,
        );
        let request = AutoScheduleRequest {
            user_id,
            start_date: date!(2026 - 08 - 17),
            end_date_exclusive: date!(2026 - 08 - 18),
            timezone: "Asia/Tokyo".into(),
            named_hours: vec!["work".into()],
            not_before: None,
        };

        assert!(occurrences.0.lock().unwrap().is_empty());
        let preview = service.preview(request.clone()).await.unwrap();
        assert!(occurrences.0.lock().unwrap().is_empty());
        assert_eq!(preview.new_habit_occurrences.len(), 1);
        let cancelled_preview = service.preview(request.clone()).await.unwrap();
        assert!(occurrences.0.lock().unwrap().is_empty());
        assert_eq!(
            preview.new_habit_occurrences[0].id,
            cancelled_preview.new_habit_occurrences[0].id
        );
        let habit_block = preview
            .proposed_blocks
            .iter()
            .find(|block| block.habit_occurrence_id.is_some())
            .unwrap();
        let task_block = preview
            .proposed_blocks
            .iter()
            .find(|block| block.task_id.as_ref() == Some(&task_id))
            .unwrap();
        assert_eq!(habit_block.start_at, datetime!(2026-08-17 09:30 +09:00));
        assert_eq!(task_block.start_at, datetime!(2026-08-17 13:30 +09:00));
        assert!(
            !preview
                .output
                .blocks
                .iter()
                .any(|block| { block.item_ref == ScheduleItemRef::Task(fixed_task_id.clone()) })
        );

        let first = service
            .apply(&preview, &preview.diff.fingerprint)
            .await
            .unwrap();
        assert_eq!(occurrences.0.lock().unwrap().len(), 1);
        let persisted_habit_id = first
            .blocks
            .iter()
            .find(|block| block.habit_occurrence_id.is_some())
            .unwrap()
            .id
            .clone();
        let occurrence = occurrences.0.lock().unwrap()[0].clone();
        assert_eq!(occurrence.state, HabitOccurrenceState::Scheduled);
        assert_eq!(
            occurrence.scheduled_start_at,
            Some(datetime!(2026-08-17 09:30 +09:00))
        );

        let second_preview = service.preview(request.clone()).await.unwrap();
        let second = service
            .apply(&second_preview, &second_preview.diff.fingerprint)
            .await
            .unwrap();
        assert_eq!(
            second
                .blocks
                .iter()
                .find(|block| block.habit_occurrence_id.is_some())
                .unwrap()
                .id,
            persisted_habit_id
        );

        let failure_preview = service.preview(request).await.unwrap();
        let blocks_before_failure = blocks.0.lock().unwrap().clone();
        occurrences.1.store(1, Ordering::SeqCst);
        assert!(matches!(
            service
                .apply(&failure_preview, &failure_preview.diff.fingerprint)
                .await,
            Err(AppError::Repository(_))
        ));
        assert_eq!(*blocks.0.lock().unwrap(), blocks_before_failure);

        let mut late_event = events.0.lock().unwrap()[0].clone();
        late_event.id = ExternalEventId::new();
        late_event.provider_event_id = "late-change".into();
        late_event.title = "New calendar conflict".into();
        late_event.location = None;
        late_event.needs_travel = Some(false);
        late_event.transparency = ExternalEventTransparency::Transparent;
        late_event.start_at = datetime!(2026-08-17 13:30 +09:00);
        late_event.end_at = datetime!(2026-08-17 15:30 +09:00);
        events.0.lock().unwrap().push(late_event);
        assert!(matches!(
            service
                .apply(&failure_preview, &failure_preview.diff.fingerprint)
                .await,
            Err(AppError::StaleSchedulePreview)
        ));
    }
}
