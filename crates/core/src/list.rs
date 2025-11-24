use crate::ids::{ListId, ProjectId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ListKind {
    Inbox,
    Personal,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ListViewType {
    List,
    Board,
    Calendar,
    Gantt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct List {
    pub id: ListId,
    pub project_id: Option<ProjectId>,
    pub name: String,
    pub is_system: bool,
    pub kind: ListKind,
    pub view_type: ListViewType,
    pub order: i32,
}
