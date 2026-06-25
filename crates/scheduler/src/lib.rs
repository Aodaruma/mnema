//! Deterministic scheduling and repair primitives for Mnema.
//!
//! This crate intentionally avoids storage, UI, and LLM dependencies. It turns
//! existing Mnema tasks into proposed schedule blocks that can later be reviewed
//! and persisted by application services.

use std::cmp::max;
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
        let mut issues = Vec::new();
        let done_status_ids = done_status_ids(&input.statuses, &input.status_groups);
        let busy = busy_windows(&input.busy_blocks);
        let mut free_slots = free_slots(&input.availability, &busy);

        if free_slots.is_empty() {
            issues.push(ScheduleIssue::NoAvailability);
        }

        let mut tasks = input
            .tasks
            .into_iter()
            .filter(|task| is_schedulable(task, &done_status_ids))
            .collect::<Vec<_>>();

        tasks.sort_by(compare_tasks);

        let mut blocks = Vec::new();
        let mut unscheduled = Vec::new();

        for task in tasks {
            let required_minutes = self.required_minutes(&task);
            match reserve_first_fit(&mut free_slots, required_minutes) {
                Some(window) => blocks.push(ProposedScheduleBlock {
                    task_id: task.id,
                    title: task.title,
                    window,
                    required_minutes,
                }),
                None => {
                    issues.push(ScheduleIssue::TaskUnscheduled {
                        task_id: task.id.clone(),
                        title: task.title,
                        required_minutes,
                    });
                    unscheduled.push(task.id);
                }
            }
        }

        SchedulingOutput {
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

fn compare_tasks(a: &Task, b: &Task) -> std::cmp::Ordering {
    compare_due_dates(a.due_date, b.due_date)
        .then_with(|| {
            b.cost_points
                .unwrap_or_default()
                .cmp(&a.cost_points.unwrap_or_default())
        })
        .then_with(|| a.created_at.cmp(&b.created_at))
        .then_with(|| a.title.cmp(&b.title))
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

fn free_slots(availability: &[AvailabilityWindow], busy: &[TimeWindow]) -> Vec<TimeWindow> {
    let mut result = Vec::new();

    for availability_window in availability {
        let mut slots = vec![availability_window.window];
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

fn reserve_first_fit(free_slots: &mut Vec<TimeWindow>, minutes: Minutes) -> Option<TimeWindow> {
    let index = free_slots
        .iter()
        .position(|slot| slot.can_fit_minutes(minutes))?;

    let slot = free_slots[index];
    let reserved = TimeWindow::new(
        slot.start,
        slot.start + Duration::minutes(i64::from(minutes)),
    );

    if reserved.end == slot.end {
        free_slots.remove(index);
    } else {
        free_slots[index] = TimeWindow::new(reserved.end, slot.end);
    }

    Some(reserved)
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
}
