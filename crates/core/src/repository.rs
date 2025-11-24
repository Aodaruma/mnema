use crate::list::List;
use crate::milestone::Milestone;
use crate::project::Project;
use crate::status::{Status, StatusGroup};
use crate::task::Task;
use crate::{
    ids::{ListId, MilestoneId, ProjectId, StatusGroupId, StatusId, TaskId},
    user_settings::UserSettings,
};
use thiserror::Error;
use time::OffsetDateTime;

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("not found")]
    NotFound,
    #[error("storage error: {0}")]
    Storage(String),
}

#[async_trait::async_trait]
pub trait TaskRepository: Send + Sync {
    async fn insert(&self, task: Task) -> CoreResult<()>;
    async fn find(&self, id: TaskId) -> CoreResult<Option<Task>>;
    async fn update(&self, task: Task) -> CoreResult<()>;
    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Task>>;
    async fn list_by_list(&self, list_id: ListId) -> CoreResult<Vec<Task>>;
    async fn soft_delete(&self, id: TaskId, deleted_at: OffsetDateTime) -> CoreResult<()>;
}

#[async_trait::async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn insert(&self, project: Project) -> CoreResult<()>;
    async fn find(&self, id: ProjectId) -> CoreResult<Option<Project>>;
    async fn list_all(&self) -> CoreResult<Vec<Project>>;
    async fn update(&self, project: Project) -> CoreResult<()>;
}

#[async_trait::async_trait]
pub trait ListRepository: Send + Sync {
    async fn insert(&self, list: List) -> CoreResult<()>;
    async fn find(&self, id: ListId) -> CoreResult<Option<List>>;
    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<List>>;
    async fn list_system(&self) -> CoreResult<Vec<List>>;
    async fn update(&self, list: List) -> CoreResult<()>;
}

#[async_trait::async_trait]
pub trait MilestoneRepository: Send + Sync {
    async fn insert(&self, milestone: Milestone) -> CoreResult<()>;
    async fn find(&self, id: MilestoneId) -> CoreResult<Option<Milestone>>;
    async fn list_by_project(&self, project_id: ProjectId) -> CoreResult<Vec<Milestone>>;
    async fn update(&self, milestone: Milestone) -> CoreResult<()>;
}

#[async_trait::async_trait]
pub trait StatusRepository: Send + Sync {
    async fn insert_group(&self, group: StatusGroup) -> CoreResult<()>;
    async fn insert_status(&self, status: Status) -> CoreResult<()>;
    async fn list_groups(&self) -> CoreResult<Vec<StatusGroup>>;
    async fn list_statuses_for_project(
        &self,
        project_id: Option<ProjectId>,
    ) -> CoreResult<Vec<Status>>;
}

#[async_trait::async_trait]
pub trait UserSettingsRepository: Send + Sync {
    async fn upsert(&self, settings: UserSettings) -> CoreResult<()>;
    async fn get(&self, user_id: crate::ids::UserId) -> CoreResult<Option<UserSettings>>;
}
