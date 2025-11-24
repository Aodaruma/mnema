use crate::ids::{ListId, MilestoneId, ProjectId, StatusId, TaskId};
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub description: Option<String>,
    pub project_id: Option<ProjectId>,
    pub list_id: Option<ListId>,
    pub status_id: StatusId,
    pub due_date: Option<Date>,
    pub start_date: Option<Date>,
    pub estimated_minutes: Option<u32>,
    pub cost_points: Option<u32>,
    pub dependencies: Vec<TaskId>,
    pub milestone_id: Option<MilestoneId>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub deleted_at: Option<OffsetDateTime>,
}
