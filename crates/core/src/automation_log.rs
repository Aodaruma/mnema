use crate::ids::{AssistantId, AutomationLogId, ListId, ProjectId, TaskId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AutomationActionType {
    Move,
    UpdateDue,
    Classify,
    CreateTask,
    UpdateStatus,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationLog {
    pub id: AutomationLogId,
    pub task_id: Option<TaskId>,
    pub project_id: Option<ProjectId>,
    pub list_id: Option<ListId>,
    pub assistant_id: AssistantId,
    pub action_type: AutomationActionType,
    pub before_state: Option<Value>,
    pub after_state: Option<Value>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub explanation: Option<String>,
}
