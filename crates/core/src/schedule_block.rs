use crate::ids::{HabitOccurrenceId, ScheduleBlockId, TaskId};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ScheduleBlockType {
    Task,
    Habit,
    ExternalEvent,
    Buffer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ScheduleBlockState {
    Proposed,
    Scheduled,
    Active,
    Done,
    Missed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ScheduleBlockSource {
    Scheduler,
    Repair,
    Manual,
    ExternalCalendar,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleBlock {
    pub id: ScheduleBlockId,
    pub task_id: Option<TaskId>,
    pub habit_occurrence_id: Option<HabitOccurrenceId>,
    pub title_snapshot: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub start_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub end_at: OffsetDateTime,
    pub block_type: ScheduleBlockType,
    pub state: ScheduleBlockState,
    pub locked: bool,
    pub source: ScheduleBlockSource,
    pub required_minutes: Option<u32>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
