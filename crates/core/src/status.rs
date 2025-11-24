use crate::ids::{ProjectId, StatusGroupId, StatusId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StatusGroupKind {
    NotStarted,
    InProgress,
    Pending,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusGroup {
    pub id: StatusGroupId,
    pub name: String,
    pub kind: StatusGroupKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub id: StatusId,
    pub project_id: Option<ProjectId>,
    pub name: String,
    pub group_id: StatusGroupId,
    pub order: i32,
}
