use crate::ids::{ProjectId, StatusGroupId};
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub title: String,
    pub description: Option<String>,
    pub start_date: Option<Date>,
    pub end_date: Option<Date>,
    pub default_status_set_id: Option<StatusGroupId>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub archived_at: Option<OffsetDateTime>,
}
