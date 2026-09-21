//! Deterministic scheduling and repair primitives for Mnema.
//!
//! This crate intentionally avoids storage, UI, and LLM dependencies. It turns
//! existing Mnema tasks into proposed schedule blocks that can later be reviewed
//! and persisted by application services.

use std::cmp::{Ordering, max, min};
use std::collections::{HashMap, HashSet};

use mnema_core::prelude::*;
use serde::{Deserialize, Serialize};
use time::{Date, Duration, OffsetDateTime};

pub type Minutes = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    #[serde(with = "time::serde::rfc3339")]
    pub start: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub end: OffsetDateTime,
}

impl TimeWindow {
    /// Creates a half-open time window: [start, end).
    ///
    /// # Panics
    ///
    /// Panics when `start >= end`.
    #[must_use]
    pub fn new(start: OffsetDateTime, end: OffsetDateTime) -> Self {
        assert!(start < end, "time window start must be before end");
        Self { start, end }
    }

    #[must_use]
    pub fn duration_minutes(self) -> Minutes {
        let minutes = (self.end - self.start).whole_minutes();
        Minutes::try_from(minutes).unwrap_or(Minutes::MAX)
    }

    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    #[must_use]
    pub fn can_fit_minutes(self, minutes: Minutes) -> bool {
        self.duration_minutes() >= minutes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BusyBlockSource {
    ExternalCalendar,
    LockedSchedule,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusyBlock {
    pub window: TimeWindow,
    pub source: BusyBlockSource,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailabilityWindow {
    pub window: TimeWindow,
}

/// Stable reference to an item that can be placed by the scheduler.
///
/// The scheduler deliberately stores only provider-independent domain IDs. It
/// does not load tasks, expand recurring habits, or persist proposals itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScheduleItemRef {
    Task(TaskId),
    HabitOccurrence(HabitOccurrenceId),
}

impl ScheduleItemRef {
    #[must_use]
    pub fn kind(&self) -> ScheduleItemKind {
        match self {
            Self::Task(_) => ScheduleItemKind::Task,
            Self::HabitOccurrence(_) => ScheduleItemKind::HabitOccurrence,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleItemKind {
    Task,
    HabitOccurrence,
}

/// Coarse deterministic allocation tier.
///
/// Hard constraints are represented separately as availability and hard-busy
/// windows. Within the remaining time, required habits are placed first,
/// followed by flexible habits and ordinary tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleItemTier {
    RequiredHabit,
    FlexibleHabit,
    Task,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulableItem {
    pub item_ref: ScheduleItemRef,
    pub kind: ScheduleItemKind,
    pub tier: ScheduleItemTier,
    pub title: String,
    pub duration_minutes: Minutes,
    pub due: Option<Date>,
    pub importance: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Empty means unrestricted within global availability. Non-empty windows
    /// are intersected with global availability and all remaining free slots.
    pub allowed_windows: Vec<TimeWindow>,
    #[serde(default)]
    pub not_before: Option<OffsetDateTime>,
    /// References not present in the plan remain blocked; callers remove
    /// already completed dependencies before submitting items.
    #[serde(default)]
    pub dependencies: Vec<ScheduleItemRef>,
    /// None keeps an item contiguous (in particular, habits).
    #[serde(default)]
    pub minimum_chunk_minutes: Option<Minutes>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedItemScheduleBlock {
    pub item_ref: ScheduleItemRef,
    pub kind: ScheduleItemKind,
    pub tier: ScheduleItemTier,
    pub title: String,
    pub window: TimeWindow,
    pub required_minutes: Minutes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnscheduledItemReason {
    InvalidDuration,
    NoAvailability,
    NoAllowedWindow,
    InsufficientContiguousTime,
    DependencyBlocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemScheduleIssue {
    NoAvailability,
    ItemUnscheduled {
        item_ref: ScheduleItemRef,
        kind: ScheduleItemKind,
        title: String,
        required_minutes: Minutes,
        reason: UnscheduledItemReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSchedulingInput {
    pub items: Vec<SchedulableItem>,
    pub availability: Vec<AvailabilityWindow>,
    pub hard_busy: Vec<BusyBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSchedulingOutput {
    pub blocks: Vec<ProposedItemScheduleBlock>,
    pub unscheduled: Vec<ScheduleItemRef>,
    pub issues: Vec<ItemScheduleIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedScheduleBlock {
    pub task_id: TaskId,
    pub title: String,
    pub window: TimeWindow,
    pub required_minutes: Minutes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleIssue {
    NoAvailability,
    TaskUnscheduled {
        task_id: TaskId,
        title: String,
        required_minutes: Minutes,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulingInput {
    pub tasks: Vec<Task>,
    pub statuses: Vec<Status>,
    pub status_groups: Vec<StatusGroup>,
    pub availability: Vec<AvailabilityWindow>,
    pub busy_blocks: Vec<BusyBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulingOutput {
    pub blocks: Vec<ProposedScheduleBlock>,
    pub unscheduled: Vec<TaskId>,
    pub issues: Vec<ScheduleIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerConfig {
    pub default_task_minutes: Minutes,
    pub buffer_minutes: Minutes,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            default_task_minutes: 30,
            buffer_minutes: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct GreedyScheduler {
    config: SchedulerConfig,
}

impl GreedyScheduler {
    #[must_use]
    pub fn new(config: SchedulerConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn plan(&self, input: SchedulingInput) -> SchedulingOutput {
        let SchedulingInput {
            tasks,
            statuses,
            status_groups,
            availability,
            busy_blocks,
        } = input;
        let done_status_ids = done_status_ids(&statuses, &status_groups);
        let completed = tasks
            .iter()
            .filter(|task| done_status_ids.contains(&task.status_id))
            .map(|task| task.id.clone())
            .collect::<HashSet<_>>();
        let offset = availability
            .first()
            .map(|slot| slot.window.start.offset())
            .unwrap_or(time::UtcOffset::UTC);
        let items = tasks
            .into_iter()
            .filter(|task| is_schedulable(task, &done_status_ids))
            .map(|task| SchedulableItem {
                item_ref: ScheduleItemRef::Task(task.id.clone()),
                kind: ScheduleItemKind::Task,
                tier: ScheduleItemTier::Task,
                title: task.title.clone(),
                duration_minutes: self.required_minutes(&task),
                due: task.due_date,
                importance: task.cost_points.unwrap_or_default(),
                created_at: task.created_at,
                allowed_windows: Vec::new(),
                not_before: task
                    .start_date
                    .map(|date| date.midnight().assume_offset(offset)),
                dependencies: task
                    .dependencies
                    .iter()
                    .filter(|id| !completed.contains(id))
                    .cloned()
                    .map(ScheduleItemRef::Task)
                    .collect(),
                minimum_chunk_minutes: Some(15),
            })
            .collect::<Vec<_>>();
        let output = self.plan_items(ItemSchedulingInput {
            items,
            availability,
            hard_busy: busy_blocks,
        });

        let blocks = output
            .blocks
            .into_iter()
            .filter_map(|block| match block.item_ref {
                ScheduleItemRef::Task(task_id) => Some(ProposedScheduleBlock {
                    task_id,
                    title: block.title,
                    window: block.window,
                    required_minutes: block.required_minutes,
                }),
                ScheduleItemRef::HabitOccurrence(_) => None,
            })
            .collect();
        let unscheduled = output
            .unscheduled
            .into_iter()
            .filter_map(|item_ref| match item_ref {
                ScheduleItemRef::Task(task_id) => Some(task_id),
                ScheduleItemRef::HabitOccurrence(_) => None,
            })
            .collect();
        let issues = output
            .issues
            .into_iter()
            .filter_map(|issue| match issue {
                ItemScheduleIssue::NoAvailability => Some(ScheduleIssue::NoAvailability),
                ItemScheduleIssue::ItemUnscheduled {
                    item_ref: ScheduleItemRef::Task(task_id),
                    title,
                    required_minutes,
                    ..
                } => Some(ScheduleIssue::TaskUnscheduled {
                    task_id,
                    title,
                    required_minutes,
                }),
                ItemScheduleIssue::ItemUnscheduled {
                    item_ref: ScheduleItemRef::HabitOccurrence(_),
                    ..
                } => None,
            })
            .collect();

        SchedulingOutput {
            blocks,
            unscheduled,
            issues,
        }
    }

    /// Plans provider-independent tasks and habit occurrences.
    ///
    /// Allocation is deterministic: tier, due date, descending importance,
    /// creation time, title, and finally the stable item ID. Each item is put in
    /// the earliest remaining contiguous slot that also satisfies its optional
    /// allowed windows.
    #[must_use]
    pub fn plan_items(&self, input: ItemSchedulingInput) -> ItemSchedulingOutput {
        let ItemSchedulingInput {
            mut items,
            availability,
            hard_busy,
        } = input;
        let availability_windows = normalized_availability(&availability);
        let busy = busy_windows(&hard_busy);
        let mut free_slots = free_slots(&availability_windows, &busy);
        let initially_has_free_time = !free_slots.is_empty();
        let mut issues = Vec::new();

        if !initially_has_free_time {
            issues.push(ItemScheduleIssue::NoAvailability);
        }

        items.sort_by(compare_items);

        let mut blocks = Vec::new();
        let mut unscheduled = Vec::new();

        let mut completed = HashMap::<ScheduleItemRef, OffsetDateTime>::new();
        while !items.is_empty() {
            let ready = items.iter().position(|item| {
                item.dependencies
                    .iter()
                    .all(|dependency| completed.contains_key(dependency))
            });
            let mut item = items.remove(ready.unwrap_or(0));
            let blocked = ready.is_none();
            if let Some(end) = item
                .dependencies
                .iter()
                .filter_map(|id| completed.get(id))
                .max()
            {
                item.not_before = Some(item.not_before.map_or(*end, |start| start.max(*end)));
            }
            let reserved = (!blocked)
                .then(|| reserve_item(&free_slots, &item))
                .flatten();
            if let Some(windows) = reserved {
                for window in windows {
                    free_slots = free_slots
                        .into_iter()
                        .flat_map(|slot| subtract_window(slot, window))
                        .collect();
                    completed.insert(item.item_ref.clone(), window.end);
                    blocks.push(ProposedItemScheduleBlock {
                        item_ref: item.item_ref.clone(),
                        kind: item.kind,
                        tier: item.tier,
                        title: item.title.clone(),
                        window,
                        required_minutes: window.duration_minutes(),
                    });
                }
                continue;
            }

            let reason = if blocked {
                UnscheduledItemReason::DependencyBlocked
            } else {
                unscheduled_reason(&item, &availability_windows, initially_has_free_time)
            };
            issues.push(ItemScheduleIssue::ItemUnscheduled {
                item_ref: item.item_ref.clone(),
                kind: item.kind,
                title: item.title,
                required_minutes: item.duration_minutes,
                reason,
            });
            unscheduled.push(item.item_ref);
        }

        ItemSchedulingOutput {
            blocks,
            unscheduled,
            issues,
        }
    }

    fn required_minutes(&self, task: &Task) -> Minutes {
        let estimate = task
            .estimated_minutes
            .filter(|minutes| *minutes > 0)
            .unwrap_or(self.config.default_task_minutes);
        estimate.saturating_add(self.config.buffer_minutes)
    }
}

fn done_status_ids(statuses: &[Status], groups: &[StatusGroup]) -> HashSet<StatusId> {
    let group_kinds = groups
        .iter()
        .map(|group| (group.id.clone(), group.kind.clone()))
        .collect::<HashMap<_, _>>();

    statuses
        .iter()
        .filter(|status| {
            matches!(
                group_kinds.get(&status.group_id),
                Some(StatusGroupKind::Done)
            )
        })
        .map(|status| status.id.clone())
        .collect()
}

fn is_schedulable(task: &Task, done_status_ids: &HashSet<StatusId>) -> bool {
    task.deleted_at.is_none() && !done_status_ids.contains(&task.status_id)
}

fn compare_items(a: &SchedulableItem, b: &SchedulableItem) -> Ordering {
    tier_rank(a.tier)
        .cmp(&tier_rank(b.tier))
        .then_with(|| compare_due_dates(a.due, b.due))
        .then_with(|| b.importance.cmp(&a.importance))
        .then_with(|| a.created_at.cmp(&b.created_at))
        .then_with(|| a.title.cmp(&b.title))
        .then_with(|| compare_item_refs(&a.item_ref, &b.item_ref))
}

fn tier_rank(tier: ScheduleItemTier) -> u8 {
    match tier {
        ScheduleItemTier::RequiredHabit => 0,
        ScheduleItemTier::FlexibleHabit => 1,
        ScheduleItemTier::Task => 2,
    }
}

fn compare_item_refs(left: &ScheduleItemRef, right: &ScheduleItemRef) -> Ordering {
    match (left, right) {
        (ScheduleItemRef::Task(left), ScheduleItemRef::Task(right)) => {
            left.0.as_bytes().cmp(right.0.as_bytes())
        }
        (ScheduleItemRef::Task(_), ScheduleItemRef::HabitOccurrence(_)) => Ordering::Less,
        (ScheduleItemRef::HabitOccurrence(_), ScheduleItemRef::Task(_)) => Ordering::Greater,
        (ScheduleItemRef::HabitOccurrence(left), ScheduleItemRef::HabitOccurrence(right)) => {
            left.0.as_bytes().cmp(right.0.as_bytes())
        }
    }
}

fn compare_due_dates(left: Option<Date>, right: Option<Date>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn busy_windows(blocks: &[BusyBlock]) -> Vec<TimeWindow> {
    let busy = blocks.iter().map(|block| block.window).collect::<Vec<_>>();
    merge_windows(busy)
}

fn normalized_availability(availability: &[AvailabilityWindow]) -> Vec<TimeWindow> {
    merge_windows(
        availability
            .iter()
            .map(|availability| availability.window)
            .collect(),
    )
}

fn free_slots(availability: &[TimeWindow], busy: &[TimeWindow]) -> Vec<TimeWindow> {
    let mut result = Vec::new();

    for availability_window in availability {
        let mut slots = vec![*availability_window];
        for busy_window in busy {
            slots = slots
                .into_iter()
                .flat_map(|slot| subtract_window(slot, *busy_window))
                .collect();
        }
        result.extend(slots);
    }

    result.sort_by_key(|window| window.start);
    result
}

fn unscheduled_reason(
    item: &SchedulableItem,
    availability: &[TimeWindow],
    initially_has_free_time: bool,
) -> UnscheduledItemReason {
    if item.duration_minutes == 0 {
        return UnscheduledItemReason::InvalidDuration;
    }
    if availability.is_empty() {
        return UnscheduledItemReason::NoAvailability;
    }
    if !item.allowed_windows.is_empty()
        && !windows_have_intersection(availability, &item.allowed_windows)
    {
        return UnscheduledItemReason::NoAllowedWindow;
    }
    if !initially_has_free_time {
        return UnscheduledItemReason::NoAvailability;
    }
    UnscheduledItemReason::InsufficientContiguousTime
}

fn windows_have_intersection(left: &[TimeWindow], right: &[TimeWindow]) -> bool {
    left.iter().any(|left_window| {
        right
            .iter()
            .any(|right_window| left_window.overlaps(*right_window))
    })
}

fn subtract_window(slot: TimeWindow, busy: TimeWindow) -> Vec<TimeWindow> {
    if !slot.overlaps(busy) {
        return vec![slot];
    }

    let mut parts = Vec::new();
    if slot.start < busy.start {
        parts.push(TimeWindow::new(slot.start, busy.start));
    }
    if busy.end < slot.end {
        parts.push(TimeWindow::new(busy.end, slot.end));
    }
    parts
}

/// Reserve atomically: an item which cannot fully fit consumes no free time.
fn reserve_item(free: &[TimeWindow], item: &SchedulableItem) -> Option<Vec<TimeWindow>> {
    if item.duration_minutes == 0 {
        return None;
    }
    let mut candidates = Vec::new();
    for slot in free {
        let allowed = if item.allowed_windows.is_empty() {
            vec![*slot]
        } else {
            item.allowed_windows.clone()
        };
        for window in allowed {
            let start = slot
                .start
                .max(window.start)
                .max(item.not_before.unwrap_or(slot.start));
            let end = slot.end.min(window.end);
            if start < end {
                candidates.push(TimeWindow::new(start, end));
            }
        }
    }
    let mut candidates = merge_windows(candidates);
    if let Some(window) =
        reserve_first_fit_with_allowed(&mut candidates, item.duration_minutes, &[])
    {
        return Some(vec![window]);
    }
    let minimum = item.minimum_chunk_minutes?.max(1);
    let mut remaining = item.duration_minutes;
    let mut result = Vec::new();
    for slot in candidates {
        let mut minutes = remaining.min(slot.duration_minutes());
        if minutes < minimum {
            continue;
        }
        // Avoid leaving a final fragment below the minimum chunk size.
        if remaining > minutes && remaining - minutes < minimum {
            minutes = remaining.saturating_sub(minimum);
        }
        if minutes < minimum {
            continue;
        }
        result.push(TimeWindow::new(
            slot.start,
            slot.start + Duration::minutes(i64::from(minutes)),
        ));
        remaining -= minutes;
        if remaining == 0 {
            return Some(result);
        }
    }
    None
}

fn reserve_first_fit(free_slots: &mut Vec<TimeWindow>, minutes: Minutes) -> Option<TimeWindow> {
    let index = free_slots
        .iter()
        .position(|slot| slot.can_fit_minutes(minutes))?;

    let slot = free_slots[index];
    let reserved = TimeWindow::new(
        slot.start,
        slot.start + Duration::minutes(i64::from(minutes)),
    );

    consume_free_window(free_slots, index, reserved);
    Some(reserved)
}

fn reserve_first_fit_with_allowed(
    free_slots: &mut Vec<TimeWindow>,
    minutes: Minutes,
    allowed_windows: &[TimeWindow],
) -> Option<TimeWindow> {
    if allowed_windows.is_empty() {
        return reserve_first_fit(free_slots, minutes);
    }

    let allowed_windows = merge_windows(allowed_windows.to_vec());
    for index in 0..free_slots.len() {
        let slot = free_slots[index];
        for allowed in &allowed_windows {
            let candidate_start = max(slot.start, allowed.start);
            let candidate_limit = min(slot.end, allowed.end);
            let candidate_end = candidate_start + Duration::minutes(i64::from(minutes));
            if candidate_start >= candidate_limit || candidate_end > candidate_limit {
                continue;
            }

            let reserved = TimeWindow::new(candidate_start, candidate_end);
            consume_free_window(free_slots, index, reserved);
            return Some(reserved);
        }
    }

    None
}

fn consume_free_window(free_slots: &mut Vec<TimeWindow>, index: usize, reserved: TimeWindow) {
    let slot = free_slots[index];
    let remaining = subtract_window(slot, reserved);
    free_slots.splice(index..=index, remaining);
}

fn merge_windows(mut windows: Vec<TimeWindow>) -> Vec<TimeWindow> {
    if windows.is_empty() {
        return windows;
    }

    windows.sort_by_key(|window| window.start);
    let mut merged = Vec::with_capacity(windows.len());
    let mut current = windows[0];

    for window in windows.into_iter().skip(1) {
        if current.end >= window.start {
            current.end = max(current.end, window.end);
        } else {
            merged.push(current);
            current = window;
        }
    }

    merged.push(current);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};

    fn status_catalog() -> (Vec<Status>, Vec<StatusGroup>, StatusId, StatusId) {
        let todo_group = StatusGroup {
            id: StatusGroupId::new(),
            name: "Not started".into(),
            kind: StatusGroupKind::NotStarted,
        };
        let done_group = StatusGroup {
            id: StatusGroupId::new(),
            name: "Done".into(),
            kind: StatusGroupKind::Done,
        };

        let todo = Status {
            id: StatusId::new(),
            project_id: None,
            name: "To do".into(),
            group_id: todo_group.id.clone(),
            order: 0,
        };
        let done = Status {
            id: StatusId::new(),
            project_id: None,
            name: "Done".into(),
            group_id: done_group.id.clone(),
            order: 1,
        };

        (
            vec![todo.clone(), done.clone()],
            vec![todo_group, done_group],
            todo.id,
            done.id,
        )
    }

    fn task(id_status: StatusId, title: &str, minutes: u32, due_date: Option<Date>) -> Task {
        Task {
            id: TaskId::new(),
            title: title.into(),
            description: None,
            project_id: None,
            list_id: None,
            status_id: id_status,
            due_date,
            start_date: None,
            estimated_minutes: Some(minutes),
            cost_points: None,
            dependencies: vec![],
            milestone_id: None,
            created_at: datetime!(2026-06-25 00:00 UTC),
            updated_at: datetime!(2026-06-25 00:00 UTC),
            deleted_at: None,
        }
    }

    fn availability(start_hour: u8, end_hour: u8) -> AvailabilityWindow {
        AvailabilityWindow {
            window: TimeWindow::new(
                datetime!(2026-06-25 00:00 UTC) + Duration::hours(i64::from(start_hour)),
                datetime!(2026-06-25 00:00 UTC) + Duration::hours(i64::from(end_hour)),
            ),
        }
    }

    fn schedulable_item(
        item_ref: ScheduleItemRef,
        tier: ScheduleItemTier,
        title: &str,
        duration_minutes: Minutes,
        allowed_windows: Vec<TimeWindow>,
    ) -> SchedulableItem {
        let kind = item_ref.kind();
        SchedulableItem {
            item_ref,
            kind,
            tier,
            title: title.into(),
            duration_minutes,
            due: None,
            importance: 0,
            created_at: datetime!(2026-06-25 00:00 UTC),
            allowed_windows,
            not_before: None,
            dependencies: Vec::new(),
            minimum_chunk_minutes: None,
        }
    }

    #[test]
    fn schedules_task_into_available_window() {
        let (statuses, status_groups, todo_status, _) = status_catalog();
        let input = SchedulingInput {
            tasks: vec![task(todo_status, "Write spec", 45, None)],
            statuses,
            status_groups,
            availability: vec![availability(9, 11)],
            busy_blocks: vec![],
        };

        let output = GreedyScheduler::default().plan(input);

        assert_eq!(output.blocks.len(), 1);
        assert!(output.unscheduled.is_empty());
        assert_eq!(output.blocks[0].required_minutes, 45);
        assert_eq!(
            output.blocks[0].window.start,
            datetime!(2026-06-25 09:00 UTC)
        );
    }

    #[test]
    fn dependency_order_and_start_cutoff_override_due_priority() {
        let first = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "First",
            60,
            vec![],
        );
        let mut next = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Next",
            30,
            vec![],
        );
        next.due = Some(date!(2026 - 06 - 25));
        next.dependencies = vec![first.item_ref.clone()];
        next.not_before = Some(datetime!(2026-06-25 11:30 UTC));
        let output = GreedyScheduler::default().plan_items(ItemSchedulingInput {
            items: vec![next, first],
            availability: vec![availability(9, 13)],
            hard_busy: vec![],
        });
        assert!(output.unscheduled.is_empty());
        assert_eq!(output.blocks[0].title, "First");
        assert_eq!(
            output.blocks[1].window.start,
            datetime!(2026-06-25 11:30 UTC)
        );
    }

    #[test]
    fn missing_and_cyclic_dependencies_do_not_consume_capacity() {
        let mut a = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "A",
            60,
            vec![],
        );
        let mut b = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "B",
            60,
            vec![],
        );
        a.dependencies = vec![b.item_ref.clone()];
        b.dependencies = vec![a.item_ref.clone()];
        let mut missing = a.clone();
        missing.item_ref = ScheduleItemRef::Task(TaskId::new());
        missing.dependencies = vec![ScheduleItemRef::Task(TaskId::new())];
        let free = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Free",
            60,
            vec![],
        );
        let output = GreedyScheduler::default().plan_items(ItemSchedulingInput {
            items: vec![a, b, missing, free],
            availability: vec![availability(9, 10)],
            hard_busy: vec![],
        });
        assert_eq!(output.blocks.len(), 1);
        assert_eq!(output.blocks[0].title, "Free");
        assert_eq!(output.unscheduled.len(), 3);
        assert!(output.issues.iter().all(|issue| matches!(
            issue,
            ItemScheduleIssue::ItemUnscheduled {
                reason: UnscheduledItemReason::DependencyBlocked,
                ..
            }
        )));
    }

    #[test]
    fn splitting_preserves_total_duration_and_dependency_end() {
        let mut task = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Split",
            90,
            vec![],
        );
        task.minimum_chunk_minutes = Some(15);
        let mut child = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Child",
            30,
            vec![],
        );
        child.dependencies = vec![task.item_ref.clone()];
        let output = GreedyScheduler::default().plan_items(ItemSchedulingInput {
            items: vec![child, task],
            availability: vec![availability(9, 10), availability(11, 12)],
            hard_busy: vec![],
        });
        assert_eq!(output.blocks.len(), 3);
        assert_eq!(
            output.blocks[0].required_minutes + output.blocks[1].required_minutes,
            90
        );
        assert_eq!(
            output.blocks[2].window.start,
            datetime!(2026-06-25 11:30 UTC)
        );
        assert!(output.unscheduled.is_empty());
    }

    #[test]
    fn failed_split_is_atomic_and_habits_remain_contiguous() {
        let mut long = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Long",
            150,
            vec![],
        );
        long.minimum_chunk_minutes = Some(15);
        let small = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Small",
            30,
            vec![],
        );
        let habit = schedulable_item(
            ScheduleItemRef::HabitOccurrence(HabitOccurrenceId::new()),
            ScheduleItemTier::RequiredHabit,
            "Habit",
            90,
            vec![],
        );
        let output = GreedyScheduler::default().plan_items(ItemSchedulingInput {
            items: vec![long, small, habit],
            availability: vec![availability(9, 10), availability(11, 12)],
            hard_busy: vec![],
        });
        assert_eq!(output.blocks.len(), 1);
        assert_eq!(output.blocks[0].title, "Small");
        assert_eq!(
            output.blocks[0].window.start,
            datetime!(2026-06-25 09:00 UTC)
        );
        assert_eq!(output.unscheduled.len(), 2);
    }

    #[test]
    fn respects_busy_blocks() {
        let (statuses, status_groups, todo_status, _) = status_catalog();
        let input = SchedulingInput {
            tasks: vec![task(todo_status, "Write spec", 45, None)],
            statuses,
            status_groups,
            availability: vec![availability(9, 12)],
            busy_blocks: vec![BusyBlock {
                window: TimeWindow::new(
                    datetime!(2026-06-25 09:00 UTC),
                    datetime!(2026-06-25 10:00 UTC),
                ),
                source: BusyBlockSource::ExternalCalendar,
                label: Some("Meeting".into()),
            }],
        };

        let output = GreedyScheduler::default().plan(input);

        assert_eq!(output.blocks.len(), 1);
        assert_eq!(
            output.blocks[0].window.start,
            datetime!(2026-06-25 10:00 UTC)
        );
    }

    #[test]
    fn marks_task_unscheduled_when_capacity_is_insufficient() {
        let (statuses, status_groups, todo_status, _) = status_catalog();
        let input = SchedulingInput {
            tasks: vec![task(todo_status, "Long task", 120, None)],
            statuses,
            status_groups,
            availability: vec![availability(9, 10)],
            busy_blocks: vec![],
        };

        let output = GreedyScheduler::default().plan(input);

        assert!(output.blocks.is_empty());
        assert_eq!(output.unscheduled.len(), 1);
        assert!(matches!(
            output.issues.as_slice(),
            [ScheduleIssue::TaskUnscheduled { .. }]
        ));
    }

    #[test]
    fn ignores_done_tasks() {
        let (statuses, status_groups, _, done_status) = status_catalog();
        let input = SchedulingInput {
            tasks: vec![task(done_status, "Already done", 30, None)],
            statuses,
            status_groups,
            availability: vec![availability(9, 10)],
            busy_blocks: vec![],
        };

        let output = GreedyScheduler::default().plan(input);

        assert!(output.blocks.is_empty());
        assert!(output.unscheduled.is_empty());
    }

    #[test]
    fn schedules_earlier_due_date_first() {
        let (statuses, status_groups, todo_status, _) = status_catalog();
        let later = task(
            todo_status.clone(),
            "Later",
            30,
            Some(date!(2026 - 06 - 27)),
        );
        let earlier = task(todo_status, "Earlier", 30, Some(date!(2026 - 06 - 26)));
        let input = SchedulingInput {
            tasks: vec![later, earlier],
            statuses,
            status_groups,
            availability: vec![availability(9, 11)],
            busy_blocks: vec![],
        };

        let output = GreedyScheduler::default().plan(input);

        assert_eq!(output.blocks[0].title, "Earlier");
        assert_eq!(output.blocks[1].title, "Later");
    }

    #[test]
    fn plans_tasks_and_habit_occurrences_by_tier() {
        let task_ref = ScheduleItemRef::Task(TaskId::new());
        let required_habit_ref = ScheduleItemRef::HabitOccurrence(HabitOccurrenceId::new());
        let flexible_habit_ref = ScheduleItemRef::HabitOccurrence(HabitOccurrenceId::new());
        let input = ItemSchedulingInput {
            items: vec![
                schedulable_item(task_ref.clone(), ScheduleItemTier::Task, "Task", 30, vec![]),
                schedulable_item(
                    flexible_habit_ref.clone(),
                    ScheduleItemTier::FlexibleHabit,
                    "Flexible habit",
                    30,
                    vec![],
                ),
                schedulable_item(
                    required_habit_ref.clone(),
                    ScheduleItemTier::RequiredHabit,
                    "Required habit",
                    30,
                    vec![],
                ),
            ],
            availability: vec![availability(9, 11)],
            hard_busy: vec![],
        };

        let output = GreedyScheduler::default().plan_items(input);

        assert_eq!(output.blocks.len(), 3);
        assert_eq!(output.blocks[0].item_ref, required_habit_ref);
        assert_eq!(output.blocks[1].item_ref, flexible_habit_ref);
        assert_eq!(output.blocks[2].item_ref, task_ref);
        assert_eq!(
            output.blocks[0].window.start,
            datetime!(2026-06-25 09:00 UTC)
        );
        assert!(output.unscheduled.is_empty());
    }

    #[test]
    fn respects_hard_busy_across_multiple_availability_windows() {
        let first_ref = ScheduleItemRef::Task(TaskId::new());
        let second_ref = ScheduleItemRef::Task(TaskId::new());
        let input = ItemSchedulingInput {
            items: vec![
                schedulable_item(first_ref, ScheduleItemTier::Task, "First", 60, vec![]),
                schedulable_item(second_ref, ScheduleItemTier::Task, "Second", 60, vec![]),
            ],
            // Deliberately unsorted to verify normalization.
            availability: vec![availability(13, 15), availability(9, 11)],
            hard_busy: vec![BusyBlock {
                window: TimeWindow::new(
                    datetime!(2026-06-25 09:00 UTC),
                    datetime!(2026-06-25 10:00 UTC),
                ),
                source: BusyBlockSource::ExternalCalendar,
                label: Some("Hard meeting".into()),
            }],
        };

        let output = GreedyScheduler::default().plan_items(input);
        let starts = output
            .blocks
            .iter()
            .map(|block| block.window.start)
            .collect::<Vec<_>>();

        assert_eq!(
            starts,
            vec![
                datetime!(2026-06-25 10:00 UTC),
                datetime!(2026-06-25 13:00 UTC),
            ]
        );
        assert!(
            output
                .blocks
                .iter()
                .all(|block| !block.window.overlaps(TimeWindow::new(
                    datetime!(2026-06-25 09:00 UTC),
                    datetime!(2026-06-25 10:00 UTC),
                )))
        );
    }

    #[test]
    fn intersects_item_allowed_windows_and_preserves_other_free_time() {
        let habit_ref = ScheduleItemRef::HabitOccurrence(HabitOccurrenceId::new());
        let task_ref = ScheduleItemRef::Task(TaskId::new());
        let input = ItemSchedulingInput {
            items: vec![
                schedulable_item(
                    task_ref.clone(),
                    ScheduleItemTier::Task,
                    "Morning task",
                    60,
                    vec![],
                ),
                schedulable_item(
                    habit_ref.clone(),
                    ScheduleItemTier::RequiredHabit,
                    "Afternoon habit",
                    60,
                    vec![TimeWindow::new(
                        datetime!(2026-06-25 13:00 UTC),
                        datetime!(2026-06-25 14:00 UTC),
                    )],
                ),
            ],
            availability: vec![availability(9, 15)],
            hard_busy: vec![],
        };

        let output = GreedyScheduler::default().plan_items(input);
        let habit = output
            .blocks
            .iter()
            .find(|block| block.item_ref == habit_ref)
            .unwrap();
        let task = output
            .blocks
            .iter()
            .find(|block| block.item_ref == task_ref)
            .unwrap();

        assert_eq!(habit.window.start, datetime!(2026-06-25 13:00 UTC));
        assert_eq!(task.window.start, datetime!(2026-06-25 09:00 UTC));
    }

    #[test]
    fn produces_the_same_plan_regardless_of_equal_item_input_order() {
        let first = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Same",
            30,
            vec![],
        );
        let second = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Same",
            30,
            vec![],
        );
        let scheduler = GreedyScheduler::default();
        let forward = scheduler.plan_items(ItemSchedulingInput {
            items: vec![first.clone(), second.clone()],
            availability: vec![availability(9, 10)],
            hard_busy: vec![],
        });
        let reversed = scheduler.plan_items(ItemSchedulingInput {
            items: vec![second, first],
            availability: vec![availability(9, 10)],
            hard_busy: vec![],
        });

        assert_eq!(forward, reversed);
    }

    #[test]
    fn orders_same_tier_by_due_importance_and_creation_time() {
        let mut due_first = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Due first",
            10,
            vec![],
        );
        due_first.due = Some(date!(2026 - 06 - 26));
        due_first.importance = 0;
        due_first.created_at = datetime!(2026-06-25 02:00 UTC);

        let mut due_later = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Due later",
            10,
            vec![],
        );
        due_later.due = Some(date!(2026 - 06 - 27));
        due_later.importance = 100;
        due_later.created_at = datetime!(2026-06-25 00:00 UTC);

        let mut important = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Important",
            10,
            vec![],
        );
        important.importance = 10;
        important.created_at = datetime!(2026-06-25 02:00 UTC);

        let mut older = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Older",
            10,
            vec![],
        );
        older.importance = 5;
        older.created_at = datetime!(2026-06-25 00:00 UTC);

        let mut newer = schedulable_item(
            ScheduleItemRef::Task(TaskId::new()),
            ScheduleItemTier::Task,
            "Newer",
            10,
            vec![],
        );
        newer.importance = 5;
        newer.created_at = datetime!(2026-06-25 01:00 UTC);

        let expected = vec![
            due_first.item_ref.clone(),
            due_later.item_ref.clone(),
            important.item_ref.clone(),
            older.item_ref.clone(),
            newer.item_ref.clone(),
        ];
        let output = GreedyScheduler::default().plan_items(ItemSchedulingInput {
            items: vec![newer, older, important, due_later, due_first],
            availability: vec![availability(9, 11)],
            hard_busy: vec![],
        });

        assert_eq!(
            output
                .blocks
                .into_iter()
                .map(|block| block.item_ref)
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn reports_specific_unscheduled_reasons() {
        let invalid_ref = ScheduleItemRef::Task(TaskId::new());
        let outside_ref = ScheduleItemRef::HabitOccurrence(HabitOccurrenceId::new());
        let too_long_ref = ScheduleItemRef::Task(TaskId::new());
        let output = GreedyScheduler::default().plan_items(ItemSchedulingInput {
            items: vec![
                schedulable_item(
                    invalid_ref.clone(),
                    ScheduleItemTier::Task,
                    "Invalid",
                    0,
                    vec![],
                ),
                schedulable_item(
                    outside_ref.clone(),
                    ScheduleItemTier::FlexibleHabit,
                    "Outside",
                    30,
                    vec![TimeWindow::new(
                        datetime!(2026-06-25 12:00 UTC),
                        datetime!(2026-06-25 13:00 UTC),
                    )],
                ),
                schedulable_item(
                    too_long_ref.clone(),
                    ScheduleItemTier::Task,
                    "Too long",
                    90,
                    vec![],
                ),
            ],
            availability: vec![availability(9, 10)],
            hard_busy: vec![],
        });

        let reason_for = |item_ref: &ScheduleItemRef| {
            output.issues.iter().find_map(|issue| match issue {
                ItemScheduleIssue::ItemUnscheduled {
                    item_ref: issue_ref,
                    reason,
                    ..
                } if issue_ref == item_ref => Some(*reason),
                _ => None,
            })
        };

        assert_eq!(
            reason_for(&invalid_ref),
            Some(UnscheduledItemReason::InvalidDuration)
        );
        assert_eq!(
            reason_for(&outside_ref),
            Some(UnscheduledItemReason::NoAllowedWindow)
        );
        assert_eq!(
            reason_for(&too_long_ref),
            Some(UnscheduledItemReason::InsufficientContiguousTime)
        );
        assert_eq!(output.unscheduled.len(), 3);
    }

    #[test]
    fn reports_no_availability_when_hard_busy_covers_everything() {
        let item_ref = ScheduleItemRef::Task(TaskId::new());
        let output = GreedyScheduler::default().plan_items(ItemSchedulingInput {
            items: vec![schedulable_item(
                item_ref.clone(),
                ScheduleItemTier::Task,
                "Blocked",
                30,
                vec![],
            )],
            availability: vec![availability(9, 10)],
            hard_busy: vec![BusyBlock {
                window: TimeWindow::new(
                    datetime!(2026-06-25 09:00 UTC),
                    datetime!(2026-06-25 10:00 UTC),
                ),
                source: BusyBlockSource::LockedSchedule,
                label: None,
            }],
        });

        assert!(output.blocks.is_empty());
        assert!(
            output
                .issues
                .iter()
                .any(|issue| matches!(issue, ItemScheduleIssue::NoAvailability))
        );
        assert!(output.issues.iter().any(|issue| matches!(
            issue,
            ItemScheduleIssue::ItemUnscheduled {
                item_ref: issue_ref,
                reason: UnscheduledItemReason::NoAvailability,
                ..
            } if issue_ref == &item_ref
        )));
    }
}
