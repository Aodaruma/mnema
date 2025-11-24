use crate::ids::{MilestoneId, ProjectId, TaskId};
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MilestoneStatus {
    NotDone,
    Overdue,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Milestone {
    pub id: MilestoneId,
    pub project_id: ProjectId,
    pub title: String,
    pub description: Option<String>,
    pub target_date: Date,
    pub status: MilestoneStatus,
    pub dependency_task_ids: Vec<TaskId>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}
